//! End-to-end: the reconnect resync diff for a **keyless** table, through the public API only.
//!
//! A keyless table has no primary key, so identity is the whole row's BSATN bytes — the SDK's own
//! refcount-by-bsatn identity. `TableRegistration::non_pk` reproduces it by serialising each row to
//! key the diff: a row changed under the "same" entity has different bytes, so it cannot be
//! correlated — it surfaces as a delete of the old plus an insert of the new, never an update. The
//! fence-state setup mirrors `resync_diff`, with a sats-deriving `Monster` so the BSATN key is real.

use bevy::prelude::*;
use stdb_bevy::__sdk::__codegen::__lib;
use stdb_bevy::__sdk::{DbContext, Table};
use stdb_bevy::test_support::{CannedDriver, FakeDbContext};
use stdb_bevy::{
    KeylessMessagesMask, RowDeleted, RowInserted, RowUpdated, StdbConnection, StdbPlugin,
    StdbPreviousConnection, StdbStatus, StdbSystemSet, TableRegistration,
};

#[derive(Clone, PartialEq, Debug, __lib::ser::Serialize)]
#[sats(crate = __lib)]
struct Monster {
    id: u32,
    name: String,
}

fn monster(id: u32, name: &str) -> Monster {
    Monster {
        id,
        name: name.to_string(),
    }
}

/// A keyless table double: impls only `Table`, never `TableWithPrimaryKey` — a real no-PK handle is
/// `Table`-only, so this is the faithful stand-in. It also makes `forward_keyless` load-bearing:
/// `forward` (PK-bounded, since it may wire `on_update`) would not compile against this type. Only
/// `iter()` carries meaning here; the diff reads the cache, and the live forward path is not driven.
struct KeylessTable<R> {
    rows: Vec<R>,
}

impl<R: 'static + Clone> Table for KeylessTable<R> {
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
    fn on_insert(&self, _cb: impl FnMut(&(), &R) + Send + 'static) -> Self::InsertCallbackId {}
    fn remove_on_insert(&self, _id: Self::InsertCallbackId) {}
    fn on_delete(&self, _cb: impl FnMut(&(), &R) + Send + 'static) -> Self::DeleteCallbackId {}
    fn remove_on_delete(&self, _id: Self::DeleteCallbackId) {}
}

/// Stand-in DbView with a `monster()` accessor, mirroring a generated `RemoteTables`. The diff reads
/// rows via `conn.db().monster().iter()`.
#[derive(Clone)]
struct GameDb {
    monsters: Vec<Monster>,
}

impl GameDb {
    fn monster(&self) -> KeylessTable<Monster> {
        KeylessTable {
            rows: self.monsters.clone(),
        }
    }
}

type Conn = FakeDbContext<GameDb>;

fn conn(monsters: Vec<Monster>) -> Conn {
    FakeDbContext::new(GameDb { monsters })
}

#[derive(Resource, Default)]
struct Deletes(Vec<Monster>);
#[derive(Resource, Default)]
struct Inserts(Vec<Monster>);
#[derive(Resource, Default)]
struct Updates(Vec<(Monster, Monster)>);

fn capture_deletes(mut reader: MessageReader<RowDeleted<Monster>>, mut out: ResMut<Deletes>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}
fn capture_inserts(mut reader: MessageReader<RowInserted<Monster>>, mut out: ResMut<Inserts>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}
fn capture_updates(mut reader: MessageReader<RowUpdated<Monster>>, mut out: ResMut<Updates>) {
    for msg in reader.read() {
        out.0.push((msg.old.clone(), msg.new.clone()));
    }
}

/// Build a fence-state app: the `monster` keyless table registered via `non_pk` (a BSATN-keyed
/// diff), the baseline holding `old`, the live connection holding `new`, status `Connected`, no
/// subscriptions. One `update` then runs the fence.
fn fence_app_keyless(old: Vec<Monster>, new: Vec<Monster>, emit: KeylessMessagesMask) -> App {
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn(vec![]))).add_tables([
            // Raw `non_pk` (no macro), so a direct break can't hide behind the macro path.
            TableRegistration::non_pk(
                |conn, fwd| fwd.forward_keyless(&conn.db().monster()),
                |c| c.db().monster().iter().collect(),
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

/// Captured rows this run, sorted by id (the diff's iteration order is unspecified).
fn deletes_sorted(app: &App) -> Vec<Monster> {
    let mut got = app.world().resource::<Deletes>().0.clone();
    got.sort_by_key(|m| m.id);
    got
}

fn inserts_sorted(app: &App) -> Vec<Monster> {
    let mut got = app.world().resource::<Inserts>().0.clone();
    got.sort_by_key(|m| m.id);
    got
}

fn updates_sorted(app: &App) -> Vec<(Monster, Monster)> {
    let mut got = app.world().resource::<Updates>().0.clone();
    got.sort_by_key(|(_, new)| new.id);
    got
}

#[test]
fn keyless_change_surfaces_as_delete_plus_insert() {
    // "b" → "B2" under the same id serialises to different bytes, so the keyless diff cannot
    // correlate it: the old bytes are a ghost, the new bytes are a fresh row.
    let mut app = fence_app_keyless(
        vec![monster(1, "b")],
        vec![monster(1, "B2")],
        KeylessMessagesMask::INSERT_DELETE,
    );

    app.update();

    assert_eq!(
        deletes_sorted(&app),
        vec![monster(1, "b")],
        "the old bytes are gone from the fresh snapshot, so they are a ghost delete",
    );
    assert_eq!(
        inserts_sorted(&app),
        vec![monster(1, "B2")],
        "the changed bytes are new to the fresh snapshot, so they are an insert",
    );
    assert!(
        updates_sorted(&app).is_empty(),
        "a keyless table has no identity to update — a change is always delete + insert",
    );
}

#[test]
fn keyless_identical_row_is_silent() {
    // Same bytes in both caches: the BSATN key matches, so the row is a survivor.
    let mut app = fence_app_keyless(
        vec![monster(1, "a")],
        vec![monster(1, "a")],
        KeylessMessagesMask::INSERT_DELETE,
    );

    app.update();

    assert!(
        deletes_sorted(&app).is_empty()
            && inserts_sorted(&app).is_empty()
            && updates_sorted(&app).is_empty(),
        "an identical row serialises to the same BSATN key — it is a survivor and emits nothing",
    );
}

#[test]
fn keyless_selection_drops_unselected() {
    // Registered for deletes but not inserts: a ghost still deletes, a genuinely new row is dropped.
    let mut app = fence_app_keyless(
        vec![monster(1, "a"), monster(2, "ghost")],
        vec![monster(1, "a"), monster(3, "fresh")],
        KeylessMessagesMask {
            insert: false,
            delete: true,
        },
    );

    app.update();

    assert_eq!(
        deletes_sorted(&app),
        vec![monster(2, "ghost")],
        "the ghost still deletes — delete is selected, and the diff did run",
    );
    assert!(
        inserts_sorted(&app).is_empty(),
        "with insert deselected, the row new to the snapshot surfaces no RowInserted",
    );
}
