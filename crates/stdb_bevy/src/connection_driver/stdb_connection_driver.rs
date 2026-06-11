use bevy::ecs::{observer::On, resource::Resource, system::Res};

use crate::{
    StdbConn, StdbConnection,
    connection::connection_events::{StdbConnect, StdbDisconnect},
    lifecycle::lifecycle_channel::{LifecycleChannel, LifecycleSink},
};

pub trait StdbConnectionDriver: Clone + Resource {
    type Conn: StdbConn;

    fn connect(&self, sink: LifecycleSink<Self::Conn>);

    fn disconnect(&self, conn: &StdbConnection<Self::Conn>, sink: LifecycleSink<Self::Conn>);

    fn tick(&self, conn: &StdbConnection<Self::Conn>);
}

pub(crate) fn connect_on_stdbconnect<Cd: StdbConnectionDriver>(
    _: On<StdbConnect>,
    driver: Res<Cd>,
    lifecycle_channel: Res<LifecycleChannel<Cd::Conn>>,
) {
    let sink = lifecycle_channel.sink();
    driver.connect(sink);
}

pub(crate) fn disconnect_on_stdbdisconnect<Cd: StdbConnectionDriver>(
    _: On<StdbDisconnect>,
    driver: Res<Cd>,
    connection: Option<Res<StdbConnection<Cd::Conn>>>,
    lifecycle_channel: Res<LifecycleChannel<Cd::Conn>>,
) {
    if let Some(connection) = connection {
        let sink = lifecycle_channel.sink();
        driver.disconnect(&connection, sink);
    }
}

pub(crate) fn tick_stdbconnectiondriver<Cd: StdbConnectionDriver>(
    driver: Res<Cd>,
    connection: Res<StdbConnection<Cd::Conn>>,
) {
    driver.tick(&connection);
}

#[cfg(test)]
mod tests {
    use crate::{
        connection::stdb_status::StdbStatus,
        test_support::{FakeConn, FakeConnectionDriver, test_app},
    };

    use super::*;

    #[test]
    fn stdb_connect_establishes_the_connection() {
        let mut app = test_app(FakeConnectionDriver::default());

        app.world_mut().trigger(StdbConnect);
        app.update();

        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some(),
            "StdbConnect should establish the connection",
        );
    }

    #[test]
    fn stdb_disconnect_closes_the_connection() {
        let mut app = test_app(FakeConnectionDriver::default());

        // Connect first.
        app.world_mut().trigger(StdbConnect);
        app.update();
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some()
        );

        // Now disconnect.
        app.world_mut().trigger(StdbDisconnect);
        app.update();

        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_none(),
            "StdbDisconnect should close the connection",
        );
    }

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Connects synchronously (like the fake) but counts how often it's ticked, so the per-frame
    /// connection pump is observable.
    #[derive(Resource, Clone, Default)]
    struct CountingDriver {
        ticks: Arc<AtomicUsize>,
    }

    impl StdbConnectionDriver for CountingDriver {
        type Conn = FakeConn;

        fn connect(&self, sink: LifecycleSink<FakeConn>) {
            sink.connected(FakeConn).unwrap();
        }

        fn tick(&self, _conn: &StdbConnection<FakeConn>) {
            self.ticks.fetch_add(1, Ordering::Relaxed);
        }

        fn disconnect(&self, _conn: &StdbConnection<FakeConn>, sink: LifecycleSink<FakeConn>) {
            sink.disconnected().unwrap();
        }
    }

    #[test]
    fn connection_is_ticked_each_frame_while_connected() {
        let driver = CountingDriver::default();
        let probe = driver.clone(); // shares the tick counter with the one the plugin owns
        let mut app = test_app(driver);

        // Not connected → never ticked.
        app.update();
        app.update();
        assert_eq!(
            probe.ticks.load(Ordering::Relaxed),
            0,
            "must not tick before a connection exists",
        );

        // Connected → ticked each frame.
        app.world_mut().trigger(StdbConnect);
        app.update();
        let after_connect = probe.ticks.load(Ordering::Relaxed);
        app.update();
        app.update();
        assert!(
            probe.ticks.load(Ordering::Relaxed) > after_connect,
            "the connection must be ticked while connected",
        );

        // Disconnected → ticking stops.
        app.world_mut().trigger(StdbDisconnect);
        app.update();
        let after_disconnect = probe.ticks.load(Ordering::Relaxed);
        app.update();
        app.update();
        assert_eq!(
            probe.ticks.load(Ordering::Relaxed),
            after_disconnect,
            "the connection must not be ticked after disconnect",
        );
    }

    #[test]
    fn connect_on_startup_arms_the_reconnect_intent() {
        use crate::StdbPlugin;
        use crate::connection::stdb_intent::StdbIntent;

        let mut app = bevy::app::App::new();
        app.add_plugins(StdbPlugin::new(FakeConnectionDriver::default()).with_connect_on_startup());
        app.insert_resource(bevy::time::Time::<()>::default());

        app.update(); // Startup connects, then the lifecycle drains.

        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        assert_eq!(
            *app.world().resource::<StdbIntent>(),
            StdbIntent::Connected,
            "connect-on-startup must set Connected intent, else auto-reconnect is never armed",
        );
    }

    #[test]
    fn reconnects_after_a_drop_when_connected_on_startup() {
        use crate::{Backoff, Jitter, ReconnectPolicy, StdbPlugin};
        use std::time::Duration;

        let driver = FakeConnectionDriver::default();
        let probe = driver.clone(); // retains the sink, to simulate an unsolicited drop
        let mut app = bevy::app::App::new();
        app.add_plugins(StdbPlugin::new(driver).with_connect_on_startup());
        app.insert_resource(bevy::time::Time::<()>::default());
        app.insert_resource(ReconnectPolicy {
            backoff: Backoff::Fixed(Duration::from_secs(1)),
            jitter: Jitter(0.0),
            max_retries: None,
        });

        // Startup connect.
        app.update();
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);

        // Unsolicited drop (the real `on_disconnect` path).
        probe.sink().disconnected().unwrap();
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );

        // Past the backoff: must reconnect — only happens if the intent was armed on startup.
        app.world_mut()
            .resource_mut::<bevy::time::Time>()
            .advance_by(Duration::from_millis(1100));
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Connected,
            "a connection established on startup must auto-reconnect after a drop",
        );
    }
}
