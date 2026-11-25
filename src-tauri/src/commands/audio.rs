use crate::commands::capture_cmd::restart_if_active;
use crate::encoder::enumerate_audio_devices;
use crate::state::{AppState, AudioDeviceInfo};
use crate::stream_config::StreamAudioConfig;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn list_audio_devices_ffmpeg(
    state: State<'_, AppState>,
    force_refresh: bool,
) -> Result<Vec<AudioDeviceInfo>, String> {
    if !force_refresh {
        let cache = state.audio_devices_cache.lock().unwrap();
        if let Some((timestamp, devices)) = cache.as_ref() {
            if timestamp.elapsed().as_secs() < 30 {
                return Ok(devices.clone());
            }
        }
    }
    let devices = tauri::async_runtime::spawn_blocking(enumerate_audio_devices)
        .await
        .map_err(|error| error.to_string())??;
    *state.audio_devices_cache.lock().unwrap() = Some((std::time::Instant::now(), devices.clone()));
    Ok(devices)
}

#[tauri::command]
pub fn get_stream_audio_config(
    state: State<'_, AppState>,
    stream_id: String,
) -> Result<StreamAudioConfig, String> {
    let config = state
        .db
        .get_stream_audio_config(&stream_id)
        .map_err(|error| error.to_string())?;
    Ok(config.unwrap_or_else(|| state.db.create_default_audio_config_from_global(&stream_id)))
}

#[tauri::command]
pub async fn save_stream_audio_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: StreamAudioConfig,
) -> Result<(), String> {
    if !matches!(config.audio_mode.as_str(), "system" | "muted") {
        return Err("Selecione áudio do sistema ou mudo.".into());
    }
    let mut capture = state.capture.lock().await;
    state
        .db
        .save_stream_audio_config(&config)
        .map_err(|error| error.to_string())?;
    restart_if_active(&app, &state, &mut capture, Some(&config.stream_id)).await?;
    Ok(())
}

#[tauri::command]
pub fn delete_stream_audio_config(state: State<AppState>, stream_id: String) -> Result<(), String> {
    state
        .db
        .delete_stream_audio_config(&stream_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reset_stream_audio_config(
    state: State<AppState>,
    stream_id: String,
) -> Result<StreamAudioConfig, String> {
    let config = state.db.create_default_audio_config_from_global(&stream_id);
    state
        .db
        .delete_stream_audio_config(&stream_id)
        .map_err(|error| error.to_string())?;
    Ok(config)
}
