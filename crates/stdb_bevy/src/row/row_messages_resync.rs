use spacetimedb_sdk::Table;

use crate::{
    RowDeleted, RowInserted, RowUpdated, StdbConn, StdbConnection, StdbPreviousConnection, StdbRow,
};
use bevy::prelude::*;

#[allow(clippy::type_complexity)]
pub(crate) fn resync_row_messages_system<C, T, R, K>(
    accessor: fn(&C) -> T,
    get_key: fn(&R) -> K,
) -> impl FnMut(
    Res<StdbPreviousConnection<C>>,
    Res<StdbConnection<C>>,
    MessageWriter<RowInserted<R>>,
    MessageWriter<RowUpdated<R>>,
    MessageWriter<RowDeleted<R>>,
)
where
    C: StdbConn,
    T: Table<Row = R>,
    R: StdbRow,
    K: Eq + Ord,
{
    move |previous_conn, conn, mut insert_writer, mut _update_writer, mut delete_writer| {
        let mut old_cache = (accessor)(&previous_conn.0)
            .iter()
            .map(|row| ((get_key)(&row), row))
            .collect::<Vec<_>>();
        old_cache.sort_by(|(k0, _), (k1, _)| k0.cmp(k1));
        let mut old_cache = old_cache.into_iter().peekable();
        let mut new_cache = (accessor)(&conn.0)
            .iter()
            .map(|row| ((get_key)(&row), row))
            .collect::<Vec<_>>();
        new_cache.sort_by(|(k0, _), (k1, _)| k0.cmp(k1));
        let mut new_cache = new_cache.into_iter().peekable();

        loop {
            match (old_cache.peek(), new_cache.peek()) {
                (Some((k0, _)), Some((k1, _))) => match k0.cmp(k1) {
                    std::cmp::Ordering::Less => {
                        let (_, old) = old_cache.next().unwrap();
                        delete_writer.write(RowDeleted(old));
                    }
                    std::cmp::Ordering::Equal => {
                        let (_, _old) = old_cache.next().unwrap();
                        let (_, _new) = new_cache.next().unwrap();
                    }
                    std::cmp::Ordering::Greater => {
                        let (_, new) = new_cache.next().unwrap();
                        insert_writer.write(RowInserted(new));
                    }
                },
                (Some(_), None) => {
                    let (_, old) = old_cache.next().unwrap();
                    delete_writer.write(RowDeleted(old));
                }
                (None, Some(_)) => {
                    let (_, new) = new_cache.next().unwrap();
                    insert_writer.write(RowInserted(new));
                }
                (None, None) => break,
            }
        }
    }
}

pub(crate) fn drop_stdbpreviousconnection_after_resync<C: StdbConn>(mut commands: Commands) {
    commands.remove_resource::<StdbPreviousConnection<C>>();
}

#[cfg(test)]
mod tests {
    //! The resync window's lifetime, observed through `StdbPreviousConnection<FakeConn>` presence:
    //! a disconnect stashes the baseline (opening the window), and the fence closes it only once a
    //! live (re)connection has every subscription settled. Until then the window stays open, so a
    //! flapping reconnect cannot close it early.
    //!
    //! Subscriptions are off (`StdbPlugin::connection`) so a `Subscription` entity's settled state
    //! can be controlled by hand (the subscriptions-on `FakeDriver` applies every sub instantly): a
    //! bare `Subscription` is in-flight, one carrying `AppliedSubscription` is settled. The fence
    //! lives in the connection build path, so `is_subscriptions_settled` gates it regardless of
    //! whether a subscription driver is installed.

    use bevy::prelude::*;

    use crate::test_support::{FakeConn, FakeDriver};
    use crate::{
        AppliedSubscription, StdbConnect, StdbPlugin, StdbPreviousConnection, Subscription,
    };

    /// A connection-only bridge plus `Time` (the reconnect tick needs it), returning a probe over
    /// the driver so a test can push an unsolicited drop / reconnect through the retained sink.
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

        // Connect then drop, with no subscriptions (so `is_subscriptions_settled` is trivially
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
}
