//! Small shared helpers.

use chrono::{DateTime, Utc};

/// Epoch milliseconds → UTC datetime.
pub(crate) fn datetime_from_ms(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or(DateTime::UNIX_EPOCH)
}

pub(crate) fn datetime_to_ms(dt: &DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

pub(crate) fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub(crate) fn bool_to_int(b: bool) -> i64 {
    i64::from(b)
}
