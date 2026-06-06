//! Minimal, module-agnostic bridge between a SpacetimeDB connection and Bevy.
//!
//! This crate knows nothing about any specific Module.

use crate::lifecycle::lifecycle_channel::LifecycleChannel;

pub use crate::lifecycle::stdb_connection::{StdbConn, StdbConnected, StdbConnection, StdbStatus};

mod lifecycle;

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

#[cfg(test)]
mod tests {
    use crate::lifecycle::stdb_connection::{
        ConnectionError, StdbConnected, StdbConnection, StdbConnectionError, StdbDisconnected,
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
}
