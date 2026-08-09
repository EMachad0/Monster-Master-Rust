//! The SDK-shaped reducer callback, kept out of the bridge's own contracts.
//!
//! The SDK's per-call `<reducer>_then` hands its closure an event context plus a nested result, a
//! shape that changes with the SDK rather than with the bridge. Translating it here means the
//! bridge's sink keeps taking a plain outcome, and a reshape lands in this file alone.

use std::fmt::Display;

use crate::reducer::reducer_channel::{ReducerOutcome, ReducerOutcomeSink};

/// Adapts the reducer outcome sink to the callback the generated `<reducer>_then` expects.
pub trait SdkReducerSinkExt {
    /// A one-shot `<reducer>_then` callback reporting the call's outcome under the Game's marker
    /// `K`. Generic over the abort error because the only thing read off it is its message, so no
    /// unstable SDK error type appears in a signature.
    fn sdk_cb<K, Ctx, E>(
        &self,
    ) -> impl FnOnce(&Ctx, Result<Result<(), String>, E>) + Send + 'static
    where
        K: Send + Sync + 'static,
        E: Display;
}

impl SdkReducerSinkExt for ReducerOutcomeSink {
    fn sdk_cb<K, Ctx, E>(&self) -> impl FnOnce(&Ctx, Result<Result<(), String>, E>) + Send + 'static
    where
        K: Send + Sync + 'static,
        E: Display,
    {
        let cb = self.cb::<K>();
        // The event context describes the transaction that ran the call, which no outcome consumer
        // reads: the Game already holds the arguments it sent.
        move |_ctx: &Ctx, result| {
            let outcome = match result {
                Ok(Ok(())) => ReducerOutcome::Committed,
                Ok(Err(error)) => ReducerOutcome::Failed(error),
                // A host abort carries nothing a caller can branch on beyond its message, so it
                // folds into the same failure a returned error produces.
                Err(abort) => ReducerOutcome::Failed(abort.to_string()),
            };
            cb(outcome);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::ReducerFailed;
    use crate::reducer::reducer_channel::{ReducerOutcomeChannel, drain_reducer_outcomes};

    // A reducer marker, the reducer counterpart of a row's marker type. Field-less: `K` is only a
    // type tag that keys the outcome event.
    struct A;

    #[derive(Resource, Default)]
    struct FailedA(Vec<String>);

    /// Stands in for the SDK's internal error: a host abort surfaces as a value whose only useful
    /// content is its `Display` output.
    struct HostAbort;

    impl std::fmt::Display for HostAbort {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("reducer aborted")
        }
    }

    #[test]
    fn host_abort_folds_into_reducer_failed() {
        let mut app = App::new();
        app.insert_resource(ReducerOutcomeChannel::new());
        app.add_systems(Update, drain_reducer_outcomes);
        app.init_resource::<FailedA>();
        app.add_observer(|on: On<ReducerFailed<A>>, mut f: ResMut<FailedA>| {
            f.0.push(on.event().error().to_string())
        });

        let sink = app.world().resource::<ReducerOutcomeChannel>().sink();
        sink.sdk_cb::<A, (), HostAbort>()(&(), Err(HostAbort));
        app.update();

        let failed = &app.world().resource::<FailedA>().0;
        assert_eq!(
            failed.len(),
            1,
            "a host abort must fire exactly one ReducerFailed",
        );
        assert!(
            !failed[0].is_empty(),
            "the folded abort must carry a non-empty error string",
        );
    }
}
