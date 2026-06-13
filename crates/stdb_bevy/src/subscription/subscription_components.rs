use bevy::prelude::*;

use crate::{
    StdbConnection, StdbSubscriptionDriver, subscription::subscription_channel::SubscriptionChannel,
};

#[derive(Debug, Default, Component)]
pub struct Subscription(Box<[String]>);

impl Subscription {
    pub fn table(table: impl Into<String>) -> Self {
        Self(Box::new([format!("SELECT * FROM {}", table.into())]))
    }

    pub fn queries(&self) -> &[String] {
        &self.0
    }
}

#[derive(Component)]
pub struct AppliedSubscription;

#[derive(Component)]
pub struct FailedSubscription;

#[derive(Component)]
pub struct IssuedSubscription;

pub fn subscribe_pending_subscriptions<Cd: StdbSubscriptionDriver>(
    subscriptions: Query<(Entity, &Subscription), Without<IssuedSubscription>>,
    driver: Res<Cd>,
    conn: Res<StdbConnection<Cd::Conn>>,
    channel: Res<SubscriptionChannel>,
    mut commands: Commands,
) {
    for (entity, subscription) in subscriptions.iter() {
        driver.subscribe(&conn, entity, subscription, channel.sink());
        commands.entity(entity).insert(IssuedSubscription);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::FakeDriver;
    use crate::{StdbConnect, StdbPlugin};

    /// A test `App` with both the connection and subscription layers installed (the connecting
    /// fake doubles as the subscription driver). Connection-only tests keep using `test_app`.
    fn app_with_subscriptions() -> App {
        let mut app = App::new();
        app.add_plugins(
            StdbPlugin::new(FakeDriver::default()).with_subscriptions(FakeDriver::default()),
        );
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
}
