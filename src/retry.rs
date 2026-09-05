use clap::ValueEnum;
use std::{
    cmp::min,
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq)]
pub enum RetryDecision {
    Wait(Duration),
    GiveUp,
}

pub trait RetryPolicy {
    fn on_failure(&mut self) -> RetryDecision;
}

pub struct ExponentialRetryPolicy {
    attempt: u32,
    start: Instant,
    reset_after_sec: u64,
}

impl Default for ExponentialRetryPolicy {
    fn default() -> Self {
        ExponentialRetryPolicy::new(ExponentialRetryPolicy::DEFAULT_RESET_AFTER)
    }
}

impl ExponentialRetryPolicy {
    const DEFAULT_RESET_AFTER: u64 = 120;

    pub fn new(reset_after: u64) -> ExponentialRetryPolicy {
        ExponentialRetryPolicy {
            attempt: 0,
            start: Instant::now(),
            reset_after_sec: reset_after,
        }
    }
}

impl RetryPolicy for ExponentialRetryPolicy {
    fn on_failure(&mut self) -> RetryDecision {
        if self.start.elapsed().as_secs() >= self.reset_after_sec {
            self.start = Instant::now();
            self.attempt = 0;
        }

        let duration_sec = 1 + 2_u64.pow(min(self.attempt, 8));
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
}

pub struct NoRetryPolicy;

impl RetryPolicy for NoRetryPolicy {
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
    use std::{thread::sleep, time::Duration};

    use crate::retry::{ExponentialRetryPolicy, RetryDecision, RetryPolicy};

    #[test]
    fn test_exponential_policy() {
        let mut policy = ExponentialRetryPolicy::default();

        // 1 + 2^0 = 2
        let decision = policy.on_failure();
        let RetryDecision::Wait(duration) = decision else {
            panic!()
        };
        assert!(duration >= Duration::from_secs(2));
        assert!(duration <= Duration::from_secs(4));

        // 1 + 2^1 = 3
        let decision = policy.on_failure();
        let RetryDecision::Wait(duration) = decision else {
            panic!()
        };
        assert!(duration >= Duration::from_secs(3));
        assert!(duration <= Duration::from_secs(5));

        // 1 + 2^7 = 129
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let _ = policy.on_failure();
        let decision = policy.on_failure();
        let RetryDecision::Wait(duration) = decision else {
            panic!()
        };
        // should cap at 120
        assert_eq!(duration, Duration::from_secs(120));
    }

    #[test]
    fn test_retry_after_sec_triggers_reset() {
        let mut policy = ExponentialRetryPolicy::new(1);

        // 1 + 2^1
        let _ = policy.on_failure();
        let decision = policy.on_failure();
        let RetryDecision::Wait(duration) = decision else {
            panic!()
        };
        assert!(duration >= Duration::from_secs(3));
        assert!(duration <= Duration::from_secs(5));

        // 1 + 2^0 after reset
        sleep(Duration::from_secs(1));
        let decision = policy.on_failure();
        let RetryDecision::Wait(duration) = decision else {
            panic!()
        };
        assert!(duration >= Duration::from_secs(2));
        assert!(duration <= Duration::from_secs(4));
    }
}
