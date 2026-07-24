//! e2e: a consumer gets reducer outcomes through the public API alone, with nothing registered.
//!
//! Drives the Bridge as the Game would: build the plugin, add an observer for a reducer marker, and
//! feed an outcome through the plugin-inserted `ReducerOutcomeSink`. No crate internals, no
//! `add_tables`-style registration. `Ctx = ()` stands in for the module's `ReducerEventContext`.

use bevy::prelude::*;
use stdb_bevy::test_support::{FakeDriver, test_app};
use stdb_bevy::{ReducerCommitted, ReducerFailed, ReducerOutcomeSink};

/// A Game reducer marker: the only per-reducer declaration a consumer writes.
struct Attack;

#[derive(Resource, Default)]
struct Committed(u32);

#[derive(Resource, Default)]
struct Failed(Vec<String>);

#[test]
fn plugin_delivers_committed_through_public_api() {
    let mut app = test_app(FakeDriver::default());
    app.init_resource::<Committed>();
    app.add_observer(|_: On<ReducerCommitted<Attack>>, mut c: ResMut<Committed>| c.0 += 1);

    // The plugin wired the sink; the Game reaches it as a resource and tags a call's outcome. No
    // per-reducer registration happened, only the observer above.
    let cb = app
        .world()
        .resource::<ReducerOutcomeSink>()
        .cb::<Attack, ()>();
    cb(&(), Ok(Ok(())));
    app.update();

    assert_eq!(
        app.world().resource::<Committed>().0,
        1,
        "a committed outcome fed through the plugin-inserted sink must reach a ReducerCommitted observer",
    );
}

#[test]
fn plugin_delivers_failed_with_message() {
    let mut app = test_app(FakeDriver::default());
    app.init_resource::<Failed>();
    app.add_observer(|on: On<ReducerFailed<Attack>>, mut f: ResMut<Failed>| {
        f.0.push(on.event().error().to_string())
    });

    let cb = app
        .world()
        .resource::<ReducerOutcomeSink>()
        .cb::<Attack, ()>();
    cb(&(), Ok(Err("denied".to_string())));
    app.update();

    assert_eq!(
        app.world().resource::<Failed>().0,
        vec!["denied".to_string()],
        "a returned error must reach a ReducerFailed observer carrying that message, through the public API",
    );
}
