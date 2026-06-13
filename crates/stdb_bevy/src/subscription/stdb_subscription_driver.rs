use bevy::ecs::{entity::Entity, resource::Resource};

use crate::{
    StdbConn, StdbConnection, Subscription, subscription::subscription_channel::SubscriptionSink,
};

pub trait StdbSubscriptionDriver: Clone + Resource {
    type Conn: StdbConn;

    fn subscribe(
        &self,
        conn: &StdbConnection<Self::Conn>,
        entity: Entity,
        subscription: &Subscription,
        sink: SubscriptionSink,
    );
}

pub struct NoSubscriptions;
