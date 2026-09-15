//! Speed caps shared by every transfer in one direction.
//!
//! A cap is split evenly between the transfers running at the moment: with a 1 MiB/s
//! upload cap and two uploads, each paces itself to 512 KiB/s. Each transfer sleeps
//! just long enough to stay on its share, so bursts smooth out without a central
//! scheduler. The cap also counts bytes, for statistics.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::time::{Instant, sleep};

#[derive(Debug, Default)]
pub struct SpeedCap {
    /// Bytes per second; 0 means no cap.
    limit: AtomicU64,
    active: AtomicUsize,
    total: AtomicU64,
}

impl SpeedCap {
    pub fn set(&self, bytes_per_second: Option<u64>) {
        self.limit.store(bytes_per_second.unwrap_or(0), Ordering::Relaxed);
    }

    /// Everything transferred through this cap since the client started.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Transfers running right now.
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }

    /// Start pacing one transfer. Dropping the pace ends it.
    pub fn start(self: &Arc<Self>) -> Pace {
        self.active.fetch_add(1, Ordering::Relaxed);
        Pace { cap: self.clone(), started: Instant::now(), bytes: 0 }
    }
}

#[derive(Debug)]
pub struct Pace {
    cap: Arc<SpeedCap>,
    started: Instant,
    bytes: u64,
}

impl Pace {
    /// Record `n` bytes and wait if this transfer is ahead of its share of the cap.
    #[allow(clippy::cast_precision_loss, reason = "pacing is approximate")]
    pub async fn record(&mut self, n: usize) {
        self.bytes += n as u64;
        self.cap.total.fetch_add(n as u64, Ordering::Relaxed);
        let limit = self.cap.limit.load(Ordering::Relaxed);
        if limit == 0 {
            return;
        }
        let share = limit as f64 / self.cap.active().max(1) as f64;
        let due = Duration::from_secs_f64(self.bytes as f64 / share);
        let elapsed = self.started.elapsed();
        if let Some(ahead) = due.checked_sub(elapsed) {
            sleep(ahead.min(Duration::from_secs(1))).await;
        }
    }
}

impl Drop for Pace {
    fn drop(&mut self) {
        self.cap.active.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn splits_the_cap_between_transfers() {
        let cap = Arc::new(SpeedCap::default());
        cap.set(Some(1000));
        let mut a = cap.start();
        let _b = cap.start();
        let started = Instant::now();
        // 1000 bytes at a 500 B/s share takes about two seconds.
        for _ in 0..10 {
            a.record(100).await;
        }
        assert!(started.elapsed() >= Duration::from_millis(1900), "{:?}", started.elapsed());
        assert_eq!(cap.total(), 1000);
        drop(a);
        assert_eq!(cap.active(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn no_cap_never_waits() {
        let cap = Arc::new(SpeedCap::default());
        let mut pace = cap.start();
        let started = Instant::now();
        pace.record(10_000_000).await;
        assert_eq!(started.elapsed(), Duration::ZERO);
    }
}
