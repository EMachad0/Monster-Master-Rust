use spacetimedb_sdk::DbContext;

use crate::{
    SdkConnectionDriver, SdkDbConnection, SdkSpacetimeModule, SdkSubscriptionBuilder,
    SdkSubscriptionDriver,
};
use stdb_bevy::{StdbBuilder, StdbConn};

pub struct SdkBuilder<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SdkSubscriptionBuilder<M>>
        + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    uri: http::Uri,
    database_name: String,
    tick: fn(&C) -> spacetimedb_sdk::Result<()>,
}

impl<M, C> SdkBuilder<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SdkSubscriptionBuilder<M>>
        + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    pub fn new<U>(
        uri: U,
        database_name: impl Into<String>,
        tick: fn(&C) -> spacetimedb_sdk::Result<()>,
    ) -> Self
    where
        U: TryInto<http::Uri>,
        U::Error: std::fmt::Debug,
    {
        Self {
            uri: uri.try_into().expect("unable to parse into uri"),
            database_name: database_name.into(),
            tick,
        }
    }
}

impl<M, C> StdbBuilder for SdkBuilder<M, C>
where
    M: SdkSpacetimeModule<DbConnection = C>,
    C: SdkDbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SdkSubscriptionBuilder<M>>
        + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    type Cd = SdkConnectionDriver<M, C>;

    type Sd = SdkSubscriptionDriver<M, C>;

    fn build_cd(&self) -> Self::Cd {
        Self::Cd::new(self.uri.clone(), self.database_name.clone(), self.tick)
    }

    fn build_sd(&self) -> Self::Sd {
        Self::Sd::new()
    }
}
