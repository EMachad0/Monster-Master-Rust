/// Read access to a connection's client cache: the `conn.db().<table>()` a table registration
/// reaches its table through.
///
/// Bridge-owned so a registration's closures and test fakes reach their tables without naming an
/// SDK type. It has no production implementor: the adapter cannot implement a trait it does not own
/// for every SDK connection, only for a type of its own, so a macro-built registration goes through
/// the SDK's own context trait instead. What is left here are the hand-written registrations, which
/// the fakes serve.
///
/// It sits beside [`StdbConn`](crate::StdbConn) rather than inside it: folding `db()` into that
/// marker would cost it its blanket impl and force a dummy `Db` type on every fake that only ever
/// exercises the lifecycle.
pub trait DbAccess {
    type Db;

    fn db(&self) -> &Self::Db;
}
