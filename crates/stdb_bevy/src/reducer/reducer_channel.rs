use bevy::ecs::{
    resource::Resource,
    system::{Commands, Res},
    world::World,
};

use crate::{ReducerCommitted, ReducerFailed};

type OutcomeCommand = Box<dyn FnOnce(&mut World) + Send>;

/// How a Reducer call this client made ended. Covers only the caller's own calls: the server never
/// reports another client's reducer runs back to this client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReducerOutcome {
    /// The Reducer ran and its transaction committed.
    Committed,
    /// The Reducer did not commit, whether it returned an error or the host aborted the call.
    /// Carries a human-readable reason.
    Failed(String),
}

#[derive(Clone, Resource)]
pub struct ReducerOutcomeSink {
    sender: crossbeam_channel::Sender<OutcomeCommand>,
}

impl ReducerOutcomeSink {
    /// A one-shot callback that reports one call's outcome under the Game's marker `K`. The caller
    /// classifies the outcome, so this signature is fixed by the bridge rather than by whatever
    /// shape the driver's own callback happens to take.
    pub fn cb<K>(&self) -> impl FnOnce(ReducerOutcome) + Send + 'static
    where
        K: Send + Sync + 'static,
    {
        let sender = self.sender.clone();
        move |outcome| {
            let command: OutcomeCommand = match outcome {
                ReducerOutcome::Committed => Box::new(|world: &mut World| {
                    world.trigger(ReducerCommitted::<K>::new());
                }),
                ReducerOutcome::Failed(error) => Box::new(move |world: &mut World| {
                    world.trigger(ReducerFailed::<K>::new(error));
                }),
            };
            sender
                .send(command)
                .unwrap_or_else(|err| bevy::log::error!("ReducerOutcomeSink send error {}", err));
        }
    }
}

/// The crate-internal channel carrying queued outcome triggers from the sink to the drain.
#[derive(Resource)]
pub(crate) struct ReducerOutcomeChannel {
    sender: crossbeam_channel::Sender<OutcomeCommand>,
    receiver: crossbeam_channel::Receiver<OutcomeCommand>,
}

impl ReducerOutcomeChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }

    pub(crate) fn sink(&self) -> ReducerOutcomeSink {
        ReducerOutcomeSink {
            sender: self.sender.clone(),
        }
    }
}

impl Default for ReducerOutcomeChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// Drain queued outcome triggers into the world. Ungated and runs every frame: an outcome is a
/// point-in-time event, not state to reconcile, so it needs no resync gate and no sink clearing.
pub(crate) fn drain_reducer_outcomes(channel: Res<ReducerOutcomeChannel>, mut commands: Commands) {
    while let Ok(command) = channel.receiver.try_recv() {
        commands.queue(command);
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::{ReducerCommitted, ReducerFailed};

    // Two distinct reducer markers, the reducer counterpart of a row's marker type. Field-less: `K`
    // is only a type tag that keys the outcome event.
    #[derive(Clone, Copy)]
    struct A;
    #[derive(Clone, Copy)]
    struct B;

    #[derive(Resource, Default)]
    struct CommittedA(u32);
    #[derive(Resource, Default)]
    struct CommittedB(u32);
    #[derive(Resource, Default)]
    struct FailedA(Vec<String>);

    /// An app with the channel and drain installed and no connection. The seam is fed directly
    /// through `cb`, the same seam the SDK adapter's callback pushes through in production.
    fn seam_app() -> App {
        let mut app = App::new();
        app.insert_resource(ReducerOutcomeChannel::new());
        app.add_systems(Update, drain_reducer_outcomes);
        app
    }

    fn sink(app: &App) -> ReducerOutcomeSink {
        app.world().resource::<ReducerOutcomeChannel>().sink()
    }

    #[test]
    fn committed_triggers_reducer_committed() {
        let mut app = seam_app();
        app.init_resource::<CommittedA>();
        app.add_observer(|_: On<ReducerCommitted<A>>, mut c: ResMut<CommittedA>| c.0 += 1);

        let cb = sink(&app).cb::<A>();
        cb(ReducerOutcome::Committed);
        app.update();

        assert_eq!(
            app.world().resource::<CommittedA>().0,
            1,
            "a committed outcome must fire exactly one ReducerCommitted for its marker",
        );
    }

    #[test]
    fn committed_does_not_trigger_failed() {
        let mut app = seam_app();
        app.init_resource::<FailedA>();
        app.add_observer(|on: On<ReducerFailed<A>>, mut f: ResMut<FailedA>| {
            f.0.push(on.event().error().to_string())
        });

        let cb = sink(&app).cb::<A>();
        cb(ReducerOutcome::Committed);
        app.update();

        assert!(
            app.world().resource::<FailedA>().0.is_empty(),
            "a committed outcome must not fire ReducerFailed",
        );
    }

    #[test]
    fn failed_outcome_triggers_reducer_failed_with_message() {
        let mut app = seam_app();
        app.init_resource::<FailedA>();
        app.add_observer(|on: On<ReducerFailed<A>>, mut f: ResMut<FailedA>| {
            f.0.push(on.event().error().to_string())
        });

        // The gameplay rejection path: the Reducer ran and refused.
        let cb = sink(&app).cb::<A>();
        cb(ReducerOutcome::Failed("out of energy".to_string()));
        app.update();

        assert_eq!(
            app.world().resource::<FailedA>().0,
            vec!["out of energy".to_string()],
            "a returned error must fire ReducerFailed carrying that error message",
        );
    }

    #[test]
    fn failed_does_not_trigger_committed() {
        let mut app = seam_app();
        app.init_resource::<CommittedA>();
        app.add_observer(|_: On<ReducerCommitted<A>>, mut c: ResMut<CommittedA>| c.0 += 1);

        let cb = sink(&app).cb::<A>();
        cb(ReducerOutcome::Failed("nope".to_string()));
        app.update();

        assert_eq!(
            app.world().resource::<CommittedA>().0,
            0,
            "a failed outcome must not fire ReducerCommitted",
        );
    }

    #[test]
    fn outcome_is_keyed_by_marker_type() {
        let mut app = seam_app();
        app.init_resource::<CommittedA>();
        app.init_resource::<CommittedB>();
        app.add_observer(|_: On<ReducerCommitted<A>>, mut c: ResMut<CommittedA>| c.0 += 1);
        app.add_observer(|_: On<ReducerCommitted<B>>, mut c: ResMut<CommittedB>| c.0 += 1);

        // Feed an A outcome only.
        let cb = sink(&app).cb::<A>();
        cb(ReducerOutcome::Committed);
        app.update();

        assert_eq!(
            app.world().resource::<CommittedA>().0,
            1,
            "the A marker must receive its own outcome",
        );
        assert_eq!(
            app.world().resource::<CommittedB>().0,
            0,
            "a different marker must not receive another marker's outcome; outcomes are keyed by type",
        );
    }

    #[test]
    fn multiple_queued_outcomes_all_drain() {
        let mut app = seam_app();
        app.init_resource::<CommittedA>();
        app.add_observer(|_: On<ReducerCommitted<A>>, mut c: ResMut<CommittedA>| c.0 += 1);

        // Queue several outcomes before a single drain: the channel is unbounded and the drain loops.
        let s = sink(&app);
        s.cb::<A>()(ReducerOutcome::Committed);
        s.cb::<A>()(ReducerOutcome::Committed);
        s.cb::<A>()(ReducerOutcome::Committed);
        app.update();

        assert_eq!(
            app.world().resource::<CommittedA>().0,
            3,
            "every queued outcome must surface: the drain must loop the channel dry",
        );
    }
}
