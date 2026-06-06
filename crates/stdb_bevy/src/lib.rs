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
    use crate::lifecycle::stdb_connection::{StdbConnected, StdbConnection};

    use super::*;

    use bevy::prelude::*;

    /// Stand-in for a real `DbConnection`. The lifecycle engine only requires `Send + Sync + 'static`.
    #[derive(Clone, Default)]
    struct FakeConn;

    /// Set by an observer so the test can assert `StdbConnected` actually fired.
    #[derive(Resource, Default)]
    struct ObserverFired(bool);

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
}
