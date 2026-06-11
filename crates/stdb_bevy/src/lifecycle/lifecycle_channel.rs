use bevy::ecs::{
    resource::Resource,
    system::{Commands, Res},
};

use crate::{
    connection::{
        stdb_connection::{StdbConn, StdbConnection},
        stdb_status::StdbStatus,
    },
    lifecycle::lifecycle_events::{
        ConnectionError, StdbConnected, StdbConnectionError, StdbDisconnected,
    },
};

pub enum LifecycleEvent<C: StdbConn> {
    Connected(C),
    Disconnected,
    ConnectionError(ConnectionError),
    Connecting,
}

pub struct LifecycleSink<C: StdbConn> {
    sender: crossbeam_channel::Sender<LifecycleEvent<C>>,
}

impl<C: StdbConn> LifecycleSink<C> {
    pub fn connected(&self, c: C) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::Connected(c))
    }

    pub fn disconnected(&self) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::Disconnected)
    }

    pub fn connection_error(
        &self,
        error: ConnectionError,
    ) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::ConnectionError(error))
    }

    pub fn connecting(&self) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::Connecting)
    }
}

impl<C: StdbConn> Clone for LifecycleSink<C> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

#[derive(Resource)]
pub(crate) struct LifecycleChannel<C: StdbConn> {
    sender: crossbeam_channel::Sender<LifecycleEvent<C>>,
    receiver: crossbeam_channel::Receiver<LifecycleEvent<C>>,
}

impl<C: StdbConn> LifecycleChannel<C> {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }

    pub fn sink(&self) -> LifecycleSink<C> {
        LifecycleSink {
            sender: self.sender.clone(),
        }
    }
}

impl<C: StdbConn> Default for LifecycleChannel<C> {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn drain_lifecycle_sink<C: StdbConn>(
    lifecycle_channel: Res<LifecycleChannel<C>>,
    mut commands: Commands,
) {
    while let Ok(stdb_event) = lifecycle_channel.receiver.try_recv() {
        match stdb_event {
            LifecycleEvent::Connected(c) => {
                commands.insert_resource(StdbConnection(c));
                commands.insert_resource(StdbStatus::Connected);
                commands.trigger(StdbConnected);
            }
            LifecycleEvent::Disconnected => {
                commands.remove_resource::<StdbConnection<C>>();
                commands.insert_resource(StdbStatus::Disconnected);
                commands.trigger(StdbDisconnected);
            }
            LifecycleEvent::ConnectionError(e) => {
                commands.remove_resource::<StdbConnection<C>>();
                commands.insert_resource(StdbStatus::Disconnected);
                commands.trigger(StdbConnectionError::new(e));
            }
            LifecycleEvent::Connecting => {
                commands.insert_resource(StdbStatus::Connecting);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use crate::lifecycle::lifecycle_channel::LifecycleChannel;
    use crate::lifecycle::lifecycle_events::ConnectionError;
    use crate::test_support::{FakeConn, FakeConnectionDriver, test_app};
    use crate::{StdbConnected, StdbConnection, StdbConnectionError, StdbDisconnected, StdbStatus};

    #[derive(Resource, Default)]
    struct ObserverFired(bool);

    #[derive(Resource, Default)]
    struct DisconnectFired(bool);

    #[derive(Resource, Default)]
    struct ConnectErrorCaptured(Option<ConnectionError>);

    #[test]
    fn connected_signal_triggers_observer_status_and_resource() {
        let mut app = test_app(FakeConnectionDriver::default());

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
        let mut app = test_app(FakeConnectionDriver::default());

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
        let mut app = test_app(FakeConnectionDriver::default());

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
}
