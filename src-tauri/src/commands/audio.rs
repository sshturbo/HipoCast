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
    Ok(normalize_audio_mode(config.unwrap_or_else(|| {
        state.db.create_default_audio_config_from_global(&stream_id)
    })))
}

#[tauri::command]
pub async fn save_stream_audio_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: StreamAudioConfig,
) -> Result<(), String> {
    let config = normalize_audio_mode(config);
    if !matches!(
        config.audio_mode.as_str(),
        "system" | "microphone" | "silent"
    ) {
        return Err("Selecione áudio do sistema, somente microfone ou mudo.".into());
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
    Ok(normalize_audio_mode(config))
}

fn normalize_audio_mode(mut config: StreamAudioConfig) -> StreamAudioConfig {
    match config.audio_mode.as_str() {
        "muted" => {
            config.audio_mode = if config.enable_microphone {
                "microphone"
            } else {
                "silent"
            }
            .into();
        }
        "microphone" => config.enable_microphone = true,
        "silent" => config.enable_microphone = false,
        _ => {}
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_legacy_audio_modes_and_keeps_silent_input_disabled() {
        for (mode, microphone, expected_mode, expected_microphone) in [
            ("muted", true, "microphone", true),
            ("muted", false, "silent", false),
            ("microphone", false, "microphone", true),
            ("silent", true, "silent", false),
            ("system", true, "system", true),
        ] {
            let normalized = normalize_audio_mode(StreamAudioConfig {
                audio_mode: mode.into(),
                enable_microphone: microphone,
                ..StreamAudioConfig::default()
            });
            assert_eq!(normalized.audio_mode, expected_mode);
            assert_eq!(normalized.enable_microphone, expected_microphone);
        }
    }
}
