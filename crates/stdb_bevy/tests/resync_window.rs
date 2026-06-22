//! End-to-end: the resync window, driven through the bridge's *public* API only.
//!
//! Slice 1 established that a disconnect retains the dropped connection as
//! `StdbPreviousConnection<C>`. That resource's *presence* is the resync-in-flight flag: it is set
//! when a disconnect stashes the baseline and must be cleared only at the **resync fence** — when a
//! live (re)connection has all its subscriptions settled. Until then the window stays open, so a
//! flapping reconnect cannot close it early.
//!
//! These tests assert the window's lifetime through `StdbPreviousConnection<FakeConn>` presence.
//! They run subscriptions-off (`StdbPlugin::connection`) so a `Subscription` entity's settled state
//! can be controlled by hand (the subscriptions-on `FakeDriver` applies every sub instantly): a
//! bare `Subscription` is in-flight, one carrying `AppliedSubscription` is settled. This assumes the
//! fence is wired in the connection build path, so `is_subscriptions_settled` gates it regardless of
//! whether a subscription driver is installed.

use bevy::prelude::*;
use stdb_bevy::test_support::{FakeConn, FakeDriver};
use stdb_bevy::{
    AppliedSubscription, StdbConnect, StdbPlugin, StdbPreviousConnection, Subscription,
};

/// A connection-only bridge plus `Time` (the reconnect tick needs it), returning a probe over the
/// driver so a test can push an unsolicited drop / reconnect through the retained sink.
fn window_app() -> (App, FakeDriver) {
    let driver = FakeDriver::default();
    let probe = driver.clone();
    let mut app = App::new();
    app.add_plugins(StdbPlugin::connection(driver));
    app.insert_resource(Time::<()>::default());
    (app, probe)
}

fn baseline_present(app: &App) -> bool {
    app.world()
        .get_resource::<StdbPreviousConnection<FakeConn>>()
        .is_some()
}

#[test]
fn window_closes_when_connected_and_subscriptions_settled() {
    let (mut app, probe) = window_app();

    // Connect, then drop: the baseline is stashed, opening the window.
    app.world_mut().trigger(StdbConnect);
    app.update();
    probe.sink().disconnected().unwrap();
    app.update();
    assert!(
        baseline_present(&app),
        "the drop must open the resync window"
    );

    // A subscription that is already settled (so the fence is unblocked once reconnected).
    app.world_mut()
        .spawn((Subscription::table("player"), AppliedSubscription));

    // Reconnect: live connection + all subscriptions settled -> the fence closes the window.
    probe.sink().connected(FakeConn).unwrap();
    app.update();

    assert!(
        !baseline_present(&app),
        "with a live connection and every subscription settled, the fence must close the window \
         by dropping the baseline",
    );
}

#[test]
fn window_stays_open_while_a_subscription_is_in_flight() {
    let (mut app, probe) = window_app();

    app.world_mut().trigger(StdbConnect);
    app.update();
    probe.sink().disconnected().unwrap();
    app.update();

    // A bare subscription is in-flight: issued-but-not-applied, so the fence must wait.
    app.world_mut().spawn(Subscription::table("player"));

    probe.sink().connected(FakeConn).unwrap();
    app.update();

    assert!(
        baseline_present(&app),
        "an in-flight subscription must keep the window open even after the reconnect lands",
    );
}

#[test]
fn window_stays_open_while_disconnected() {
    let (mut app, probe) = window_app();

    // Connect then drop, with no subscriptions at all (so `is_subscriptions_settled` is trivially
    // true). Do not reconnect, and do not advance time (so auto-reconnect stays dormant).
    app.world_mut().trigger(StdbConnect);
    app.update();
    probe.sink().disconnected().unwrap();
    app.update();
    app.update();

    assert!(
        baseline_present(&app),
        "while disconnected the window must stay open — settled subscriptions must not close it \
         without a live connection",
    );
}

#[test]
fn window_survives_repeated_unsettled_reconnects() {
    let (mut app, probe) = window_app();

    app.world_mut().trigger(StdbConnect);
    app.update();

    // An in-flight subscription that never settles, so no reconnect can reach the fence.
    app.world_mut().spawn(Subscription::table("player"));

    // Drop -> reconnect -> drop again -> reconnect: a flap, all while unsettled.
    probe.sink().disconnected().unwrap();
    app.update();
    probe.sink().connected(FakeConn).unwrap();
    app.update();
    probe.sink().disconnected().unwrap();
    app.update();
    probe.sink().connected(FakeConn).unwrap();
    app.update();

    assert!(
        baseline_present(&app),
        "the window must survive a flapping reconnect: it closes only at the fence, which the \
         in-flight subscription never lets reconnect reach",
    );
}
