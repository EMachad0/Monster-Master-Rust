use std::ops::Deref;

use bevy::ecs::{observer::On, resource::Resource, system::Res};

use crate::{
    StdbConn, StdbConnection,
    lifecycle::{
        lifecycle_channel::{LifecycleChannel, LifecycleSink},
        stdb_connection::{StdbConnect, StdbDisconnect},
    },
};

pub trait Connector: Resource {
    type Conn: StdbConn;

    fn connect(&self, sink: LifecycleSink<Self::Conn>);

    fn disconnect(&self, conn: &StdbConnection<Self::Conn>, sink: LifecycleSink<Self::Conn>);

    fn tick(&self, conn: &StdbConnection<Self::Conn>);
}

pub(crate) fn connect_on_stdbconnect<Cn: Connector>(
    _: On<StdbConnect>,
    connector: Res<Cn>,
    lifecycle_channel: Res<LifecycleChannel<Cn::Conn>>,
) {
    let sink = lifecycle_channel.sink();
    connector.connect(sink);
}

pub(crate) fn disconnect_on_stdbdisconnect<Cn: Connector>(
    _: On<StdbDisconnect>,
    connector: Res<Cn>,
    connicion: Res<StdbConnection<Cn::Conn>>,
    lifecycle_channel: Res<LifecycleChannel<Cn::Conn>>,
) {
    let sink = lifecycle_channel.sink();
    connector.disconnect(connicion.deref(), sink);
}

#[cfg(test)]
mod tests {
    use bevy::{app::App, ecs::resource::Resource};

    use crate::{
        StdbConnection, StdbStatus, install_fulfillment, install_lifecycle,
        lifecycle::stdb_connection::{StdbConnect, StdbDisconnect},
    };

    use super::*;

    /// Stand-in for a real `DbConnection`. The lifecycle engine only requires `Send + Sync + 'static`.
    #[derive(Clone, Default)]
    struct FakeConn;

    /// A connector whose I/O is synchronous and in-memory, so fulfillment can be tested with no socket.
    #[derive(Resource)]
    struct FakeConnector;

    impl Connector for FakeConnector {
        type Conn = FakeConn;

        fn connect(&self, sink: LifecycleSink<FakeConn>) {
            // Synchronous success: hand the connection straight back through the sink.
            sink.connected(FakeConn).unwrap();
        }

        fn tick(&self, _conn: &StdbConnection<FakeConn>) {}

        fn disconnect(&self, _conn: &StdbConnection<FakeConn>, sink: LifecycleSink<FakeConn>) {
            sink.disconnected().unwrap();
        }
    }

    #[test]
    fn stdb_connect_establishes_the_connection() {
        let mut app = App::new();
        install_lifecycle::<FakeConn>(&mut app);
        install_fulfillment(&mut app, FakeConnector);

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
        let mut app = App::new();
        install_lifecycle::<FakeConn>(&mut app);
        install_fulfillment(&mut app, FakeConnector);

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
