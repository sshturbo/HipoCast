use crate::state::{AppSettings, AppState};
use tauri::State;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.db.get_settings().unwrap_or_default()
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, new_settings: AppSettings) {
    if let Err(e) = state.db.update_settings(&new_settings) {
        tracing::error!("Failed to update settings: {}", e);
    }
}
