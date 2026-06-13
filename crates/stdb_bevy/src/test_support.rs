//! Reusable fakes for exercising the bridge without a real SpacetimeDB connection.
//!
//! Available to this crate's own tests, and to downstream crates (e.g. `game`) and the bridge's
//! public-API e2e suite via the `test-support` feature.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bevy::ecs::resource::Resource;
use spacetimedb_sdk::{
    ConnectionId, DbContext, Identity, Table as SdkTable,
    TableWithPrimaryKey as SdkTableWithPrimaryKey,
};

use crate::StdbBevyError;
use crate::StdbSubscriptionDriver;
use crate::{LifecycleSink, StdbConn, StdbConnection, StdbConnectionDriver};

/// Stand-in for a real `DbConnection`. The engine only requires `Send + Sync + 'static`.
#[derive(Clone, Default)]
pub struct FakeConn;

/// A driver that connects synchronously (announcing `Connecting` then `Connected`) and retains the
/// sink it was handed, so a test can later push an unsolicited drop or error through the public
/// `LifecycleSink`.
#[derive(Resource, Clone, Default)]
pub struct FakeDriver {
    sink: Arc<Mutex<Option<LifecycleSink<FakeConn>>>>,
}

impl FakeDriver {
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
        &self,
        _conn: &StdbConnection<Self::Conn>,
        entity: bevy::ecs::entity::Entity,
        _subscription: &crate::Subscription,
        sink: crate::subscription::subscription_channel::SubscriptionSink,
    ) {
        sink.applied(entity);
    }
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

/// A fake SDK table handle that delivers its canned rows the instant a callback is registered, so a
/// `RowForwarder` (handle callback → message) is exercisable without a real connection. The SDK
/// `Table` trait leaves `Row`/`EventContext` unconstrained, so a trivial `EventContext = ()` and
/// unit callback ids suffice.
#[derive(Default)]
pub struct FakeTable<R> {
    pub inserts: Vec<R>,
    pub updates: Vec<(R, R)>,
    pub deletes: Vec<R>,
}

impl<R: 'static> SdkTable for FakeTable<R> {
    type Row = R;
    type EventContext = ();
    type InsertCallbackId = ();
    type DeleteCallbackId = ();

    fn count(&self) -> u64 {
        self.inserts.len() as u64
    }
    fn iter(&self) -> impl Iterator<Item = R> + '_ {
        std::iter::empty()
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

impl<R: 'static> SdkTableWithPrimaryKey for FakeTable<R> {
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

/// A fake connection that satisfies the SDK's [`DbContext`] so the `stdb_table!` macro's
/// `conn.db().<table>()` body runs in a unit test. Generic over the DbView `V`, so each test
/// supplies its own table accessors (mirroring a generated `RemoteTables`). Only `db()` is
/// meaningful; the rest are stubs the macro never reaches.
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

/// Build a test `App` with the bridge installed for `driver`, plus a `Time` resource — the reconnect
/// system needs `Time`, which production supplies via the Game's `TimePlugin`.
pub fn test_app<Cd: StdbConnectionDriver + Clone>(driver: Cd) -> bevy::app::App {
    let mut app = bevy::app::App::new();
    app.add_plugins(crate::StdbPlugin::new(driver));
    app.insert_resource(bevy::time::Time::<()>::default());
    app
}
