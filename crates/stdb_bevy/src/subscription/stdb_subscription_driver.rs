use std::marker::PhantomData;

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

pub trait StdbSubscriptionDriver: Resource + Component<Mutability = Mutable> {
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

/// The subscription driver for a connection-only setup: it satisfies the driver slot but never
/// issues a subscription, so the plugin keeps a single uniform shape instead of a separate
/// subscription-free code path. A connection-only app spawns no `Subscription` entities, so
/// `subscribe` is never actually reached.
#[derive(Resource)]
pub struct NoSubscriptions<C: StdbConn> {
    _conn: PhantomData<C>,
}

impl<C: StdbConn> Default for NoSubscriptions<C> {
    fn default() -> Self {
        Self { _conn: PhantomData }
    }
}

impl<C: StdbConn> Clone for NoSubscriptions<C> {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl<C: StdbConn> StdbSubscriptionDriver for NoSubscriptions<C> {
    type Conn = C;

    fn subscribe(
        &mut self,
        _conn: &StdbConnection<Self::Conn>,
        _sink: SubscriptionSink,
        _subscription: &Subscription,
    ) -> SubscriptionId {
        SubscriptionId::new(0)
    }

    fn unsubscribe(&mut self, _sink: SubscriptionSink, _subscription_id: &SubscriptionId) {}

    fn clear(&mut self) {}
}
