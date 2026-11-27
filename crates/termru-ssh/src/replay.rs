//! Byte-offset replay ring.
//!
//! A bounded window over the most recent output of a session, used to give a
//! (re-)attaching view the same scrollback as the views that were already
//! there. The ring never loses track of *where* it is in the stream:
//! [`ReplayRing::total_written`] counts every byte ever pushed, so a caller
//! that takes a snapshot and registers a live consumer under one lock can
//! prove that the consumer saw exactly `snapshot ++ every later byte`, with no
//! gap and no duplicate.

use std::collections::VecDeque;

/// Bounded byte ring with a monotonic write offset.
#[derive(Debug, Clone)]
pub struct ReplayRing {
    buf: VecDeque<u8>,
    limit: usize,
    /// Bytes ever pushed. `total - buf.len()` is the stream offset of the
    /// oldest retained byte.
    total: u64,
}

impl ReplayRing {
    /// A ring that retains at most `limit` bytes (`0` retains nothing but
    /// still counts the offset).
    pub fn new(limit: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            limit,
            total: 0,
        }
    }

    /// Appends `data`, dropping the oldest bytes beyond the limit.
    ///
    /// Both the eviction (`drain` of a prefix) and the append (`extend` from a
    /// slice) are block operations on the underlying storage, not per-byte
    /// loops: this runs once per output chunk of a flooding session.
    pub fn push(&mut self, data: &[u8]) {
        self.total = self.total.saturating_add(data.len() as u64);
        if self.limit == 0 || data.is_empty() {
            return;
        }
        if data.len() >= self.limit {
            // The chunk alone fills the ring: older content is gone anyway.
            self.buf.clear();
            self.buf.extend(&data[data.len() - self.limit..]);
            return;
        }
        let excess = (self.buf.len() + data.len()).saturating_sub(self.limit);
        if excess > 0 {
            self.buf.drain(..excess);
        }
        self.buf.extend(data);
    }

    /// Number of bytes currently retained.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Maximum number of retained bytes.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Bytes ever pushed: the stream offset **after** the newest byte.
    pub fn total_written(&self) -> u64 {
        self.total
    }

    /// Stream offset of the oldest retained byte.
    pub fn start_offset(&self) -> u64 {
        self.total - self.buf.len() as u64
    }

    /// Copy of the retained bytes (oldest first).
    pub fn snapshot(&self) -> Vec<u8> {
        let (front, back) = self.buf.as_slices();
        let mut out = Vec::with_capacity(self.buf.len());
        out.extend_from_slice(front);
        out.extend_from_slice(back);
        out
    }

    /// Snapshot together with the stream offset it ends at. A consumer that
    /// registers under the same lock receives every byte from this offset on.
    pub fn snapshot_with_offset(&self) -> (Vec<u8>, u64) {
        (self.snapshot(), self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_newest_bytes_and_counts_the_offset() {
        let mut ring = ReplayRing::new(8);
        ring.push(b"abcd");
        ring.push(b"efgh");
        assert_eq!(ring.snapshot(), b"abcdefgh");
        ring.push(b"ij");
        assert_eq!(ring.snapshot(), b"cdefghij");
        assert_eq!(ring.total_written(), 10);
        assert_eq!(ring.start_offset(), 2);
        assert_eq!(ring.len(), 8);
    }

    #[test]
    fn a_chunk_larger_than_the_limit_replaces_everything() {
        let mut ring = ReplayRing::new(4);
        ring.push(b"xy");
        ring.push(b"0123456789");
        assert_eq!(ring.snapshot(), b"6789");
        assert_eq!(ring.total_written(), 12);
        assert_eq!(ring.start_offset(), 8);
    }

    #[test]
    fn a_zero_limit_retains_nothing_but_still_counts() {
        let mut ring = ReplayRing::new(0);
        ring.push(b"hello");
        assert!(ring.is_empty());
        assert_eq!(ring.total_written(), 5);
        assert_eq!(ring.start_offset(), 5);
    }

    #[test]
    fn snapshot_is_contiguous_after_the_storage_wraps() {
        let mut ring = ReplayRing::new(16);
        for round in 0..200u32 {
            let chunk: Vec<u8> = (0..7).map(|i| ((round + i) % 251) as u8).collect();
            ring.push(&chunk);
        }
        let snapshot = ring.snapshot();
        assert_eq!(snapshot.len(), 16);
        // The last chunk is the tail of the snapshot, whatever the wrap point.
        let last: Vec<u8> = (0..7).map(|i| ((199 + i) % 251) as u8).collect();
        assert!(snapshot.ends_with(&last));
    }

    /// Regression test (attach while streaming): a consumer registered at a
    /// snapshot offset sees `snapshot ++ later bytes` equal to the stream
    /// suffix, with no gap and no duplicate.
    #[test]
    fn snapshot_plus_later_bytes_equals_the_stream_suffix() {
        let stream: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let mut ring = ReplayRing::new(1024);
        let mut live: Vec<u8> = Vec::new();
        let mut registered_at: Option<u64> = None;
        for (index, chunk) in stream.chunks(37).enumerate() {
            if index == 60 {
                let (snapshot, offset) = ring.snapshot_with_offset();
                live.extend_from_slice(&snapshot);
                registered_at = Some(offset);
            }
            ring.push(chunk);
            if registered_at.is_some() && index >= 60 {
                live.extend_from_slice(chunk);
            }
        }
        let offset = registered_at.expect("registered") as usize;
        let retained_from = offset.saturating_sub(1024);
        assert_eq!(live, stream[retained_from..]);
    }
}
