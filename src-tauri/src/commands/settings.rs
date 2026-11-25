use crate::commands::capture_cmd::restart_if_active;
use crate::state::{AppSettings, AppState};
use tauri::{AppHandle, State};

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.db.get_settings().unwrap_or_default()
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    new_settings: AppSettings,
) -> Result<bool, String> {
    validate_settings(&new_settings)?;
    let mut capture = state.capture.lock().await;
    state
        .db
        .update_settings(&new_settings)
        .map_err(|error| format!("Falha ao salvar as configurações: {error}"))?;
    restart_if_active(&app, &state, &mut capture, None).await
}

pub(crate) fn validate_settings(settings: &AppSettings) -> Result<(), String> {
    if !(1..=240).contains(&settings.framerate)
        || !(2..=8192).contains(&settings.width)
        || !(2..=8192).contains(&settings.height)
        || settings.bitrate < 1000
    {
        return Err("Resolução, FPS ou taxa de bits inválidos.".into());
    }
    if !(0.01..=60.0).contains(&settings.hls_time) {
        return Err(
            "Informe uma duração de segmento HLS de até 60 segundos, maior que zero.".into(),
        );
    }
    Ok(())
}
