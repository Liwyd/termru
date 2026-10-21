//! Window commands.
//!
//! A session is owned by the app, not by a window: moving a tab to another
//! window opens a new view that *attaches* to the running session (the SSH
//! connection is never dropped).

use tauri::State;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::app_error::CommandError;
use crate::state::AppState;

/// Opens a new window rendering an existing session.
///
/// The new window receives the session id in its URL query
/// (`index.html?attach=<session id>`); the frontend attaches to it and shows
/// the same scrollback. The session itself keeps running in the background.
#[tauri::command]
pub fn window_open_session(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<String, CommandError> {
    let info = state
        .sessions
        .info(&id)
        .ok_or_else(|| CommandError::not_found("session", &id))?;
    if !matches!(info.status, crate::state::SessionStatusDto::Ready) {
        return Err(CommandError::new(
            "only a connected session can be moved to a new window",
        ));
    }

    let label = format!("session-{id}");
    if let Some(existing) = app.get_webview_window(&label) {
        // Already open: focus it instead of creating a duplicate view.
        let _ = existing.set_focus();
        return Ok(label);
    }

    let url = format!("index.html?attach={id}");
    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(1000.0, 700.0)
        .build()
        .map_err(|error| CommandError::new(format!("could not open the window: {error}")))?;
    let _ = window.set_focus();
    Ok(label)
}

/// Closes a window (its views go away; sessions stay owned by the app).
#[tauri::command]
pub fn window_close(app: AppHandle, label: String) -> Result<(), CommandError> {
    if let Some(window) = app.get_webview_window(&label) {
        window
            .close()
            .map_err(|error| CommandError::new(format!("could not close the window: {error}")))?;
    }
    Ok(())
}
