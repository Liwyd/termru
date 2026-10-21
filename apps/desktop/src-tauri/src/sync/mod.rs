//! The sync engine: signs in, pushes local changes, pulls remote changes.
//!
//! The engine is deliberately boring: it owns no state of its own beyond a
//! cached transport, so the local database stays the single source of truth
//! for "what changed" and "what conflicts".
//!
//! Failure handling follows the offline-first rule: a failed cycle records the
//! error and leaves the queue untouched, so the next cycle retries. The
//! SSH engine is never affected by the state of the cloud service.

pub mod client;
pub mod tokens;

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use termru_core::sync::{PushMutation, PushRequest, RecordKind};
use termru_store::Store;

use crate::app_error::CommandError;
use crate::sync::client::{AuthResponse, HttpTransport, SyncError, SyncTransport};
use crate::sync::tokens::{KeyringTokens, TokenStorage};

/// The number of records a single pull page may contain.
const PULL_LIMIT: i64 = 200;

/// The settings of the sync feature. None of them is a secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncSettings {
    /// The URL of the sync server (empty = the feature is off).
    pub server_url: String,
    /// The account email.
    pub email: String,
    /// The display name of this installation.
    pub device_name: String,
    /// The stable id of this installation.
    pub device_id: String,
}

/// The state of the sync feature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncStatus {
    /// The current settings.
    pub settings: SyncSettings,
    /// Whether usable tokens are stored.
    pub signed_in: bool,
    /// The number of local changes waiting to be pushed.
    pub pending: usize,
    /// The number of unresolved conflicts.
    pub conflicts: usize,
    /// The revision the local database is up to.
    pub cursor: i64,
    /// When the last successful cycle ran.
    pub last_sync_at: Option<DateTime<Utc>>,
    /// The message of the last failed cycle.
    pub last_error: Option<String>,
}

/// The result of one sync cycle.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncOutcome {
    /// The number of mutations the server accepted.
    pub pushed: usize,
    /// The number of records the server sent.
    pub pulled: usize,
    /// The number of pulled records that were written locally.
    pub applied: usize,
    /// The number of conflicts that are open after the cycle.
    pub conflicts: usize,
    /// The revision the local database is up to.
    pub cursor: i64,
}

/// A conflict between the local and the remote copy of a record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConflictView {
    /// The id of the record.
    pub record_id: String,
    /// The kind of the record.
    pub kind: RecordKind,
    /// The revision the server reported.
    pub server_rev: i64,
    /// When the conflict was observed.
    pub observed_at: DateTime<Utc>,
    /// The local copy of the record.
    pub local: Option<serde_json::Value>,
    /// The copy the server holds.
    pub remote: Option<serde_json::Value>,
}

/// Where the transport comes from.
enum TransportSource {
    /// A fixed transport (used by the tests).
    Injected(Arc<dyn SyncTransport>),
    /// The transport of the currently configured server URL.
    Cached {
        url: String,
        transport: Arc<dyn SyncTransport>,
    },
}

/// Drives the sync between the local database and the cloud server.
pub struct SyncEngine {
    store: Arc<Store>,
    /// Where the tokens are kept.
    tokens: Arc<dyn TokenStorage>,
    /// The transport for the currently configured server URL (`None` until
    /// the first cycle).
    transport: Mutex<Option<TransportSource>>,
}

impl SyncEngine {
    /// Creates an engine for the given store.
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            tokens: Arc::new(KeyringTokens),
            transport: Mutex::new(None),
        }
    }

    /// Creates an engine with an injected transport and token store. Used by
    /// the tests to exercise the cycle without a network.
    pub fn with_transport(
        store: Arc<Store>,
        transport: Arc<dyn SyncTransport>,
        tokens: Arc<dyn TokenStorage>,
    ) -> Self {
        Self {
            store,
            tokens,
            transport: Mutex::new(Some(TransportSource::Injected(transport))),
        }
    }

    /// The current settings.
    pub fn settings(&self) -> Result<SyncSettings, StoreErrorLike> {
        let prefs = self.store.repos().prefs().device_prefs()?;
        Ok(SyncSettings {
            server_url: prefs.sync_url,
            email: prefs.sync_email,
            device_name: prefs.device_name,
            device_id: prefs.sync_device_id,
        })
    }

    /// Signs in and stores the tokens in the credential store.
    pub async fn sign_in(
        &self,
        server_url: &str,
        email: &str,
        password: &str,
        device_name: &str,
    ) -> Result<AuthResponse, SyncError> {
        let server_url = crate::sync::client::validate_server_url(server_url)?;
        if email.trim().is_empty() {
            return Err(SyncError::InvalidUrl(
                "the email address is empty".to_string(),
            ));
        }

        let device_id = self.device_id()?;
        let transport = self.transport_for(&server_url).await?;
        let response = transport
            .sign_in(email.trim(), password, &device_id, device_name)
            .await?;

        self.tokens
            .set(&response)
            .map_err(|error| SyncError::Store(error.message))?;

        self.save_settings(&server_url, email, device_name, &device_id)?;
        Ok(response)
    }

    /// Forgets the tokens and the settings.
    pub fn sign_out(&self) -> Result<(), CommandError> {
        self.tokens.delete()?;
        let mut prefs = self.store.repos().prefs().device_prefs()?;
        prefs.sync_url = String::new();
        prefs.sync_email = String::new();
        self.store.repos().prefs().set_device_prefs(&prefs)?;
        Ok(())
    }

    /// Runs one sync cycle: push the local changes, then pull the remote ones.
    pub async fn run_cycle(&self) -> Result<SyncOutcome, SyncError> {
        let settings = self.settings()?;
        if settings.server_url.is_empty() {
            return Err(SyncError::NotSignedIn);
        }
        let Some(tokens) = self.tokens.get() else {
            return Err(SyncError::NotSignedIn);
        };

        let transport = self.transport_for(&settings.server_url).await?;
        let mut outcome = SyncOutcome::default();

        // --- push ---------------------------------------------------------
        let queued = self.store.repos().sync().build_push()?;
        if !queued.is_empty() {
            let mutations: Vec<PushMutation> = queued
                .iter()
                .map(|record| PushMutation {
                    mutation_id: record.mutation_id,
                    id: record.record_id.clone(),
                    kind: record.kind,
                    base_rev: record.base_rev,
                    deleted: record.deleted,
                    payload: record.payload.clone(),
                    updated_at: record.updated_at,
                })
                .collect();
            let request = PushRequest { mutations };
            let response = transport.push(&tokens.access_token, request).await?;

            for accepted in &response.accepted {
                if let Some(record) = queued
                    .iter()
                    .find(|record| record.mutation_id == accepted.mutation_id)
                {
                    self.store.repos().sync().acknowledge(
                        &record.record_id,
                        record.kind,
                        accepted.rev,
                    )?;
                }
                outcome.pushed += 1;
            }

            for conflict in &response.conflicts {
                if let Some(server_record) = &conflict.server_record {
                    self.store
                        .repos()
                        .sync()
                        .observe_push_conflict(server_record)?;
                }
                outcome.conflicts += 1;
            }
        }

        // --- pull ---------------------------------------------------------
        let mut cursor = self.store.repos().sync().meta()?.cursor;
        loop {
            let page = transport
                .pull(&tokens.access_token, cursor, PULL_LIMIT)
                .await?;
            outcome.pulled += page.records.len();
            for record in &page.records {
                let outcome_of_record = self.store.repos().sync().apply_remote(record)?;
                if matches!(
                    outcome_of_record,
                    termru_store::ApplyOutcome::Applied | termru_store::ApplyOutcome::RemoteDeleted
                ) {
                    outcome.applied += 1;
                }
            }
            cursor = page.next_rev;
            if !page.has_more {
                break;
            }
        }

        self.store
            .repos()
            .sync()
            .record_sync_outcome(Some(cursor), None)?;
        // The stored cursor never moves backwards, so the outcome reports the
        // effective cursor and not the raw value of the last page.
        outcome.cursor = self.store.repos().sync().meta()?.cursor;
        outcome.conflicts = self.store.repos().conflicts().list()?.len();
        Ok(outcome)
    }

    /// The current status of the sync feature.
    pub fn status(&self) -> Result<SyncStatus, StoreErrorLike> {
        let settings = self.settings()?;
        let meta = self.store.repos().sync().meta()?;
        let pending = self.store.repos().sync().pending()?.len();
        let conflicts = self.store.repos().conflicts().list()?.len();
        Ok(SyncStatus {
            signed_in: self.tokens.get().is_some(),
            settings,
            pending,
            conflicts,
            cursor: meta.cursor,
            last_sync_at: meta.last_sync_at,
            last_error: meta.last_error,
        })
    }

    /// The open conflicts, with the local and the remote copy.
    pub fn conflicts(&self) -> Result<Vec<ConflictView>, StoreErrorLike> {
        let conflicts = self.store.repos().conflicts().list()?;
        let mut views = Vec::with_capacity(conflicts.len());
        for conflict in conflicts {
            let local = self
                .store
                .repos()
                .sync()
                .local_payload(conflict.kind, &conflict.record_id)?;
            views.push(ConflictView {
                record_id: conflict.record_id,
                kind: conflict.kind,
                server_rev: conflict.server_rev,
                observed_at: conflict.observed_at,
                local,
                remote: conflict.server_record.payload.clone(),
            });
        }
        Ok(views)
    }

    /// Resolves a conflict in favour of the local or the remote copy.
    pub fn resolve_conflict(
        &self,
        record_id: &str,
        keep_local: bool,
    ) -> Result<(), StoreErrorLike> {
        if keep_local {
            self.store.repos().sync().resolve_keep_local(record_id)?;
        } else {
            self.store.repos().sync().resolve_take_remote(record_id)?;
        }
        Ok(())
    }

    /// Records the message of a failed cycle, so the UI can show it.
    pub fn record_failure(&self, message: &str) {
        let _ = self
            .store
            .repos()
            .sync()
            .record_sync_outcome(None, Some(message.to_string()));
    }

    /// The transport for the given URL, reusing the cached one.
    async fn transport_for(&self, server_url: &str) -> Result<Arc<dyn SyncTransport>, SyncError> {
        {
            let cache = self.transport.lock().unwrap_or_else(|e| e.into_inner());
            match cache.as_ref() {
                Some(TransportSource::Injected(transport)) => return Ok(transport.clone()),
                Some(TransportSource::Cached { url, transport }) if url == server_url => {
                    return Ok(transport.clone());
                }
                _ => {}
            }
        }
        let transport: Arc<dyn SyncTransport> = Arc::new(HttpTransport::new(server_url)?);
        let mut cache = self.transport.lock().unwrap_or_else(|e| e.into_inner());
        *cache = Some(TransportSource::Cached {
            url: server_url.to_string(),
            transport: transport.clone(),
        });
        Ok(transport)
    }

    /// The stable id of this installation (generated on first use).
    fn device_id(&self) -> Result<String, SyncError> {
        let mut prefs = self.store.repos().prefs().device_prefs()?;
        if prefs.sync_device_id.is_empty() {
            prefs.sync_device_id = uuid::Uuid::new_v4().to_string();
            self.store.repos().prefs().set_device_prefs(&prefs)?;
        }
        Ok(prefs.sync_device_id)
    }

    /// Stores the settings in the device-local preferences.
    fn save_settings(
        &self,
        server_url: &str,
        email: &str,
        device_name: &str,
        device_id: &str,
    ) -> Result<(), StoreErrorLike> {
        let mut prefs = self.store.repos().prefs().device_prefs()?;
        prefs.sync_url = server_url.to_string();
        prefs.sync_email = email.trim().to_string();
        if !device_name.trim().is_empty() {
            prefs.device_name = device_name.trim().to_string();
        }
        prefs.sync_device_id = device_id.to_string();
        self.store.repos().prefs().set_device_prefs(&prefs)?;
        Ok(())
    }
}

/// A local error of the sync engine (the store refused an operation).
pub type StoreErrorLike = termru_store::StoreError;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use termru_core::Host;
    use termru_core::sync::{PullResponse, PushResponse};

    use crate::sync::tokens::MemoryTokens;

    /// A transport that records the calls and answers with fixed data.
    struct FakeTransport {
        pushed: Mutex<Vec<PushRequest>>,
        pull_since: Mutex<Vec<i64>>,
        pull_page: Mutex<PullResponse>,
        accepted: Mutex<Vec<(uuid::Uuid, i64)>>,
    }

    impl FakeTransport {
        fn new() -> Self {
            Self {
                pushed: Mutex::new(Vec::new()),
                pull_since: Mutex::new(Vec::new()),
                pull_page: Mutex::new(PullResponse {
                    records: Vec::new(),
                    next_rev: 0,
                    has_more: false,
                    server_time: Utc::now(),
                }),
                accepted: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl SyncTransport for FakeTransport {
        async fn sign_in(
            &self,
            _email: &str,
            _password: &str,
            _device_id: &str,
            _device_name: &str,
        ) -> Result<AuthResponse, SyncError> {
            Ok(AuthResponse {
                user_id: uuid::Uuid::nil(),
                device_id: uuid::Uuid::nil(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                access_expires_at: Utc::now(),
                refresh_expires_at: Utc::now(),
            })
        }

        async fn refresh(&self, _refresh_token: &str) -> Result<AuthResponse, SyncError> {
            Err(SyncError::NotSignedIn)
        }

        async fn pull(
            &self,
            _access_token: &str,
            since: i64,
            _limit: i64,
        ) -> Result<PullResponse, SyncError> {
            self.pull_since
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(since);
            Ok(self
                .pull_page
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        async fn push(
            &self,
            _access_token: &str,
            request: PushRequest,
        ) -> Result<PushResponse, SyncError> {
            let accepted = self
                .accepted
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            self.pushed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(request);
            Ok(PushResponse {
                accepted: accepted
                    .into_iter()
                    .map(|(mutation_id, rev)| termru_core::sync::AcceptedMutation {
                        mutation_id,
                        id: String::new(),
                        rev,
                    })
                    .collect(),
                conflicts: Vec::new(),
                server_time: Utc::now(),
            })
        }
    }

    fn engine_with(
        transport: Arc<FakeTransport>,
    ) -> (tempfile::TempDir, Arc<Store>, SyncEngine, Arc<MemoryTokens>) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Store::open(&dir.path().join("sync.sqlite")).expect("store"));
        let tokens = Arc::new(MemoryTokens::new());
        let engine = SyncEngine::with_transport(store.clone(), transport, tokens.clone());
        (dir, store, engine, tokens)
    }

    fn engine() -> (tempfile::TempDir, SyncEngine) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Store::open(&dir.path().join("sync.sqlite")).expect("store"));
        let engine = SyncEngine::new(store);
        (dir, engine)
    }

    /// Configures the engine as if the user had signed in.
    fn configure(store: &Store) {
        let mut prefs = store.repos().prefs().device_prefs().expect("prefs");
        prefs.sync_url = "https://sync.example.com".to_string();
        prefs.sync_email = "user@example.com".to_string();
        prefs.sync_device_id = uuid::Uuid::new_v4().to_string();
        store
            .repos()
            .prefs()
            .set_device_prefs(&prefs)
            .expect("store the prefs");
    }

    fn signed_in_tokens() -> AuthResponse {
        AuthResponse {
            user_id: uuid::Uuid::nil(),
            device_id: uuid::Uuid::nil(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            access_expires_at: Utc::now(),
            refresh_expires_at: Utc::now(),
        }
    }

    #[test]
    fn the_settings_default_to_disabled() {
        let (_dir, engine) = engine();
        let settings = engine.settings().expect("settings");
        assert!(settings.server_url.is_empty());
        assert!(settings.email.is_empty());
    }

    #[test]
    fn a_cycle_without_a_server_url_is_refused() {
        let (_dir, engine) = engine();
        let error = block_on(engine.run_cycle()).expect_err("must fail");
        assert!(matches!(error, SyncError::NotSignedIn));
    }

    #[test]
    fn the_status_reports_the_pending_changes() {
        let (_dir, engine) = engine();
        let mut host = Host::new(
            "web-1".to_string(),
            "web-1.example.com".to_string(),
            "deploy".to_string(),
        );
        engine
            .store
            .repos()
            .hosts()
            .save(&mut host)
            .expect("save the host");

        let status = engine.status().expect("status");
        assert_eq!(status.pending, 1);
        assert_eq!(status.conflicts, 0);
        assert!(!status.signed_in);
    }

    #[test]
    fn a_cycle_pushes_the_local_changes_and_clears_the_queue() {
        let transport = Arc::new(FakeTransport::new());
        let (_dir, store, engine, tokens) = engine_with(transport.clone());
        configure(&store);
        tokens.set(&signed_in_tokens()).expect("store the tokens");

        let mut host = Host::new(
            "web-1".to_string(),
            "web-1.example.com".to_string(),
            "deploy".to_string(),
        );
        store
            .repos()
            .hosts()
            .save(&mut host)
            .expect("save the host");

        let outcome = block_on(engine.run_cycle()).expect("the cycle must succeed");
        assert_eq!(outcome.pushed, 0, "the fake server accepts nothing");
        assert_eq!(outcome.pulled, 0);

        // The mutation was handed to the server.
        let pushed = transport.pushed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].mutations.len(), 1);
        assert_eq!(pushed[0].mutations[0].id, host.id.to_string());
        assert_eq!(pushed[0].mutations[0].kind, RecordKind::Host);

        // The pull started at the stored cursor.
        let since = transport
            .pull_since
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(since.len(), 1);
        assert_eq!(since[0], 0);
    }

    #[test]
    fn a_cycle_applies_the_records_the_server_sends() {
        let transport = Arc::new(FakeTransport::new());
        let (_dir, store, engine, tokens) = engine_with(transport.clone());
        configure(&store);
        tokens.set(&signed_in_tokens()).expect("store the tokens");

        // The server holds a host this device does not know yet.
        let mut remote = Host::new(
            "web-2".to_string(),
            "web-2.example.com".to_string(),
            "deploy".to_string(),
        );
        remote.id = uuid::Uuid::new_v4().into();
        let wire = termru_core::sync::WireRecord {
            id: remote.id.to_string(),
            kind: RecordKind::Host,
            rev: 4,
            updated_at: Utc::now(),
            deleted: false,
            payload: Some(serde_json::to_value(&remote).expect("serialize the host")),
        };
        {
            let mut page = transport
                .pull_page
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            page.records = vec![wire];
            page.next_rev = 4;
        }

        let outcome = block_on(engine.run_cycle()).expect("the cycle must succeed");
        assert_eq!(outcome.pulled, 1);
        assert_eq!(outcome.applied, 1);
        assert_eq!(outcome.cursor, 4);

        // The record is in the local database now.
        let stored = store
            .repos()
            .hosts()
            .list()
            .expect("list the hosts")
            .into_iter()
            .find(|host| host.id == remote.id)
            .expect("the host was applied");
        assert_eq!(stored.name, "web-2");
        assert_eq!(stored.server_rev, Some(4));
    }

    #[test]
    fn a_failed_cycle_keeps_the_queue_and_the_local_copies() {
        // A transport that always fails.
        struct FailingTransport;
        #[async_trait]
        impl SyncTransport for FailingTransport {
            async fn sign_in(
                &self,
                _email: &str,
                _password: &str,
                _device_id: &str,
                _device_name: &str,
            ) -> Result<AuthResponse, SyncError> {
                Err(SyncError::Network("connection refused".to_string()))
            }

            async fn refresh(&self, _refresh_token: &str) -> Result<AuthResponse, SyncError> {
                Err(SyncError::Network("connection refused".to_string()))
            }

            async fn pull(
                &self,
                _access_token: &str,
                _since: i64,
                _limit: i64,
            ) -> Result<PullResponse, SyncError> {
                Err(SyncError::Network("connection refused".to_string()))
            }

            async fn push(
                &self,
                _access_token: &str,
                _request: PushRequest,
            ) -> Result<PushResponse, SyncError> {
                Err(SyncError::Network("connection refused".to_string()))
            }
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Store::open(&dir.path().join("sync.sqlite")).expect("store"));
        let tokens = Arc::new(MemoryTokens::new());
        tokens.set(&signed_in_tokens()).expect("store the tokens");
        let engine = SyncEngine::with_transport(store.clone(), Arc::new(FailingTransport), tokens);

        let mut prefs = store.repos().prefs().device_prefs().expect("prefs");
        prefs.sync_url = "https://sync.example.com".to_string();
        store
            .repos()
            .prefs()
            .set_device_prefs(&prefs)
            .expect("store the prefs");

        let mut host = Host::new(
            "web-1".to_string(),
            "web-1.example.com".to_string(),
            "deploy".to_string(),
        );
        store
            .repos()
            .hosts()
            .save(&mut host)
            .expect("save the host");

        let error = block_on(engine.run_cycle()).expect_err("the cycle must fail");
        assert!(matches!(error, SyncError::Network(_)));

        // The local change is still queued: a later cycle pushes it.
        let pending = store.repos().sync().pending().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].record_id, host.id.to_string());
        let stored = store
            .repos()
            .hosts()
            .get(host.id)
            .expect("get")
            .expect("the host is still there");
        assert_eq!(stored.name, "web-1");
    }

    #[test]
    fn the_cursor_never_moves_backwards() {
        let transport = Arc::new(FakeTransport::new());
        let (_dir, store, engine, tokens) = engine_with(transport.clone());
        configure(&store);
        tokens.set(&signed_in_tokens()).expect("store the tokens");

        store
            .repos()
            .sync()
            .record_sync_outcome(Some(12), None)
            .expect("set the cursor");

        let outcome = block_on(engine.run_cycle()).expect("the cycle must succeed");
        assert_eq!(outcome.cursor, 12);
    }

    #[test]
    fn a_conflict_is_surfaced_and_can_be_resolved() {
        let transport = Arc::new(FakeTransport::new());
        let (_dir, store, engine, tokens) = engine_with(transport.clone());
        configure(&store);
        tokens.set(&signed_in_tokens()).expect("store the tokens");

        // A local host that the server also changed.
        let mut local = Host::new(
            "web-1".to_string(),
            "web-1.example.com".to_string(),
            "deploy".to_string(),
        );
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");
        store
            .repos()
            .sync()
            .acknowledge(&local.id.to_string(), RecordKind::Host, 3)
            .expect("acknowledge");

        local.name = "local-name".to_string();
        local.server_rev = Some(3);
        store
            .repos()
            .hosts()
            .save(&mut local)
            .expect("save the host");

        let mut remote = local.clone();
        remote.name = "remote-name".to_string();
        let wire = termru_core::sync::WireRecord {
            id: local.id.to_string(),
            kind: RecordKind::Host,
            rev: 4,
            updated_at: Utc::now(),
            deleted: false,
            payload: Some(serde_json::to_value(&remote).expect("serialize the host")),
        };
        {
            let mut page = transport
                .pull_page
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            page.records = vec![wire];
            page.next_rev = 4;
        }

        block_on(engine.run_cycle()).expect("the cycle must succeed");

        let conflicts = engine.conflicts().expect("conflicts");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].record_id, local.id.to_string());
        assert_eq!(conflicts[0].server_rev, 4);
        assert!(conflicts[0].local.is_some());
        assert!(conflicts[0].remote.is_some());

        // Keeping the local copy queues it again against the server revision.
        engine
            .resolve_conflict(&local.id.to_string(), true)
            .expect("resolve");
        assert!(engine.conflicts().expect("conflicts").is_empty());
        let push = store.repos().sync().build_push().expect("build the push");
        assert_eq!(push.len(), 1);
        assert_eq!(push[0].base_rev, Some(4));
    }

    #[test]
    fn signing_in_stores_the_tokens_and_the_settings() {
        let transport = Arc::new(FakeTransport::new());
        let (_dir, _store, engine, tokens) = engine_with(transport.clone());

        block_on(engine.sign_in(
            "https://sync.example.com",
            "user@example.com",
            "a long passphrase",
            "my laptop",
        ))
        .expect("sign in");

        assert!(tokens.get().is_some());
        let settings = engine.settings().expect("settings");
        assert_eq!(settings.server_url, "https://sync.example.com");
        assert_eq!(settings.email, "user@example.com");
        assert_eq!(settings.device_name, "my laptop");
        assert!(!settings.device_id.is_empty());
    }

    #[test]
    fn signing_in_refuses_an_insecure_server_url() {
        let transport = Arc::new(FakeTransport::new());
        let (_dir, _store, engine, _tokens) = engine_with(transport);
        let error = block_on(engine.sign_in(
            "http://sync.example.com",
            "user@example.com",
            "pw",
            "laptop",
        ))
        .expect_err("must be refused");
        assert!(error.to_string().contains("https"));
    }

    #[test]
    fn signing_out_forgets_the_tokens() {
        let transport = Arc::new(FakeTransport::new());
        let (_dir, store, engine, tokens) = engine_with(transport.clone());
        configure(&store);
        tokens.set(&signed_in_tokens()).expect("store the tokens");

        engine.sign_out().expect("sign out");
        assert!(tokens.get().is_none());
        assert!(engine.settings().expect("settings").server_url.is_empty());
    }

    /// Runs a future on a current-thread runtime: the engine's async parts
    /// only await a transport that never blocks.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(future)
    }
}
