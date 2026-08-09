//! End-to-end: row-drain suppression during the resync window, through the public API only.
//!
//! On every (re)connect the SDK re-delivers the whole Snapshot as `on_insert` callbacks. Slice 7:
//! while the resync window is open the bridge must **discard** those re-fired inserts (the fence
//! diff is the sole source of truth), so a row that merely survived the outage does not re-surface
//! as `RowInserted` — the double-count / ghost bug Resync fixes.
//!
//! Driven through a real connect → disconnect → reconnect cycle. The `player` table's `FakeTable`
//! carries both `rows` (the cache the diff reads) and `inserts` (re-fired by `on_insert` on each
//! (re)connect, the Snapshot re-delivery). `CannedDriver` hands a fresh clone per connect, so the
//! baseline and the reconnected connection are independent.

use bevy::prelude::*;
use stdb_bevy::test_support::{CannedDriver, FakeDbContext, FakeTable};
use stdb_bevy::{DbAccess, RowCollection};
use stdb_bevy::{
    RowInserted, RowMessagesMask, StdbConnect, StdbDisconnect, StdbPlugin, StdbSystemSet,
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

/// DbView whose `player()` table both **re-fires** its rows through `on_insert` (the Snapshot
/// re-delivery) and **presents** them through `rows()` (the cache the diff reads).
#[derive(Clone)]
struct GameDb {
    players: Vec<Player>,
}

impl GameDb {
    fn player(&self) -> FakeTable<Player> {
        FakeTable {
            rows: self.players.clone(),
            inserts: self.players.clone(),
            updates: vec![],
            deletes: vec![],
        }
    }
}

fn conn(players: Vec<Player>) -> FakeDbContext<GameDb> {
    FakeDbContext::new(GameDb { players })
}

#[derive(Resource, Default)]
struct Inserts(Vec<Player>);

fn capture_inserts(mut reader: MessageReader<RowInserted<Player>>, mut out: ResMut<Inserts>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}

/// A connection-only bridge with the `player` PK table registered, capturing every `RowInserted`.
fn app(players: Vec<Player>) -> App {
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn(players))).add_tables([
            // Raw `pk` (no macro), so a direct break can't hide behind the macro path.
            TableRegistration::pk(
                |conn, fwd| fwd.forward(&conn.db().player()),
                |c| c.db().player().rows(),
                |p| p.id,
                RowMessagesMask::ALL,
                "player",
            ),
        ]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<Inserts>();
    app.add_systems(Update, capture_inserts.in_set(StdbSystemSet::Main));
    app
}

fn inserts(app: &App) -> Vec<Player> {
    app.world().resource::<Inserts>().0.clone()
}

#[test]
fn the_first_connect_emits_inserts_normally() {
    let mut app = app(vec![player(1, "a")]);

    app.world_mut().trigger(StdbConnect);
    app.update();

    assert_eq!(
        inserts(&app),
        vec![player(1, "a")],
        "a first connect has no baseline, so the window is closed and inserts flow normally",
    );
}

#[test]
fn a_survivor_does_not_refire_as_insert_across_reconnect() {
    let mut app = app(vec![player(1, "a")]);

    app.world_mut().trigger(StdbConnect);
    app.update();
    assert_eq!(
        inserts(&app).len(),
        1,
        "precondition: A inserted once on connect"
    );

    // Drop, then reconnect: A's snapshot re-fire must be suppressed and the diff sees no change.
    app.world_mut().trigger(StdbDisconnect);
    app.update();
    app.world_mut().trigger(StdbConnect);
    app.update();

    assert_eq!(
        inserts(&app),
        vec![player(1, "a")],
        "a surviving row's re-fired snapshot insert is suppressed — A is inserted exactly once \
         across the reconnect, not twice",
    );
}

#[test]
fn suppressed_refires_do_not_leak_after_the_window_closes() {
    let mut app = app(vec![player(1, "a")]);

    app.world_mut().trigger(StdbConnect);
    app.update();
    app.world_mut().trigger(StdbDisconnect);
    app.update();
    app.world_mut().trigger(StdbConnect);
    app.update();

    // One more frame after the fence closed the window: the discarded re-fires must not resurface.
    app.update();

    assert_eq!(
        inserts(&app),
        vec![player(1, "a")],
        "suppression must drain-and-discard, not skip — a skipped channel would leak the re-fired \
         inserts once the window closes",
    );
}
