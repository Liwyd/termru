mod commands;

use tracing_subscriber::EnvFilter;

/// Application state managed by Tauri. The SSH `SessionManager` (Phase 5)
/// will own every live session here — single owner across all windows.
pub struct AppState;

pub fn run() {
    init_logging();

    let result = tauri::Builder::default()
        .manage(AppState)
        .invoke_handler(tauri::generate_handler![commands::app_info::app_info])
        .run(tauri::generate_context!());

    if let Err(err) = result {
        eprintln!("termru: fatal: {err}");
        std::process::exit(1);
    }
}

fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,termru=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
