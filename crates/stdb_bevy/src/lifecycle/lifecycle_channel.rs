use bevy::ecs::{
    resource::Resource,
    system::{Commands, Res},
};

use crate::{
    StdbBevyError, StdbBevyErrorEvent,
    connection::{
        stdb_connection::{StdbConn, StdbConnection},
        stdb_status::StdbStatus,
    },
    lifecycle::lifecycle_events::{StdbConnected, StdbDisconnected},
};

pub enum LifecycleEvent<C: StdbConn> {
    Connected(C),
    Disconnected,
    ConnectionError(StdbBevyError),
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
        error: StdbBevyError,
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
                commands.trigger(StdbBevyErrorEvent::new(e));
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

    use crate::test_support::{FakeConn, FakeDriver, test_app};

    use super::*;

    #[derive(Resource, Default)]
    struct ObserverFired(bool);

    #[derive(Resource, Default)]
    struct DisconnectFired(bool);

    #[test]
    fn connected_signal_triggers_observer_status_and_resource() {
        let mut app = test_app(FakeDriver::default());

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
        let mut app = test_app(FakeDriver::default());

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
}
