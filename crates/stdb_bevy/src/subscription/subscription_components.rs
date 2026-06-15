use bevy::prelude::*;

use crate::{
    StdbConnection, StdbDisconnected, StdbSubscriptionDriver, SubscriptionId,
    subscription::subscription_channel::SubscriptionChannel,
};

#[derive(Debug, Default, Component)]
pub struct Subscription(Box<[String]>);

impl Subscription {
    pub fn new(queries: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(queries.into_iter().map(|q| q.into()).collect())
    }

    pub fn query(query: impl Into<String>) -> Self {
        Self(Box::new([query.into()]))
    }

    pub fn table(table: impl Into<String>) -> Self {
        Self(Box::new([format!("SELECT * FROM {}", table.into())]))
    }

    pub fn queries(&self) -> &[String] {
        &self.0
    }
}

#[derive(Component)]
pub struct IssuedSubscription {
    pub id: SubscriptionId,
}

impl IssuedSubscription {
    pub fn new(id: SubscriptionId) -> Self {
        Self { id }
    }
}

#[derive(Component)]
pub struct AppliedSubscription;

#[derive(Component)]
pub struct FailedSubscription;

#[allow(clippy::type_complexity)]
pub fn is_subscriptions_settled(
    inflight: Query<
        (),
        (
            With<Subscription>,
            Without<AppliedSubscription>,
            Without<FailedSubscription>,
        ),
    >,
) -> bool {
    inflight.is_empty()
}

pub(crate) fn subscribe_pending_subscriptions<Sd: StdbSubscriptionDriver>(
    subscriptions: Query<(Entity, &Subscription), Without<IssuedSubscription>>,
    mut driver: ResMut<Sd>,
    conn: Res<StdbConnection<Sd::Conn>>,
    channel: Res<SubscriptionChannel>,
    mut commands: Commands,
) {
    for (entity, subscription) in subscriptions.iter() {
        let handle = driver.subscribe(&conn, channel.sink(entity), subscription);
        commands
            .entity(entity)
            .insert(IssuedSubscription::new(handle));
    }
}

pub(crate) fn reset_subscriptions_on_stdbdisconnected<Sd: StdbSubscriptionDriver>(
    _: On<StdbDisconnected>,
    subscriptions: Query<Entity, With<Subscription>>,
    mut driver: ResMut<Sd>,
    mut commands: Commands,
) {
    for entity in subscriptions.iter() {
        commands
            .entity(entity)
            .remove::<(IssuedSubscription, AppliedSubscription, FailedSubscription)>();
    }
    driver.clear();
}

pub(crate) fn unsubscribe_on_subscription_despawn<Sd: StdbSubscriptionDriver>(
    observer: On<Remove, Subscription>,
    subscriptions: Query<&IssuedSubscription, With<Subscription>>,
    mut driver: ResMut<Sd>,
    channel: Res<SubscriptionChannel>,
) {
    let entity = observer.entity;
    if let Ok(IssuedSubscription { id }) = subscriptions.get(entity) {
        driver.unsubscribe(channel.sink(entity), id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bevy::ecs::system::RunSystemOnce;

    use crate::lifecycle::lifecycle_channel::LifecycleChannel;
    use crate::test_support::{FakeConn, FakeDriver};
    use crate::{StdbConnect, StdbDisconnected, StdbPlugin, SubscriptionId};

    /// A test `App` with the subscription layer on. Connection-only tests keep using `test_app`.
    fn app_with_subscriptions() -> App {
        let mut app = App::new();
        app.add_plugins(StdbPlugin::new(
            FakeDriver::default(),
            FakeDriver::default(),
        ));
        app.insert_resource(Time::<()>::default());
        app
    }

    #[test]
    fn table_constructor_builds_select_star() {
        let sub = Subscription::table("player");

        assert_eq!(
            sub.queries(),
            ["SELECT * FROM player"],
            "table(name) is sugar for a one-query set selecting every row of that table",
        );
    }

    #[test]
    fn new_constructor_stores_the_query_set() {
        let sub = Subscription::new(["SELECT * FROM player", "SELECT * FROM monster"]);

        assert_eq!(
            sub.queries(),
            ["SELECT * FROM player", "SELECT * FROM monster"],
            "new keeps the whole query set (one atomic subscribe) in declared order",
        );
    }

    #[test]
    fn query_constructor_builds_a_single_query_set() {
        let sub = Subscription::query("SELECT * FROM player WHERE online = true");

        assert_eq!(
            sub.queries(),
            ["SELECT * FROM player WHERE online = true"],
            "query is sugar for a one-element set carrying the given SQL verbatim",
        );
    }

    #[test]
    fn connected_spawn_is_issued_and_applied() {
        let mut app = app_with_subscriptions();

        // Live connection first — subscribe needs a socket.
        app.world_mut().trigger(StdbConnect);
        app.update();

        app.world_mut().spawn(Subscription::table("player"));
        app.update(); // reconcile issues -> driver.subscribe -> queues Applied
        app.update(); // drain turns the Applied signal into the marker

        let mut applied = app
            .world_mut()
            .query_filtered::<&Subscription, With<AppliedSubscription>>();
        let applied = applied.iter(app.world()).collect::<Vec<_>>();

        // AppliedSubscription only appears if the driver was actually called (the fake applies on
        // subscribe), so this proves issuance via observable world state — no call-log needed.
        assert_eq!(
            applied.len(),
            1,
            "a connected, spawned Subscription must be issued to the driver and become applied",
        );
        assert_eq!(
            applied[0].queries(),
            ["SELECT * FROM player"],
            "the applied subscription carries the declared query set",
        );
    }

    #[test]
    fn issued_subscription_is_no_longer_pending() {
        let mut app = app_with_subscriptions();

        app.world_mut().trigger(StdbConnect);
        app.update();

        app.world_mut().spawn(Subscription::table("player"));
        app.update();
        app.update();
        app.update();

        // Idempotency as a world invariant: an issued sub carries SubscriptionHandle, so the
        // reconcile's `Without<SubscriptionHandle>` filter can never pick it up again. If the
        // marker were not inserted, the sub would stay pending here.
        let mut pending = app
            .world_mut()
            .query_filtered::<(), (With<Subscription>, Without<IssuedSubscription>)>();
        assert!(
            pending.iter(app.world()).next().is_none(),
            "an issued subscription must be marked so reconcile won't re-issue it",
        );
    }

    #[test]
    fn disconnected_spawn_stays_pending() {
        let mut app = app_with_subscriptions();

        // No StdbConnect — stays disconnected.
        app.world_mut().spawn(Subscription::table("player"));
        app.update();

        let mut applied = app
            .world_mut()
            .query_filtered::<(), With<AppliedSubscription>>();
        assert!(
            applied.iter(app.world()).next().is_none(),
            "with no live connection nothing is issued, so nothing applies",
        );

        let mut pending = app
            .world_mut()
            .query_filtered::<(), (With<Subscription>, Without<IssuedSubscription>)>();
        assert_eq!(
            pending.iter(app.world()).count(),
            1,
            "the subscription stays pending until a connection exists",
        );
    }

    #[test]
    fn stdb_disconnected_resets_every_subscription_marker() {
        let mut app = app_with_subscriptions();

        // Two subs that were issued + resolved on the (now dropping) connection.
        let applied = app
            .world_mut()
            .spawn((
                Subscription::table("a"),
                IssuedSubscription {
                    id: SubscriptionId::from(0),
                },
                AppliedSubscription,
            ))
            .id();
        let failed = app
            .world_mut()
            .spawn((
                Subscription::table("b"),
                IssuedSubscription {
                    id: SubscriptionId::from(1),
                },
                FailedSubscription,
            ))
            .id();

        app.world_mut().trigger(StdbDisconnected);
        app.update();

        for entity in [applied, failed] {
            assert!(
                app.world().get::<IssuedSubscription>(entity).is_none()
                    && app.world().get::<AppliedSubscription>(entity).is_none()
                    && app.world().get::<FailedSubscription>(entity).is_none(),
                "a disconnect must drop every bridge marker so the sub is pending again",
            );
            assert!(
                app.world().get::<Subscription>(entity).is_some(),
                "the durable Subscription query must survive a disconnect",
            );
        }
    }

    #[test]
    fn a_subscription_is_reissued_after_a_reconnect() {
        let mut app = app_with_subscriptions();

        app.world_mut().trigger(StdbConnect);
        app.update();

        let sub = app.world_mut().spawn(Subscription::table("player")).id();
        app.update(); // reconcile issues
        app.update(); // drain applies
        assert!(
            app.world().get::<AppliedSubscription>(sub).is_some(),
            "precondition: the sub applied on the first connection",
        );

        // Drop the connection (no Time advance, so auto-reconnect stays dormant).
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.disconnected().unwrap();
        app.update();
        assert!(
            app.world().get::<AppliedSubscription>(sub).is_none()
                && app.world().get::<IssuedSubscription>(sub).is_none(),
            "a drop must reset the sub to pending — markers cleared, not left stale",
        );

        // Reconnect: the reconcile must re-issue the pending sub on the fresh connection.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        app.update(); // connect lands
        app.update(); // reconcile re-issues
        app.update(); // drain re-applies
        assert!(
            app.world().get::<AppliedSubscription>(sub).is_some(),
            "the sub must be re-issued and re-applied on the reconnected socket",
        );
    }

    /// A test `App` with the subscription layer on, returning a probe over the driver so a test can
    /// read `unsubscribes()`. `new` uses the one driver as both connection and subscription driver,
    /// so the probe shares its unsubscribe counter.
    fn app_with_sub_probe() -> (App, FakeDriver) {
        let sub_driver = FakeDriver::default();
        let probe = sub_driver.clone(); // shares the unsubscribe counter with the sub driver
        let mut app = App::new();
        app.add_plugins(StdbPlugin::new(FakeDriver::default(), sub_driver));
        app.insert_resource(Time::<()>::default());
        (app, probe)
    }

    #[test]
    fn despawning_an_issued_subscription_unsubscribes() {
        let (mut app, probe) = app_with_sub_probe();

        app.world_mut().trigger(StdbConnect);
        app.update();
        let sub = app.world_mut().spawn(Subscription::table("player")).id();
        app.update(); // reconcile issues -> IssuedSubscription(token)

        app.world_mut().entity_mut(sub).despawn();

        assert_eq!(
            probe.unsubscribes(),
            1,
            "despawning an issued subscription must call its unsubscribe token exactly once",
        );
    }

    #[test]
    fn despawning_a_disconnected_subscription_does_not_unsubscribe() {
        let (mut app, probe) = app_with_sub_probe();

        app.world_mut().trigger(StdbConnect);
        app.update();
        let sub = app.world_mut().spawn(Subscription::table("player")).id();
        app.update(); // issued

        // Drop: the strip removes IssuedSubscription, dropping the token uncalled.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.disconnected().unwrap();
        app.update();

        app.world_mut().entity_mut(sub).despawn();

        assert_eq!(
            probe.unsubscribes(),
            0,
            "after a drop the handle is dead, so despawning must not unsubscribe it",
        );
    }

    #[test]
    fn despawning_a_never_issued_subscription_does_not_unsubscribe() {
        let (mut app, probe) = app_with_sub_probe();

        // Spawned while disconnected: the reconcile never runs, so no token is ever stored.
        let sub = app.world_mut().spawn(Subscription::table("player")).id();
        app.update();

        app.world_mut().entity_mut(sub).despawn();

        assert_eq!(
            probe.unsubscribes(),
            0,
            "despawning a never-issued subscription must not unsubscribe",
        );
    }

    #[test]
    fn no_subscriptions_is_settled() {
        let mut world = World::new();

        assert!(
            world.run_system_once(is_subscriptions_settled).unwrap(),
            "with no subscriptions there is nothing to wait for",
        );
    }

    #[test]
    fn all_applied_is_settled() {
        let mut world = World::new();
        world.spawn((Subscription::table("a"), AppliedSubscription));
        world.spawn((Subscription::table("b"), AppliedSubscription));

        assert!(
            world.run_system_once(is_subscriptions_settled).unwrap(),
            "every subscription applied means the world is settled",
        );
    }

    #[test]
    fn a_failed_subscription_still_counts_as_settled() {
        let mut world = World::new();
        world.spawn((Subscription::table("a"), AppliedSubscription));
        world.spawn((Subscription::table("b"), FailedSubscription));

        assert!(
            world.run_system_once(is_subscriptions_settled).unwrap(),
            "a terminally-failed subscription is resolved, so it must not block settled",
        );
    }

    #[test]
    fn an_in_flight_subscription_is_not_settled() {
        let mut world = World::new();
        world.spawn((Subscription::table("a"), AppliedSubscription));
        world.spawn(Subscription::table("b")); // in-flight: neither applied nor failed

        assert!(
            !world.run_system_once(is_subscriptions_settled).unwrap(),
            "an in-flight subscription must keep the world unsettled",
        );
    }
}
