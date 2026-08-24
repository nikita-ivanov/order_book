use chrono::{DateTime, Utc};
use clap::ValueEnum;
use std::{
    cmp::{max, min},
    time::Duration,
};

#[derive(Debug, PartialEq, PartialOrd)]
pub enum RetryDecision {
    Wait(Duration),
    GiveUp,
}

pub trait RetryPolicy {
    fn on_success(&mut self);
    fn on_failure(&mut self) -> RetryDecision;
}

pub struct ExponentialRetryPolicy {
    attempt: u32,
    start: DateTime<Utc>,
}

impl ExponentialRetryPolicy {
    pub fn new() -> ExponentialRetryPolicy {
        ExponentialRetryPolicy {
            attempt: 0,
            start: DateTime::UNIX_EPOCH,
        }
    }
}

impl RetryPolicy for ExponentialRetryPolicy {
    fn on_failure(&mut self) -> RetryDecision {
        let elapsed = DateTime::UNIX_EPOCH - self.start;

        if elapsed.num_seconds() > 120 {
            self.start = DateTime::UNIX_EPOCH;
        }

        let duration_sec = 1 + 2_u64.pow(self.attempt);
        println!("duration sec {duration_sec}");
        let jitter_ms = rand::random_range(0..2000);
        println!("jitter {jitter_ms}");

        // cap at 2 mins
        let uncapped_duration = duration_sec * 1000 + jitter_ms;
        println!("uncapped duration {uncapped_duration}");
        let sleep_duration_ms = min(uncapped_duration, 120_000);
        println!("sleep duration ms {sleep_duration_ms}");

        self.attempt += 1;

        RetryDecision::Wait(Duration::from_millis(sleep_duration_ms))
    }

    fn on_success(&mut self) {
        self.start = DateTime::UNIX_EPOCH;
    }
}

pub struct NoRetryPolicy;

impl RetryPolicy for NoRetryPolicy {
    fn on_success(&mut self) {}
    fn on_failure(&mut self) -> RetryDecision {
        RetryDecision::GiveUp
    }
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum RetryPolicyKind {
    Exponential,
    NoRetry,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::retry::{ExponentialRetryPolicy, RetryDecision, RetryPolicy};

    #[test]
    fn test_exponential_policy() {
        let mut policy = ExponentialRetryPolicy::new();

        // 1 + 2^0
        let decision = policy.on_failure();
        assert!(decision > RetryDecision::Wait(Duration::from_secs(2)));
        assert!(decision < RetryDecision::Wait(Duration::from_secs(4)));

        // 1 + 2^1
        let decision = policy.on_failure();
        assert!(decision > RetryDecision::Wait(Duration::from_secs(3)));
        assert!(decision < RetryDecision::Wait(Duration::from_secs(5)));

        // 1 + 2^7
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let decision = policy.on_failure();
        assert_eq!(decision, RetryDecision::Wait(Duration::from_secs(120)));
    }
}
