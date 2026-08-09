//! Bridge-owned table capabilities: everything the row path asks of a table handle.
//!
//! One trait per callback the forwarder can wire, plus the whole-table read the resync diff makes.
//! The SDK adapter implements all of them on one newtype wrapping any SDK table handle, so a
//! generated handle satisfies these bounds with no per-table code; test fakes implement them
//! directly and never name an SDK type.
//!
//! The callbacks carry the row payload alone. The event context the SDK hands them describes the
//! transaction that caused the change, which no row path reads, and the callback id it returns is
//! for de-registration, which nothing here does: a reconnect builds a fresh connection whose
//! callbacks start empty, so the bridge re-wires on every connect and lets the dropped connection
//! retire the old callbacks.

use crate::row::row_channel::StdbRow;

/// A table handle that announces inserted rows.
pub trait RowInsertSource {
    type Row: StdbRow;

    /// Installs `cb` to run on every inserted row, for the lifetime of the connection that owns
    /// this handle.
    fn on_insert(&self, cb: impl FnMut(&Self::Row) + Send + 'static);
}

/// A table handle that announces deleted rows.
pub trait RowDeleteSource {
    type Row: StdbRow;

    /// Installs `cb` to run on every deleted row, for the lifetime of the connection that owns
    /// this handle.
    fn on_delete(&self, cb: impl FnMut(&Self::Row) + Send + 'static);
}

/// A table handle that announces updated rows as `(old, new)`.
///
/// Only a primary-keyed table has one: without a row identity the server cannot pair the two
/// versions, and the change arrives as a delete plus an insert instead.
pub trait RowUpdateSource {
    type Row: StdbRow;

    /// Installs `cb` to run on every updated row, for the lifetime of the connection that owns
    /// this handle.
    fn on_update(&self, cb: impl FnMut(&Self::Row, &Self::Row) + Send + 'static);
}

/// A table handle whose current rows can be read out in full: the read the resync diff makes
/// against both sides of a reconnect.
pub trait RowCollection {
    type Row: StdbRow;

    /// The rows this handle currently holds. Owned rather than borrowed, because the caller keeps
    /// them past the handle: a generated accessor returns a fresh, short-lived handle per call.
    fn rows(&self) -> Vec<Self::Row>;
}
