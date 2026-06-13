use bevy::prelude::*;

use crate::StdbBevyError;

#[derive(EntityEvent)]
pub struct SubscriptionApplied {
    pub entity: Entity,
}

impl SubscriptionApplied {
    pub fn new(entity: Entity) -> Self {
        Self { entity }
    }
}

impl From<Entity> for SubscriptionApplied {
    fn from(value: Entity) -> Self {
        Self::new(value)
    }
}

#[derive(EntityEvent)]
pub struct SubscriptionFailed {
    pub entity: Entity,
    pub error: StdbBevyError,
}

impl SubscriptionFailed {
    pub fn new(entity: Entity, error: StdbBevyError) -> Self {
        Self { entity, error }
    }
}
