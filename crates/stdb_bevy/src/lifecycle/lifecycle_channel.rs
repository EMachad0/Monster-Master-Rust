use bevy::ecs::{
    resource::Resource,
    system::{Commands, Res},
};

use crate::lifecycle::stdb_connection::{
    StdbConn, StdbConnected, StdbConnection, StdbDisconnected, StdbStatus,
};

pub(crate) enum Lifecycle<C: StdbConn> {
    Connected(C),
    Disconnected,
}

pub(crate) struct LifecycleSink<C: StdbConn> {
    pub sender: crossbeam_channel::Sender<Lifecycle<C>>,
}

impl<C: StdbConn> LifecycleSink<C> {
    pub fn connected(&self, c: C) -> Result<(), crossbeam_channel::SendError<Lifecycle<C>>> {
        self.sender.send(Lifecycle::Connected(c))
    }

    pub fn disconnected(&self) -> Result<(), crossbeam_channel::SendError<Lifecycle<C>>> {
        self.sender.send(Lifecycle::Disconnected)
    }
}

#[derive(Resource)]
pub(crate) struct LifecycleChannel<C: StdbConn> {
    sender: crossbeam_channel::Sender<Lifecycle<C>>,
    receiver: crossbeam_channel::Receiver<Lifecycle<C>>,
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
            Lifecycle::Connected(c) => {
                commands.insert_resource(StdbConnection(c));
                commands.insert_resource(StdbStatus::Connected);
                commands.trigger(StdbConnected);
            }
            Lifecycle::Disconnected => {
                commands.remove_resource::<StdbConnection<C>>();
                commands.insert_resource(StdbStatus::Disconnected);
                commands.trigger(StdbDisconnected);
            }
        }
    }
}
