mod app_error;
mod commands;
mod state;

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
                tauri::async_runtime::block_on(state.sessions.shutdown());
            }
        }
    });
}

fn build() -> tauri::Result<tauri::App> {
    tauri::Builder::default()
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
            commands::hosts::host_set_favorite,
            commands::hosts::identity_list,
            commands::hosts::identity_save,
            commands::hosts::identity_delete,
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
            commands::sessions::host_key_prompt_resolve,
            commands::layout::layout_save,
            commands::layout::layout_load,
        ])
        .build(tauri::generate_context!())
}

/// Keeps the non-blocking writer's flush guard alive for the process
/// lifetime (dropping it would stop writing to the log file).
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Logs to stderr **and** to a daily rotating file in the app data dir.
/// Secrets never reach the log: credential values are never formatted.
fn init_logging(data_dir: &std::path::Path) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,termru=debug"));

    let log_dir = data_dir.join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        // Logging to a file is best-effort; stderr keeps working.
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
        return;
    }

    let appender = tracing_appender::rolling::daily(&log_dir, "termru.log");
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
