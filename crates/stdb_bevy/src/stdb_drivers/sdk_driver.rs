use std::fmt::Debug;

use bevy::ecs::{entity::Entity, resource::Resource};
use spacetimedb_sdk::{
    DbConnectionBuilder, DbContext, Result as SdkResult, SubscriptionHandle as SdkHandle,
};

use crate::{
    StdbBevyError, StdbConn, StdbConnectionDriver, StdbSubscriptionDriver, StdbToken,
    SubscriptionHandle,
};

pub use spacetimedb_sdk::__codegen::{
    DbConnection as SdkDbConnection, SpacetimeModule as SdkSpacetimeModule,
    SubscriptionBuilder as SdkSubscriptionBuilder,
};

#[derive(Resource)]
pub struct SdkDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
{
    pub uri: http::Uri,
    pub database_name: String,
    pub tick: fn(&C) -> spacetimedb_sdk::Result<()>,
    pub token: StdbToken,
}

impl<M, C> SdkDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
{
    pub fn new<U>(
        uri: U,
        database_name: impl Into<String>,
        tick: fn(&C) -> spacetimedb_sdk::Result<()>,
    ) -> Self
    where
        U: TryInto<http::Uri>,
        U::Error: Debug,
    {
        Self {
            uri: uri.try_into().expect("unable to parse into uri"),
            database_name: database_name.into(),
            tick,
            token: StdbToken::default(),
        }
    }

    pub fn with_token(self, token: impl Into<String>) -> Self {
        self.token.set(token);
        self
    }
}

impl<M, C> StdbConnectionDriver for SdkDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M> + DbContext + StdbConn,
{
    type Conn = C;

    fn connect(&self, sink: crate::LifecycleSink<Self::Conn>) {
        sink.connecting()
            .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));

        let builder = DbConnectionBuilder::<C::Module>::new()
            .with_uri(self.uri.clone())
            .with_database_name(self.database_name.clone())
            .with_token(self.token.get())
            .on_connect({
                let stdb_token = self.token.clone();
                move |_connection, identity, token| {
                    bevy::log::info!("Connected to SpacetimeDb {}", identity);
                    stdb_token.set(token);
                }
            })
            .on_disconnect({
                let sink = sink.clone();
                move |_error_ctx, error| {
                    if let Some(err) = error {
                        bevy::log::error!("Disconnection Error {}", err);
                    }
                    sink.disconnected()
                        .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));
                }
            })
            .on_connect_error({
                let sink = sink.clone();
                move |_error_ctx, error| {
                    bevy::log::error!("Connection Error {}", error);
                    sink.connection_error(StdbBevyError::from(error))
                        .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));
                }
            });

        #[cfg(not(target_arch = "wasm32"))]
        match builder.build() {
            Ok(conn) => {
                sink.connected(conn)
                    .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));
            }
            Err(err) => {
                bevy::log::error!("SpacetimeDB build failed: {err}");
                sink.connection_error(StdbBevyError::from(err))
                    .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));
            }
        }

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            match builder.build().await {
                Ok(conn) => {
                    sink.connected(conn)
                        .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));
                }
                Err(err) => {
                    bevy::log::error!("SpacetimeDB build failed: {err}");
                    sink.connection_error(StdbBevyError::from(err))
                        .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));
                }
            }
        });
    }

    fn disconnect(
        &self,
        conn: &crate::StdbConnection<Self::Conn>,
        sink: crate::LifecycleSink<Self::Conn>,
    ) {
        conn.disconnect()
            .unwrap_or_else(|err| bevy::log::error!("Disconnection Error {}", err));
        sink.disconnected()
            .unwrap_or_else(|err| bevy::log::error!("ChannelError: {}", err));
    }

    fn tick(&self, conn: &crate::StdbConnection<Self::Conn>) {
        (self.tick)(conn).unwrap_or_else(|err| bevy::log::error!("TickError: {}", err));
    }
}

pub struct SdkSubscriptionHandle {
    disconnect: Box<dyn Fn() -> SdkResult<()> + Sync + Send>,
}

impl SubscriptionHandle for SdkSubscriptionHandle {
    fn unsubscribe(&self) -> Result<(), StdbBevyError> {
        (self.disconnect)().map_err(StdbBevyError::from)
    }
}

impl<M, C> StdbSubscriptionDriver for SdkDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SdkSubscriptionBuilder<M>>
        + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    type Conn = C;
    type Handle = SdkSubscriptionHandle;

    fn subscribe(
        &self,
        conn: &crate::StdbConnection<Self::Conn>,
        entity: Entity,
        subscription: &crate::subscription::subscription_components::Subscription,
        sink: crate::SubscriptionSink,
    ) -> Self::Handle {
        let sdk_handle = conn
            .subscription_builder()
            .on_applied({
                bevy::log::info!("Subscription applied {:?}", subscription.queries());
                let sink = sink.clone();
                move |_ctx| sink.applied(entity)
            })
            .on_error({
                let sink = sink.clone();
                move |_ctx, err| {
                    bevy::log::error!("SpacetimeDB build failed: {err}");
                    sink.error(entity, StdbBevyError::from(err));
                }
            })
            .subscribe(subscription.queries());

        SdkSubscriptionHandle {
            disconnect: Box::new(move || sdk_handle.clone().unsubscribe()),
        }
    }
}

impl<C, M> Clone for SdkDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
{
    fn clone(&self) -> Self {
        Self {
            uri: self.uri.clone(),
            database_name: self.database_name.clone(),
            tick: self.tick,
            token: self.token.clone(),
        }
    }
}
