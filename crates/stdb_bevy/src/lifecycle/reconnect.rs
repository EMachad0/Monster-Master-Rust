use std::{ops::Deref, time::Duration};

use bevy::ecs::{
    observer::On,
    resource::Resource,
    system::{Commands, Res, ResMut},
};

use crate::{
    StdbConnectionDriver, StdbStatus,
    connection::stdb_intent::StdbIntent,
    lifecycle::{lifecycle_channel::LifecycleChannel, lifecycle_events::StdbDisconnected},
    utils::backoff::{Backoff, Jitter},
};

#[derive(Debug, Clone, Copy, Resource)]
pub struct ReconnectPolicy {
    pub backoff: Backoff,
    pub jitter: Jitter,
    pub max_retries: Option<u32>,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        let backoff = Backoff::Exponential {
            base: Duration::from_millis(500),
            factor: 2.0,
            max: Duration::from_secs(30),
        };
        let jitter = Jitter(0.2);
        let max_retries = None;

        Self {
            backoff,
            jitter,
            max_retries,
        }
    }
}

#[derive(Debug, Clone, Copy, Resource)]
pub struct ReconnectState {
    retry_count: u32,
    elapsed: Duration,
}

impl ReconnectState {
    pub fn new() -> Self {
        Self {
            retry_count: 0,
            elapsed: Duration::ZERO,
        }
    }

    pub fn tick(
        &mut self,
        policy: &ReconnectPolicy,
        delta: Duration,
        sample: f64,
    ) -> ReconnectAction {
        if let Some(max_retries) = policy.max_retries
            && self.retry_count >= max_retries
        {
            return ReconnectAction::GiveUp;
        }

        self.elapsed += delta;
        let delay = policy.backoff.delay(self.retry_count);
        let threshold = policy.jitter.apply(delay, sample);
        if self.elapsed >= threshold {
            self.retry_count += 1;
            self.elapsed = Duration::ZERO;
            ReconnectAction::Reconnect
        } else {
            ReconnectAction::Wait
        }
    }

    pub fn reset(&mut self) {
        self.retry_count = 0;
        self.elapsed = Duration::ZERO;
    }
}

impl Default for ReconnectState {
    fn default() -> Self {
        Self::new()
    }
}

pub enum ReconnectAction {
    Wait,
    Reconnect,
    GiveUp,
}

pub fn reset_reconnectstate_on_stdbdisconnected(
    _: On<StdbDisconnected>,
    mut commands: Commands,
    intent: Res<StdbIntent>,
) {
    if *intent == StdbIntent::Connected {
        commands.insert_resource(ReconnectState::default());
    }
}

pub fn tick_reconnectstate<Cd: StdbConnectionDriver>(
    mut state: ResMut<ReconnectState>,
    policy: Res<ReconnectPolicy>,
    time: Res<bevy::time::Time>,
    connection_driver: Res<Cd>,
    lifecycle_channel: Res<LifecycleChannel<Cd::Conn>>,
) {
    let sample = time.elapsed().subsec_nanos() as f64 / 1_000_000_000.0;
    match state.tick(policy.deref(), time.delta(), sample) {
        ReconnectAction::Wait => {}
        ReconnectAction::Reconnect => {
            let sink = lifecycle_channel.sink();
            connection_driver.connect(sink);
        }
        ReconnectAction::GiveUp => {}
    }
}

pub fn should_tick_reconnectstate(intent: Res<StdbIntent>, status: Res<StdbStatus>) -> bool {
    *status == StdbStatus::Disconnected && *intent == StdbIntent::Connected
}

#[cfg(test)]
mod tests {
    use super::*;

    use bevy::time::Time;

    use crate::{
        StdbStatus,
        connection::connection_events::{StdbConnect, StdbDisconnect},
        lifecycle::lifecycle_channel::LifecycleChannel,
        test_support::{FakeConn, FakeConnectionDriver, test_app},
    };

    #[test]
    fn reconnect_waits_until_the_backoff_delay_elapses() {
        use std::time::Duration;

        let policy = ReconnectPolicy {
            backoff: Backoff::Fixed(Duration::from_secs(1)),
            jitter: Jitter(0.0),
            max_retries: None,
        };
        let mut state = ReconnectState::default();

        assert!(matches!(
            state.tick(&policy, Duration::from_millis(400), 0.0),
            ReconnectAction::Wait
        ));
        assert!(matches!(
            state.tick(&policy, Duration::from_millis(400), 0.0),
            ReconnectAction::Wait
        ));
        assert_eq!(state.retry_count, 0, "no attempt before the delay elapses");

        // 1200ms total >= 1s threshold → reconnect.
        assert!(matches!(
            state.tick(&policy, Duration::from_millis(400), 0.0),
            ReconnectAction::Reconnect
        ));
        assert_eq!(state.retry_count, 1);
        assert_eq!(
            state.elapsed,
            Duration::ZERO,
            "elapsed resets after an attempt"
        );
    }

    #[test]
    fn reconnect_threshold_grows_with_each_attempt() {
        use std::time::Duration;

        let policy = ReconnectPolicy {
            backoff: Backoff::Exponential {
                base: Duration::from_millis(500),
                factor: 2.0,
                max: Duration::from_secs(30),
            },
            jitter: Jitter(0.0),
            max_retries: None,
        };
        let mut state = ReconnectState::default();

        // Attempt 0: threshold delay(0) = 500ms.
        assert!(matches!(
            state.tick(&policy, Duration::from_millis(500), 0.0),
            ReconnectAction::Reconnect
        ));
        assert_eq!(state.retry_count, 1);

        // Attempt 1: threshold delay(1) = 1s — one 500ms tick is not enough.
        assert!(matches!(
            state.tick(&policy, Duration::from_millis(500), 0.0),
            ReconnectAction::Wait
        ));
        assert!(matches!(
            state.tick(&policy, Duration::from_millis(500), 0.0),
            ReconnectAction::Reconnect
        ));
        assert_eq!(state.retry_count, 2);
    }

    #[test]
    fn reconnect_gives_up_at_max_retries() {
        use std::time::Duration;

        let policy = ReconnectPolicy {
            backoff: Backoff::Fixed(Duration::from_millis(1)),
            jitter: Jitter(0.0),
            max_retries: Some(2),
        };
        let mut state = ReconnectState::default();

        assert!(matches!(
            state.tick(&policy, Duration::from_secs(1), 0.0),
            ReconnectAction::Reconnect
        ));
        assert!(matches!(
            state.tick(&policy, Duration::from_secs(1), 0.0),
            ReconnectAction::Reconnect
        ));
        assert_eq!(state.retry_count, 2);

        // retry (2) has reached max_retries (2) → give up, no further attempts.
        assert!(matches!(
            state.tick(&policy, Duration::from_secs(1), 0.0),
            ReconnectAction::GiveUp
        ));
        assert_eq!(state.retry_count, 2);
    }

    #[test]
    fn reset_returns_to_the_first_attempt() {
        use std::time::Duration;

        let policy = ReconnectPolicy {
            backoff: Backoff::Exponential {
                base: Duration::from_millis(500),
                factor: 2.0,
                max: Duration::from_secs(30),
            },
            jitter: Jitter(0.0),
            max_retries: None,
        };
        let mut state = ReconnectState::default();

        let _ = state.tick(&policy, Duration::from_millis(500), 0.0); // retry → 1
        let _ = state.tick(&policy, Duration::from_secs(1), 0.0); // retry → 2
        assert_eq!(state.retry_count, 2);

        state.reset();
        assert_eq!(state.retry_count, 0);
        assert_eq!(state.elapsed, Duration::ZERO);

        // First threshold is delay(0) = 500ms again.
        assert!(matches!(
            state.tick(&policy, Duration::from_millis(500), 0.0),
            ReconnectAction::Reconnect
        ));
        assert_eq!(state.retry_count, 1);
    }

    #[test]
    fn reconnect_system_reconnects_after_a_drop_once_backoff_elapses() {
        use std::time::Duration;

        let mut app = test_app(FakeConnectionDriver::default());
        app.insert_resource(ReconnectPolicy {
            backoff: Backoff::Fixed(Duration::from_secs(1)),
            jitter: Jitter(0.0),
            max_retries: None,
        });

        // Connect.
        app.world_mut().trigger(StdbConnect);
        app.update();
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);

        // Unsolicited drop: push Disconnected while intent stays Connected.
        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();
        sink.disconnected().unwrap();
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );

        // Before the 1s backoff elapses: no reconnect.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(500));
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected,
            "must not reconnect before the backoff elapses",
        );

        // Past the backoff: reconnect.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(600));
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Connected,
            "must reconnect once the backoff elapses",
        );
    }

    #[test]
    fn reconnect_system_does_not_reconnect_after_explicit_disconnect() {
        use std::time::Duration;

        let mut app = test_app(FakeConnectionDriver::default());
        app.insert_resource(ReconnectPolicy {
            backoff: Backoff::Fixed(Duration::from_millis(1)),
            jitter: Jitter(0.0),
            max_retries: None,
        });

        // Connect, then explicitly disconnect (intent → Disconnected).
        app.world_mut().trigger(StdbConnect);
        app.update();
        assert_eq!(*app.world().resource::<StdbStatus>(), StdbStatus::Connected);
        app.world_mut().trigger(StdbDisconnect);
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );

        // Well past the 1ms backoff: must stay disconnected — intent is Disconnected.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(5));
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected,
            "explicit disconnect must suppress auto-reconnect",
        );
    }

    /// A driver whose every connect *fails the build* (announces Connecting, then ConnectionError) —
    /// mirroring the real adapter when the server is down. Counts attempts so the retry loop is
    /// observable across repeated failures.
    #[derive(bevy::ecs::resource::Resource, Clone, Default)]
    struct FailingDriver {
        attempts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl StdbConnectionDriver for FailingDriver {
        type Conn = FakeConn;

        fn connect(&self, sink: crate::LifecycleSink<FakeConn>) {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            sink.connecting().unwrap();
            sink.connection_error(
                crate::lifecycle::lifecycle_events::ConnectionError::ConnectionRefused,
            )
            .unwrap();
        }

        fn tick(&self, _conn: &crate::StdbConnection<FakeConn>) {}

        fn disconnect(
            &self,
            _conn: &crate::StdbConnection<FakeConn>,
            sink: crate::LifecycleSink<FakeConn>,
        ) {
            sink.disconnected().unwrap();
        }
    }

    /// A connect attempt that ends in `ConnectionError` must return status to `Disconnected` and
    /// re-arm the backoff, so the engine keeps retrying until the server returns — not just once.
    /// (The real adapter regression: a failed build that only logs leaves status stuck at
    /// `Connecting`, stalling the loop after one attempt.)
    #[test]
    fn keeps_retrying_after_each_failed_connect() {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let driver = FailingDriver::default();
        let probe = driver.clone(); // shares the attempt counter
        let mut app = test_app(driver);
        app.insert_resource(ReconnectPolicy {
            backoff: Backoff::Fixed(Duration::from_secs(1)),
            jitter: Jitter(0.0),
            max_retries: None,
        });

        // Initial attempt fails → ConnectionError → Disconnected (reconnect armed).
        app.world_mut().trigger(StdbConnect);
        app.update();
        app.update();
        assert_eq!(probe.attempts.load(Ordering::Relaxed), 1);
        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected,
            "a failed connect must leave status Disconnected, not stuck Connecting",
        );

        // Each elapsed backoff window drives one more attempt.
        for expected in 2..=4 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(1100));
            app.update();
            app.update();
            assert!(
                probe.attempts.load(Ordering::Relaxed) >= expected,
                "must keep retrying after repeated failures (expected >= {expected})",
            );
        }
    }
}
