use bevy::ecs::{
    entity::Entity,
    resource::Resource,
    system::{Commands, Res},
};

use crate::{
    StdbBevyError, SubscriptionApplied, SubscriptionFailed,
    subscription::subscription_components::{AppliedSubscription, FailedSubscription},
};

pub enum SubscriptionEvent {
    Applied(Entity),
    Error(Entity, StdbBevyError),
}

#[derive(Clone)]
pub struct SubscriptionSink {
    pub entity: Entity,
    pub sender: crossbeam_channel::Sender<SubscriptionEvent>,
}

impl SubscriptionSink {
    pub fn applied(&self) {
        self.sender
            .send(SubscriptionEvent::Applied(self.entity))
            .unwrap_or_else(|err| {
                bevy::log::error!("SubscriptionSink applied sender error {}", err)
            });
    }

    pub fn error(&self, error: StdbBevyError) {
        self.sender
            .send(SubscriptionEvent::Error(self.entity, error))
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

    pub fn sink(&self, entity: Entity) -> SubscriptionSink {
        SubscriptionSink {
            entity,
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
                if let Ok(mut commands) = commands.get_spawned_entity(entity) {
                    commands
                        .insert(AppliedSubscription)
                        .trigger(SubscriptionApplied::from);
                }
            }
            SubscriptionEvent::Error(entity, error) => {
                if let Ok(mut commands) = commands.get_spawned_entity(entity) {
                    commands
                        .insert(FailedSubscription)
                        .trigger(|entity| SubscriptionFailed::new(entity, error));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bevy::prelude::*;

    #[derive(Resource, Default)]
    struct AppliedTargets(Vec<Entity>);

    #[derive(Resource, Default)]
    struct FailedTargets(Vec<(Entity, String)>);

    /// A minimal app: the subscription channel + its drain + observers recording every
    /// `SubscriptionApplied` / `SubscriptionFailed` target. The drain is fed directly through the
    /// sink — the same seam the SDK `on_applied` / `on_error` callbacks use in production.
    fn drain_app() -> App {
        let mut app = App::new();
        app.insert_resource(SubscriptionChannel::new());
        app.add_systems(Update, drain_subscription_sink);
        app.init_resource::<AppliedTargets>();
        app.add_observer(
            |on: On<SubscriptionApplied>, mut targets: ResMut<AppliedTargets>| {
                targets.0.push(on.entity);
            },
        );
        app.init_resource::<FailedTargets>();
        app.add_observer(
            |on: On<SubscriptionFailed>, mut targets: ResMut<FailedTargets>| {
                targets.0.push((on.entity, on.error.to_string()));
            },
        );
        app
    }

    #[test]
    fn applied_marks_the_entity_and_fires_subscription_applied() {
        let mut app = drain_app();
        let entity = app.world_mut().spawn_empty().id();

        // Push an applied signal through the same seam the SDK on_applied callback uses.
        let sink = app.world().resource::<SubscriptionChannel>().sink(entity);
        sink.applied();

        app.update();

        assert!(
            app.world().get::<AppliedSubscription>(entity).is_some(),
            "an applied signal must mark the subscription entity AppliedSubscription",
        );
        assert_eq!(
            app.world().resource::<AppliedTargets>().0,
            vec![entity],
            "an applied signal must fire SubscriptionApplied targeting that entity",
        );
    }

    #[test]
    fn applied_for_a_despawned_entity_is_ignored() {
        let mut app = drain_app();
        let entity = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(entity).despawn();

        // The applied signal can land a frame or more after the Game despawned the subscription.
        let sink = app.world().resource::<SubscriptionChannel>().sink(entity);
        sink.applied();

        app.update(); // must not panic

        assert!(
            app.world().resource::<AppliedTargets>().0.is_empty(),
            "an applied signal for a despawned subscription must be ignored, not fired",
        );
    }

    #[test]
    fn error_marks_failed_and_fires_subscription_failed() {
        let mut app = drain_app();
        let entity = app.world_mut().spawn_empty().id();

        // Push an error signal through the same seam the SDK on_error callback uses.
        let sink = app.world().resource::<SubscriptionChannel>().sink(entity);
        sink.error(StdbBevyError::ConnectionRefused);

        app.update();

        assert!(
            app.world().get::<FailedSubscription>(entity).is_some(),
            "an error signal must mark the subscription entity FailedSubscription",
        );
        assert_eq!(
            app.world().resource::<FailedTargets>().0,
            vec![(entity, "Connection Refused".to_string())],
            "an error signal must fire SubscriptionFailed targeting that entity, carrying the cause",
        );
    }

    #[test]
    fn error_for_a_despawned_entity_is_ignored() {
        let mut app = drain_app();
        let entity = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(entity).despawn();

        // The error signal can land a frame or more after the Game despawned the subscription.
        let sink = app.world().resource::<SubscriptionChannel>().sink(entity);
        sink.error(StdbBevyError::ConnectionRefused);

        app.update(); // must not panic

        assert!(
            app.world().resource::<FailedTargets>().0.is_empty(),
            "an error signal for a despawned subscription must be ignored, not fired",
        );
    }
}
