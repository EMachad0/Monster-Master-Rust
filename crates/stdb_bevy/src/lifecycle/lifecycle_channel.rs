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
        }
    }
}
