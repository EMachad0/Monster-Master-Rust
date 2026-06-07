use std::ops::Deref;

use bevy::ecs::{observer::On, resource::Resource, system::Res};

use crate::{
    StdbConn, StdbConnection,
    connection::connection_events::{StdbConnect, StdbDisconnect},
    lifecycle::lifecycle_channel::{LifecycleChannel, LifecycleSink},
};

pub trait StdbConnectionDriver: Resource {
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
        driver.disconnect(connection.deref(), sink);
    }
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
        let mut app = test_app(FakeConnectionDriver);

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
        let mut app = test_app(FakeConnectionDriver);

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
}
