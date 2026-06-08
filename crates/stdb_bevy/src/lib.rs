//! Minimal, module-agnostic bridge between a SpacetimeDB connection and Bevy.
//!
//! This crate knows nothing about any specific Module.

use bevy::ecs::schedule::IntoScheduleConfigs;

use crate::connection::stdb_intent::{
    StdbIntent, update_intent_on_stdbconnect, update_intent_on_stdbdisconnect,
};
use crate::connection_driver::stdb_connection_driver::{
    connect_on_stdbconnect, disconnect_on_stdbdisconnect, tick_stdbconnectiondriver,
};
use crate::lifecycle::lifecycle_channel::{LifecycleChannel, drain_lifecycle_sink};
use crate::lifecycle::reconnect::{
    reset_reconnectstate_on_stdbdisconnected, should_tick_reconnectstate, tick_reconnectstate,
};
use crate::row::row_channel::{RowChannel, RowSink, drain_row_sink};

pub use crate::connection::connection_events::{StdbConnect, StdbDisconnect};
pub use crate::connection::stdb_connection::{StdbConn, StdbConnection};
pub use crate::connection::stdb_status::{StdbStatus, stdb_connected as is_stdb_connected};
pub use crate::connection_driver::{
    sdk_connection_driver::SdkConnectionDriver, stdb_connection_driver::StdbConnectionDriver,
};
pub use crate::lifecycle::lifecycle_channel::LifecycleSink;
pub use crate::lifecycle::lifecycle_events::{
    StdbConnected, StdbConnectionError, StdbDisconnected,
};
pub use crate::lifecycle::reconnect::{ReconnectAction, ReconnectPolicy, ReconnectState};
pub use crate::row::row_messages::{RowDeleted, RowInserted, RowUpdated};
pub use crate::utils::backoff::{Backoff, Jitter};

mod connection;
mod connection_driver;
mod lifecycle;
mod row;
mod utils;

/// Wires a SpacetimeDB connection into a Bevy `App`:
#[derive(Clone, Copy, Default)]
pub struct StdbPlugin<Cd: StdbConnectionDriver> {
    driver: Cd,
    connect_on_startup: bool,
}

impl<Cd: StdbConnectionDriver> StdbPlugin<Cd> {
    pub fn new(driver: Cd) -> Self {
        Self {
            driver,
            connect_on_startup: false,
        }
    }

    pub fn with_connect_on_startup(mut self) -> Self {
        self.connect_on_startup = true;
        self
    }
}

impl<Cd: StdbConnectionDriver> bevy::app::Plugin for StdbPlugin<Cd> {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(StdbIntent::Disconnected);
        app.insert_resource(StdbStatus::Disconnected);
        app.insert_resource(LifecycleChannel::<Cd::Conn>::new());
        app.insert_resource(self.driver.clone());
        app.init_resource::<ReconnectPolicy>();
        app.init_resource::<ReconnectState>();

        app.add_observer(update_intent_on_stdbconnect);
        app.add_observer(update_intent_on_stdbdisconnect);
        app.add_observer(connect_on_stdbconnect::<Cd>);
        app.add_observer(disconnect_on_stdbdisconnect::<Cd>);
        app.add_observer(reset_reconnectstate_on_stdbdisconnected);

        app.add_systems(
            bevy::app::Update,
            (
                drain_lifecycle_sink::<Cd::Conn>,
                tick_stdbconnectiondriver::<Cd>.run_if(is_stdb_connected),
                tick_reconnectstate::<Cd>.run_if(should_tick_reconnectstate),
            ),
        );

        if self.connect_on_startup {
            app.add_systems(
                bevy::app::Startup,
                connection_driver::stdb_connection_driver::connect::<Cd>,
            );
        }
    }
}

fn add_stdb_table<C, T>(app: &mut bevy::app::App, register: fn(&StdbConnection<C>, RowSink<T>))
where
    C: StdbConn,
    T: 'static + Send + Sync,
{
    app.insert_resource(RowChannel::<T>::new());
    app.add_message::<RowInserted<T>>();
    app.add_message::<RowUpdated<T>>();
    app.add_message::<RowDeleted<T>>();

    app.add_observer(
        move |_: bevy::ecs::observer::On<StdbConnected>,
              connection: bevy::ecs::system::Res<StdbConnection<C>>,
              row_channel: bevy::ecs::system::Res<RowChannel<T>>| {
            let sink = row_channel.sink();
            (register)(&connection, sink);
        },
    );

    app.add_systems(bevy::app::Update, drain_row_sink::<T>);
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    use bevy::ecs::resource::Resource;

    /// Stand-in for a real `DbConnection`. The engine only requires `Send + Sync + 'static`.
    #[derive(Clone, Default)]
    pub(crate) struct FakeConn;

    /// A connection driver whose I/O is synchronous and in-memory, so the connection layer can be tested
    /// through `StdbPlugin` with no socket.
    #[derive(Resource, Clone, Default)]
    pub(crate) struct FakeConnectionDriver;

    impl StdbConnectionDriver for FakeConnectionDriver {
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

    /// Build a test `App` with the bridge installed for `StdbConnectionDriver`, plus a `Time` resource — the
    /// reconnect system needs `Time`, which production supplies via the Game's `TimePlugin`.
    pub(crate) fn test_app<Cd: StdbConnectionDriver + Clone>(driver: Cd) -> bevy::app::App {
        let mut app = bevy::app::App::new();
        app.add_plugins(crate::StdbPlugin::new(driver));
        app.insert_resource(bevy::time::Time::<()>::default());
        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bevy::prelude::*;

    use crate::{
        lifecycle::lifecycle_events::{
            ConnectionError, StdbConnected, StdbConnectionError, StdbDisconnected,
        },
        test_support::{FakeConn, FakeConnectionDriver, test_app},
    };

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
        let mut app = test_app(FakeConnectionDriver);

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
        let mut app = test_app(FakeConnectionDriver);

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
        let mut app = test_app(FakeConnectionDriver);

        app.init_resource::<ConnectErrorCaptured>();
        app.add_observer(
            |on: On<StdbConnectionError>, mut captured: ResMut<ConnectErrorCaptured>| {
                captured.0 = Some(on.event().error().clone());
            },
        );

        // Push a connect-error signal through the same seam the SDK adapter uses in production.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connection_error(ConnectionError::ConnectionRefused)
            .unwrap();

        app.update();

        let error = app.world().resource::<ConnectErrorCaptured>().0.clone();
        assert!(error.is_some());
        assert_eq!(
            format!("{}", error.unwrap()),
            "Connection Refused",
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
        let mut app = test_app(FakeConnectionDriver);
        app.init_resource::<RunCount>();
        app.add_systems(Update, count_up.run_if(is_stdb_connected));

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
        add_stdb_table::<FakeConn, Foo>(&mut app, |_, _| {});
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
        add_stdb_table::<FakeConn, Foo>(&mut app, |_, _| {});
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
        add_stdb_table::<FakeConn, Foo>(&mut app, |_, _| {});
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
        add_stdb_table::<FakeConn, Foo>(&mut app, |_, _| {});
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
        let app = test_app(FakeConnectionDriver);

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
        let mut app = test_app(FakeConnectionDriver);

        app.world_mut().trigger(StdbConnect);
        app.update();

        assert_eq!(*app.world().resource::<StdbIntent>(), StdbIntent::Connected);
    }

    #[test]
    fn stdb_disconnect_request_sets_intent_disconnected() {
        let mut app = test_app(FakeConnectionDriver);

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

    use crate::row::row_channel::RowSink;

    /// A stand-in row type. `add_stdb_table` needs only `Clone + Send + Sync + 'static`.
    #[derive(Clone, PartialEq, Debug)]
    struct Widget {
        id: u32,
    }

    /// Accumulates `RowInserted<Widget>` across frames, like a Game system would.
    #[derive(Resource, Default)]
    struct WidgetInserts(Vec<Widget>);

    fn capture_widget_inserts(
        mut reader: MessageReader<RowInserted<Widget>>,
        mut captured: ResMut<WidgetInserts>,
    ) {
        for msg in reader.read() {
            captured.0.push(msg.0.clone());
        }
    }

    /// The registrar `add_stdb_table` re-runs on every connect. Production registers the SDK's
    /// `on_insert`/`on_update`/`on_delete` here; the test instead pushes one canned row per call,
    /// so the number of `RowInserted<Widget>` messages equals how many times the registrar ran —
    /// and proves the `RowSink` it was handed is wired through to the message stream.
    fn register_widget(_conn: &StdbConnection<FakeConn>, sink: RowSink<Widget>) {
        sink.insert(Widget { id: 1 }).unwrap();
    }

    #[test]
    fn add_stdb_table_does_not_register_before_connect() {
        let mut app = test_app(FakeConnectionDriver);
        add_stdb_table::<FakeConn, Widget>(&mut app, register_widget);
        app.init_resource::<WidgetInserts>();
        app.add_systems(Update, capture_widget_inserts);

        // No connection yet: the registrar must not run.
        app.update();
        app.update();

        assert!(
            app.world().resource::<WidgetInserts>().0.is_empty(),
            "the registrar must not run while disconnected",
        );
    }

    #[test]
    fn add_stdb_table_registers_on_connect_and_wires_the_message_pipeline() {
        let mut app = test_app(FakeConnectionDriver);
        add_stdb_table::<FakeConn, Widget>(&mut app, register_widget);
        app.init_resource::<WidgetInserts>();
        app.add_systems(Update, capture_widget_inserts);

        app.world_mut().trigger(StdbConnect);
        app.update();
        app.update();
        app.update();

        assert_eq!(
            app.world().resource::<WidgetInserts>().0,
            vec![Widget { id: 1 }],
            "on connect the registrar runs once and its RowSink reaches RowInserted<Widget>",
        );
    }

    #[test]
    fn add_stdb_table_re_registers_on_every_reconnect() {
        use std::time::Duration;

        let mut app = test_app(FakeConnectionDriver);
        app.insert_resource(ReconnectPolicy {
            backoff: Backoff::Fixed(Duration::from_secs(1)),
            jitter: Jitter(0.0),
            max_retries: None,
        });
        add_stdb_table::<FakeConn, Widget>(&mut app, register_widget);
        app.init_resource::<WidgetInserts>();
        app.add_systems(Update, capture_widget_inserts);

        // First connect: registrar runs once.
        app.world_mut().trigger(StdbConnect);
        app.update();
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<WidgetInserts>().0.len(),
            1,
            "the registrar runs on the first connect",
        );

        // Unsolicited drop — intent stays Connected, so auto-reconnect is armed.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.disconnected().unwrap();
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );

        // Past the backoff the connection is rebuilt — the registrar MUST run again on the new
        // connection, or row messages silently stop after the first drop.
        app.world_mut()
            .resource_mut::<bevy::time::Time>()
            .advance_by(Duration::from_millis(1100));
        app.update();
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Connected,
            "should have reconnected once the backoff elapsed",
        );
        assert_eq!(
            app.world().resource::<WidgetInserts>().0.len(),
            2,
            "the registrar must re-register on the rebuilt connection",
        );
    }
}
