use bevy::ecs::{
    resource::Resource,
    system::{Commands, Res},
    world::World,
};

use crate::{
    StdbBevyError, StdbBevyErrorEvent, StdbIdentity, StdbPreviousConnection,
    connection::{
        stdb_connection::{StdbConn, StdbConnection},
        stdb_intent::StdbIntent,
        stdb_status::StdbStatus,
    },
    lifecycle::lifecycle_events::{StdbConnected, StdbDisconnected},
};

pub enum LifecycleEvent<C: StdbConn> {
    Connected(C),
    Disconnected,
    ConnectionError(StdbBevyError),
    Connecting,
    Identified(spacetimedb_sdk::Identity),
}

pub struct LifecycleSink<C: StdbConn> {
    sender: crossbeam_channel::Sender<LifecycleEvent<C>>,
}

impl<C: StdbConn> LifecycleSink<C> {
    pub fn connected(&self, c: C) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::Connected(c))
    }

    pub fn disconnected(&self) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::Disconnected)
    }

    pub fn identified(
        &self,
        identity: spacetimedb_sdk::Identity,
    ) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::Identified(identity))
    }

    pub fn connection_error(
        &self,
        error: StdbBevyError,
    ) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::ConnectionError(error))
    }

    pub fn connecting(&self) -> Result<(), crossbeam_channel::SendError<LifecycleEvent<C>>> {
        self.sender.send(LifecycleEvent::Connecting)
    }
}

impl<C: StdbConn> Clone for LifecycleSink<C> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

#[derive(Resource)]
pub(crate) struct LifecycleChannel<C: StdbConn> {
    sender: crossbeam_channel::Sender<LifecycleEvent<C>>,
    receiver: crossbeam_channel::Receiver<LifecycleEvent<C>>,
}

impl<C: StdbConn> LifecycleChannel<C> {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }

    pub fn sink(&self) -> LifecycleSink<C> {
        LifecycleSink {
            sender: self.sender.clone(),
        }
    }
}

impl<C: StdbConn> Default for LifecycleChannel<C> {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn drain_lifecycle_sink<C: StdbConn>(
    lifecycle_channel: Res<LifecycleChannel<C>>,
    intent: Res<StdbIntent>,
    mut commands: Commands,
) {
    while let Ok(stdb_event) = lifecycle_channel.receiver.try_recv() {
        match stdb_event {
            // The SDK adapter logs `connected` with the identity (the only place it has it).
            LifecycleEvent::Connected(c) => {
                commands.insert_resource(StdbConnection(c));
                commands.insert_resource(StdbStatus::Connected);
                commands.trigger(StdbConnected);
            }
            LifecycleEvent::Disconnected => {
                // Intent tells a dropped link (still Connected) from a deliberate one; `had_connection`
                // is only known inside the queued world access, so the log lives there too.
                let intent = *intent;
                commands.queue(move |world: &mut World| {
                    let conn = world.remove_resource::<StdbConnection<C>>();
                    let had_connection = conn.is_some();
                    if let Some(StdbConnection(conn)) = conn
                        && !world.contains_resource::<StdbPreviousConnection<C>>()
                    {
                        world.insert_resource(StdbPreviousConnection(conn));
                    }
                    match intent {
                        StdbIntent::Connected => {
                            bevy::log::warn!(had_connection, "unintended disconnect")
                        }
                        StdbIntent::Disconnected => bevy::log::trace!("disconnect"),
                    }
                });
                commands.remove_resource::<StdbIdentity>();
                commands.insert_resource(StdbStatus::Disconnected);
                commands.trigger(StdbDisconnected);
            }
            LifecycleEvent::ConnectionError(e) => {
                // Expected while the server is down (the retry loop drives the next attempt), so warn
                // rather than error. `StdbBevyError` is transparent over the SDK cause.
                bevy::log::warn!(error = %e, "connect failed");
                commands.insert_resource(StdbStatus::Disconnected);
                commands.trigger(StdbBevyErrorEvent::new(e));
            }
            LifecycleEvent::Connecting => {
                commands.insert_resource(StdbStatus::Connecting);
            }
            LifecycleEvent::Identified(identity) => {
                commands.insert_resource(StdbIdentity(identity));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use spacetimedb_sdk::Identity;

    use crate::test_support::{CannedDriver, FakeConn, FakeDriver, test_app};
    use crate::{StdbIdentity, StdbPreviousConnection, Subscription};

    use super::*;

    /// A connection value carrying an identity tag, so a test can tell two connections apart —
    /// `FakeConn` is field-less and indistinguishable. Auto-satisfies `StdbConn` (Send+Sync+'static).
    #[derive(Clone)]
    struct TaggedConn(u32);

    #[derive(Resource, Default)]
    struct ObserverFired(bool);

    #[derive(Resource, Default)]
    struct DisconnectFired(bool);

    #[derive(Resource, Default)]
    struct StatusSeenOnDisconnect(Option<StdbStatus>);

    #[test]
    fn connected_signal_triggers_observer_status_and_resource() {
        let mut app = test_app(FakeDriver::default());

        app.init_resource::<ObserverFired>();
        app.add_observer(|_on: On<StdbConnected>, mut fired: ResMut<ObserverFired>| fired.0 = true);

        // Before any signal: disconnected, no connection resource yet.
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_none()
        );

        // Push a `Connected` signal through the same seam the SDK adapter uses in production.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();

        app.update();

        assert!(
            app.world().resource::<ObserverFired>().0,
            "the StdbConnected observer should fire on a Connected signal",
        );
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some(),
            "StdbConnection<C> should be inserted on connect",
        );
    }

    #[test]
    fn disconnect_moves_the_connection_into_previous_connection() {
        let mut app = test_app(FakeDriver::default());

        app.init_resource::<DisconnectFired>();
        app.add_observer(
            |_on: On<StdbDisconnected>, mut fired: ResMut<DisconnectFired>| fired.0 = true,
        );

        // Connect first, so there is a live connection to drop.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        app.update();
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some()
        );

        // Now drop the connection.
        sink.disconnected().unwrap();
        app.update();

        assert!(
            app.world().resource::<DisconnectFired>().0,
            "the StdbDisconnected observer should fire on a Disconnected signal",
        );
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_none(),
            "the live StdbConnection<C> must be removed on disconnect",
        );
        assert!(
            app.world()
                .get_resource::<StdbPreviousConnection<FakeConn>>()
                .is_some(),
            "on disconnect the dropped connection must be retained as the resync baseline",
        );
    }

    #[test]
    fn first_connect_leaves_no_baseline() {
        let mut app = test_app(FakeDriver::default());

        // A first connect, with no prior disconnect.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        app.update();

        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some(),
            "a first connect installs the live connection",
        );
        assert!(
            app.world()
                .get_resource::<StdbPreviousConnection<FakeConn>>()
                .is_none(),
            "with no prior disconnect there is nothing to reconcile, so no baseline exists",
        );
    }

    #[test]
    fn reconnect_does_not_overwrite_the_baseline() {
        let mut app = test_app(CannedDriver::new(TaggedConn(0)));

        // An in-flight subscription holds the resync window open, so the fence cannot consume the
        // baseline on reconnect — isolating the Connected arm's "never touch the baseline" behavior.
        app.world_mut().spawn(Subscription::table("x"));

        let sink = app
            .world()
            .resource::<LifecycleChannel<TaggedConn>>()
            .sink();

        // Connect, then drop: conn 1 becomes the baseline.
        sink.connected(TaggedConn(1)).unwrap();
        app.update();
        sink.disconnected().unwrap();
        app.update();

        // Reconnect with a different connection.
        sink.connected(TaggedConn(2)).unwrap();
        app.update();

        let baseline = app
            .world()
            .get_resource::<StdbPreviousConnection<TaggedConn>>()
            .expect("the baseline must survive a reconnect");
        assert_eq!(
            baseline.0.0, 1,
            "Connected must never touch the baseline — it still holds the pre-outage connection",
        );
    }

    #[test]
    fn flapping_preserves_the_original_baseline() {
        let mut app = test_app(CannedDriver::new(TaggedConn(0)));

        // An in-flight subscription holds the resync window open across the flap, so the fence
        // cannot consume the baseline — isolating the flapping guard's "keep the original" behavior.
        app.world_mut().spawn(Subscription::table("x"));

        let sink = app
            .world()
            .resource::<LifecycleChannel<TaggedConn>>()
            .sink();

        // Connect(1) -> drop: conn 1 is the baseline.
        sink.connected(TaggedConn(1)).unwrap();
        app.update();
        sink.disconnected().unwrap();
        app.update();

        // Reconnect(2) -> drop again before any resync consumed the baseline.
        sink.connected(TaggedConn(2)).unwrap();
        app.update();
        sink.disconnected().unwrap();
        app.update();

        let baseline = app
            .world()
            .get_resource::<StdbPreviousConnection<TaggedConn>>()
            .expect("a flapping reconnect must keep a baseline");
        assert_eq!(
            baseline.0.0, 1,
            "the second disconnect must not clobber the baseline — the original pre-outage \
             connection is preserved (stash only when PreviousConnection is empty)",
        );
    }

    #[test]
    fn status_is_disconnected_when_the_disconnected_observer_fires() {
        let mut app = test_app(FakeDriver::default());

        app.init_resource::<StatusSeenOnDisconnect>();
        app.add_observer(
            |_on: On<StdbDisconnected>,
             status: Res<StdbStatus>,
             mut seen: ResMut<StatusSeenOnDisconnect>| { seen.0 = Some(*status) },
        );

        // Connect first, so there is a live connection to drop.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        app.update();
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);

        // Drop it.
        sink.disconnected().unwrap();
        app.update();

        assert_eq!(
            app.world().resource::<StatusSeenOnDisconnect>().0,
            Some(StdbStatus::Disconnected),
            "an StdbDisconnected observer must see StdbStatus already flipped to Disconnected, \
             not the stale Connected value — status must be set before the trigger",
        );
    }

    #[test]
    fn identified_signal_inserts_the_stdb_identity_resource() {
        let mut app = test_app(FakeDriver::default());

        let id = Identity::from_byte_array([7; 32]);
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.identified(id).unwrap();

        app.update();

        let identity = app
            .world()
            .get_resource::<StdbIdentity>()
            .expect("the Identified signal must insert StdbIdentity");
        assert_eq!(
            **identity, id,
            "StdbIdentity must carry the identity delivered by the Identified signal",
        );
    }

    #[test]
    fn disconnect_removes_the_stdb_identity_resource() {
        let mut app = test_app(FakeDriver::default());

        let id = Identity::from_byte_array([7; 32]);
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        sink.identified(id).unwrap();
        app.update();
        assert!(
            app.world().get_resource::<StdbIdentity>().is_some(),
            "precondition: the Identified signal installs StdbIdentity",
        );

        sink.disconnected().unwrap();
        app.update();

        assert!(
            app.world().get_resource::<StdbIdentity>().is_none(),
            "disconnect must clear StdbIdentity — identity is per-connection, not retained",
        );
    }

    #[test]
    fn stdb_identity_is_absent_before_the_identified_signal() {
        let mut app = test_app(FakeDriver::default());

        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.connected(FakeConn).unwrap();
        app.update();

        assert!(
            app.world()
                .get_resource::<StdbConnection<FakeConn>>()
                .is_some(),
            "precondition: Connected installs the connection",
        );
        assert!(
            app.world().get_resource::<StdbIdentity>().is_none(),
            "Connected fires at build time, before the server issues the identity — \
             StdbIdentity must not exist until the Identified signal arrives",
        );
    }

    #[test]
    fn reconnect_reinstalls_the_stdb_identity_resource() {
        let mut app = test_app(FakeDriver::default());

        let id = Identity::from_byte_array([7; 32]);
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();

        // Connect and identify, then drop.
        sink.connected(FakeConn).unwrap();
        sink.identified(id).unwrap();
        app.update();
        sink.disconnected().unwrap();
        app.update();
        assert!(
            app.world().get_resource::<StdbIdentity>().is_none(),
            "precondition: disconnect clears the identity",
        );

        // Reconnect and re-identify: token reuse yields the same identity.
        sink.connected(FakeConn).unwrap();
        sink.identified(id).unwrap();
        app.update();

        let identity = app
            .world()
            .get_resource::<StdbIdentity>()
            .expect("a reconnect that re-identifies must reinstall StdbIdentity");
        assert_eq!(
            **identity, id,
            "the reinstalled identity is the same across a same-session reconnect (token reuse)",
        );
    }
}
