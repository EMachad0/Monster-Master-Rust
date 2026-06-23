//! End-to-end: the reconnect resync **diff** producing ghost deletes, through the public API only.
//!
//! Slice 4 (tracer bullet): at the resync fence the bridge diffs the retained
//! `StdbPreviousConnection` against the fresh `StdbConnection` by primary key and emits `RowDeleted`
//! for every row that is gone (`old ∉ new`) — the Ghost rows a reconnect Snapshot never signals.
//! Updates and genuine inserts come in later slices; here the diff emits **deletes only**.
//!
//! Each test puts the world directly in the post-reconnect fence state — the baseline and the fresh
//! connection present their caches through the slice-3 `FakeDbContext` seam, the status is
//! `Connected`, and there are no subscriptions (so `is_subscriptions_settled` is true) — then a
//! single `update` runs the fence and the per-table diff.

use bevy::prelude::*;
use stdb_bevy::__sdk::DbContext;
use stdb_bevy::test_support::{CannedDriver, FakeDbContext, FakeTable};
use stdb_bevy::{
    RowDeleted, StdbConnection, StdbPlugin, StdbPreviousConnection, StdbStatus, StdbSystemSet,
    TableRegistration,
};

#[derive(Clone, PartialEq, Debug)]
struct Player {
    id: u32,
    name: String,
}

fn player(id: u32, name: &str) -> Player {
    Player {
        id,
        name: name.to_string(),
    }
}

/// Stand-in DbView with a `player()` accessor, mirroring a generated `RemoteTables`. The diff reads
/// rows via `conn.db().player().iter()`.
#[derive(Clone)]
struct GameDb {
    players: Vec<Player>,
}

impl GameDb {
    fn player(&self) -> FakeTable<Player> {
        FakeTable::with_rows(self.players.clone())
    }
}

type Conn = FakeDbContext<GameDb>;

fn conn(players: Vec<Player>) -> Conn {
    FakeDbContext::new(GameDb { players })
}

#[derive(Resource, Default)]
struct Deletes(Vec<Player>);

fn capture_deletes(mut reader: MessageReader<RowDeleted<Player>>, mut out: ResMut<Deletes>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}

/// Build a fence-state app: the `player` table registered with a PK diff, the baseline holding
/// `old`, the live connection holding `new`, status `Connected`, no subscriptions. One `update` then
/// runs the fence.
fn fence_app(old: Vec<Player>, new: Vec<Player>) -> App {
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn(vec![])))
            .add_tables([TableRegistration::pk(|c| c.db().player(), |p| p.id)]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<Deletes>();
    app.add_systems(Update, capture_deletes.in_set(StdbSystemSet::Main));

    app.insert_resource(StdbPreviousConnection(conn(old)));
    app.insert_resource(StdbConnection(conn(new)));
    app.insert_resource(StdbStatus::Connected);
    app
}

/// Deletes captured this run, sorted by id (the diff's map iteration order is unspecified).
fn deletes_sorted(app: &App) -> Vec<Player> {
    let mut got = app.world().resource::<Deletes>().0.clone();
    got.sort_by_key(|p| p.id);
    got
}

#[test]
fn ghost_row_is_deleted_at_the_fence() {
    // B was deleted server-side during the outage; the reconnect Snapshot re-delivers only A.
    let mut app = fence_app(vec![player(1, "a"), player(2, "b")], vec![player(1, "a")]);

    app.update();

    assert_eq!(
        deletes_sorted(&app),
        vec![player(2, "b")],
        "a row gone from the fresh snapshot is a ghost and must be deleted, with its full old body",
    );
}

#[test]
fn survivors_are_not_deleted() {
    let mut app = fence_app(
        vec![player(1, "a"), player(2, "b")],
        vec![player(1, "a"), player(2, "b")],
    );

    app.update();

    assert!(
        deletes_sorted(&app).is_empty(),
        "rows present in both the baseline and the fresh snapshot are survivors, never ghosts",
    );
}

#[test]
fn a_narrowed_subscription_deletes_every_ghost() {
    // The subscription narrowed during the outage: the fresh snapshot matches nothing.
    let mut app = fence_app(vec![player(1, "a"), player(2, "b")], vec![]);

    app.update();

    assert_eq!(
        deletes_sorted(&app),
        vec![player(1, "a"), player(2, "b")],
        "an empty fresh snapshot makes every retained row a ghost, so all are deleted",
    );
}

#[test]
fn a_new_row_is_not_deleted() {
    // C appeared during the outage (a genuine insert); only deletes are emitted this slice.
    let mut app = fence_app(vec![player(1, "a")], vec![player(1, "a"), player(3, "c")]);

    app.update();

    assert!(
        deletes_sorted(&app).is_empty(),
        "a row new to the fresh snapshot is an insert, not a ghost — it must not be deleted",
    );
}
