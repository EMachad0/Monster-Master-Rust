//! Minimal, module-agnostic bridge between a SpacetimeDB connection and Bevy.
//!
//! This crate knows nothing about any specific Module.

use crate::lifecycle::lifecycle_channel::LifecycleChannel;
use crate::row_channel::{RowChannel, RowDeleted, RowUpdated};

pub use crate::lifecycle::stdb_connection::{StdbConn, StdbConnected, StdbConnection, StdbStatus};
pub use crate::row_channel::RowInserted;

mod lifecycle;
mod row_channel;

/// Wires a SpacetimeDB connection into a Bevy `App`:
#[derive(Clone, Copy, Default)]
pub struct StdbPlugin<C: StdbConn> {
    mark: std::marker::PhantomData<C>,
}

impl<C: StdbConn> bevy::app::Plugin for StdbPlugin<C> {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(StdbStatus::Connecting);
        app.insert_resource(LifecycleChannel::<C>::new());
        app.add_systems(
            bevy::app::Update,
            lifecycle::lifecycle_channel::drain_lifecycle_sink::<C>,
        );
    }
}

fn register_table_events<T: 'static + Send + Sync>(app: &mut bevy::app::App) {
    app.insert_resource(RowChannel::<T>::new());
    app.add_message::<RowInserted<T>>();
    app.add_message::<RowUpdated<T>>();
    app.add_message::<RowDeleted<T>>();
    app.add_systems(bevy::app::Update, row_channel::drain_row_sink::<T>);
}

#[cfg(test)]
mod tests {
    use crate::lifecycle::stdb_connection::{
        stdb_connected, ConnectionError, StdbConnected, StdbConnection, StdbConnectionError,
        StdbDisconnected,
    };

    use super::*;

    use bevy::prelude::*;

    /// Stand-in for a real `DbConnection`. The lifecycle engine only requires `Send + Sync + 'static`.
    #[derive(Clone, Default)]
    struct FakeConn;

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
        let mut app = App::new();
        app.add_plugins(StdbPlugin::<FakeConn>::default());

        app.init_resource::<ObserverFired>();
        app.add_observer(|_on: On<StdbConnected>, mut fired: ResMut<ObserverFired>| fired.0 = true);

        // Before any signal: connecting, no connection resource yet.
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Connecting
        );
        assert!(app
            .world()
            .get_resource::<StdbConnection<FakeConn>>()
            .is_none());

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
        let mut app = App::new();
        app.add_plugins(StdbPlugin::<FakeConn>::default());

        app.init_resource::<DisconnectFired>();
        app.add_observer(
            |_on: On<StdbDisconnected>, mut fired: ResMut<DisconnectFired>| fired.0 = true,
        );

        // Connect first, so there is a live connection to drop.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        app.update();
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        assert!(app
            .world()
            .get_resource::<StdbConnection<FakeConn>>()
            .is_some());

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
        let mut app = App::new();
        app.add_plugins(StdbPlugin::<FakeConn>::default());

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
        let mut app = App::new();
        app.add_plugins(StdbPlugin::<FakeConn>::default());
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
        register_table_events::<Foo>(&mut app);
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
        register_table_events::<Foo>(&mut app);
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
        register_table_events::<Foo>(&mut app);
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
}
