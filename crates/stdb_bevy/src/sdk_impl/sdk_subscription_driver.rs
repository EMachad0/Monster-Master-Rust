use bevy::{ecs::resource::Resource, platform::collections::HashMap};
use spacetimedb_sdk::{DbContext, SubscriptionHandle};

use crate::{StdbBevyError, StdbConn, StdbSubscriptionDriver, SubscriptionId};

use super::{SdkDbConnection, SdkSpacetimeModule, SdkSubscriptionBuilder};

#[derive(Debug, Resource)]
pub struct SdkSubscriptionDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
    M::SubscriptionHandle: Send + Sync,
{
    pub id_mint: u64,
    pub subscription_handles: HashMap<SubscriptionId, M::SubscriptionHandle>,
}

impl<M, C> SdkSubscriptionDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
    M::SubscriptionHandle: Send + Sync,
{
    pub fn new() -> Self {
        Self {
            id_mint: 0,
            subscription_handles: HashMap::default(),
        }
    }
}

impl<M, C> StdbSubscriptionDriver for SdkSubscriptionDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SdkSubscriptionBuilder<M>>
        + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    type Conn = C;

    fn subscribe(
        &mut self,
        conn: &crate::StdbConnection<Self::Conn>,
        sink: crate::SubscriptionSink,
        subscription: &crate::subscription::subscription_components::Subscription,
    ) -> SubscriptionId {
        let subscription_id = {
            let old_id = self.id_mint;
            self.id_mint = old_id + 1;
            SubscriptionId::new(old_id)
        };

        let sdk_handle = conn
            .subscription_builder()
            .on_applied({
                let sink = sink.clone();
                move |_ctx| {
                    bevy::log::info!("Subscription applied");
                    sink.applied()
                }
            })
            .on_error({
                let sink = sink.clone();
                move |_ctx, err| {
                    bevy::log::error!("SpacetimeDB subscription failed: {err}");
                    sink.error(StdbBevyError::from(err));
                }
            })
            .subscribe(subscription.queries());

        self.subscription_handles
            .insert(subscription_id, sdk_handle);
        subscription_id
    }

    fn unsubscribe(&mut self, sink: crate::SubscriptionSink, subscription_id: &SubscriptionId) {
        if let Some(handle) = self.subscription_handles.remove(subscription_id) {
            sink.unsubscribe();
            handle.unsubscribe().unwrap_or_else(|err| {
                bevy::log::error!("error while unsubscribing {}", err);
            });
        }
    }

    fn clear(&mut self) {
        self.subscription_handles.clear();
    }
}

impl<C, M> Default for SdkSubscriptionDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
    M::SubscriptionHandle: Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<C, M> Clone for SdkSubscriptionDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
    M::SubscriptionHandle: Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            id_mint: 0,
            subscription_handles: self.subscription_handles.clone(),
        }
    }
}
