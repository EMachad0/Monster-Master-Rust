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

pub struct TableRegistration<C: StdbConn> {
    install: Box<dyn Fn(&mut bevy::app::App) + Send + Sync + 'static>,
    mark: std::marker::PhantomData<C>,
}

impl<C: StdbConn> TableRegistration<C> {
    pub fn new<R>(_messages_callback: fn(&StdbConnection<C>, RowForwarder<R>)) -> Self
    where
        R: StdbRow,
    {
        Self {
            install: Box::new(move |_app| {
                // add_stdb_table(app, messages_callback);
            }),
            mark: std::marker::PhantomData,
        }
    }

    pub fn pk<R, K>(
        forward: fn(&StdbConnection<C>, RowForwarder<R>) -> RowForwarder<R>,
        snapshot: fn(&C) -> Vec<R>,
        key: fn(&R) -> K,
        messages_mask: RowMessagesMask,
    ) -> Self
    where
        R: StdbRow,
        K: 'static + Eq + Ord,
    {
        Self {
            install: Box::new(move |app| {
                add_stdb_table(app, forward, snapshot, key, messages_mask);
            }),
            mark: std::marker::PhantomData,
        }
    }

    pub fn register(&self, app: &mut bevy::app::App) {
        (self.install)(app)
    }
}

pub(crate) fn add_stdb_table<C, R, K>(
    app: &mut bevy::app::App,
    forward: fn(&StdbConnection<C>, RowForwarder<R>) -> RowForwarder<R>,
    snapshot: fn(&C) -> Vec<R>,
    key: fn(&R) -> K,
    messages_mask: RowMessagesMask,
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
        resync_row_messages_system(snapshot, key, messages_mask)
            .in_set(StdbSystemSet::Resync)
            .before(drop_stdbpreviousconnection_after_resync::<C>),
    );

    app.add_systems(
        bevy::app::Update,
        (
            drain_row_sink::<R>.run_if(not(resource_exists::<StdbPreviousConnection<C>>)),
            clear_row_sink::<R>.run_if(resource_exists::<StdbPreviousConnection<C>>),
        )
            .in_set(StdbSystemSet::RowMessagesPush),
    );
}

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
        )
    };

    ($accessor:ident => $row:ty, [$($cb:ident),+$(,)?]) => {
        $crate::TableRegistration::new(|conn, mut fwd| {
            use $crate::__sdk::DbContext as _;
            $(
                fwd = stdb_table!(@forward fwd, conn, $accessor, $cb);
            )+
        })
    };

    (@forward $fwd:ident, $conn:ident, $accessor:ident, insert) => {
        $fwd.inserts(&$conn.db().$accessor())
    };

    (@forward $fwd:ident, $conn:ident, $accessor:ident, update) => {
        $fwd.updates(&$conn.db().$accessor())
    };

    (@forward $fwd:ident, $conn:ident, $accessor:ident, delete) => {
        $fwd.deletes(&$conn.db().$accessor())
    };
}
