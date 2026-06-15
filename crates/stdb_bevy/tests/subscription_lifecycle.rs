//! End-to-end: bridge-owned subscriptions driven through the bridge's *public* API only.
//!
//! These prove a real Game can build a subscription-enabled plugin, declare a `Subscription` by
//! spawning it, and observe it apply — and that the `is_subscriptions_settled` fence tracks that
//! state — using only public items (`StdbPlugin::with_subscriptions`, the `FakeDriver` fake,
//! `Subscription`, `AppliedSubscription`, `SubscriptionApplied`, `is_subscriptions_settled`), with
//! no access to crate internals.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use stdb_bevy::test_support::FakeDriver;
use stdb_bevy::{
    AppliedSubscription, StdbConnect, StdbPlugin, Subscription, SubscriptionApplied,
    is_subscriptions_settled,
};

/// A Game-shaped app: the connection + subscription layers wired through the public builder, plus
/// the `Time` the reconnect engine needs.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(
        StdbPlugin::new(FakeDriver::default()).with_subscriptions(FakeDriver::default()),
    );
    app.insert_resource(Time::<()>::default());
    app
}

#[derive(Resource, Default)]
struct AppliedTargets(Vec<Entity>);

#[test]
fn a_declared_subscription_applies_and_fires_the_event() {
    let mut app = app();
    app.init_resource::<AppliedTargets>();
    app.add_observer(
        |on: On<SubscriptionApplied>, mut targets: ResMut<AppliedTargets>| {
            targets.0.push(on.entity);
        },
    );

    app.world_mut().trigger(StdbConnect);
    app.update();

    let sub = app.world_mut().spawn(Subscription::table("player")).id();
    app.update(); // reconcile issues
    app.update(); // drain applies

    assert!(
        app.world().get::<AppliedSubscription>(sub).is_some(),
        "a declared subscription must apply on a live connection",
    );
    assert_eq!(
        app.world().resource::<AppliedTargets>().0,
        vec![sub],
        "applying must fire SubscriptionApplied targeting the subscription entity",
    );
}

#[test]
fn the_settled_fence_tracks_subscription_state() {
    let mut app = app();

    app.world_mut().trigger(StdbConnect);
    app.update();

    app.world_mut().spawn(Subscription::table("player"));
    app.update(); // issued, but the Applied signal is not drained until next frame

    assert!(
        !app.world_mut()
            .run_system_once(is_subscriptions_settled)
            .unwrap(),
        "an in-flight subscription must leave the fence unsettled",
    );

    app.update(); // drain applies

    assert!(
        app.world_mut()
            .run_system_once(is_subscriptions_settled)
            .unwrap(),
        "once the subscription applies the fence settles",
    );
}
