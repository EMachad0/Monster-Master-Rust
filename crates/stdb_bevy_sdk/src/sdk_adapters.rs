//! An SDK table handle wearing the bridge's row-path capabilities, and the macro that puts it on.
//!
//! The bounds are the SDK's capability-shaped table traits (`TableLike` and its `With*` subtraits),
//! not the kind-shaped ones (`Table`, `TableWithPrimaryKey`): the two families are independent, and
//! the capability traits name exactly what each adapter forwards.

use spacetimedb_sdk::table::{TableLike, WithDelete, WithInsert, WithUpdate};
use stdb_bevy::{RowCollection, RowDeleteSource, RowInsertSource, RowUpdateSource, StdbRow};

/// A generated table handle, carrying the bridge's row-path capabilities.
///
/// A newtype rather than blanket impls straight onto the handles: the capability traits belong to
/// the core crate, and from here a foreign trait may only be implemented for a type this crate
/// owns, never for a bare type parameter. The wrap is free (it holds the handle by value) and
/// every generated handle still needs no per-table code.
pub struct SdkTable<T>(T);

impl<T> SdkTable<T> {
    pub fn new(table: T) -> Self {
        Self(table)
    }
}

impl<T> RowInsertSource for SdkTable<T>
where
    T: WithInsert,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn on_insert(&self, mut cb: impl FnMut(&Self::Row) + Send + 'static) {
        // The event context and the returned callback id both stop at this seam: the row path
        // reads neither, and the connection retires its callbacks when it is dropped.
        let _id = WithInsert::on_insert(&self.0, move |_ctx, row| cb(row));
    }
}

impl<T> RowDeleteSource for SdkTable<T>
where
    T: WithDelete,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn on_delete(&self, mut cb: impl FnMut(&Self::Row) + Send + 'static) {
        let _id = WithDelete::on_delete(&self.0, move |_ctx, row| cb(row));
    }
}

impl<T> RowUpdateSource for SdkTable<T>
where
    T: WithUpdate,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn on_update(&self, mut cb: impl FnMut(&Self::Row, &Self::Row) + Send + 'static) {
        let _id = WithUpdate::on_update(&self.0, move |_ctx, old, new| cb(old, new));
    }
}

impl<T> RowCollection for SdkTable<T>
where
    T: TableLike,
    T::Row: StdbRow,
{
    type Row = T::Row;

    fn rows(&self) -> Vec<Self::Row> {
        TableLike::iter(&self.0).collect()
    }
}

/// Builds a [`TableRegistration`](stdb_bevy::TableRegistration) for one table.
///
/// `stdb_table!(accessor => Row, key = <field>)` forwards all events; a trailing
/// `[insert, delete, ...]` selects a subset. `key` names the primary key: it is the identity the
/// reconnect diff pairs rows by, not any key a mirror uses to locate entities. Only primary-keyed
/// tables can register, since a keyless table or view has no diffable row identity.
///
/// It lives in the adapter because its expansion is what wraps each handle in a [`SdkTable`], which
/// the core crate cannot name.
#[macro_export]
macro_rules! stdb_table {
    ($accessor:ident => $row:ty, key = $key:ident) => {
        $crate::__macro_support::TableRegistration::pk(
            |conn, fwd| {
                use $crate::__macro_support::DbContext as _;
                fwd.forward(&$crate::SdkTable::new(conn.db().$accessor()))
            },
            |conn| {
                use $crate::__macro_support::{DbContext as _, RowCollection as _};
                $crate::SdkTable::new(conn.db().$accessor()).rows()
            },
            |row| row.$key.clone(),
            $crate::__macro_support::RowMessagesMask::ALL,
            stringify!($accessor),
        )
    };

    ($accessor:ident => $row:ty, key = $key:ident, [$($cb:ident),+ $(,)?]) => {
        $crate::__macro_support::TableRegistration::pk(
            |conn, fwd| {
                use $crate::__macro_support::DbContext as _;
                fwd.forward(&$crate::SdkTable::new(conn.db().$accessor()))
            },
            |conn| {
                use $crate::__macro_support::{DbContext as _, RowCollection as _};
                $crate::SdkTable::new(conn.db().$accessor()).rows()
            },
            |row| row.$key.clone(),
            $crate::__macro_support::RowMessagesMask {
                $($cb: true,)+
                ..$crate::__macro_support::RowMessagesMask::NONE
            },
            stringify!($accessor),
        )
    };
}
