//! Reusable fakes for exercising the bridge without a real SpacetimeDB connection.
//!
//! Available to this crate's own tests, and to downstream crates (e.g. `game`) and the bridge's
//! public-API e2e suite via the `test-support` feature.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bevy::ecs::resource::Resource;
use spacetimedb_sdk::{Table as SdkTable, TableWithPrimaryKey as SdkTableWithPrimaryKey};

use crate::lifecycle::lifecycle_events::ConnectionError;
use crate::{LifecycleSink, StdbConnection, StdbConnectionDriver};

/// Stand-in for a real `DbConnection`. The engine only requires `Send + Sync + 'static`.
#[derive(Clone, Default)]
pub struct FakeConn;

/// A driver that connects synchronously (announcing `Connecting` then `Connected`) and retains the
/// sink it was handed, so a test can later push an unsolicited drop or error through the public
/// `LifecycleSink`.
#[derive(Resource, Clone, Default)]
pub struct FakeConnectionDriver {
    sink: Arc<Mutex<Option<LifecycleSink<FakeConn>>>>,
}

impl FakeConnectionDriver {
    /// The sink handed to the most recent `connect`, for simulating a drop/error after a connect.
    pub fn sink(&self) -> LifecycleSink<FakeConn> {
        self.sink
            .lock()
            .unwrap()
            .clone()
            .expect("connect() has not run yet")
    }
}

impl StdbConnectionDriver for FakeConnectionDriver {
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
            .connection_error(ConnectionError::ConnectionRefused)
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

    fn tick(&self, _conn: &StdbConnection<FakeConn>) {}

    fn disconnect(&self, _conn: &StdbConnection<FakeConn>, sink: LifecycleSink<FakeConn>) {
        sink.disconnected().unwrap();
    }
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

/// Build a test `App` with the bridge installed for `driver`, plus a `Time` resource — the reconnect
/// system needs `Time`, which production supplies via the Game's `TimePlugin`.
pub fn test_app<Cd: StdbConnectionDriver + Clone>(driver: Cd) -> bevy::app::App {
    let mut app = bevy::app::App::new();
    app.add_plugins(crate::StdbPlugin::new(driver));
    app.insert_resource(bevy::time::Time::<()>::default());
    app
}
