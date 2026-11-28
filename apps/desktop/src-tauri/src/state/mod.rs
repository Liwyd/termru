mod auth_prompts;
mod host_keys;
mod prompts;
pub(crate) mod sessions;

pub use auth_prompts::AuthPrompts;
pub(crate) use auth_prompts::ENGINE_PROMPT_TIMEOUT;
pub use host_keys::HostKeyPrompts;
pub use sessions::{ForwardUpdate, SessionInfo, SessionManager, SessionStatusDto, SessionUpdate};

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use termru_ssh::ConnectParams;
use termru_store::Store;

use self::prompts::{PromptEmitter, SessionDirectory, TauriEmitter};
use crate::sync::SyncEngine;
use crate::sync::scheduler::SyncScheduler;

/// Name of the frontend event emitted when a sync cycle applied remote
/// changes (hosts, groups, prefs, ... may have changed under the UI).
pub const SYNC_APPLIED_EVENT: &str = "sync-applied";

/// Payload of the [`SYNC_APPLIED_EVENT`] event.
#[derive(Debug, Clone, Serialize)]
pub struct SyncApplied {
    /// How many remote changes were applied locally (always at least 1).
    pub received: usize,
}

/// The authentication questions of a jump host: the session's own prompt, with
/// the question saying which host asks (the dialog shows the session's host,
/// which is not the one that wants a code).
struct HopPrompt {
    label: String,
    inner: Arc<dyn termru_ssh::AuthPrompt>,
}

#[async_trait::async_trait]
impl termru_ssh::AuthPrompt for HopPrompt {
    async fn ask(
        &self,
        mut request: termru_ssh::AuthPromptRequest,
    ) -> termru_ssh::AuthPromptResponse {
        let whose = format!("Jump host “{}”", self.label);
        request.instructions = if request.instructions.trim().is_empty() {
            whose
        } else {
            format!("{whose}\n{}", request.instructions)
        };
        self.inner.ask(request).await
    }

    fn is_interactive(&self) -> bool {
        self.inner.is_interactive()
    }
}

/// Everything the command layer needs, managed by Tauri and shared by all
/// windows (single owner model: one store, one session manager).
pub struct AppState {
    pub store: Arc<Store>,
    pub sessions: Arc<SessionManager>,
    pub host_keys: Arc<HostKeyPrompts>,
    pub auth_prompts: Arc<AuthPrompts>,
    pub known_hosts_path: PathBuf,
    /// The user's home directory, when the platform tells (where `~/.ssh`
    /// lives).
    pub home_dir: Option<PathBuf>,
    /// The user's own `~/.ssh/known_hosts`: a **read-only** extra source of
    /// trusted host keys (see `ConnectParams::extra_known_hosts`). The app's
    /// own file stays the only one that is written.
    pub system_known_hosts: Option<PathBuf>,
    /// Where the keys Termru generates for identities are written.
    pub keys_dir: PathBuf,
    /// The cloud sync engine (off until the user signs in).
    pub sync: Arc<SyncEngine>,
    /// The background sync task.
    pub sync_scheduler: SyncScheduler,
}

impl AppState {
    pub fn new(store: Store, handle: AppHandle, data_dir: PathBuf) -> Self {
        let store = Arc::new(store);
        let sync = Arc::new(SyncEngine::new(store.clone()));
        // Both the background scheduler and `sync_now` go through
        // `SyncEngine::run_cycle`, so one hook covers every pull.
        let home_dir = handle.path().home_dir().ok();
        let notifier = handle.clone();
        sync.set_applied_hook(Arc::new(move |received| {
            if let Err(error) = notifier.emit(SYNC_APPLIED_EVENT, SyncApplied { received }) {
                tracing::warn!(%error, "could not emit the sync-applied event");
            }
        }));
        // The background sync starts right away; it does nothing until the
        // user signs in.
        let sync_scheduler =
            crate::sync::scheduler::spawn(sync.clone(), crate::sync::scheduler::DEFAULT_INTERVAL);
        let sessions = Arc::new(SessionManager::new());
        let directory: Arc<dyn SessionDirectory> = sessions.clone();
        let emitter: Arc<dyn PromptEmitter> = Arc::new(TauriEmitter { app: handle });
        let host_keys = Arc::new(HostKeyPrompts::new(directory.clone(), emitter.clone()));
        let auth_prompts = Arc::new(AuthPrompts::new(directory, emitter));
        // A question belongs to its session: when the session goes, so does
        // the question (and the dialog that shows it).
        {
            let host_keys = host_keys.clone();
            let auth_prompts = auth_prompts.clone();
            sessions.set_end_hook(Arc::new(move |session_id| {
                host_keys.cancel_session(session_id);
                auth_prompts.cancel_session(session_id);
            }));
        }
        Self {
            known_hosts_path: data_dir.join("known_hosts"),
            system_known_hosts: home_dir
                .as_ref()
                .map(|home| home.join(".ssh").join("known_hosts")),
            home_dir,
            keys_dir: data_dir.join("keys"),
            sync,
            sync_scheduler,
            store,
            sessions,
            host_keys,
            auth_prompts,
        }
    }

    /// Binds a connection's questions (host key, authentication) to its
    /// session, so they carry the session id and reach the windows that show
    /// it. Every way of starting a session goes through here, a duplicated
    /// session included (it gets its own id).
    pub fn scope_prompts(&self, mut params: ConnectParams, session_id: &str) -> ConnectParams {
        params.host_key_prompt = self.host_keys.for_session(session_id);
        params.auth_prompt = self.auth_prompts.for_session(session_id);
        // Every jump host asks its own questions (its host key, its
        // authentication) through the same session: the dialog names the
        // host the question is about.
        for hop in &mut params.setup.jump {
            hop.params.host_key_prompt = self.host_keys.for_session(session_id);
            hop.params.auth_prompt = Arc::new(HopPrompt {
                label: hop.label.clone(),
                inner: self.auth_prompts.for_session(session_id),
            });
        }
        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termru_ssh::{AuthPrompt, AuthPromptKind, AuthPromptRequest, AuthPromptResponse};

    /// Records the question it was asked.
    struct Recorder(std::sync::Mutex<Vec<AuthPromptRequest>>);

    #[async_trait::async_trait]
    impl AuthPrompt for Recorder {
        async fn ask(&self, request: AuthPromptRequest) -> AuthPromptResponse {
            self.0.lock().expect("recorder").push(request);
            AuthPromptResponse::Cancelled
        }
        fn is_interactive(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn a_jump_hosts_question_says_whose_it_is() {
        let recorder = Arc::new(Recorder(std::sync::Mutex::new(Vec::new())));
        let prompt = HopPrompt {
            label: "bastion".into(),
            inner: recorder.clone(),
        };
        assert!(prompt.is_interactive());
        for instructions in ["", "Enter the code"] {
            let _ = prompt
                .ask(AuthPromptRequest {
                    kind: AuthPromptKind::KeyboardInteractive,
                    name: String::new(),
                    instructions: instructions.into(),
                    prompts: Vec::new(),
                })
                .await;
        }
        let asked = recorder.0.lock().expect("recorder");
        assert_eq!(asked[0].instructions, "Jump host “bastion”");
        assert_eq!(asked[1].instructions, "Jump host “bastion”\nEnter the code");
    }

    /// The frontend listens for `sync-applied` and reads `received`.
    #[test]
    fn sync_applied_event_contract() {
        assert_eq!(SYNC_APPLIED_EVENT, "sync-applied");
        let json = serde_json::to_string(&SyncApplied { received: 3 }).unwrap();
        assert_eq!(json, r#"{"received":3}"#);
    }
}
