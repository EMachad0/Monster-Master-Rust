//! Minimal, module-agnostic bridge between a SpacetimeDB connection and Bevy.
//!
//! This crate knows nothing about any specific Module.

use std::fmt::Debug;

use bevy::prelude::*;

use crate::connection::stdb_connection_driver::{
    StdbConnectionDriver, connect_on_stdbconnect, disconnect_on_stdbdisconnect,
    tick_stdbconnectiondriver,
};
use crate::connection::stdb_intent::{
    StdbIntent, intends_to_be_connected, update_intent_on_stdbconnect,
    update_intent_on_stdbdisconnect,
};
use crate::lifecycle::lifecycle_channel::{LifecycleChannel, drain_lifecycle_sink};
use crate::lifecycle::reconnect::{
    reset_reconnectstate_on_stdbdisconnected, should_tick_reconnectstate, tick_reconnectstate,
};
use crate::reducer::reducer_channel::{ReducerOutcomeChannel, drain_reducer_outcomes};
use crate::row::row_messages_resync::drop_stdbpreviousconnection_after_resync;
use crate::subscription::subscription_channel::{SubscriptionChannel, drain_subscription_sink};
use crate::subscription::subscription_components::{
    reset_subscriptions_on_stdbdisconnected, subscribe_pending_subscriptions,
    unsubscribe_on_subscription_despawn,
};

pub use crate::component_sync::row_entity_mapping::RowEntities;
pub use crate::component_sync::stdb_sync::StdbSync;
pub use crate::component_sync::sync_app_ext::SyncAppExt;
pub use crate::connection::connection_events::{StdbConnect, StdbDisconnect};
pub use crate::connection::stdb_connection::{StdbConn, StdbConnection, StdbPreviousConnection};
pub use crate::connection::stdb_identity::StdbIdentity;
pub use crate::connection::stdb_status::{StdbStatus, is_stdb_connected};
pub use crate::connection::stdb_token::StdbToken;
pub use crate::error::{StdbBevyError, StdbBevyErrorEvent};
pub use crate::lifecycle::lifecycle_channel::LifecycleSink;
pub use crate::lifecycle::lifecycle_events::{StdbConnected, StdbDisconnected};
pub use crate::lifecycle::reconnect::{ReconnectAction, ReconnectPolicy, ReconnectState};
pub use crate::reducer::reducer_channel::ReducerOutcomeSink;
pub use crate::reducer::reducer_events::{ReducerCommitted, ReducerFailed};
pub use crate::row::row_channel::StdbRow;
pub use crate::row::row_forwarder::RowForwarder;
pub use crate::row::row_messages::{RowDeleted, RowInserted, RowMessagesMask, RowUpdated};
pub use crate::row::table_registration::TableRegistration;
pub use crate::sdk_impl::{
    sdk_builder::SdkBuilder, sdk_connection_driver::SdkConnectionDriver,
    sdk_subscription_driver::SdkSubscriptionDriver,
};
pub use crate::subscription::stdb_subscription_driver::{
    NoSubscriptions, StdbSubscriptionDriver, SubscriptionId,
};
pub use crate::subscription::subscription_channel::SubscriptionSink;
pub use crate::subscription::subscription_components::{
    AppliedSubscription, FailedSubscription, IssuedSubscription, Subscription,
    is_subscriptions_settled,
};
pub use crate::subscription::subscription_events::{
    SubscriptionApplied, SubscriptionFailed, SubscriptionUnsubscribed,
};
pub use crate::utils::backoff::{Backoff, Jitter};
pub use spacetimedb_sdk as __sdk;
pub use stdb_builder::{Drivers, StdbBuilder};

mod component_sync;
mod connection;
mod error;
mod lifecycle;
mod reducer;
mod row;
mod sdk_impl;
mod stdb_builder;
mod subscription;
mod utils;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum StdbSystemSet {
    LifecycleEvents,
    RowMessagesPush,
    Resync,
    Main,
}

/// Wires a SpacetimeDB connection into a Bevy `App`:
pub struct StdbPlugin<B, Cd, Sd>
where
    B: StdbBuilder<Cd = Cd, Sd = Sd>,
    Cd: StdbConnectionDriver,
    Sd: StdbSubscriptionDriver,
{
    builder: B,
    tables: Vec<TableRegistration<Cd::Conn>>,
    connect_on_startup: bool,
}

impl<B, Cd, Sd> StdbPlugin<B, Cd, Sd>
where
    B: StdbBuilder<Cd = Cd, Sd = Sd>,
    Cd: StdbConnectionDriver,
    Sd: StdbSubscriptionDriver,
{
    pub fn new(builder: B) -> Self {
        Self {
            builder,
            tables: Vec::new(),
            connect_on_startup: false,
        }
    }

    pub(crate) fn build(&self, app: &mut bevy::app::App) {
        self.build_lifecyle_app(app);
        self.build_reconnect_app(app);
        self.build_tables_app(app);
        self.build_reducer_app(app);
        self.build_subscription_app(app);
    }

    pub fn with_connect_on_startup(mut self) -> Self {
        self.connect_on_startup = true;
        self
    }

    pub fn add_tables<const N: usize>(
        mut self,
        registrators: [TableRegistration<Cd::Conn>; N],
    ) -> Self {
        self.tables.extend(registrators);
        self
    }

    fn build_lifecyle_app(&self, app: &mut bevy::app::App) {
        app.insert_resource(self.builder.build_cd());
        app.insert_resource(StdbIntent::Disconnected);
        app.insert_resource(StdbStatus::Disconnected);
        app.insert_resource(LifecycleChannel::<Cd::Conn>::new());

        app.add_observer(update_intent_on_stdbconnect);
        app.add_observer(update_intent_on_stdbdisconnect);
        app.add_observer(connect_on_stdbconnect::<Cd>);
        app.add_observer(
            disconnect_on_stdbdisconnect::<Cd>.run_if(resource_exists::<StdbConnection<Cd::Conn>>),
        );

        app.configure_sets(
            bevy::app::Update,
            (
                StdbSystemSet::LifecycleEvents,
                StdbSystemSet::RowMessagesPush,
                StdbSystemSet::Resync.run_if(
                    resource_exists::<StdbPreviousConnection<Cd::Conn>>
                        .and_then(is_stdb_connected)
                        .and_then(is_subscriptions_settled),
                ),
                StdbSystemSet::Main,
            )
                .chain(),
        );

        app.add_systems(
            bevy::app::Update,
            tick_stdbconnectiondriver::<Cd>
                .run_if(is_stdb_connected)
                .in_set(StdbSystemSet::Main),
        );

        app.add_systems(
            bevy::app::Update,
            drain_lifecycle_sink::<Cd::Conn>.in_set(StdbSystemSet::LifecycleEvents),
        );

        if self.connect_on_startup {
            app.add_systems(
                bevy::app::Startup,
                crate::connection::connection_events::trigger_connect,
            );
        }
    }

    fn build_reconnect_app(&self, app: &mut bevy::app::App) {
        app.init_resource::<ReconnectPolicy>();
        app.init_resource::<ReconnectState>();

        app.add_observer(reset_reconnectstate_on_stdbdisconnected.run_if(intends_to_be_connected));

        app.add_systems(
            bevy::app::Update,
            tick_reconnectstate::<Cd>
                .run_if(should_tick_reconnectstate)
                .in_set(StdbSystemSet::Main),
        );
        app.add_systems(
            bevy::app::Update,
            drop_stdbpreviousconnection_after_resync::<Cd::Conn>.in_set(StdbSystemSet::Resync),
        );
    }

    fn build_tables_app(&self, app: &mut bevy::app::App) {
        for registrator in self.tables.iter() {
            registrator.register(app);
        }
    }

    fn build_reducer_app(&self, app: &mut bevy::app::App) {
        let channel = ReducerOutcomeChannel::new();
        app.insert_resource(channel.sink());
        app.insert_resource(channel);
        app.add_systems(
            bevy::app::Update,
            drain_reducer_outcomes.in_set(StdbSystemSet::Main),
        );
    }

    fn build_subscription_app(&self, app: &mut bevy::app::App) {
        app.insert_resource(self.builder.build_sd());
        app.init_resource::<SubscriptionChannel>();

        app.add_observer(reset_subscriptions_on_stdbdisconnected::<Sd>);
        app.add_observer(unsubscribe_on_subscription_despawn::<Sd>);

        app.add_systems(
            bevy::app::Update,
            subscribe_pending_subscriptions::<Sd>
                .run_if(is_stdb_connected)
                .in_set(StdbSystemSet::Main),
        );

        app.add_systems(
            bevy::app::Update,
            drain_subscription_sink.in_set(StdbSystemSet::LifecycleEvents),
        );
    }
}

impl<M, C> StdbPlugin<SdkBuilder<M, C>, SdkConnectionDriver<M, C>, SdkSubscriptionDriver<M, C>>
where
    M: sdk_impl::SdkSpacetimeModule<DbConnection = C>,
    C: sdk_impl::SdkDbConnection<Module = M>
        + spacetimedb_sdk::DbContext<SubscriptionBuilder = sdk_impl::SdkSubscriptionBuilder<M>>
        + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    /// Wires the SDK connection and subscription drivers from a URI, database name, and per-frame
    /// tick, so a Game never names either SDK driver.
    pub fn sdk<U>(
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

impl<Cd> StdbPlugin<Drivers<Cd, NoSubscriptions<Cd::Conn>>, Cd, NoSubscriptions<Cd::Conn>>
where
    Cd: StdbConnectionDriver + Clone,
{
    /// Wires a connection driver with subscriptions off: the driver slot is filled by the no-op
    /// [`NoSubscriptions`], so connection behaviour runs without a real subscription driver.
    pub fn connection(conn_driver: Cd) -> Self {
        Self::new(Drivers::new(conn_driver, NoSubscriptions::default()))
    }
}

impl<B, Cd, Sd> bevy::app::Plugin for StdbPlugin<B, Cd, Sd>
where
    B: StdbBuilder<Cd = Cd, Sd = Sd>,
    Cd: StdbConnectionDriver,
    Sd: StdbSubscriptionDriver,
{
    fn build(&self, app: &mut bevy::app::App) {
        self.build(app);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
