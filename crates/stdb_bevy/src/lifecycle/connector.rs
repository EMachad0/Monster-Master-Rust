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
    connection: Option<Res<StdbConnection<Cn::Conn>>>,
    lifecycle_channel: Res<LifecycleChannel<Cn::Conn>>,
) {
    if let Some(connection) = connection {
        let sink = lifecycle_channel.sink();
        connector.disconnect(connection.deref(), sink);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        StdbConnection, StdbStatus,
        lifecycle::stdb_connection::{StdbConnect, StdbDisconnect},
        test_support::{FakeConn, FakeConnector, test_app},
    };

    #[test]
    fn stdb_connect_establishes_the_connection() {
        let mut app = test_app(FakeConnector);

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
        let mut app = test_app(FakeConnector);

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
