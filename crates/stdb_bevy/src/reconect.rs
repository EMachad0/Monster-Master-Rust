use std::time::Duration;

use crate::backoff::{Backoff, Jitter};

pub struct ReconnectPolicy {
    backoff: Backoff,
    jitter: Jitter,
    max_retries: Option<u32>,
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
