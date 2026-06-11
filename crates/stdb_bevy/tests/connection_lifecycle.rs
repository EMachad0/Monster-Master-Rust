//! End-to-end: the connection lifecycle driven through the bridge's *public* API only.
//!
//! These exercise the `Connecting` guard and auto-reconnect the way a real Game would — a
//! `StdbConnectionDriver` (here the `DeferredDriver` fake), `StdbConnect`, and the public
//! `StdbStatus` — with no access to crate internals.

use std::time::Duration;

use bevy::prelude::*;
use stdb_bevy::test_support::{DeferredDriver, test_app};
use stdb_bevy::{Backoff, Jitter, ReconnectPolicy, StdbConnect, StdbStatus};

fn fixed_backoff(ms: u64) -> ReconnectPolicy {
    ReconnectPolicy {
        backoff: Backoff::Fixed(Duration::from_millis(ms)),
        jitter: Jitter(0.0),
        max_retries: None,
    }
}

#[test]
fn connect_in_flight_is_connecting() {
    let mut app = test_app(DeferredDriver::default());

    app.world_mut().trigger(StdbConnect);
    app.update();

    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Connecting,
        "while a build is in flight the status is Connecting, not Disconnected",
    );
}

#[test]
fn reconnect_does_not_fire_while_a_connect_is_in_flight() {
    let driver = DeferredDriver::default();
    let probe = driver.clone(); // shares the connect counter with the plugin's copy
    let mut app = test_app(driver);
    app.insert_resource(fixed_backoff(500));

    app.world_mut().trigger(StdbConnect);
    app.update();
    assert_eq!(probe.connects(), 1);
    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Connecting
    );

    // Let far more than the backoff elapse while the build is still in flight.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(5));
    app.update();
    app.update();

    assert_eq!(
        probe.connects(),
        1,
        "Connecting must suppress auto-reconnect — no second build may be kicked in flight",
    );
    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Connecting
    );
}

#[test]
fn connecting_resolves_to_connected_when_the_build_lands() {
    let driver = DeferredDriver::default();
    let probe = driver.clone();
    let mut app = test_app(driver);

    app.world_mut().trigger(StdbConnect);
    app.update();
    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Connecting
    );

    // Deliver the parked connection, as a real async build would on a later frame.
    probe.deliver_connected();
    app.update();

    assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
}

#[test]
fn connecting_resolves_to_disconnected_on_error_and_rearms_reconnect() {
    let driver = DeferredDriver::default();
    let probe = driver.clone();
    let mut app = test_app(driver);
    app.insert_resource(fixed_backoff(500));

    app.world_mut().trigger(StdbConnect);
    app.update();
    assert_eq!(probe.connects(), 1);
    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Connecting
    );

    // The in-flight build fails.
    probe.deliver_error();
    app.update();
    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Disconnected,
        "a failed build leaves Connecting for Disconnected",
    );

    // Disconnected + intent Connected → auto-reconnect re-arms and fires after the backoff.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(600));
    app.update();
    app.update();
    assert_eq!(
        probe.connects(),
        2,
        "after a failed build, auto-reconnect kicks a fresh build once the backoff elapses",
    );
}
