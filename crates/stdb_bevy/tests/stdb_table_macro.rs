//! End-to-end: the `stdb_table!` macro forms, driven through the bridge's *public* API only.
//!
//! `stdb_table!(accessor => Row, key = <field>)` registers a PK table (`TableRegistration::pk`),
//! the only form there is; an optional `[..]` list narrows which events the table surfaces. A
//! `FakeDbContext` plays the connection; a hand-written `GameDb` DbView exposes the table accessors
//! returning a `FakeTable` — the same shape a generated `RemoteTables` has, so the macro body runs
//! unchanged. The connection type is named **zero times**: `C` is inferred backward from
//! `add_tables`'s `[TableRegistration<Cd::Conn>; N]`.

use bevy::prelude::*;
use stdb_bevy::test_support::{CannedDriver, FakeDbContext, FakeTable};
use stdb_bevy::{
    RowDeleted, RowInserted, RowUpdated, StdbConnect, StdbConnection, StdbPlugin,
    StdbPreviousConnection, StdbStatus, StdbSystemSet, stdb_table,
};

#[derive(Clone, PartialEq, Debug)]
struct Widget {
    id: u32,
}

#[derive(Clone, PartialEq, Debug)]
struct Gadget {
    id: u32,
}

/// Canned contents for one table, materialized into a fresh `FakeTable` on each accessor call (the
/// SDK accessor likewise returns a fresh handle per call). `rows` is the cache the diff reads; the
/// callback fields drive the live forwarder.
#[derive(Clone)]
struct Canned<R> {
    rows: Vec<R>,
    inserts: Vec<R>,
    updates: Vec<(R, R)>,
    deletes: Vec<R>,
}

impl<R> Default for Canned<R> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            inserts: Vec::new(),
            updates: Vec::new(),
            deletes: Vec::new(),
        }
    }
}

impl<R: Clone + 'static> Canned<R> {
    fn table(&self) -> FakeTable<R> {
        FakeTable {
            rows: self.rows.clone(),
            inserts: self.inserts.clone(),
            updates: self.updates.clone(),
            deletes: self.deletes.clone(),
        }
    }
}

/// Stand-in for the generated `RemoteTables`: a DbView the macro reaches via `conn.db().<table>()`.
#[derive(Clone, Default)]
struct GameDb {
    widget: Canned<Widget>,
    gadget: Canned<Gadget>,
}

impl GameDb {
    fn widget(&self) -> FakeTable<Widget> {
        self.widget.table()
    }
    fn gadget(&self) -> FakeTable<Gadget> {
        self.gadget.table()
    }
}

type Conn = FakeDbContext<GameDb>;

#[derive(Resource, Default)]
struct WidgetInserts(Vec<Widget>);
#[derive(Resource, Default)]
struct WidgetUpdates(Vec<(Widget, Widget)>);
#[derive(Resource, Default)]
struct WidgetDeletes(Vec<Widget>);
#[derive(Resource, Default)]
struct GadgetInserts(Vec<Gadget>);

fn capture_widget_inserts(
    mut reader: MessageReader<RowInserted<Widget>>,
    mut out: ResMut<WidgetInserts>,
) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}
fn capture_widget_updates(
    mut reader: MessageReader<RowUpdated<Widget>>,
    mut out: ResMut<WidgetUpdates>,
) {
    for msg in reader.read() {
        out.0.push((msg.old.clone(), msg.new.clone()));
    }
}
fn capture_widget_deletes(
    mut reader: MessageReader<RowDeleted<Widget>>,
    mut out: ResMut<WidgetDeletes>,
) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}
fn capture_gadget_inserts(
    mut reader: MessageReader<RowInserted<Gadget>>,
    mut out: ResMut<GadgetInserts>,
) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}
/// The `key =` form wires the full forwarder: on connect, a dumped insert, update, and delete each
/// surface one message (no resync window on a first connect, so nothing is suppressed).
#[test]
fn macro_key_form_forwards_insert_update_delete() {
    let conn: Conn = FakeDbContext::new(GameDb {
        widget: Canned {
            inserts: vec![Widget { id: 1 }],
            updates: vec![(Widget { id: 1 }, Widget { id: 2 })],
            deletes: vec![Widget { id: 3 }],
            ..default()
        },
        ..default()
    });

    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn))
            .add_tables([stdb_table!(widget => Widget, key = id)]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetInserts>();
    app.init_resource::<WidgetUpdates>();
    app.init_resource::<WidgetDeletes>();
    app.add_systems(
        Update,
        (
            capture_widget_inserts,
            capture_widget_updates,
            capture_widget_deletes,
        )
            .in_set(StdbSystemSet::Main),
    );

    app.world_mut().trigger(StdbConnect);
    app.update();

    assert_eq!(
        app.world().resource::<WidgetInserts>().0,
        vec![Widget { id: 1 }],
        "the key form must wire on_insert",
    );
    assert_eq!(
        app.world().resource::<WidgetUpdates>().0,
        vec![(Widget { id: 1 }, Widget { id: 2 })],
        "the key form must wire on_update",
    );
    assert_eq!(
        app.world().resource::<WidgetDeletes>().0,
        vec![Widget { id: 3 }],
        "the key form must wire on_delete",
    );
}

/// The headline ergonomic: many heterogeneous tables declared in one `add_tables([..])`, the
/// connection type named **zero times** — `C` is inferred backward from `Cd::Conn`.
#[test]
fn add_tables_takes_many_macro_tables_without_naming_the_connection_type() {
    let conn: Conn = FakeDbContext::new(GameDb {
        widget: Canned {
            inserts: vec![Widget { id: 1 }],
            ..default()
        },
        gadget: Canned {
            inserts: vec![Gadget { id: 2 }],
            ..default()
        },
    });

    let mut app = App::new();
    app.add_plugins(StdbPlugin::connection(CannedDriver::new(conn)).add_tables([
        stdb_table!(widget => Widget, key = id),
        stdb_table!(gadget => Gadget, key = id),
    ]));
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetInserts>();
    app.init_resource::<GadgetInserts>();
    app.add_systems(
        Update,
        (capture_widget_inserts, capture_gadget_inserts).in_set(StdbSystemSet::Main),
    );

    app.world_mut().trigger(StdbConnect);
    app.update();

    assert_eq!(
        app.world().resource::<WidgetInserts>().0,
        vec![Widget { id: 1 }],
        "the first macro-declared table forwards on connect",
    );
    assert_eq!(
        app.world().resource::<GadgetInserts>().0,
        vec![Gadget { id: 2 }],
        "every other macro-declared table forwards on connect too",
    );
}

/// The `key =` form wires the resync diff too, using the **generated key extractor**: at the fence
/// a row gone from the fresh cache (`old ∉ new`, correlated by `id`) surfaces as a ghost delete. A
/// wrong-field extractor would not show in the forward-only tests above.
#[test]
fn macro_key_form_emits_a_ghost_delete_at_the_fence() {
    let old: Conn = FakeDbContext::new(GameDb {
        widget: Canned {
            rows: vec![Widget { id: 1 }, Widget { id: 2 }],
            ..default()
        },
        ..default()
    });
    let new: Conn = FakeDbContext::new(GameDb {
        widget: Canned {
            rows: vec![Widget { id: 1 }],
            ..default()
        },
        ..default()
    });

    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(FakeDbContext::new(GameDb::default())))
            .add_tables([stdb_table!(widget => Widget, key = id)]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetDeletes>();
    app.add_systems(Update, capture_widget_deletes.in_set(StdbSystemSet::Main));

    // Post-reconnect fence state: baseline holds {1,2}, the fresh snapshot only {1}.
    app.insert_resource(StdbPreviousConnection(old));
    app.insert_resource(StdbConnection(new));
    app.insert_resource(StdbStatus::Connected);

    app.update();

    assert_eq!(
        app.world().resource::<WidgetDeletes>().0,
        vec![Widget { id: 2 }],
        "the macro's key extractor must correlate by id, so the row gone from the snapshot is a \
         ghost delete",
    );
}

/// `stdb_table!(widget => Widget, key = id, [insert, delete])` — the PK selection form wires only the
/// listed callbacks. The fake has an update queued, but with update deselected no `RowUpdated` must
/// surface.
#[test]
fn macro_pk_selection_drops_updates() {
    let conn: Conn = FakeDbContext::new(GameDb {
        widget: Canned {
            inserts: vec![Widget { id: 1 }],
            updates: vec![(Widget { id: 1 }, Widget { id: 2 })],
            deletes: vec![Widget { id: 3 }],
            ..default()
        },
        ..default()
    });

    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn))
            .add_tables([stdb_table!(widget => Widget, key = id, [insert, delete])]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetInserts>();
    app.init_resource::<WidgetUpdates>();
    app.init_resource::<WidgetDeletes>();
    app.add_systems(
        Update,
        (
            capture_widget_inserts,
            capture_widget_updates,
            capture_widget_deletes,
        )
            .in_set(StdbSystemSet::Main),
    );

    app.world_mut().trigger(StdbConnect);
    app.update();

    assert_eq!(
        app.world().resource::<WidgetInserts>().0,
        vec![Widget { id: 1 }],
        "the [insert, delete] selection wires on_insert",
    );
    assert_eq!(
        app.world().resource::<WidgetDeletes>().0,
        vec![Widget { id: 3 }],
        "the [insert, delete] selection wires on_delete",
    );
    assert!(
        app.world().resource::<WidgetUpdates>().0.is_empty(),
        "with update deselected, a PK table wires no on_update — no RowUpdated surfaces",
    );
}
