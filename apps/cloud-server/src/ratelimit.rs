//! Rate limiting.
//!
//! A per-key token bucket: every key (client IP or access token) may spend
//! `capacity` tokens per minute. Refills are continuous, so a client that
//! stays under the limit is never blocked, while a burst is smoothed out.
//!
//! The limiter is **in-process**. With more than one server instance each
//! instance enforces its own budget; that is a deliberate trade-off (no shared
//! state, no extra round trip) and is documented in the threat model.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// One token bucket.
#[derive(Debug, Clone)]
struct Bucket {
    /// Remaining tokens.
    tokens: f64,
    /// When the bucket was last refilled.
    updated: Instant,
}

/// In-memory token-bucket rate limiter.
#[derive(Debug)]
pub struct RateLimiter {
    capacity: f64,
    per_second: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    /// Creates a limiter that allows `per_minute` requests per key.
    pub fn per_minute(per_minute: u32) -> Self {
        let per_minute = f64::from(per_minute.max(1));
        Self {
            capacity: per_minute,
            per_second: per_minute / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Tries to spend one token for `key`.
    ///
    /// Returns the number of whole seconds the caller has to wait when the
    /// budget is exhausted, or `None` when the request may proceed.
    pub fn check(&self, key: &str) -> Option<u64> {
        self.check_at(key, Instant::now())
    }

    /// [`RateLimiter::check`] with an explicit clock, for tests.
    pub fn check_at(&self, key: &str, now: Instant) -> Option<u64> {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: self.capacity,
            updated: now,
        });

        let elapsed = now.saturating_duration_since(bucket.updated).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.per_second).min(self.capacity);
        bucket.updated = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            return None;
        }

        let missing = 1.0 - bucket.tokens;
        Some((missing / self.per_second).ceil() as u64)
    }

    /// Drops the bucket of a key (used after a successful authentication so a
    /// legitimate client is not punished for earlier failures).
    pub fn forget(&self, key: &str) {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        buckets.remove(key);
    }

    /// Number of tracked keys (test helper).
    pub fn tracked_keys(&self) -> usize {
        self.buckets.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn requests_within_the_budget_pass() {
        let limiter = RateLimiter::per_minute(3);
        for _ in 0..3 {
            assert!(limiter.check("1.2.3.4").is_none());
        }
    }

    #[test]
    fn the_budget_is_exhausted_and_recovers_over_time() {
        let limiter = RateLimiter::per_minute(60);
        let start = Instant::now();
        for _ in 0..60 {
            assert!(limiter.check("key").is_none());
        }
        let wait = limiter.check_at("key", start).expect("budget exhausted");
        assert!(wait >= 1);

        // One second refills exactly one token at 60 requests/minute.
        assert!(
            limiter
                .check_at("key", start + Duration::from_secs(1))
                .is_none()
        );
    }

    #[test]
    fn keys_are_independent() {
        let limiter = RateLimiter::per_minute(1);
        assert!(limiter.check("a").is_none());
        assert!(limiter.check("b").is_none());
        assert!(limiter.check("a").is_some());
    }
}
