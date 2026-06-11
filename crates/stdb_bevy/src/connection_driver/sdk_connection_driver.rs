use std::fmt::Debug;

use bevy::ecs::resource::Resource;
use spacetimedb_sdk::{DbConnectionBuilder, DbContext};

use crate::{
    StdbConn, StdbConnectionDriver, StdbToken, lifecycle::lifecycle_events::ConnectionError,
};

pub use spacetimedb_sdk::__codegen::{
    DbConnection as SdkDbConnection, SpacetimeModule as SdkSpacetimeModule,
};

#[derive(Resource)]
pub struct SdkConnectionDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>,
{
    pub uri: http::Uri,
    pub database_name: String,
    pub tick: fn(&C) -> spacetimedb_sdk::Result<()>,
    pub token: StdbToken,
}

impl<M, C> SdkConnectionDriver<M, C>
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

impl<M, C> StdbConnectionDriver for SdkConnectionDriver<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M> + StdbConn + DbContext,
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
                    sink.connection_error(ConnectionError::from(error))
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
                sink.connection_error(ConnectionError::from(err))
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
                    sink.connection_error(ConnectionError::from(err))
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

impl<C, M> Clone for SdkConnectionDriver<M, C>
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
