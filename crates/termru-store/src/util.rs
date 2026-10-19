//! Small shared helpers.

use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Utc};

/// Epoch milliseconds → UTC datetime.
pub(crate) fn datetime_from_ms(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or(DateTime::UNIX_EPOCH)
}

pub(crate) fn datetime_to_ms(dt: &DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

/// Process-wide monotonic source for [`now_ms`].
static LAST_MS: AtomicI64 = AtomicI64::new(0);

/// Epoch milliseconds, **strictly increasing across calls in this process**.
///
/// Raw wall-clock milliseconds collide within the same millisecond, which
/// makes recency ordering (`recents`, `workspaces`, sync queue) and
/// last-write bookkeeping non-deterministic: the correct row could lose an
/// `ORDER BY … DESC` tie. Each call therefore returns
/// `max(wall_clock, last + 1)`; drift is bounded by same-millisecond bursts
/// and self-corrects once the wall clock catches up (and never runs
/// backwards across a clock step).
///
/// Single-writer assumption: the desktop app is single-instance, so one
/// process owns the database. Cross-process writers would not share this
/// sequence and could still tie — do not rely on concurrent external writers.
pub(crate) fn now_ms() -> i64 {
    let wall = Utc::now().timestamp_millis();
    let mut last = LAST_MS.load(Ordering::Relaxed);
    loop {
        let next = wall.max(last + 1);
        match LAST_MS.compare_exchange_weak(last, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(observed) => last = observed,
        }
    }
}

pub(crate) fn bool_to_int(b: bool) -> i64 {
    i64::from(b)
}

#[cfg(test)]
mod tests {
    use super::now_ms;

    #[test]
    fn now_ms_is_strictly_increasing_within_a_process() {
        // The property ordering depends on: successive calls in one process
        // never collide, regardless of wall-clock granularity or other tests
        // calling concurrently in this process.
        let values: Vec<i64> = (0..64).map(|_| now_ms()).collect();
        for pair in values.windows(2) {
            assert!(pair[0] < pair[1], "not strictly increasing: {pair:?}");
        }
    }
}
