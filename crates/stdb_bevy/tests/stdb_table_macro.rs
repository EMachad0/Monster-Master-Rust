//! End-to-end: the `stdb_table!` macro, driven through the bridge's *public* API only.
//!
//! `stdb_table!(widget => Widget)` expands to a `TableRegistration` whose registrar calls
//! `conn.db().widget()`. Here a `FakeDbContext` plays the connection and a hand-written `GameDb`
//! DbView exposes `widget()` / `gadget()` accessors returning a `FakeTable` of canned rows — the
//! same shape a generated Bindings `RemoteTables` has, so the macro body runs unchanged.
//!
//! The headline property under test: the macro names **no connection type**. `C` is inferred
//! backward from `add_tables`'s `[TableRegistration<Cd::Conn>; N]` (which is why `add_tables` takes
//! a concrete array, not `impl IntoIterator`).

use bevy::prelude::*;
use stdb_bevy::test_support::{CannedDriver, FakeDbContext, FakeTable};
use stdb_bevy::{
    RowDeleted, RowInserted, RowUpdated, StdbConnect, StdbPlugin, StdbSystemSet, stdb_table,
};

#[derive(Clone, PartialEq, Debug)]
struct Widget {
    id: u32,
}

#[derive(Clone, PartialEq, Debug)]
struct Gadget {
    id: u32,
}

/// Canned rows for one table, materialized into a fresh `FakeTable` on each accessor call (the SDK
/// accessor likewise returns a fresh handle per call).
#[derive(Clone)]
struct Canned<R> {
    inserts: Vec<R>,
    updates: Vec<(R, R)>,
    deletes: Vec<R>,
}

impl<R> Default for Canned<R> {
    fn default() -> Self {
        Self {
            inserts: Vec::new(),
            updates: Vec::new(),
            deletes: Vec::new(),
        }
    }
}

impl<R: Clone + 'static> Canned<R> {
    fn table(&self) -> FakeTable<R> {
        FakeTable {
            rows: vec![],
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

/// `stdb_table!(widget => Widget)` (no callback list) wires all three callbacks: a connect that
/// dumps an insert, an update, and a delete surfaces one message of each.
#[test]
fn macro_all_callbacks_forwards_insert_update_delete() {
    let conn: Conn = FakeDbContext::new(GameDb {
        widget: Canned {
            inserts: vec![Widget { id: 1 }],
            updates: vec![(Widget { id: 1 }, Widget { id: 2 })],
            deletes: vec![Widget { id: 3 }],
        },
        ..default()
    });

    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn)).add_tables([stdb_table!(widget => Widget)]),
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
    app.update();

    assert_eq!(
        app.world().resource::<WidgetInserts>().0,
        vec![Widget { id: 1 }],
        "no-list form must wire on_insert",
    );
    assert_eq!(
        app.world().resource::<WidgetUpdates>().0,
        vec![(Widget { id: 1 }, Widget { id: 2 })],
        "no-list form must wire on_update",
    );
    assert_eq!(
        app.world().resource::<WidgetDeletes>().0,
        vec![Widget { id: 3 }],
        "no-list form must wire on_delete",
    );
}

/// `stdb_table!(widget => Widget, [insert, delete])` wires only the listed callbacks. The fake has
/// an update queued, but no `RowUpdated` must surface — also the shape a no-PK table uses, since the
/// SDK only offers `on_update` on primary-key tables.
#[test]
fn macro_selection_wires_only_listed_callbacks() {
    let conn: Conn = FakeDbContext::new(GameDb {
        widget: Canned {
            inserts: vec![Widget { id: 1 }],
            updates: vec![(Widget { id: 1 }, Widget { id: 2 })],
            deletes: vec![Widget { id: 3 }],
        },
        ..default()
    });

    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn))
            .add_tables([stdb_table!(widget => Widget, [insert, delete])]),
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
    app.update();

    assert_eq!(
        app.world().resource::<WidgetInserts>().0,
        vec![Widget { id: 1 }],
        "[insert, delete] must wire on_insert",
    );
    assert_eq!(
        app.world().resource::<WidgetDeletes>().0,
        vec![Widget { id: 3 }],
        "[insert, delete] must wire on_delete",
    );
    assert!(
        app.world().resource::<WidgetUpdates>().0.is_empty(),
        "[insert, delete] must NOT wire on_update, so no RowUpdated surfaces",
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
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(conn))
            .add_tables([stdb_table!(widget => Widget), stdb_table!(gadget => Gadget)]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetInserts>();
    app.init_resource::<GadgetInserts>();
    app.add_systems(
        Update,
        (capture_widget_inserts, capture_gadget_inserts).in_set(StdbSystemSet::Main),
    );

    app.world_mut().trigger(StdbConnect);
    app.update();
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
