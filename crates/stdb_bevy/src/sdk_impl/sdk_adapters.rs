//! Blanket adapters from the SDK's traits onto the bridge-owned capabilities, so every generated
//! handle and connection satisfies the row-path bounds with no per-table code.
//!
//! The bounds are the SDK's capability-shaped table traits (`TableLike` and its `With*` subtraits),
//! not the kind-shaped ones (`Table`, `TableWithPrimaryKey`): the two families are independent, and
//! the capability traits name exactly what each adapter forwards.

use spacetimedb_sdk::table::{TableLike, WithDelete, WithInsert, WithUpdate};

use crate::connection::db_access::DbAccess;
use crate::row::row_channel::StdbRow;
use crate::row::table_capabilities::{
    RowCollection, RowDeleteSource, RowInsertSource, RowUpdateSource,
};

impl<T> RowInsertSource for T
where
    T: WithInsert,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn on_insert(&self, mut cb: impl FnMut(&Self::Row) + Send + 'static) {
        // The event context and the returned callback id both stop at this seam: the row path
        // reads neither, and the connection retires its callbacks when it is dropped.
        let _id = WithInsert::on_insert(self, move |_ctx, row| cb(row));
    }
}

impl<T> RowDeleteSource for T
where
    T: WithDelete,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn on_delete(&self, mut cb: impl FnMut(&Self::Row) + Send + 'static) {
        let _id = WithDelete::on_delete(self, move |_ctx, row| cb(row));
    }
}

impl<T> RowUpdateSource for T
where
    T: WithUpdate,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn on_update(&self, mut cb: impl FnMut(&Self::Row, &Self::Row) + Send + 'static) {
        let _id = WithUpdate::on_update(self, move |_ctx, old, new| cb(old, new));
    }
}

impl<T> RowCollection for T
where
    T: TableLike,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn rows(&self) -> Vec<Self::Row> {
        self.iter().collect()
    }
}

impl<C> DbAccess for C
where
    C: spacetimedb_sdk::DbContext,
{
    type Db = C::DbView;

    fn db(&self) -> &Self::Db {
        spacetimedb_sdk::DbContext::db(self)
    }
}
