use std::sync::Arc;

use bevy::ecs::event::Event;

#[derive(Debug, Clone, Event)]
pub struct StdbBevyErrorEvent(StdbBevyError);

impl StdbBevyErrorEvent {
    pub fn new(error: StdbBevyError) -> Self {
        Self(error)
    }

    pub fn error(&self) -> &StdbBevyError {
        &self.0
    }
}

/// A failure the Bridge surfaces to the Game.
///
/// The driver's cause stays type-erased rather than being enumerated into variants. The Bridge's
/// core names no driver type, and a driver reports most causes as message strings anyway, so
/// typing them finer would mean parsing those strings and re-deciding the mapping on every driver
/// upgrade. A consumer that needs the concrete cause downcasts the `Driver` payload; a semantic
/// variant is only worth adding once a Game needs the distinction and the failure mode has been
/// observed in practice.
#[derive(Debug, Clone, thiserror::Error)]
pub enum StdbBevyError {
    #[error("Connection Refused")]
    ConnectionRefused,

    #[error(transparent)]
    Driver(Arc<dyn std::error::Error + Send + Sync>),
}

impl StdbBevyError {
    /// Wraps a cause reported by the driver. `Arc` keeps the error `Clone`, which the Bridge's
    /// events require, without giving up the cause itself.
    pub fn driver(cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Driver(Arc::new(cause))
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::{observer::On, resource::Resource, system::ResMut};

    use crate::{
        StdbStatus,
        lifecycle::lifecycle_channel::LifecycleChannel,
        test_support::{FakeConn, FakeDriver, test_app},
    };

    use super::*;

    #[derive(Resource, Default)]
    struct ConnectErrorCaptured(Option<StdbBevyError>);

    #[test]
    fn connect_error_signal_triggers_observer_with_message_and_status() {
        let mut app = test_app(FakeDriver::default());

        app.init_resource::<ConnectErrorCaptured>();
        app.add_observer(
            |on: On<StdbBevyErrorEvent>, mut captured: ResMut<ConnectErrorCaptured>| {
                captured.0 = Some(on.event().error().clone());
            },
        );

        // Push a connect-error signal through the same seam the SDK adapter uses in production.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connection_error(StdbBevyError::ConnectionRefused)
            .unwrap();

        app.update();

        let error = app.world().resource::<ConnectErrorCaptured>().0.clone();
        assert!(error.is_some());
        assert_eq!(
            format!("{}", error.unwrap()),
            "Connection Refused",
            "the StdbBevyErrorEvent observer should fire carrying the error message",
        );
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected,
            "a connection error must leave the status Disconnected",
        );
    }

    #[test]
    fn driver_error_displays_the_underlying_cause_verbatim() {
        let error = StdbBevyError::driver(std::io::Error::other("host unreachable"));

        assert_eq!(
            format!("{error}"),
            "host unreachable",
            "the driver cause displays unwrapped, so the connect-failure log reads as the driver \
             reported it",
        );
    }
}
