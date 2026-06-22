//! End-to-end: table registration driven through the bridge's *public* API only.
//!
//! A Game declares tables with `StdbPlugin::add_tables([TableRegistration::new(..)])`, connects, and
//! reads the resulting `RowInserted<T>` messages. The registrar forwards from a `FakeTable` instead
//! of a real `conn.db().player()` accessor; everything else is exactly the public surface a Game uses.

use std::time::Duration;

use bevy::prelude::*;
use stdb_bevy::test_support::{FakeConn, FakeDriver, FakeTable};
use stdb_bevy::{
    Backoff, Jitter, ReconnectPolicy, RowForwarder, RowInserted, StdbConnect, StdbConnection,
    StdbPlugin, StdbStatus, StdbSystemSet, TableRegistration,
};

#[derive(Clone, PartialEq, Debug)]
struct Widget {
    id: u32,
}

#[derive(Clone, PartialEq, Debug)]
struct Gadget {
    id: u32,
}

#[derive(Resource, Default)]
struct WidgetInserts(Vec<Widget>);

#[derive(Resource, Default)]
struct GadgetInserts(Vec<Gadget>);

fn capture_widgets(mut reader: MessageReader<RowInserted<Widget>>, mut out: ResMut<WidgetInserts>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}

fn capture_gadgets(mut reader: MessageReader<RowInserted<Gadget>>, mut out: ResMut<GadgetInserts>) {
    for msg in reader.read() {
        out.0.push(msg.0.clone());
    }
}

// Production writes `|conn, fwd| fwd.forward(conn.db().player())`; here we forward from a fake table.
fn forward_widget(_conn: &StdbConnection<FakeConn>, fwd: RowForwarder<Widget>) {
    fwd.inserts(&FakeTable {
        rows: vec![],
        inserts: vec![Widget { id: 1 }],
        updates: vec![],
        deletes: vec![],
    });
}

fn forward_gadget(_conn: &StdbConnection<FakeConn>, fwd: RowForwarder<Gadget>) {
    fwd.inserts(&FakeTable {
        rows: vec![],
        inserts: vec![Gadget { id: 2 }],
        updates: vec![],
        deletes: vec![],
    });
}

#[test]
fn add_tables_forwards_every_table_on_connect() {
    let mut app = App::new();
    app.add_plugins(StdbPlugin::connection(FakeDriver::default()).add_tables([
        TableRegistration::new(forward_widget),
        TableRegistration::new(forward_gadget),
    ]));
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetInserts>();
    app.init_resource::<GadgetInserts>();
    app.add_systems(
        Update,
        (capture_widgets, capture_gadgets).in_set(StdbSystemSet::Main),
    );

    app.world_mut().trigger(StdbConnect);
    app.update();
    app.update();

    assert_eq!(
        app.world().resource::<WidgetInserts>().0,
        vec![Widget { id: 1 }],
        "the first declared table forwards on connect",
    );
    assert_eq!(
        app.world().resource::<GadgetInserts>().0,
        vec![Gadget { id: 2 }],
        "every other declared table forwards on connect too",
    );
}

#[test]
fn does_not_forward_before_connect() {
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(FakeDriver::default())
            .add_tables([TableRegistration::new(forward_widget)]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetInserts>();
    app.add_systems(Update, capture_widgets.in_set(StdbSystemSet::Main));

    app.update();
    app.update();

    assert!(
        app.world().resource::<WidgetInserts>().0.is_empty(),
        "no table forwards while disconnected",
    );
}

#[test]
fn rows_reach_after_set_readers_in_the_same_frame() {
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(FakeDriver::default())
            .add_tables([TableRegistration::new(forward_widget)]),
    );
    app.insert_resource(Time::<()>::default());
    app.init_resource::<WidgetInserts>();
    // A reader ordered after the bridge's ingest sees this frame's rows, not a frame late.
    app.add_systems(Update, capture_widgets.in_set(StdbSystemSet::Main));

    app.world_mut().trigger(StdbConnect);
    app.update(); // single frame

    assert_eq!(
        app.world().resource::<WidgetInserts>().0,
        vec![Widget { id: 1 }],
        "a row forwarded on connect reaches an in-Main reader the same frame",
    );
}

#[test]
fn re_registers_on_reconnect() {
    let driver = FakeDriver::default();
    let probe = driver.clone(); // retains the sink, to simulate an unsolicited drop
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::connection(driver).add_tables([TableRegistration::new(forward_widget)]),
    );
    app.insert_resource(Time::<()>::default());
    app.insert_resource(ReconnectPolicy {
        backoff: Backoff::Fixed(Duration::from_secs(1)),
        jitter: Jitter(0.0),
        max_retries: None,
    });
    app.init_resource::<WidgetInserts>();
    app.add_systems(Update, capture_widgets.in_set(StdbSystemSet::Main));

    // First connect: the table forwards once.
    app.world_mut().trigger(StdbConnect);
    app.update();
    app.update();
    app.update();
    assert_eq!(app.world().resource::<WidgetInserts>().0.len(), 1);

    // Unsolicited drop — intent stays Connected, so auto-reconnect is armed.
    probe.sink().disconnected().unwrap();
    app.update();
    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Disconnected
    );

    // Past the backoff: the connection is rebuilt and the table re-registers, so its rows surface
    // again — otherwise row messages would silently stop after the first drop.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(1100));
    app.update();
    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Connected,
        "should have reconnected once the backoff elapsed",
    );
    assert_eq!(
        app.world().resource::<WidgetInserts>().0.len(),
        2,
        "the table must re-register on the rebuilt connection",
    );
}
