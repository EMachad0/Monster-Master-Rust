use std::fmt::Debug;

use spacetimedb_sdk::DbContext;

use crate::sdk_impl::{SdkDbConnection, SdkSpacetimeModule, SdkSubscriptionBuilder};
use crate::{SdkBuilder, SdkConnectionDriver, SdkSubscriptionDriver, StdbConn, StdbPlugin};

/// Plugin constructor for the SDK drivers. An extension trait rather than an inherent constructor,
/// so the SDK types it names stay beside the drivers instead of on the plugin's own definition.
pub trait SdkPluginExt: Sized {
    /// The SDK connection the per-frame tick advances.
    type Conn;

    /// Wires the SDK connection and subscription drivers from a URI, database name, and per-frame
    /// tick, so a Game never names either SDK driver.
    fn sdk<U>(
        uri: U,
        database_name: impl Into<String>,
        tick: fn(&Self::Conn) -> spacetimedb_sdk::Result<()>,
    ) -> Self
    where
        U: TryInto<http::Uri>,
        U::Error: Debug;
}

impl<M, C> SdkPluginExt
    for StdbPlugin<SdkBuilder<M, C>, SdkConnectionDriver<M, C>, SdkSubscriptionDriver<M, C>>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SdkSubscriptionBuilder<M>>
        + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    type Conn = C;

    fn sdk<U>(
        uri: U,
        database_name: impl Into<String>,
        tick: fn(&C) -> spacetimedb_sdk::Result<()>,
    ) -> Self
    where
        U: TryInto<http::Uri>,
        U::Error: Debug,
    {
        Self::new(SdkBuilder::new(uri, database_name, tick))
    }
}
