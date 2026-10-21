//! The background sync task.
//!
//! The desktop app syncs on its own: every few minutes the queue is pushed
//! and the changes of the other devices are pulled. A failure is retried
//! with a bounded backoff, so a server that is down does not turn into a
//! busy loop, and a server that comes back is picked up without the user
//! doing anything.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::sync::SyncEngine;
use crate::sync::client::SyncError;

/// How long the app waits between two cycles.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// The shortest backoff after a failure.
const MIN_BACKOFF: Duration = Duration::from_secs(30);

/// The longest backoff after a failure.
const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);

/// Stops the background sync.
pub struct SyncScheduler {
    cancel: watch::Sender<bool>,
}

impl SyncScheduler {
    /// Asks the background task to stop.
    pub fn shutdown(&self) {
        let _ = self.cancel.send(true);
    }
}

/// Starts the background sync.
///
/// The task stops when the returned handle is asked to shut down.
pub fn spawn(engine: Arc<SyncEngine>, interval: Duration) -> SyncScheduler {
    let (cancel, mut cancelled) = watch::channel(false);
    let scheduler = SyncScheduler { cancel };

    tokio::spawn(async move {
        let mut backoff = MIN_BACKOFF;
        loop {
            // Wait for the next cycle (or for the shutdown).
            tokio::select! {
                _ = cancelled.changed() => {
                    if *cancelled.borrow() {
                        tracing::info!("the background sync stopped");
                        return;
                    }
                }
                _ = tokio::time::sleep(interval) => {}
            }

            match engine.run_cycle().await {
                Ok(outcome) => {
                    backoff = MIN_BACKOFF;
                    if outcome.pushed > 0 || outcome.applied > 0 {
                        tracing::info!(
                            pushed = outcome.pushed,
                            applied = outcome.applied,
                            conflicts = outcome.conflicts,
                            "background sync finished"
                        );
                    }
                }
                Err(SyncError::NotSignedIn) => {
                    // Not an error: the feature is simply switched off.
                    backoff = MIN_BACKOFF;
                }
                Err(error) => {
                    tracing::warn!(%error, ?backoff, "the background sync failed");
                    engine.record_failure(&error.to_string());
                    tokio::select! {
                        _ = cancelled.changed() => {
                            if *cancelled.borrow() {
                                return;
                            }
                        }
                        _ = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
        }
    });

    scheduler
}
