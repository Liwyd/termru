mod app_error;
mod commands;
mod state;

use tauri::Manager;

pub use app_error::CommandError;
pub use state::{AppState, HostKeyPrompts, SessionManager};

use tracing_subscriber::EnvFilter;

pub fn run() {
    init_logging();

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
        ])
        .build(tauri::generate_context!())
}

fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,termru=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
