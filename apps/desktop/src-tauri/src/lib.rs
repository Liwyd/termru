mod app_error;
mod commands;
mod secrets;
mod state;
pub mod sync;

use std::sync::OnceLock;

use tauri::Manager;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

pub use app_error::CommandError;
pub use state::sessions::spawn_connection;
pub use state::{
    AppState, HostKeyPrompts, SessionInfo, SessionManager, SessionStatusDto, SessionUpdate,
};

pub fn run() {
    let app = match build() {
        Ok(app) => app,
        Err(error) => {
            eprintln!("termru: fatal: {error}");
            std::process::exit(1);
        }
    };

    // Single owner across windows: on exit, close every live session so no
    // SSH connection outlives the process (orphan prevention).
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                state.sync_scheduler.shutdown();
                tauri::async_runtime::block_on(state.sessions.shutdown());
            }
        }
    });
}

fn build() -> tauri::Result<tauri::App> {
    tauri::Builder::default()
        // The plugin has to be registered first: it decides whether this
        // process is the first instance.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // A second launch does not open a second window: it focuses the
            // one that is already running.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        // The native file picker: the user chooses the private key file
        // instead of typing its path.
        .plugin(tauri_plugin_dialog::init())
        // A window that is closed releases its views. A session that no
        // window shows any more is closed as well — its SSH connection
        // would stay open for nothing. A session that another window still
        // shows keeps running.
        .on_window_event(|window, event| {
            if !matches!(event, tauri::WindowEvent::Destroyed) {
                return;
            }
            let state = window.app_handle().state::<AppState>();
            let label = window.label().to_string();
            let orphaned = state.sessions.drop_window_views(&label);
            for id in orphaned {
                let manager = state.sessions.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = manager.close(&id).await;
                });
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            init_logging(&data_dir);
            install_panic_hook();
            let store = termru_store::Store::open(&data_dir.join("termru.sqlite"))
                .map_err(Box::<dyn std::error::Error>::from)?;
            app.manage(AppState::new(store, handle, data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info::app_info,
            commands::hosts::host_list,
            commands::hosts::host_save,
            commands::hosts::host_delete,
            commands::hosts::recent_list,
            commands::hosts::host_set_favorite,
            commands::hosts::group_list,
            commands::hosts::group_save,
            commands::hosts::group_delete,
            commands::hosts::tag_list,
            commands::hosts::tag_save,
            commands::hosts::tag_delete,
            commands::hosts::identity_list,
            commands::hosts::identity_save,
            commands::hosts::identity_delete,
            commands::hosts::identity_bindings,
            commands::hosts::identity_bind,
            commands::hosts::identity_unbind,
            commands::prefs::prefs_get,
            commands::prefs::prefs_set,
            commands::sessions::session_open,
            commands::sessions::session_retry,
            commands::sessions::session_duplicate,
            commands::sessions::session_close,
            commands::sessions::session_write,
            commands::sessions::session_resize,
            commands::sessions::session_list,
            commands::sessions::session_attach,
            commands::sessions::host_key_prompt_resolve,
            commands::layout::layout_save,
            commands::layout::layout_load,
            commands::windows::window_open_session,
            commands::windows::window_close,
            commands::secrets::secret_set,
            commands::secrets::secret_delete,
            commands::secrets::secret_has,
            commands::sync::sync_status,
            commands::sync::sync_sign_in,
            commands::sync::sync_sign_out,
            commands::sync::sync_now,
            commands::sync::sync_conflicts,
            commands::sync::sync_resolve_conflict,
        ])
        .build(tauri::generate_context!())
}

/// Keeps the non-blocking writer's flush guard alive for the process
/// lifetime (dropping it would stop writing to the log file).
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Logs to stderr **and** to a daily rotating file in the app data dir.
/// Secrets never reach the log: credential values are never formatted.
fn init_logging(data_dir: &std::path::Path) {
    init_logging_in(&data_dir.join("logs"));
}

/// Initializes logging into `log_dir` (stderr + a daily rotating file).
fn init_logging_in(log_dir: &std::path::Path) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,termru=debug"));

    if std::fs::create_dir_all(log_dir).is_err() {
        // Logging to a file is best-effort; stderr keeps working.
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
        return;
    }

    let appender = tracing_appender::rolling::daily(log_dir, "termru.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let _ = LOG_GUARD.set(guard);

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(tracing_subscriber::fmt::layer().with_writer(writer))
        .init();
}

/// Panic hygiene: record the panic (with location) in the log before the
/// default hook prints it, so a crash is diagnosable after the fact.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(
            panic = %info,
            location = ?info.location(),
            "application panicked"
        );
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use termru_ssh::SecretValue;

    const PASSWORD: &str = "audit-password-4f2c9a";
    const PASSPHRASE: &str = "audit-passphrase-8b71de";

    fn log_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.is_file())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    /// The app's log files must never contain secret material.
    #[test]
    fn log_files_never_contain_secret_material() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log_dir = dir.path().join("logs");

        // The real logging setup (stderr + daily rotating file).
        init_logging_in(&log_dir);

        // Exercise the paths that touch credentials: both values are wrapped
        // in the redacting `SecretValue`, and `AuthChoice`'s `Debug` is
        // redacted as well.
        let password = SecretValue::new(PASSWORD.to_string());
        let passphrase = SecretValue::new(PASSPHRASE.to_string());
        let auth = termru_ssh::AuthChoice::PrivateKey {
            key_path: std::path::PathBuf::from("/home/user/.ssh/id_ed25519"),
            passphrase: Some(passphrase),
        };
        tracing::info!("connecting with {auth:?} and password {password:?}");
        tracing::warn!("retrying the connection for the same host");

        // The non-blocking writer flushes asynchronously: wait for the
        // expected line to appear before auditing the file.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut content = String::new();
        while std::time::Instant::now() < deadline {
            content = log_files(&log_dir)
                .iter()
                .filter_map(|file| std::fs::read_to_string(file).ok())
                .collect::<Vec<_>>()
                .join("");
            if content.contains("SecretValue([redacted])") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(
            !content.contains(PASSWORD),
            "the log file contains the password:\n{content}"
        );
        assert!(
            !content.contains(PASSPHRASE),
            "the log file contains the passphrase:\n{content}"
        );
        assert!(
            content.contains("SecretValue([redacted])"),
            "expected a redacted secret in the log:\n{content}"
        );
    }
}
