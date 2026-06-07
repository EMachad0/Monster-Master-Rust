//! Minimal, module-agnostic bridge between a SpacetimeDB connection and Bevy.
//!
//! This crate knows nothing about any specific Module.

use bevy::ecs::schedule::IntoScheduleConfigs;

use crate::lifecycle::connector::{connect_on_stdbconnect, disconnect_on_stdbdisconnect};
use crate::lifecycle::lifecycle_channel::{LifecycleChannel, drain_lifecycle_sink};
use crate::lifecycle::stdb_connection::{
    StdbIntent, update_intent_on_stdbconnect, update_intent_on_stdbdisconnect,
};
use crate::reconnect::{
    reset_reconnectstate_on_stdbdisconnected, should_tick_reconnectstate, tick_reconnectstate,
};
use crate::row_channel::RowChannel;

pub use crate::backoff::{Backoff, Jitter};
pub use crate::lifecycle::connector::Connector;
pub use crate::lifecycle::stdb_connection::{StdbConn, StdbConnected, StdbConnection, StdbStatus};
pub use crate::reconnect::{ReconnectAction, ReconnectPolicy, ReconnectState};
pub use crate::row_channel::{RowDeleted, RowInserted, RowUpdated};

mod backoff;
mod lifecycle;
mod reconnect;
mod row_channel;

/// Wires a SpacetimeDB connection into a Bevy `App`:
#[derive(Clone, Copy, Default)]
pub struct StdbPlugin<Cn: Connector> {
    connector: Cn,
}

impl<Cn: Clone + Connector> bevy::app::Plugin for StdbPlugin<Cn> {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(StdbIntent::Disconnected);
        app.insert_resource(StdbStatus::Disconnected);
        app.insert_resource(LifecycleChannel::<Cn::Conn>::new());
        app.insert_resource(self.connector.clone());
        app.init_resource::<ReconnectPolicy>();
        app.init_resource::<ReconnectState>();

        app.add_observer(update_intent_on_stdbconnect);
        app.add_observer(update_intent_on_stdbdisconnect);
        app.add_observer(connect_on_stdbconnect::<Cn>);
        app.add_observer(disconnect_on_stdbdisconnect::<Cn>);
        app.add_observer(reset_reconnectstate_on_stdbdisconnected);

        app.add_systems(
            bevy::app::Update,
            (
                drain_lifecycle_sink::<Cn::Conn>,
                tick_reconnectstate::<Cn>.run_if(should_tick_reconnectstate),
            ),
        );
    }
}

fn install_table_events<T: 'static + Send + Sync>(app: &mut bevy::app::App) {
    app.insert_resource(RowChannel::<T>::new());
    app.add_message::<RowInserted<T>>();
    app.add_message::<RowUpdated<T>>();
    app.add_message::<RowDeleted<T>>();
    app.add_systems(bevy::app::Update, row_channel::drain_row_sink::<T>);
}

#[cfg(test)]
pub(crate) mod test_support {
    use bevy::ecs::resource::Resource;

    use crate::{Connector, StdbConnection, lifecycle::lifecycle_channel::LifecycleSink};

    /// Stand-in for a real `DbConnection`. The engine only requires `Send + Sync + 'static`.
    #[derive(Clone, Default)]
    pub(crate) struct FakeConn;

    /// A connector whose I/O is synchronous and in-memory, so the connection layer can be tested
    /// through `StdbPlugin` with no socket.
    #[derive(Resource, Clone, Default)]
    pub(crate) struct FakeConnector;

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

    /// Build a test `App` with the bridge installed for `connector`, plus a `Time` resource — the
    /// reconnect system needs `Time`, which production supplies via the Game's `TimePlugin`.
    pub(crate) fn test_app<Cn: Connector + Clone>(connector: Cn) -> bevy::app::App {
        let mut app = bevy::app::App::new();
        app.add_plugins(crate::StdbPlugin { connector });
        app.insert_resource(bevy::time::Time::<()>::default());
        app
    }
}

#[cfg(test)]
mod tests {
    use crate::lifecycle::stdb_connection::{
        ConnectionError, StdbConnect, StdbConnected, StdbConnection, StdbConnectionError,
        StdbDisconnect, StdbDisconnected, StdbIntent, stdb_connected,
    };

    use super::*;

    use bevy::prelude::*;

    use crate::test_support::{FakeConn, FakeConnector, test_app};

    /// Set by an observer so the test can assert `StdbConnected` actually fired.
    #[derive(Resource, Default)]
    struct ObserverFired(bool);

    /// Set by an observer so the test can assert `StdbDisconnected` actually fired.
    #[derive(Resource, Default)]
    struct DisconnectFired(bool);

    /// Captures the error message an observer received, so the test can assert it round-trips.
    #[derive(Resource, Default)]
    struct ConnectErrorCaptured(Option<ConnectionError>);

    #[test]
    fn connected_signal_triggers_observer_status_and_resource() {
        let mut app = test_app(FakeConnector);

        app.init_resource::<ObserverFired>();
        app.add_observer(|_on: On<StdbConnected>, mut fired: ResMut<ObserverFired>| fired.0 = true);

        // Before any signal: disconnected, no connection resource yet.
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_none()
        );

        // Push a `Connected` signal through the same seam the SDK adapter uses in production.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();

        app.update();

        assert!(
            app.world().resource::<ObserverFired>().0,
            "the StdbConnected observer should fire on a Connected signal",
        );
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some(),
            "StdbConnection<C> should be inserted on connect",
        );
    }

    #[test]
    fn disconnected_signal_triggers_observer_status_and_removes_resource() {
        let mut app = test_app(FakeConnector);

        app.init_resource::<DisconnectFired>();
        app.add_observer(
            |_on: On<StdbDisconnected>, mut fired: ResMut<DisconnectFired>| fired.0 = true,
        );

        // Connect first, so there is a live connection to drop.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        app.update();
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some()
        );

        // Now drop the connection.
        sink.disconnected().unwrap();
        app.update();

        assert!(
            app.world().resource::<DisconnectFired>().0,
            "the StdbDisconnected observer should fire on a Disconnected signal",
        );
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_none(),
            "StdbConnection<C> should be removed on disconnect",
        );
    }

    #[test]
    fn connect_error_signal_triggers_observer_with_message_and_status() {
        let mut app = test_app(FakeConnector);

        app.init_resource::<ConnectErrorCaptured>();
        app.add_observer(
            |on: On<StdbConnectionError>, mut captured: ResMut<ConnectErrorCaptured>| {
                captured.0 = Some(on.event().error());
            },
        );

        // Push a connect-error signal through the same seam the SDK adapter uses in production.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connection_error(ConnectionError::ConnectionRefused)
            .unwrap();

        app.update();

        assert_eq!(
            app.world().resource::<ConnectErrorCaptured>().0,
            Some(ConnectionError::ConnectionRefused),
            "the StdbConnectionError observer should fire carrying the error message",
        );
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
    }

    /// Counts how many times a `stdb_connected`-gated system runs.
    #[derive(Resource, Default)]
    struct RunCount(u32);

    fn count_up(mut count: ResMut<RunCount>) {
        count.0 += 1;
    }

    #[test]
    fn stdb_connected_run_condition_gates_systems() {
        let mut app = test_app(FakeConnector);
        app.init_resource::<RunCount>();
        app.add_systems(Update, count_up.run_if(stdb_connected::<FakeConn>));

        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();

        // Disconnected: the gated system never runs.
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<RunCount>().0,
            0,
            "gated system must not run before a connection exists",
        );

        // Connected: it runs. (Two frames: one to drain+insert the resource, one to run.)
        sink.connected(FakeConn).unwrap();
        app.update();
        app.update();
        let while_connected = app.world().resource::<RunCount>().0;
        assert!(while_connected > 0, "gated system must run while connected");

        // Disconnected again: it stops.
        sink.disconnected().unwrap();
        app.update();
        let after_disconnect = app.world().resource::<RunCount>().0;
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<RunCount>().0,
            after_disconnect,
            "gated system must stop running after disconnect",
        );
    }

    /// A stand-in row type. A real Module row is `Clone + Send + Sync + 'static`.
    #[derive(Clone, PartialEq, Debug)]
    struct Foo {
        id: u32,
    }

    /// Collects `RowInserted<Foo>` through the public `MessageReader`, like a Game system would.
    #[derive(Resource, Default)]
    struct CapturedInserts(Vec<Foo>);

    fn capture_inserts(
        mut reader: MessageReader<RowInserted<Foo>>,
        mut captured: ResMut<CapturedInserts>,
    ) {
        for msg in reader.read() {
            captured.0.push(msg.0.clone());
        }
    }

    #[test]
    fn insert_event_becomes_row_inserted_message() {
        let mut app = App::new();
        install_table_events::<Foo>(&mut app);
        app.init_resource::<CapturedInserts>();
        app.add_systems(Update, capture_inserts);

        // Push a row insert through the same seam the SDK on_insert callback uses in production.
        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.insert(Foo { id: 7 }).unwrap();

        // One frame to drain the channel into a message, one for the reader to observe it.
        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedInserts>().0;
        assert_eq!(
            captured.len(),
            1,
            "one queued insert should produce exactly one RowInserted message",
        );
        assert_eq!(captured[0], Foo { id: 7 });
    }

    /// Collects `RowUpdated<Foo>` (old, new) through the public `MessageReader`.
    #[derive(Resource, Default)]
    struct CapturedUpdates(Vec<(Foo, Foo)>);

    fn capture_updates(
        mut reader: MessageReader<RowUpdated<Foo>>,
        mut captured: ResMut<CapturedUpdates>,
    ) {
        for msg in reader.read() {
            captured.0.push((msg.old.clone(), msg.new.clone()));
        }
    }

    #[test]
    fn update_event_becomes_row_updated_message() {
        let mut app = App::new();
        install_table_events::<Foo>(&mut app);
        app.init_resource::<CapturedUpdates>();
        app.add_systems(Update, capture_updates);

        // Push a row update (previous + new) through the same seam the SDK on_update callback uses.
        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.update(Foo { id: 1 }, Foo { id: 2 }).unwrap();

        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedUpdates>().0;
        assert_eq!(
            captured.len(),
            1,
            "one queued update should produce exactly one RowUpdated message",
        );
        assert_eq!(captured[0], (Foo { id: 1 }, Foo { id: 2 }));
    }

    /// Collects `RowDeleted<Foo>` through the public `MessageReader`.
    #[derive(Resource, Default)]
    struct CapturedDeletes(Vec<Foo>);

    fn capture_deletes(
        mut reader: MessageReader<RowDeleted<Foo>>,
        mut captured: ResMut<CapturedDeletes>,
    ) {
        for msg in reader.read() {
            captured.0.push(msg.0.clone());
        }
    }

    #[test]
    fn delete_event_becomes_row_deleted_message() {
        let mut app = App::new();
        install_table_events::<Foo>(&mut app);
        app.init_resource::<CapturedDeletes>();
        app.add_systems(Update, capture_deletes);

        // Push a row delete through the same seam the SDK on_delete callback uses in production.
        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.delete(Foo { id: 9 }).unwrap();

        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedDeletes>().0;
        assert_eq!(
            captured.len(),
            1,
            "one queued delete should produce exactly one RowDeleted message",
        );
        assert_eq!(captured[0], Foo { id: 9 });
    }

    #[test]
    fn bulk_inserts_preserve_count_and_order() {
        let mut app = App::new();
        install_table_events::<Foo>(&mut app);
        app.init_resource::<CapturedInserts>();
        app.add_systems(Update, capture_inserts);

        // Queue several inserts before a single update — the initial-subscription dump.
        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.insert(Foo { id: 1 }).unwrap();
        sink.insert(Foo { id: 2 }).unwrap();
        sink.insert(Foo { id: 3 }).unwrap();

        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedInserts>().0;
        assert_eq!(
            *captured,
            vec![Foo { id: 1 }, Foo { id: 2 }, Foo { id: 3 }],
            "all queued inserts should surface as messages, in send order",
        );
    }

    #[test]
    fn engine_starts_disconnected_with_disconnected_intent() {
        let mut app = test_app(FakeConnector);

        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
        assert_eq!(
            *app.world().resource::<StdbIntent>(),
            StdbIntent::Disconnected
        );
    }

    #[test]
    fn stdb_connect_request_sets_intent_connected() {
        let mut app = test_app(FakeConnector);

        app.world_mut().trigger(StdbConnect);
        app.update();

        assert_eq!(*app.world().resource::<StdbIntent>(), StdbIntent::Connected);
    }

    #[test]
    fn stdb_disconnect_request_sets_intent_disconnected() {
        let mut app = test_app(FakeConnector);

        // Reach Connected intent first, so the flip back is observable.
        app.world_mut().trigger(StdbConnect);
        app.update();
        assert_eq!(*app.world().resource::<StdbIntent>(), StdbIntent::Connected);

        app.world_mut().trigger(StdbDisconnect);
        app.update();

        assert_eq!(
            *app.world().resource::<StdbIntent>(),
            StdbIntent::Disconnected
        );
    }
}
