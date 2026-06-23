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
    StdbIntent, update_intent_on_stdbconnect, update_intent_on_stdbdisconnect,
};
use crate::lifecycle::lifecycle_channel::{LifecycleChannel, drain_lifecycle_sink};
use crate::lifecycle::reconnect::{
    reset_reconnectstate_on_stdbdisconnected, should_tick_reconnectstate, tick_reconnectstate,
};
use crate::row::row_messages_resync::drop_stdbpreviousconnection_after_resync;
use crate::subscription::subscription_channel::{SubscriptionChannel, drain_subscription_sink};
use crate::subscription::subscription_components::{
    reset_subscriptions_on_stdbdisconnected, subscribe_pending_subscriptions,
    unsubscribe_on_subscription_despawn,
};

pub use crate::connection::connection_events::{StdbConnect, StdbDisconnect};
pub use crate::connection::stdb_connection::{StdbConn, StdbConnection, StdbPreviousConnection};
pub use crate::connection::stdb_status::{StdbStatus, is_stdb_connected};
pub use crate::connection::stdb_token::StdbToken;
pub use crate::error::{StdbBevyError, StdbBevyErrorEvent};
pub use crate::lifecycle::lifecycle_channel::LifecycleSink;
pub use crate::lifecycle::lifecycle_events::{StdbConnected, StdbDisconnected};
pub use crate::lifecycle::reconnect::{ReconnectAction, ReconnectPolicy, ReconnectState};
pub use crate::row::row_channel::StdbRow;
pub use crate::row::row_forwarder::RowForwarder;
pub use crate::row::row_messages::{RowDeleted, RowInserted, RowUpdated};
pub use crate::row::table_registration::TableRegistration;
pub use crate::sdk_impl::{
    sdk_connection_driver::SdkConnectionDriver, sdk_subscription_driver::SdkSubscriptionDriver,
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

mod connection;
mod error;
mod lifecycle;
mod row;
mod sdk_impl;
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
#[derive(Default)]
pub struct StdbPlugin<Cd: StdbConnectionDriver, Sd = Cd> {
    conn_driver: Cd,
    sub_driver: Sd,
    tables: Vec<TableRegistration<Cd::Conn>>,
    connect_on_startup: bool,
}

impl<Cd, Sd> StdbPlugin<Cd, Sd>
where
    Cd: StdbConnectionDriver,
    Sd: StdbSubscriptionDriver<Conn = Cd::Conn>,
{
    pub fn new(conn_driver: Cd, sub_driver: Sd) -> Self {
        Self {
            conn_driver,
            sub_driver,
            tables: Vec::new(),
            connect_on_startup: false,
        }
    }

    fn build_subscription_app(&self, app: &mut bevy::app::App) {
        app.init_resource::<SubscriptionChannel>();
        app.insert_resource(self.sub_driver.clone());

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

    pub(crate) fn build(&self, app: &mut bevy::app::App) {
        self.build_connection(app);
        self.build_subscription_app(app);
    }
}

impl<Cd: StdbConnectionDriver, Sd> StdbPlugin<Cd, Sd> {
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
        app.insert_resource(StdbIntent::Disconnected);
        app.insert_resource(StdbStatus::Disconnected);
        app.insert_resource(LifecycleChannel::<Cd::Conn>::new());
        app.insert_resource(self.conn_driver.clone());

        app.add_observer(update_intent_on_stdbconnect);
        app.add_observer(update_intent_on_stdbdisconnect);
        app.add_observer(connect_on_stdbconnect::<Cd>);
        app.add_observer(disconnect_on_stdbdisconnect::<Cd>);

        app.configure_sets(
            bevy::app::Update,
            (
                StdbSystemSet::LifecycleEvents,
                StdbSystemSet::RowMessagesPush,
                StdbSystemSet::Resync.run_if(
                    resource_exists::<StdbPreviousConnection<Cd::Conn>>
                        .and(is_stdb_connected)
                        .and(is_subscriptions_settled),
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

        app.add_observer(reset_reconnectstate_on_stdbdisconnected);

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

    pub(crate) fn build_connection(&self, app: &mut bevy::app::App) {
        self.build_lifecyle_app(app);
        self.build_reconnect_app(app);
        self.build_tables_app(app);
    }
}

impl<Cd: StdbConnectionDriver> StdbPlugin<Cd, NoSubscriptions> {
    pub fn connection(conn_driver: Cd) -> Self {
        Self {
            conn_driver,
            sub_driver: NoSubscriptions,
            connect_on_startup: false,
            tables: Vec::new(),
        }
    }

    pub fn with_subscription<Sd>(self, sub_driver: Sd) -> StdbPlugin<Cd, Sd>
    where
        Sd: StdbSubscriptionDriver<Conn = Cd::Conn>,
    {
        StdbPlugin {
            conn_driver: self.conn_driver,
            sub_driver,
            connect_on_startup: self.connect_on_startup,
            tables: self.tables,
        }
    }
}

impl<M, C> StdbPlugin<SdkConnectionDriver<M, C>, SdkSubscriptionDriver<M, C>>
where
    M: sdk_impl::SdkSpacetimeModule<DbConnection = C>,
    C: sdk_impl::SdkDbConnection<Module = M> + spacetimedb_sdk::DbContext + StdbConn,
    M::SubscriptionHandle: Send + Sync,
{
    pub fn sdk<U>(
        uri: U,
        database_name: impl Into<String>,
        tick: fn(&C) -> spacetimedb_sdk::Result<()>,
    ) -> Self
    where
        U: TryInto<http::Uri>,
        U::Error: Debug,
    {
        let conn_driver = SdkConnectionDriver::new(uri, database_name, tick);
        let sub_driver = SdkSubscriptionDriver::default();
        Self {
            conn_driver,
            sub_driver,
            tables: Vec::new(),
            connect_on_startup: false,
        }
    }
}

impl<Cd: StdbConnectionDriver> bevy::app::Plugin for StdbPlugin<Cd, NoSubscriptions> {
    fn build(&self, app: &mut bevy::app::App) {
        self.build_connection(app);
    }
}

impl<Cd, Sd> bevy::app::Plugin for StdbPlugin<Cd, Sd>
where
    Cd: StdbConnectionDriver,
    Sd: StdbSubscriptionDriver<Conn = Cd::Conn>,
{
    fn build(&self, app: &mut bevy::app::App) {
        self.build(app);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
