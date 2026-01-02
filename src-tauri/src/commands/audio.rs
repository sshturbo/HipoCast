use crate::encoder::get_ffmpeg_path;
use crate::state::{AppState, AudioDeviceInfo};
use crate::stream_config;
use tauri::State;

// Implementação real: lista dispositivos de áudio via FFmpeg DirectShow
#[tauri::command]
pub fn list_audio_devices_ffmpeg(
    state: State<'_, AppState>,
    force_refresh: bool,
) -> Result<Vec<AudioDeviceInfo>, String> {
    use std::process::Command;

    // Check cache
    {
        let cache = state.audio_devices_cache.lock().unwrap();
        if !force_refresh {
            if let Some((timestamp, devices)) = cache.as_ref() {
                // Return cached if less than 30 seconds old
                if timestamp.elapsed() < std::time::Duration::from_secs(30) {
                    tracing::debug!(
                        "💾 Using cached audio devices ({}s old)",
                        timestamp.elapsed().as_secs()
                    );
                    return Ok(devices.clone());
                }
            }
        }
    }

    tracing::info!("🎧 Listando dispositivos de áudio via FFmpeg DirectShow...");

    let ffmpeg_path = get_ffmpeg_path();

    // Executar FFmpeg para listar dispositivos
    let output = Command::new(&ffmpeg_path)
        .args(["-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .output()
        .map_err(|e| format!("Falha ao executar FFmpeg: {}", e))?;

    // O output vem no stderr
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut devices = Vec::new();

    // Parsear linhas do output
    // Formato: [dshow @ 0x...] "NOME DO DISPOSITIVO" (audio)
    for line in stderr.lines() {
        // Procurar por linhas que contêm dispositivos de áudio
        if line.contains("(audio)") && line.contains("[dshow") {
            // Extrair nome entre aspas
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    let device_name = &line[start + 1..start + 1 + end];

                    // Classificar tipo de dispositivo
                    let device_type = if device_name.to_lowercase().contains("stereo mix")
                        || device_name.to_lowercase().contains("mixagem")
                        || device_name.to_lowercase().contains("wave out")
                        || device_name.to_lowercase().contains("what you hear")
                    {
                        "loopback".to_string()
                    } else {
                        "input".to_string()
                    };

                    // Log antes de mover device_type
                    tracing::debug!(
                        "  ✓ Encontrado: {} ({})",
                        device_name,
                        if device_type == "loopback" {
                            "Sistema"
                        } else {
                            "Entrada"
                        }
                    );

                    devices.push(AudioDeviceInfo {
                        name: device_name.to_string(),
                        device_type,
                    });
                }
            }
        }
    }

    tracing::info!("✅ Total de dispositivos encontrados: {}", devices.len());

    if devices.is_empty() {
        Err("Nenhum dispositivo de áudio encontrado".to_string())
    } else {
        // Update cache
        {
            let mut cache = state.audio_devices_cache.lock().unwrap();
            *cache = Some((std::time::Instant::now(), devices.clone()));
        }
        Ok(devices)
    }
}

#[tauri::command]
pub fn get_stream_audio_config(
    state: State<AppState>,
    stream_id: String,
) -> Result<stream_config::StreamAudioConfig, String> {
    state
        .db
        .get_stream_audio_config(&stream_id)
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "Config not found".to_string())
}

#[tauri::command]
pub fn save_stream_audio_config(
    state: State<AppState>,
    config: stream_config::StreamAudioConfig,
) -> Result<(), String> {
    state
        .db
        .save_stream_audio_config(&config)
        .map_err(|e| format!("Failed to save config: {}", e))
}

#[tauri::command]
pub fn delete_stream_audio_config(state: State<AppState>, stream_id: String) -> Result<(), String> {
    state
        .db
        .delete_stream_audio_config(&stream_id)
        .map_err(|e| format!("Failed to delete config: {}", e))
}

#[tauri::command]
pub fn reset_stream_audio_config(
    state: State<AppState>,
    stream_id: String,
) -> Result<stream_config::StreamAudioConfig, String> {
    // Deleta config existente
    let _ = state.db.delete_stream_audio_config(&stream_id);

    // Cria novo config com defaults otimizados (sem filtros pesados)
    let new_config = stream_config::StreamAudioConfig {
        stream_id: stream_id.clone(),
        audio_mode: "system".to_string(),
        target_pid: None,
        target_process_name: None,
        enable_microphone: false,
        microphone_device: "".to_string(),
        audio_effects: stream_config::AudioEffects {
            enabled: false,
            preset: stream_config::AudioPreset::None,
            custom_filters: None,
            master_volume: 1.0,
        },
        microphone_effects: stream_config::MicrophoneEffects {
            enabled: false,
            preset: stream_config::MicrophonePreset::None,
            custom_filters: None,
            volume: 1.0,
        },
    };

    state
        .db
        .save_stream_audio_config(&new_config)
        .map_err(|e| format!("Failed to save reset config: {}", e))?;

    Ok(new_config)
}
