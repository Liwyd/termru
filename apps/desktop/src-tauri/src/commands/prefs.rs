use tauri::State;
use termru_core::Preferences;

use crate::app_error::CommandError;
use crate::state::AppState;

#[tauri::command]
pub fn prefs_get(state: State<'_, AppState>) -> Result<Preferences, CommandError> {
    Ok(state.store.repos().prefs().portable()?)
}

#[tauri::command]
pub fn prefs_set(state: State<'_, AppState>, mut prefs: Preferences) -> Result<(), CommandError> {
    prefs.validate()?;
    state.store.repos().prefs().set_portable(&mut prefs)?;
    Ok(())
}
