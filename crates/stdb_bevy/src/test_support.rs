//! Reusable fakes for exercising the bridge without a real SpacetimeDB connection.
//!
//! Available to this crate's own tests, and to downstream crates (e.g. `game`) and the bridge's
//! public-API e2e suite via the `test-support` feature.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bevy::ecs::resource::Resource;
use spacetimedb_sdk::{
    ConnectionId, DbContext, Identity, Table as SdkTable,
    TableWithPrimaryKey as SdkTableWithPrimaryKey,
};

use crate::StdbSubscriptionDriver;
use crate::{LifecycleSink, StdbConn, StdbConnection, StdbConnectionDriver};
use crate::{StdbBevyError, SubscriptionId};

/// A minimal connection value for tests that never read the connection itself — the bridge's
/// `StdbConn` bound asks only for `Send + Sync + 'static`.
#[derive(Clone, Default)]
pub struct FakeConn;

/// A driver that connects synchronously (announcing `Connecting` then `Connected`) and retains the
/// sink it was handed, so a test can later push an unsolicited drop or error through the public
/// `LifecycleSink`.
#[derive(Resource, Clone, Default)]
pub struct FakeDriver {
    sink: Arc<Mutex<Option<LifecycleSink<FakeConn>>>>,
    unsubscribes: Arc<AtomicUsize>,
    next_id: Arc<AtomicU64>,
}

impl FakeDriver {
    /// How many times a handle issued by this driver has been unsubscribed.
    pub fn unsubscribes(&self) -> usize {
        self.unsubscribes.load(Ordering::Relaxed)
    }

    /// The sink handed to the most recent `connect`, for simulating a drop/error after a connect.
    pub fn sink(&self) -> LifecycleSink<FakeConn> {
        self.sink
            .lock()
            .unwrap()
            .clone()
            .expect("connect() has not run yet")
    }
}

impl StdbConnectionDriver for FakeDriver {
    type Conn = FakeConn;

    fn connect(&self, sink: LifecycleSink<FakeConn>) {
        // Mirror the real driver contract: announce Connecting, then complete synchronously.
        sink.connecting().unwrap();
        sink.connected(FakeConn).unwrap();
        *self.sink.lock().unwrap() = Some(sink);
    }

    fn tick(&self, _conn: &StdbConnection<FakeConn>) {}

    fn disconnect(&self, _conn: &StdbConnection<FakeConn>, sink: LifecycleSink<FakeConn>) {
        sink.disconnected().unwrap();
    }
}

impl StdbSubscriptionDriver for FakeDriver {
    type Conn = FakeConn;

    fn subscribe(
        &mut self,
        _conn: &StdbConnection<Self::Conn>,
        sink: crate::subscription::subscription_channel::SubscriptionSink,
        _subscription: &crate::Subscription,
    ) -> SubscriptionId {
        // Mint an id (for the handle map) and apply immediately via the entity-bound sink,
        // mirroring the real driver's on_applied.
        sink.applied();
        SubscriptionId::from(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn unsubscribe(
        &mut self,
        _sink: crate::subscription::subscription_channel::SubscriptionSink,
        _subscription_id: &SubscriptionId,
    ) {
        self.unsubscribes.fetch_add(1, Ordering::Relaxed);
    }

    fn clear(&mut self) {}
}

/// A driver whose `connect` does **not** complete: it announces `Connecting`, counts the kick, and
/// parks the sink so a test can observe the in-flight window and deliver the result later —
/// mimicking wasm's multi-frame `build().await`.
#[derive(Resource, Clone, Default)]
pub struct DeferredDriver {
    connects: Arc<AtomicUsize>,
    parked_sink: Arc<Mutex<Option<LifecycleSink<FakeConn>>>>,
}

impl DeferredDriver {
    /// How many times `connect` has been kicked.
    pub fn connects(&self) -> usize {
        self.connects.load(Ordering::Relaxed)
    }

    /// Take the parked sink to deliver the connection result (connected / error).
    pub fn take_parked_sink(&self) -> LifecycleSink<FakeConn> {
        self.parked_sink
            .lock()
            .unwrap()
            .take()
            .expect("no parked sink — connect() has not run")
    }

    /// Resolve the parked build successfully.
    pub fn deliver_connected(&self) {
        self.take_parked_sink().connected(FakeConn).unwrap();
    }

    /// Fail the parked build with a connection error (lets callers simulate a failed build without
    /// naming the internal `ConnectionError`).
    pub fn deliver_error(&self) {
        self.take_parked_sink()
            .connection_error(StdbBevyError::ConnectionRefused)
            .unwrap();
    }
}

impl StdbConnectionDriver for DeferredDriver {
    type Conn = FakeConn;

    fn connect(&self, sink: LifecycleSink<FakeConn>) {
        sink.connecting().unwrap();
        self.connects.fetch_add(1, Ordering::Relaxed);
        *self.parked_sink.lock().unwrap() = Some(sink); // parked: not connected yet
    }

    fn disconnect(&self, _conn: &StdbConnection<FakeConn>, sink: LifecycleSink<FakeConn>) {
        sink.disconnected().unwrap();
    }

    fn tick(&self, _conn: &StdbConnection<FakeConn>) {}
}

/// A table that presents rows two independent ways, so the bridge's row paths run without a live
/// connection:
///
/// - **callbacks** (`inserts`/`updates`/`deletes`): replayed into a callback the moment it is
///   registered, driving the `RowForwarder` (callback → message);
/// - **cache** (`rows`): yielded by `iter()`, the row set the resync diff reads from a
///   `StdbPreviousConnection` / `StdbConnection`.
///
/// Its event context and callback ids are unit types — this table carries no event payload.
#[derive(Default)]
pub struct FakeTable<R> {
    /// The rows the table "contains" — yielded by `iter()`, the path the resync diff reads.
    pub rows: Vec<R>,
    pub inserts: Vec<R>,
    pub updates: Vec<(R, R)>,
    pub deletes: Vec<R>,
}

impl<R> FakeTable<R> {
    /// A table whose cache (`iter()`) holds `rows`, with the callback fields left empty — the shape
    /// the resync diff reads. Pairs with `FakeDbContext` to present a connection's per-table rows.
    pub fn with_rows(rows: Vec<R>) -> Self {
        Self {
            rows,
            inserts: Vec::new(),
            updates: Vec::new(),
            deletes: Vec::new(),
        }
    }
}

impl<R: 'static + Clone> SdkTable for FakeTable<R> {
    type Row = R;
    type EventContext = ();
    type InsertCallbackId = ();
    type DeleteCallbackId = ();

    fn count(&self) -> u64 {
        self.rows.len() as u64
    }
    fn iter(&self) -> impl Iterator<Item = R> + '_ {
        self.rows.iter().cloned()
    }
    fn on_insert(&self, mut cb: impl FnMut(&(), &R) + Send + 'static) -> Self::InsertCallbackId {
        for r in &self.inserts {
            cb(&(), r);
        }
    }
    fn remove_on_insert(&self, _id: Self::InsertCallbackId) {}
    fn on_delete(&self, mut cb: impl FnMut(&(), &R) + Send + 'static) -> Self::DeleteCallbackId {
        for r in &self.deletes {
            cb(&(), r);
        }
    }
    fn remove_on_delete(&self, _id: Self::DeleteCallbackId) {}
}

impl<R: 'static + Clone> SdkTableWithPrimaryKey for FakeTable<R> {
    type UpdateCallbackId = ();
    fn on_update(
        &self,
        mut cb: impl FnMut(&(), &R, &R) + Send + 'static,
    ) -> Self::UpdateCallbackId {
        for (old, new) in &self.updates {
            cb(&(), old, new);
        }
    }
    fn remove_on_update(&self, _id: Self::UpdateCallbackId) {}
}

// The granular capability traits the `RowForwarder` binds to, alongside the `Table` /
// `TableWithPrimaryKey` impls above: the two families are independent (nothing blanket-implements
// one from the other), and generated table handles carry both, so a fake standing in for one has to
// as well. The paths are fully qualified and deliberately not imported: `FakeTable` presents
// `count`/`iter` under two traits at once, and bringing both into scope makes every unqualified call
// in this module ambiguous.
impl<R: 'static + Clone> spacetimedb_sdk::table::TableLike for FakeTable<R> {
    type Row = R;
    type EventContext = ();

    fn count(&self) -> u64 {
        self.rows.len() as u64
    }
    fn iter(&self) -> impl Iterator<Item = R> + '_ {
        self.rows.iter().cloned()
    }
}

impl<R: 'static + Clone> spacetimedb_sdk::table::WithInsert for FakeTable<R> {
    type InsertCallbackId = ();
    fn on_insert(&self, mut cb: impl FnMut(&(), &R) + Send + 'static) -> Self::InsertCallbackId {
        for r in &self.inserts {
            cb(&(), r);
        }
    }
    fn remove_on_insert(&self, _id: Self::InsertCallbackId) {}
}

impl<R: 'static + Clone> spacetimedb_sdk::table::WithDelete for FakeTable<R> {
    type DeleteCallbackId = ();
    fn on_delete(&self, mut cb: impl FnMut(&(), &R) + Send + 'static) -> Self::DeleteCallbackId {
        for r in &self.deletes {
            cb(&(), r);
        }
    }
    fn remove_on_delete(&self, _id: Self::DeleteCallbackId) {}
}

impl<R: 'static + Clone> spacetimedb_sdk::table::WithUpdate for FakeTable<R> {
    type UpdateCallbackId = ();
    fn on_update(
        &self,
        mut cb: impl FnMut(&(), &R, &R) + Send + 'static,
    ) -> Self::UpdateCallbackId {
        for (old, new) in &self.updates {
            cb(&(), old, new);
        }
    }
    fn remove_on_update(&self, _id: Self::UpdateCallbackId) {}
}

/// A driver that connects synchronously, handing back a caller-supplied connection value. Generic
/// over the connection type `C`, so a test can drive the bridge with a `FakeDbContext<V>` whose
/// DbView exposes table accessors — the shape the `stdb_table!` macro's `conn.db().<table>()` body
/// needs. (`FakeConnectionDriver` can't: its `Conn` is the field-less `FakeConn`.)
#[derive(Resource, Clone)]
pub struct CannedDriver<C: StdbConn + Clone> {
    conn: C,
}

impl<C: StdbConn + Clone> CannedDriver<C> {
    pub fn new(conn: C) -> Self {
        Self { conn }
    }
}

impl<C: StdbConn + Clone> StdbConnectionDriver for CannedDriver<C> {
    type Conn = C;

    fn connect(&self, sink: LifecycleSink<C>) {
        sink.connecting().unwrap();
        sink.connected(self.conn.clone()).unwrap();
    }

    fn disconnect(&self, _conn: &StdbConnection<C>, sink: LifecycleSink<C>) {
        sink.disconnected().unwrap();
    }

    fn tick(&self, _conn: &StdbConnection<C>) {}
}

/// A connection whose `db()` exposes a caller-supplied DbView `V`, so the `stdb_table!` macro's
/// `conn.db().<table>()` body — and the resync diff's per-table reads — run in a unit test. Each
/// test supplies its own table accessors through `V`, the way a generated `RemoteTables` does. Only
/// `db()` carries meaning here; the other accessors are unused stubs.
#[derive(Clone)]
pub struct FakeDbContext<V> {
    db: V,
}

impl<V> FakeDbContext<V> {
    pub fn new(db: V) -> Self {
        Self { db }
    }
}

impl<V: Send + Sync + 'static> DbContext for FakeDbContext<V> {
    type DbView = V;
    type Reducers = ();
    type Procedures = ();
    type SubscriptionBuilder = ();

    fn db(&self) -> &V {
        &self.db
    }
    fn reducers(&self) -> &() {
        &()
    }
    fn procedures(&self) -> &() {
        &()
    }
    fn is_active(&self) -> bool {
        true
    }
    fn disconnect(&self) -> spacetimedb_sdk::Result<()> {
        unimplemented!("FakeDbContext is a read-only test double")
    }
    fn subscription_builder(&self) {}
    fn try_identity(&self) -> Option<Identity> {
        None
    }
    fn connection_id(&self) -> ConnectionId {
        unimplemented!("FakeDbContext has no ConnectionId")
    }
    fn try_connection_id(&self) -> Option<ConnectionId> {
        None
    }
}

/// Build a test `App` with the **connection-only** bridge installed for `driver`, plus a `Time`
/// resource — the reconnect system needs `Time`, which production supplies via the Game's
/// `TimePlugin`. Subscription tests build their own app via `StdbPlugin::new` (subscriptions on).
pub fn test_app<Cd: StdbConnectionDriver + Clone>(driver: Cd) -> bevy::app::App {
    let mut app = bevy::app::App::new();
    app.add_plugins(crate::StdbPlugin::connection(driver));
    app.insert_resource(bevy::time::Time::<()>::default());
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Debug)]
    struct Player {
        id: u32,
    }
    #[derive(Clone, PartialEq, Debug)]
    struct Monster {
        id: u32,
    }

    /// Stand-in DbView: per-table accessors returning each table's cached rows, mirroring a
    /// generated `RemoteTables`. The resync diff reaches rows via `conn.db().<table>().iter()`.
    #[derive(Clone)]
    struct GameDb {
        players: Vec<Player>,
        monsters: Vec<Monster>,
    }

    impl GameDb {
        fn player(&self) -> FakeTable<Player> {
            FakeTable::with_rows(self.players.clone())
        }
        fn monster(&self) -> FakeTable<Monster> {
            FakeTable::with_rows(self.monsters.clone())
        }
    }

    #[test]
    fn fake_table_iter_yields_its_rows() {
        let table = FakeTable::with_rows(vec![Player { id: 1 }, Player { id: 2 }]);

        assert_eq!(
            table.iter().collect::<Vec<_>>(),
            vec![Player { id: 1 }, Player { id: 2 }],
            "iter() must present the cached rows — the path the resync diff reads — not only callbacks",
        );
    }

    #[test]
    fn fake_table_iter_is_empty_with_no_rows() {
        let table: FakeTable<Player> = FakeTable::with_rows(vec![]);

        assert_eq!(
            table.iter().count(),
            0,
            "an empty cache yields no rows, so the diff reads the table as empty (ghost-everything)",
        );
    }

    #[test]
    fn fake_connection_presents_each_tables_rows_via_db() {
        let conn = FakeDbContext::new(GameDb {
            players: vec![Player { id: 1 }],
            monsters: vec![Monster { id: 7 }, Monster { id: 8 }],
        });

        assert_eq!(
            conn.db().player().iter().collect::<Vec<_>>(),
            vec![Player { id: 1 }],
            "db().<table>().iter() presents that table's cache — the diff's read path",
        );
        assert_eq!(
            conn.db().monster().iter().collect::<Vec<_>>(),
            vec![Monster { id: 7 }, Monster { id: 8 }],
            "each table accessor presents its own rows independently",
        );
    }
}
