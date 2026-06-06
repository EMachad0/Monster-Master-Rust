use std::time::Duration;

pub enum Backoff {
    Fixed(Duration),
    Exponential {
        base: Duration,
        factor: f64,
        max: Duration,
    },
}

impl Backoff {
    pub fn delay(&self, retry_count: u32) -> Duration {
        match self {
            Backoff::Fixed(d) => *d,
            Backoff::Exponential { base, factor, max } => {
                let delay_secs = base.as_secs_f64() * factor.powi(retry_count as i32);
                let delay_duration =
                    Duration::try_from_secs_f64(delay_secs).unwrap_or(Duration::MAX);
                delay_duration.min(*max)
            }
        }
    }
}

fn with_jitter(base: Duration, jitter: f64, sample: f64) -> Duration {
    let scale = 1.0 - jitter + 2.0 * jitter * sample;
    let secs = base.as_secs_f64() * scale;
    Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_backoff_is_constant() {
        use std::time::Duration;

        let backoff = Backoff::Fixed(Duration::from_secs(2));
        assert_eq!(backoff.delay(0), Duration::from_secs(2));
        assert_eq!(backoff.delay(5), Duration::from_secs(2));
    }

    #[test]
    fn exponential_backoff_grows_by_factor_and_caps_at_max() {
        use std::time::Duration;

        let backoff = Backoff::Exponential {
            base: Duration::from_millis(500),
            factor: 2.0,
            max: Duration::from_secs(30),
        };
        assert_eq!(backoff.delay(0), Duration::from_millis(500));
        assert_eq!(backoff.delay(1), Duration::from_secs(1));
        assert_eq!(backoff.delay(2), Duration::from_secs(2));
        assert_eq!(backoff.delay(3), Duration::from_secs(4));
        assert_eq!(backoff.delay(20), Duration::from_secs(30)); // capped at max
    }

    #[test]
    fn exponential_backoff_does_not_overflow_at_high_retry() {
        use std::time::Duration;

        let backoff = Backoff::Exponential {
            base: Duration::from_millis(500),
            factor: 2.0,
            max: Duration::from_secs(30),
        };
        // With unlimited retries and a capped delay, retry_count grows without bound.
        // A very large retry must still clamp to `max`, never overflow/panic.
        assert_eq!(backoff.delay(100), Duration::from_secs(30));
    }

    #[test]
    fn jitter_zero_returns_base_for_any_sample() {
        use std::time::Duration;

        let base = Duration::from_secs(1);
        assert_eq!(with_jitter(base, 0.0, 0.0), base);
        assert_eq!(with_jitter(base, 0.0, 0.5), base);
        assert_eq!(with_jitter(base, 0.0, 0.99), base);
    }

    #[test]
    fn jitter_maps_sample_to_symmetric_bounds() {
        use std::time::Duration;

        // jitter 0.5 → scale in [0.5, 1.5]; values chosen to be exact in f64.
        let base = Duration::from_secs(1);
        assert_eq!(with_jitter(base, 0.5, 0.0), Duration::from_millis(500)); // lower bound
        assert_eq!(with_jitter(base, 0.5, 0.5), Duration::from_secs(1)); // midpoint = base
        assert_eq!(with_jitter(base, 0.5, 1.0), Duration::from_millis(1500)); // upper bound
    }

    #[test]
    fn jitter_stays_within_bounds_for_any_sample() {
        use std::time::Duration;

        let base = Duration::from_secs(10);
        let jitter = 0.2;
        let lower = base.mul_f64(1.0 - jitter);
        let upper = base.mul_f64(1.0 + jitter);
        for i in 0..=10 {
            let sample = i as f64 / 10.0;
            let delay = with_jitter(base, jitter, sample);
            assert!(
                delay >= lower && delay <= upper,
                "sample {sample}: {delay:?} not in [{lower:?}, {upper:?}]",
            );
        }
    }
}
