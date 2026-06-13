use bevy::ecs::{
    entity::Entity,
    resource::Resource,
    system::{Commands, Res},
};

use crate::{
    StdbBevyError, StdbError,
    subscription::subscription_components::{AppliedSubscription, FailedSubscription},
};

pub enum SubscriptionEvent {
    Applied(Entity),
    Error(Entity, StdbBevyError),
}

#[derive(Clone)]
pub struct SubscriptionSink {
    pub sender: crossbeam_channel::Sender<SubscriptionEvent>,
}

impl SubscriptionSink {
    pub fn applied(&self, entity: Entity) {
        self.sender
            .send(SubscriptionEvent::Applied(entity))
            .unwrap_or_else(|err| {
                bevy::log::error!("SubscriptionSink applied sender error {}", err)
            });
    }

    pub fn error(&self, entity: Entity, error: StdbBevyError) {
        self.sender
            .send(SubscriptionEvent::Error(entity, error))
            .unwrap_or_else(|err| {
                bevy::log::error!("SubscriptionSink applied sender error {}", err)
            });
    }
}

#[derive(Resource)]
pub(crate) struct SubscriptionChannel {
    sender: crossbeam_channel::Sender<SubscriptionEvent>,
    receiver: crossbeam_channel::Receiver<SubscriptionEvent>,
}

impl SubscriptionChannel {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }

    pub fn sink(&self) -> SubscriptionSink {
        SubscriptionSink {
            sender: self.sender.clone(),
        }
    }
}

impl Default for SubscriptionChannel {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn drain_subscription_sink(
    row_channel: Res<SubscriptionChannel>,
    mut commands: Commands,
) {
    while let Ok(stdb_event) = row_channel.receiver.try_recv() {
        match stdb_event {
            SubscriptionEvent::Applied(entity) => {
                commands.entity(entity).insert(AppliedSubscription);
            }
            SubscriptionEvent::Error(entity, error) => {
                commands.entity(entity).insert(FailedSubscription);
                commands.trigger(StdbError::new(error));
            }
        }
    }
}
