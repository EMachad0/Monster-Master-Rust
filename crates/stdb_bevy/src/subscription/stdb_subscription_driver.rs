use bevy::ecs::{entity::Entity, resource::Resource};

use crate::{
    StdbBevyError, StdbConn, StdbConnection, Subscription,
    subscription::subscription_channel::SubscriptionSink,
};

pub trait SubscriptionHandle: Send + Sync {
    fn unsubscribe(&self) -> Result<(), StdbBevyError>;
}

pub trait StdbSubscriptionDriver: Clone + Resource {
    type Conn: StdbConn;
    type Handle: SubscriptionHandle;

    fn subscribe(
        &self,
        conn: &StdbConnection<Self::Conn>,
        entity: Entity,
        subscription: &Subscription,
        sink: SubscriptionSink,
    ) -> Self::Handle;
}

pub struct NoSubscriptions;
