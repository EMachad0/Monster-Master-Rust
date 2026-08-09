//! End-to-end: the `stdb_table!` macro forms, driven through the *public* API of both bridge crates.
//!
//! `stdb_table!(accessor => Row, key = <field>)` registers a PK table (`TableRegistration::pk`),
//! the only form there is; an optional `[..]` list narrows which events the table surfaces. The
//! macro expands to SDK-shaped calls, so the doubles here are SDK-shaped too: `CachedTable` wears
//! the SDK's capability traits the way a generated handle does, and `CachedConnection` is the
//! `DbContext` whose DbView exposes the accessors. That also exercises the `SdkTable` adapters the
//! expansion wraps each handle in. The connection type is named **zero times** in the registration:
//! `C` is inferred backward from `add_tables`'s `[TableRegistration<Cd::Conn>; N]`.

use bevy::prelude::*;
use spacetimedb_sdk::table::{TableLike, WithDelete, WithInsert, WithUpdate};
use spacetimedb_sdk::{ConnectionId, DbContext, Identity};
use stdb_bevy::test_support::CannedDriver;
use stdb_bevy::{
    RowDeleted, RowInserted, RowUpdated, StdbConnect, StdbConnection, StdbPlugin,
    StdbPreviousConnection, StdbStatus, StdbSystemSet,
};
use stdb_bevy_sdk::stdb_table;

#[derive(Clone, PartialEq, Debug)]
struct Widget {
    id: u32,
}

#[derive(Clone, PartialEq, Debug)]
struct Gadget {
    id: u32,
}

/// Canned contents for one table, materialized into a fresh `CachedTable` on each accessor call
/// (the generated accessor likewise returns a fresh handle per call). `rows` is the client cache
/// the diff reads; the callback fields drive the live forwarder.
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
    fn table(&self) -> CachedTable<R> {
        CachedTable {
            rows: self.rows.clone(),
            inserts: self.inserts.clone(),
            updates: self.updates.clone(),
            deletes: self.deletes.clone(),
        }
    }
}

/// Stand-in for a generated table handle: it presents a client cache through `TableLike` and
/// replays its canned changes the moment a callback is registered. Only the capability traits are
/// implemented, since those are the ones the row-path adapters bind to.
struct CachedTable<R> {
    rows: Vec<R>,
    inserts: Vec<R>,
    updates: Vec<(R, R)>,
    deletes: Vec<R>,
}

impl<R: Clone + 'static> TableLike for CachedTable<R> {
    type Row = R;
    // The transaction context the SDK hands every callback, which the row path never reads.
    type EventContext = ();

    fn count(&self) -> u64 {
        self.rows.len() as u64
    }

    fn iter(&self) -> impl Iterator<Item = R> + '_ {
        self.rows.iter().cloned()
    }
}

impl<R: Clone + 'static> WithInsert for CachedTable<R> {
    // Nothing here de-registers, so the id a real handle mints has no counterpart.
    type InsertCallbackId = ();

    fn on_insert(&self, mut callback: impl FnMut(&(), &R) + Send + 'static) {
        for row in &self.inserts {
            callback(&(), row);
        }
    }

    fn remove_on_insert(&self, _callback: ()) {}
}

impl<R: Clone + 'static> WithDelete for CachedTable<R> {
    type DeleteCallbackId = ();

    fn on_delete(&self, mut callback: impl FnMut(&(), &R) + Send + 'static) {
        for row in &self.deletes {
            callback(&(), row);
        }
    }

    fn remove_on_delete(&self, _callback: ()) {}
}

impl<R: Clone + 'static> WithUpdate for CachedTable<R> {
    type UpdateCallbackId = ();

    fn on_update(&self, mut callback: impl FnMut(&(), &R, &R) + Send + 'static) {
        for (old, new) in &self.updates {
            callback(&(), old, new);
        }
    }

    fn remove_on_update(&self, _callback: ()) {}
}

/// Stand-in for the generated `RemoteTables`: the DbView the macro reaches via
/// `conn.db().<table>()`.
#[derive(Clone, Default)]
struct GameDb {
    widget: Canned<Widget>,
    gadget: Canned<Gadget>,
}

impl GameDb {
    fn widget(&self) -> CachedTable<Widget> {
        self.widget.table()
    }
    fn gadget(&self) -> CachedTable<Gadget> {
        self.gadget.table()
    }
}

/// Stand-in for the generated `DbConnection`: the macro's route to the tables is `DbContext::db`,
/// so a double has to wear that trait. Everything past the DbView is inert, because a table
/// registration touches nothing else on the connection.
#[derive(Clone)]
struct CachedConnection<V> {
    db: V,
}

impl<V> CachedConnection<V> {
    fn new(db: V) -> Self {
        Self { db }
    }
}

impl<V: Send + Sync + 'static> DbContext for CachedConnection<V> {
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
        Ok(())
    }

    fn subscription_builder(&self) {}

    fn try_identity(&self) -> Option<Identity> {
        None
    }

    fn connection_id(&self) -> ConnectionId {
        ConnectionId::from(0u128)
    }

    fn try_connection_id(&self) -> Option<ConnectionId> {
        None
    }
}

type Conn = CachedConnection<GameDb>;

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
    let conn: Conn = CachedConnection::new(GameDb {
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
    let conn: Conn = CachedConnection::new(GameDb {
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
    let old: Conn = CachedConnection::new(GameDb {
        widget: Canned {
            rows: vec![Widget { id: 1 }, Widget { id: 2 }],
            ..default()
        },
        ..default()
    });
    let new: Conn = CachedConnection::new(GameDb {
        widget: Canned {
            rows: vec![Widget { id: 1 }],
            ..default()
        },
        ..default()
    });

    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(CannedDriver::new(CachedConnection::new(GameDb::default())))
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
    let conn: Conn = CachedConnection::new(GameDb {
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
