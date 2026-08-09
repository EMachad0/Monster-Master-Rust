use std::fmt::Debug;

use bevy::ecs::resource::Resource;
use spacetimedb_sdk::{DbConnectionBuilder, DbContext};

use stdb_bevy::{StdbBevyError, StdbConn, StdbConnectionDriver, StdbIdentity, StdbToken};

use crate::{SdkDbConnection, SdkSpacetimeModule};

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
    C: SdkDbConnection<Module = M> + DbContext + StdbConn,
{
    type Conn = C;

    fn connect(&self, sink: stdb_bevy::LifecycleSink<Self::Conn>) {
        sink.connecting()
            .unwrap_or_else(|err| bevy::log::error!(%err, "lifecycle channel send failed"));

        let builder = DbConnectionBuilder::<C::Module>::new()
            .with_uri(self.uri.clone())
            .with_database_name(self.database_name.clone())
            .with_token(self.token.get())
            .on_connect({
                let sink = sink.clone();
                let stdb_token = self.token.clone();
                move |_connection, identity, token| {
                    sink.identified(StdbIdentity::new(identity.to_byte_array()))
                        .unwrap_or_else(
                            |err| bevy::log::error!(%err, "lifecycle channel send failed"),
                        );
                    // Logged from the SDK value, which displays hex where the Bridge's newtype
                    // would print raw bytes.
                    bevy::log::info!(%identity, "connected");
                    stdb_token.set(token);
                }
            })
            .on_disconnect({
                let sink = sink.clone();
                move |_error_ctx, error| {
                    // The drain logs the headline `unintended disconnect`; this is just the SDK's
                    // cause, kept at trace as drill-in detail (a drop is expected, not an error).
                    if let Some(err) = error {
                        bevy::log::trace!(%err, "sdk reported disconnect cause");
                    }
                    sink.disconnected().unwrap_or_else(
                        |err| bevy::log::error!(%err, "lifecycle channel send failed"),
                    );
                }
            })
            .on_connect_error({
                let sink = sink.clone();
                move |_error_ctx, error| {
                    // The drain logs `connect failed` (warn) once from the ConnectionError event;
                    // logging here too would double up at two levels.
                    sink.connection_error(StdbBevyError::driver(error))
                        .unwrap_or_else(
                            |err| bevy::log::error!(%err, "lifecycle channel send failed"),
                        );
                }
            });

        #[cfg(not(target_arch = "wasm32"))]
        match builder.build() {
            Ok(conn) => {
                sink.connected(conn)
                    .unwrap_or_else(|err| bevy::log::error!(%err, "lifecycle channel send failed"));
            }
            // The drain logs the failure once as `connect failed` (warn) off this event.
            Err(err) => {
                sink.connection_error(StdbBevyError::driver(err))
                    .unwrap_or_else(|err| bevy::log::error!(%err, "lifecycle channel send failed"));
            }
        }

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            match builder.build().await {
                Ok(conn) => {
                    sink.connected(conn).unwrap_or_else(
                        |err| bevy::log::error!(%err, "lifecycle channel send failed"),
                    );
                }
                // The drain logs the failure once as `connect failed` (warn) off this event.
                Err(err) => {
                    sink.connection_error(StdbBevyError::driver(err))
                        .unwrap_or_else(
                            |err| bevy::log::error!(%err, "lifecycle channel send failed"),
                        );
                }
            }
        });
    }

    fn disconnect(
        &self,
        conn: &stdb_bevy::StdbConnection<Self::Conn>,
        sink: stdb_bevy::LifecycleSink<Self::Conn>,
    ) {
        conn.disconnect()
            .unwrap_or_else(|err| bevy::log::warn!(%err, "disconnect call failed"));
        sink.disconnected()
            .unwrap_or_else(|err| bevy::log::error!(%err, "lifecycle channel send failed"));
    }

    fn tick(&self, conn: &stdb_bevy::StdbConnection<Self::Conn>) {
        (self.tick)(conn).unwrap_or_else(|err| bevy::log::error!(%err, "frame_tick failed"));
    }
}
