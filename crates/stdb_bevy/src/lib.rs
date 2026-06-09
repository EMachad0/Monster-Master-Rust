//! Minimal, module-agnostic bridge between a SpacetimeDB connection and Bevy.
//!
//! This crate knows nothing about any specific Module.

use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};

use crate::connection::stdb_intent::{
    StdbIntent, update_intent_on_stdbconnect, update_intent_on_stdbdisconnect,
};
use crate::connection_driver::stdb_connection_driver::{
    connect_on_stdbconnect, disconnect_on_stdbdisconnect, tick_stdbconnectiondriver,
};
use crate::lifecycle::lifecycle_channel::{LifecycleChannel, drain_lifecycle_sink};
use crate::lifecycle::reconnect::{
    reset_reconnectstate_on_stdbdisconnected, should_tick_reconnectstate, tick_reconnectstate,
};

pub use crate::connection::connection_events::{StdbConnect, StdbDisconnect};
pub use crate::connection::stdb_connection::{StdbConn, StdbConnection};
pub use crate::connection::stdb_status::{StdbStatus, stdb_connected as is_stdb_connected};
pub use crate::connection_driver::{
    sdk_connection_driver::SdkConnectionDriver, stdb_connection_driver::StdbConnectionDriver,
};
pub use crate::lifecycle::lifecycle_channel::LifecycleSink;
pub use crate::lifecycle::lifecycle_events::{
    StdbConnected, StdbConnectionError, StdbDisconnected,
};
pub use crate::lifecycle::reconnect::{ReconnectAction, ReconnectPolicy, ReconnectState};
pub use crate::row::row_channel::StdbRow;
pub use crate::row::row_forwarder::RowForwarder;
pub use crate::row::row_messages::{RowDeleted, RowInserted, RowUpdated};
pub use crate::row::table_registration::TableRegistration;
pub use crate::utils::backoff::{Backoff, Jitter};

mod connection;
mod connection_driver;
mod lifecycle;
mod row;
mod utils;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum StdbSystemSet {
    LifecycleEvents,
    RowMessagesPush,
    Main,
}

/// Wires a SpacetimeDB connection into a Bevy `App`:
#[derive(Default)]
pub struct StdbPlugin<Cd: StdbConnectionDriver> {
    driver: Cd,
    connect_on_startup: bool,
    tables: Vec<TableRegistration>,
}

impl<Cd: StdbConnectionDriver> StdbPlugin<Cd> {
    pub fn new(driver: Cd) -> Self {
        Self {
            driver,
            connect_on_startup: false,
            tables: Vec::new(),
        }
    }

    pub fn with_connect_on_startup(mut self) -> Self {
        self.connect_on_startup = true;
        self
    }

    pub fn add_tables(mut self, registrators: impl IntoIterator<Item = TableRegistration>) -> Self {
        self.tables.extend(registrators);
        self
    }
}

impl<Cd: StdbConnectionDriver> bevy::app::Plugin for StdbPlugin<Cd> {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(StdbIntent::Disconnected);
        app.insert_resource(StdbStatus::Disconnected);
        app.insert_resource(LifecycleChannel::<Cd::Conn>::new());
        app.insert_resource(self.driver.clone());
        app.init_resource::<ReconnectPolicy>();
        app.init_resource::<ReconnectState>();

        app.add_observer(update_intent_on_stdbconnect);
        app.add_observer(update_intent_on_stdbdisconnect);
        app.add_observer(connect_on_stdbconnect::<Cd>);
        app.add_observer(disconnect_on_stdbdisconnect::<Cd>);
        app.add_observer(reset_reconnectstate_on_stdbdisconnected);

        app.configure_sets(
            bevy::app::Update,
            (
                StdbSystemSet::LifecycleEvents,
                StdbSystemSet::RowMessagesPush,
                StdbSystemSet::Main,
            )
                .chain(),
        );

        app.add_systems(
            bevy::app::Update,
            drain_lifecycle_sink::<Cd::Conn>.in_set(StdbSystemSet::LifecycleEvents),
        );
        app.add_systems(
            bevy::app::Update,
            (
                tick_stdbconnectiondriver::<Cd>.run_if(is_stdb_connected),
                tick_reconnectstate::<Cd>.run_if(should_tick_reconnectstate),
            )
                .in_set(StdbSystemSet::Main),
        );

        if self.connect_on_startup {
            app.add_systems(
                bevy::app::Startup,
                connection_driver::stdb_connection_driver::connect::<Cd>,
            );
        }

        for registrator in self.tables.iter() {
            registrator.register(app);
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
