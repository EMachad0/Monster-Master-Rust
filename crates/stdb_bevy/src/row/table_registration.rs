use bevy::prelude::*;

use crate::{
    RowDeleted, RowForwarder, RowInserted, RowUpdated, StdbConn, StdbConnected, StdbConnection,
    StdbRow, StdbSystemSet,
    row::{
        row_channel::{RowChannel, drain_row_sink},
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

    pub fn pk<T, R, K>(accessor: fn(&C) -> T, key: fn(&R) -> K) -> Self
    where
        T: 'static + spacetimedb_sdk::TableWithPrimaryKey<Row = R>,
        R: StdbRow,
        K: 'static + Eq + Ord,
    {
        let messages_callback = move |connection: &StdbConnection<C>, fwd: RowForwarder<R>| {
            fwd.forward(&(accessor)(connection));
        };
        Self {
            install: Box::new(move |app| {
                add_stdb_table(app, messages_callback, accessor, key);
            }),
            mark: std::marker::PhantomData,
        }
    }

    pub fn register(&self, app: &mut bevy::app::App) {
        (self.install)(app)
    }
}

pub(crate) fn add_stdb_table<C, R, T, K>(
    app: &mut bevy::app::App,
    messages_callback: impl Fn(&StdbConnection<C>, RowForwarder<R>) + 'static + Send + Sync,
    accessor: fn(&C) -> T,
    key: fn(&R) -> K,
) where
    C: StdbConn,
    R: StdbRow,
    T: 'static + spacetimedb_sdk::Table<Row = R>,
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
            let fwd = RowForwarder::new(sink.clone());
            (messages_callback)(&connection, fwd);
        },
    );

    app.add_systems(
        bevy::app::Update,
        resync_row_messages_system(accessor, key)
            .in_set(StdbSystemSet::Resync)
            .before(drop_stdbpreviousconnection_after_resync::<C>),
    );

    app.add_systems(
        bevy::app::Update,
        drain_row_sink::<R>.in_set(StdbSystemSet::RowMessagesPush),
    );
}

#[macro_export]
macro_rules! stdb_table {
    ($accessor:ident => $row:ty) => {
        $crate::TableRegistration::new(|conn, fwd: $crate::RowForwarder<$row>| {
            use $crate::__sdk::DbContext as _;
            fwd.forward(&conn.db().$accessor());
        })
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
    };}
