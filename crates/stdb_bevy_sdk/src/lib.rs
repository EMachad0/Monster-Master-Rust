//! The bridge's SpacetimeDB SDK adapter: every place the SDK is named.
//!
//! The core `stdb_bevy` crate does not depend on `spacetimedb-sdk` at all, so an SDK type can only
//! be named here. That makes an SDK version migration's blast radius this crate plus the generated
//! bindings, and lets every engine-layer test run against SDK-free fakes.

pub use crate::sdk_adapters::SdkTable;
pub use crate::sdk_builder::SdkBuilder;
pub use crate::sdk_connection_driver::SdkConnectionDriver;
pub use crate::sdk_plugin_ext::SdkPluginExt;
pub use crate::sdk_reducer_sink_ext::SdkReducerSinkExt;
pub use crate::sdk_subscription_driver::SdkSubscriptionDriver;

mod sdk_adapters;
mod sdk_builder;
mod sdk_connection_driver;
mod sdk_plugin_ext;
mod sdk_reducer_sink_ext;
mod sdk_subscription_driver;

pub(crate) use spacetimedb_sdk::__codegen::{
    DbConnection as SdkDbConnection, SpacetimeModule as SdkSpacetimeModule,
    SubscriptionBuilder as SdkSubscriptionBuilder,
};

/// Everything [`stdb_table!`](crate::stdb_table) expands to, re-exported so the expansion reaches it
/// through `$crate`.
///
/// The macro lives here but builds a core type, and a Game may depend on the core under any name or
/// not name it directly at all; expanding to `::stdb_bevy::..` would break in both cases, while
/// `$crate::..` always resolves to wherever the caller got the macro from.
#[doc(hidden)]
pub mod __macro_support {
    pub use spacetimedb_sdk::DbContext;
    pub use stdb_bevy::{RowCollection, RowMessagesMask, TableRegistration};
}
