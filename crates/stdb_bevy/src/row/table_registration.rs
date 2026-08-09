use bevy::prelude::*;

use crate::{
    RowDeleted, RowForwarder, RowInserted, RowMessagesMask, RowUpdated, StdbConn, StdbConnected,
    StdbConnection, StdbPreviousConnection, StdbRow, StdbSystemSet,
    row::{
        row_channel::{RowChannel, clear_row_sink, drain_row_sink},
        row_messages_resync::{
            drop_stdbpreviousconnection_after_resync, resync_row_messages_system,
        },
    },
};

/// One table's opt-in to row-change events: the row channel, the `RowInserted` / `RowUpdated` /
/// `RowDeleted` messages, the per-connect callback wiring, and the reconnect diff, for a single row
/// type. Built by [`stdb_table!`] and installed by [`register`](Self::register).
pub struct TableRegistration<C: StdbConn> {
    install: Box<dyn Fn(&mut bevy::app::App) + Send + Sync + 'static>,
    mark: std::marker::PhantomData<C>,
}

impl<C: StdbConn> TableRegistration<C> {
    /// Registration for a table with a primary key.
    ///
    /// `key` extracts that key from a row; it is the identity the reconnect diff pairs rows by, so it
    /// must be unique and stable per row.
    pub fn pk<R, K>(
        forward: fn(&StdbConnection<C>, RowForwarder<R>) -> RowForwarder<R>,
        snapshot: fn(&C) -> Vec<R>,
        key: fn(&R) -> K,
        messages_mask: RowMessagesMask,
        label: &'static str,
    ) -> Self
    where
        R: StdbRow,
        K: 'static + Eq + Ord,
    {
        Self {
            install: Box::new(move |app| {
                add_stdb_table(app, forward, snapshot, key, messages_mask, label);
            }),
            mark: std::marker::PhantomData,
        }
    }

    /// Installs this registration into `app`.
    pub fn register(&self, app: &mut bevy::app::App) {
        (self.install)(app)
    }
}

/// Wires one table into `app`: its row channel and messages, a per-connect observer that re-installs
/// the SDK callbacks (a rebuilt reconnection starts with none), the reconnect diff, and the drain that
/// turns buffered rows into messages.
pub(crate) fn add_stdb_table<C, R, K>(
    app: &mut bevy::app::App,
    forward: fn(&StdbConnection<C>, RowForwarder<R>) -> RowForwarder<R>,
    snapshot: fn(&C) -> Vec<R>,
    key: fn(&R) -> K,
    messages_mask: RowMessagesMask,
    label: &'static str,
) where
    C: StdbConn,
    R: StdbRow,
    K: 'static + Eq + Ord,
{
    let row_channel = RowChannel::new();
    let sink = row_channel.sink();

    app.insert_resource(row_channel);
    app.add_message::<RowInserted<R>>();
    app.add_message::<RowUpdated<R>>();
    app.add_message::<RowDeleted<R>>();

    app.add_observer(
        move |_: On<StdbConnected>, connection: Res<StdbConnection<C>>| {
            let fwd = RowForwarder::new(sink.clone()).with_filter(messages_mask);
            (forward)(&connection, fwd);
        },
    );

    app.add_systems(
        bevy::app::Update,
        resync_row_messages_system(snapshot, key, messages_mask, label)
            .in_set(StdbSystemSet::Resync)
            .before(drop_stdbpreviousconnection_after_resync::<C>),
    );

    app.add_systems(
        bevy::app::Update,
        (
            drain_row_sink::<R>(label).run_if(not(resource_exists::<StdbPreviousConnection<C>>)),
            clear_row_sink::<R>.run_if(resource_exists::<StdbPreviousConnection<C>>),
        )
            .in_set(StdbSystemSet::RowMessagesPush),
    );
}

/// Builds a [`TableRegistration`] for one table.
///
/// `stdb_table!(accessor => Row, key = <field>)` forwards all events; a trailing
/// `[insert, delete, ...]` selects a subset. `key` names the primary key: it is the identity the
/// reconnect diff pairs rows by, not any key a mirror uses to locate entities. Only primary-keyed
/// tables can register, since a keyless table or view has no diffable row identity.
#[macro_export]
macro_rules! stdb_table {
    ($accessor:ident => $row:ty, key = $key:ident) => {
        $crate::TableRegistration::pk(
            |conn, fwd| {
                use $crate::__sdk::DbContext as _;
                fwd.forward(&conn.db().$accessor())
            },
            |conn| {
                use $crate::__sdk::{DbContext as _, Table as _};
                conn.db().$accessor().iter().collect()
            },
            |row| row.$key.clone(),
            $crate::RowMessagesMask::ALL,
            stringify!($accessor),
        )
    };

    ($accessor:ident => $row:ty, key = $key:ident, [$($cb:ident),+ $(,)?]) => {
        $crate::TableRegistration::pk(
            |conn, fwd| {
                use $crate::__sdk::DbContext as _;
                fwd.forward(&conn.db().$accessor())
            },
            |conn| {
                use $crate::__sdk::{DbContext as _, Table as _};
                conn.db().$accessor().iter().collect()
            },
            |row| row.$key.clone(),
            $crate::RowMessagesMask { $($cb: true,)+ ..$crate::RowMessagesMask::NONE },
            stringify!($accessor),
        )
    };
}
