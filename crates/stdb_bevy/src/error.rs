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

#[derive(Debug, Clone, thiserror::Error)]
pub enum StdbBevyError {
    #[error("Connection Refused")]
    ConnectionRefused,

    #[error(transparent)]
    SdkError(#[from] spacetimedb_sdk::Error),
}

#[cfg(test)]
mod tests {
    use bevy::ecs::{observer::On, resource::Resource, system::ResMut};

    use crate::{
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
            "the StdbConnectionError observer should fire carrying the error message",
        );
    }
}
