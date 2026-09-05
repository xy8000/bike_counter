//! A thread-safe minimum-interval rate limiter used by the scraping client.
//!
//! Scraping dozens of pages per import run must be gentle on the target server;
//! the default is **one request per second** ([`RateLimiter::new(1.0)`]). The
//! limiter enforces a minimum interval between consecutive requests and can be
//! built with a zero interval (or disabled) for tests.

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

/// Enforces a minimum interval between [`acquire`](RateLimiter::acquire) calls.
pub struct RateLimiter {
    min_interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl RateLimiter {
    /// Builds a limiter for `requests_per_second` (a non-positive value
    /// disables throttling — no requests per second means no sleep).
    pub fn new(requests_per_second: f64) -> Self {
        let min_interval = if requests_per_second.is_finite() && requests_per_second > 0.0 {
            Duration::from_secs_f64(1.0 / requests_per_second)
        } else {
            Duration::ZERO
        };
        Self {
            min_interval,
            last: Mutex::new(None),
        }
    }

    /// Blocks until the minimum interval since the previous call has elapsed.
    ///
    /// The slot is reserved *before* sleeping (`now + wait`), so concurrent
    /// callers queue behind each other instead of bursting once the lock is
    /// released.
    pub fn acquire(&self) {
        let mut last = self.last.lock().unwrap();
        let now = Instant::now();
        let wait = match *last {
            Some(previous) => {
                let elapsed = now.duration_since(previous);
                if elapsed < self.min_interval {
                    self.min_interval - elapsed
                } else {
                    Duration::ZERO
                }
            }
            None => Duration::ZERO,
        };
        *last = Some(now + wait);
        drop(last);
        if !wait.is_zero() {
            thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::RateLimiter;

    #[test]
    fn zero_interval_does_not_block() {
        let limiter = RateLimiter::new(0.0);
        let start = Instant::now();
        limiter.acquire();
        limiter.acquire();
        assert!(start.elapsed() < std::time::Duration::from_millis(50));
    }

    #[test]
    fn enforces_minimum_interval() {
        let limiter = RateLimiter::new(200.0); // 5 ms between requests
        limiter.acquire();
        let start = Instant::now();
        limiter.acquire();
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(4),
            "second acquire should have waited at least ~5ms"
        );
    }

    #[test]
    fn negative_or_nan_disables_throttling() {
        assert!(RateLimiter::new(-1.0).min_interval.is_zero());
        assert!(RateLimiter::new(f64::NAN).min_interval.is_zero());
    }
}
