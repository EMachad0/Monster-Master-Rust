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
use stdb_bevy::__sdk::{DbContext, Table};
use stdb_bevy::test_support::{CannedDriver, FakeDbContext, FakeTable};
use stdb_bevy::{
    RowDeleted, RowInserted, RowMessagesMask, RowUpdated, StdbConnection, StdbPlugin,
    StdbPreviousConnection, StdbStatus, StdbSystemSet, TableRegistration,
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
#[derive(Resource, Default)]
struct Inserts(Vec<Player>);
#[derive(Resource, Default)]
struct Updates(Vec<(Player, Player)>);

fn capture_deletes(mut reader: MessageReader<RowDeleted<Player>>, mut out: ResMut<Deletes>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}
fn capture_inserts(mut reader: MessageReader<RowInserted<Player>>, mut out: ResMut<Inserts>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}
fn capture_updates(mut reader: MessageReader<RowUpdated<Player>>, mut out: ResMut<Updates>) {
    for msg in reader.read() {
        out.0.push((msg.old.clone(), msg.new.clone()));
    }
}

/// Build a fence-state app: the `player` table registered with a PK diff, the baseline holding
/// `old`, the live connection holding `new`, status `Connected`, no subscriptions. One `update` then
/// runs the fence.
fn fence_app(old: Vec<Player>, new: Vec<Player>) -> App {
    // The default selection surfaces every event — the behavior the diff has always had.
    fence_app_emitting(old, new, RowMessagesMask::ALL)
}

/// Like `fence_app`, but registers the `player` PK table with an explicit `RowMessagesMask`, so a
/// test can pin which diff branches that selection gates.
fn fence_app_emitting(old: Vec<Player>, new: Vec<Player>, emit: RowMessagesMask) -> App {
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn(vec![]))).add_tables([
            // Raw `pk` (no macro), so a direct break can't hide behind the macro path.
            TableRegistration::pk(
                |conn, fwd| fwd.forward(&conn.db().player()),
                |c| c.db().player().iter().collect(),
                |p| p.id,
                emit,
            ),
        ]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<Deletes>();
    app.init_resource::<Inserts>();
    app.init_resource::<Updates>();
    app.add_systems(
        Update,
        (capture_deletes, capture_inserts, capture_updates).in_set(StdbSystemSet::Main),
    );

    app.insert_resource(StdbPreviousConnection(conn(old)));
    app.insert_resource(StdbConnection(conn(new)));
    app.insert_resource(StdbStatus::Connected);
    app
}

/// Deletes captured this run, sorted by id (the diff's iteration order is unspecified).
fn deletes_sorted(app: &App) -> Vec<Player> {
    let mut got = app.world().resource::<Deletes>().0.clone();
    got.sort_by_key(|p| p.id);
    got
}

fn inserts_sorted(app: &App) -> Vec<Player> {
    let mut got = app.world().resource::<Inserts>().0.clone();
    got.sort_by_key(|p| p.id);
    got
}

fn updates_sorted(app: &App) -> Vec<(Player, Player)> {
    let mut got = app.world().resource::<Updates>().0.clone();
    got.sort_by_key(|(_, new)| new.id);
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
    // C appeared during the outage (a genuine insert); it must not surface as a ghost delete.
    let mut app = fence_app(vec![player(1, "a")], vec![player(1, "a"), player(3, "c")]);

    app.update();

    assert!(
        deletes_sorted(&app).is_empty(),
        "a row new to the fresh snapshot is an insert, not a ghost — it must not be deleted",
    );
}

#[test]
fn a_changed_row_emits_an_update_with_both_bodies() {
    // Same key, changed body: a row updated server-side during the outage.
    let mut app = fence_app(vec![player(1, "old")], vec![player(1, "new")]);

    app.update();

    assert_eq!(
        updates_sorted(&app),
        vec![(player(1, "old"), player(1, "new"))],
        "a row whose body changed under the same key is an update carrying old (from the baseline) \
         and new (from the fresh snapshot)",
    );
    assert!(
        deletes_sorted(&app).is_empty() && inserts_sorted(&app).is_empty(),
        "a changed row is an update only — never a delete or insert",
    );
}

#[test]
fn an_unchanged_row_emits_nothing() {
    let mut app = fence_app(vec![player(1, "a")], vec![player(1, "a")]);

    app.update();

    assert!(
        updates_sorted(&app).is_empty()
            && deletes_sorted(&app).is_empty()
            && inserts_sorted(&app).is_empty(),
        "a row identical in both caches is unchanged — it must emit no message at all",
    );
}

#[test]
fn only_the_changed_row_updates() {
    // A unchanged, B changed.
    let mut app = fence_app(
        vec![player(1, "a"), player(2, "b")],
        vec![player(1, "a"), player(2, "b2")],
    );

    app.update();

    assert_eq!(
        updates_sorted(&app),
        vec![(player(2, "b"), player(2, "b2"))],
        "only the row whose body changed updates; the identical row emits nothing",
    );
}

#[test]
fn a_new_row_emits_an_insert() {
    // C appeared server-side during the outage.
    let mut app = fence_app(vec![player(1, "a")], vec![player(1, "a"), player(3, "c")]);

    app.update();

    assert_eq!(
        inserts_sorted(&app),
        vec![player(3, "c")],
        "a row present only in the fresh snapshot is a genuine insert",
    );
    assert!(
        deletes_sorted(&app).is_empty(),
        "a genuine insert must not also surface as a delete",
    );
}

#[test]
fn a_widened_subscription_inserts_everything() {
    // The subscription widened during the outage: the baseline was empty, the snapshot brings rows.
    let mut app = fence_app(vec![], vec![player(1, "a"), player(2, "b")]);

    app.update();

    assert_eq!(
        inserts_sorted(&app),
        vec![player(1, "a"), player(2, "b")],
        "every row in the fresh snapshot but absent from the baseline is an insert",
    );
}

#[test]
fn update_selection_off_drops_a_same_key_change() {
    // A same-key body change during the outage, alongside a ghost.
    let mut app = fence_app_emitting(
        vec![player(1, "old"), player(2, "ghost")],
        vec![player(1, "new")],
        RowMessagesMask {
            insert: true,
            update: false,
            delete: true,
        },
    );

    app.update();

    assert!(
        updates_sorted(&app).is_empty(),
        "with update deselected, a same-key body change surfaces no RowUpdated",
    );
    assert_eq!(
        deletes_sorted(&app),
        vec![player(2, "ghost")],
        "the ghost still deletes — the selection gates only updates, and the diff did run",
    );
}

#[test]
fn insert_selection_off_drops_a_new_row() {
    // A same-key body change during the outage, alongside a genuine new row.
    let mut app = fence_app_emitting(
        vec![player(1, "old")],
        vec![player(1, "new"), player(3, "fresh")],
        RowMessagesMask {
            insert: false,
            update: true,
            delete: true,
        },
    );

    app.update();

    assert!(
        inserts_sorted(&app).is_empty(),
        "with insert deselected, a row new to the snapshot surfaces no RowInserted",
    );
    assert_eq!(
        updates_sorted(&app),
        vec![(player(1, "old"), player(1, "new"))],
        "the same-key change still updates — the selection gates only inserts, and the diff did run",
    );
}

#[test]
fn delete_selection_off_keeps_a_ghost() {
    // A ghost (deleted during the outage), alongside a genuine new row.
    let mut app = fence_app_emitting(
        vec![player(1, "a"), player(2, "ghost")],
        vec![player(1, "a"), player(3, "fresh")],
        RowMessagesMask {
            insert: true,
            update: true,
            delete: false,
        },
    );

    app.update();

    assert!(
        deletes_sorted(&app).is_empty(),
        "with delete deselected, a ghost gone from the snapshot surfaces no RowDeleted",
    );
    assert_eq!(
        inserts_sorted(&app),
        vec![player(3, "fresh")],
        "the new row still inserts — the selection gates only deletes, and the diff did run",
    );
}
