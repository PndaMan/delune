//! Search rate limiting.
//!
//! The Soulseek server temporarily bans accounts that search too often. We keep a
//! sliding window of recent search times and refuse to send more than `max` within
//! `window`. The default, 34 searches per 220 seconds, is the limit sockseek (formerly
//! sldl) has used for years without triggering bans.
//!
//! The limiter is plain data with an injected clock so it can be tested without
//! waiting; the client turns a refusal into a sleep.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

pub const DEFAULT_MAX_SEARCHES: usize = 34;
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(220);

#[derive(Debug)]
pub struct SearchLimiter {
    max: usize,
    window: Duration,
    sent: VecDeque<Instant>,
}

impl SearchLimiter {
    /// # Panics
    /// If `max` is zero.
    #[must_use]
    pub fn new(max: usize, window: Duration) -> Self {
        assert!(max > 0, "a search limit of zero would block forever");
        Self { max, window, sent: VecDeque::with_capacity(max) }
    }

    /// Record a search at `now` if allowed; otherwise return how long to wait.
    pub fn try_acquire(&mut self, now: Instant) -> Result<(), Duration> {
        while self.sent.front().is_some_and(|&t| now.duration_since(t) >= self.window) {
            self.sent.pop_front();
        }
        if self.sent.len() < self.max {
            self.sent.push_back(now);
            return Ok(());
        }
        let oldest = self.sent.front().copied().unwrap_or(now);
        Err(self.window.saturating_sub(now.duration_since(oldest)))
    }
}

impl Default for SearchLimiter {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SEARCHES, DEFAULT_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_then_reports_wait() {
        let start = Instant::now();
        let mut limiter = SearchLimiter::new(3, Duration::from_secs(10));
        for i in 0..3 {
            assert!(limiter.try_acquire(start + Duration::from_secs(i)).is_ok());
        }
        // Fourth search at t=3s must wait until the first one (t=0) leaves the window.
        assert_eq!(limiter.try_acquire(start + Duration::from_secs(3)), Err(Duration::from_secs(7)));
        // At t=10s the first slot has expired.
        assert!(limiter.try_acquire(start + Duration::from_secs(10)).is_ok());
        assert!(limiter.try_acquire(start + Duration::from_secs(10)).is_err());
    }

    #[test]
    fn refusals_are_not_counted() {
        let start = Instant::now();
        let mut limiter = SearchLimiter::new(1, Duration::from_secs(5));
        assert!(limiter.try_acquire(start).is_ok());
        for _ in 0..10 {
            assert!(limiter.try_acquire(start + Duration::from_secs(1)).is_err());
        }
        assert!(limiter.try_acquire(start + Duration::from_secs(5)).is_ok());
    }
}
