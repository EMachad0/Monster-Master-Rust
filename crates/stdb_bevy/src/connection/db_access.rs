/// Read access to a connection's client cache, the `conn.db()` every `stdb_table!` expansion makes.
///
/// Bridge-owned so macro expansions and test fakes reach their tables without naming an SDK type.
/// The SDK adapter blanket-implements it for every SDK connection.
///
/// It sits beside [`StdbConn`](crate::StdbConn) rather than inside it: folding `db()` into that
/// marker would cost it its blanket impl and force a dummy `Db` type on every fake that only ever
/// exercises the lifecycle.
pub trait DbAccess {
    type Db;

    fn db(&self) -> &Self::Db;
}
