use bevy::ecs::component::{Component, Mutable};
use bevy::ecs::resource::Resource;

use crate::{
    StdbConn, StdbConnection, Subscription, subscription::subscription_channel::SubscriptionSink,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl From<u64> for SubscriptionId {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

pub trait StdbSubscriptionDriver: Clone + Resource + Component<Mutability = Mutable> {
    type Conn: StdbConn;

    fn subscribe(
        &mut self,
        conn: &StdbConnection<Self::Conn>,
        sink: SubscriptionSink,
        subscription: &Subscription,
    ) -> SubscriptionId;

    fn unsubscribe(&mut self, sink: SubscriptionSink, subscription_id: &SubscriptionId);

    fn clear(&mut self);
}

pub struct NoSubscriptions;
