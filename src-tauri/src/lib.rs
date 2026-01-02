mod capture;
mod hls;
mod server;

mod db;
mod ffmpeg;
mod stream_config;

use capture::{list_windows, CaptureSource};
use db::{Database, DbStream};
use std::sync::Arc;
use std::sync::Mutex;
use tauri::{Manager, State};

use serde::{Deserialize, Serialize};

// Re-export settings from DB module to keep API consistent
pub use db::DbSettings as AppSettings;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamStatus {
    pub id: String,
    pub title: String,
    pub status: String,
    pub hls_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub streams: Vec<StreamStatus>,
    pub active: usize,
}

#[allow(dead_code)]
struct AppState {
    capture_handle: Mutex<Option<Box<dyn Send + Sync + 'static>>>,
    active_stream_id: Mutex<Option<String>>,
    browser_windows: Mutex<std::collections::HashMap<String, tauri::WebviewWindow>>, // Múltiplas janelas por stream_id
    db: Arc<Database>,
    streams_path: std::path::PathBuf,
    audio_devices_cache: Mutex<Option<(std::time::Instant, Vec<AudioDeviceInfo>)>>,
}

#[tauri::command]
async fn start_browser_stream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
    title: String,
    custom_id: Option<String>,
) -> Result<StatusResponse, String> {
    // Determinar o stream_id primeiro
    let stream_id = custom_id
        .clone()
        .unwrap_or_else(|| format!("browser_{}", uuid::Uuid::new_v4()));

    println!(
        "🔍 Verificando janela existente para stream_id: '{}'",
        stream_id
    );

    // Verificar se já existe uma janela para este stream_id e obter o HWND
    let capture_source_id = {
        let b_wins = state.browser_windows.lock().unwrap();
        if let Some(existing_window) = b_wins.get(&stream_id) {
            // Tentar obter HWND da janela existente
            if let Ok(hwnd) = existing_window.hwnd() {
                let hwnd_id = format!("window:{}", hwnd.0 as usize);
                println!(
                    "♻️ Reutilizando janela existente para stream '{}' (HWND: {})",
                    stream_id, hwnd.0 as usize
                );
                Some(hwnd_id)
            } else {
                println!("⚠️ Janela existe mas HWND inválido, será criada nova janela");
                None
            }
        } else {
            println!(
                "🆕 Nenhuma janela existente para '{}', criando nova",
                stream_id
            );
            None
        }
    };

    // Se não existe janela válida, criar uma nova
    let capture_source_id = if let Some(hwnd_id) = capture_source_id {
        hwnd_id
    } else {
        // Criar nova janela
        let label = format!("source_{}", uuid::Uuid::new_v4());

        // Get width/height from settings for matching capture resolution
        let settings = state.db.get_settings().unwrap_or_default();
        let win_width = settings.width as f64;
        let win_height = settings.height as f64;

        let new_window = tauri::WebviewWindowBuilder::new(
            &app,
            label,
            tauri::WebviewUrl::External(url.parse().map_err(|_| "URL inválida")?),
        )
        .title(&format!("🔴 {} - HipoCast", title))
        .inner_size(win_width, win_height)
        .min_inner_size(640.0, 480.0)
        .decorations(false)
        .resizable(true)
        .shadow(true)
        .center()
        .build()
        .map_err(|e| format!("Falha ao criar janela: {}", e))?;

        let hwnd = new_window
            .hwnd()
            .map_err(|_| "Falha ao obter HWND da janela")?;
        let hwnd_id = format!("window:{}", hwnd.0 as usize);

        println!(
            "✅ Nova janela criada para stream '{}' (HWND: {})",
            stream_id, hwnd.0 as usize
        );

        // Salvar janela no HashMap
        {
            let mut b_wins = state.browser_windows.lock().unwrap();
            b_wins.insert(stream_id.clone(), new_window);
        }

        hwnd_id
    };

    // Delegate to existing capture logic with the custom ID
    // Pass both the stream_id (for folders/files) and capture_source_id (for actual capture)
    start_source_capture_browser(
        app,
        state,
        stream_id,
        capture_source_id,
        title,
        "browser".to_string(),
        Some(url),
    )
}

#[tauri::command]
fn get_capture_sources() -> Vec<CaptureSource> {
    list_windows()
}

#[tauri::command]
fn start_source_capture(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<StatusResponse, String> {
    start_source_capture_internal(app, state, id, title, "window".to_string(), None)
}

fn start_source_capture_internal(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    title: String,
    source_type: String,
    source_url: Option<String>,
) -> Result<StatusResponse, String> {
    let mut handle = state.capture_handle.lock().unwrap();
    let mut active_id = state.active_stream_id.lock().unwrap();

    if handle.is_some() {
        return Err("Capture already running".into());
    }

    // Use the canonical global streams path from AppState
    let streams_dir = &state.streams_path;
    let streams_dir_str = streams_dir.to_string_lossy().into_owned();
    let app_handle = app.clone();

    // Fetch settings from DB
    let settings = state.db.get_settings().unwrap_or_default();
    let db = state.db.clone();

    match capture::start_capture(
        app_handle,
        id.clone(),
        streams_dir_str.clone(),
        settings,
        db,
    ) {
        Ok(control) => {
            *handle = Some(Box::new(control));
            *active_id = Some(id.clone());

            // Add to DB
            let hls_path = format!(
                "/streams/{}/{}.m3u8",
                id.replace(":", "_"),
                id.replace(":", "_")
            );
            let stream = DbStream {
                id: id.clone(),
                title: title.clone(),
                status: "running".to_string(),
                hls_path,
                created_at: chrono::Local::now().to_rfc3339(),
                source_type,
                source_url,
            };

            if let Err(e) = state.db.add_stream(&stream) {
                eprintln!("Failed to add stream to DB: {}", e);
            }

            // Criar configuração de áudio padrão baseada nas configurações globais apenas se não existir
            if state
                .db
                .get_stream_audio_config(&id)
                .ok()
                .flatten()
                .is_none()
            {
                let default_audio_config = state.db.create_default_audio_config_from_global(&id);
                if let Err(e) = state.db.save_stream_audio_config(&default_audio_config) {
                    eprintln!("Failed to save default audio config: {}", e);
                }
            }

            // Return updated status
            Ok(get_status(state.clone()))
        }
        Err(e) => Err(e.to_string()),
    }
}

// Special version for browser streams that need to separate stream_id (for folders/files) from capture_source_id (for actual capture)
fn start_source_capture_browser(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    stream_id: String,  // Custom ID for folders/files/database
    capture_id: String, // Real window ID for capture
    title: String,
    source_type: String,
    source_url: Option<String>,
) -> Result<StatusResponse, String> {
    let mut handle = state.capture_handle.lock().unwrap();
    let mut active_id = state.active_stream_id.lock().unwrap();

    // FORCE STOP any existing capture before starting new one
    if handle.is_some() {
        println!("⚠️ Stopping existing capture before starting new one...");
        let start_stop = std::time::Instant::now();
        *handle = None; // Drop previous capture - fecha channel, para threads
        drop(handle); // Libera lock para permitir cleanup
        drop(active_id);

        // Aguarda threads finalizarem completamente (FFmpeg + audio)
        // Tempo maior para garantir que FFmpeg flush buffers e feche pipes
        println!("⏳ Waiting for FFmpeg and audio threads to finish...");
        std::thread::sleep(std::time::Duration::from_millis(2500));

        let elapsed = start_stop.elapsed();
        println!(
            "✅ Previous capture stopped in {:.2}s, starting new one",
            elapsed.as_secs_f64()
        );

        // Readquire locks
        handle = state.capture_handle.lock().unwrap();
        active_id = state.active_stream_id.lock().unwrap();
    }

    // Use the canonical global streams path from AppState
    let streams_dir = &state.streams_path;
    let streams_dir_str = streams_dir.to_string_lossy().into_owned();
    let app_handle = app.clone();

    // Fetch settings from DB
    let settings = state.db.get_settings().unwrap_or_default();
    let db = state.db.clone();

    // Use capture_id for the actual capture but stream_id for everything else
    match capture::start_capture_with_ids(
        app_handle,
        stream_id.clone(),
        capture_id,
        streams_dir_str.clone(),
        settings,
        db,
    ) {
        Ok(control) => {
            *handle = Some(Box::new(control));
            *active_id = Some(stream_id.clone());

            // Add to DB using stream_id
            let hls_path = format!(
                "/streams/{}/{}.m3u8",
                stream_id.replace(":", "_"),
                stream_id.replace(":", "_")
            );
            let stream = DbStream {
                id: stream_id.clone(),
                title: title.clone(),
                status: "running".to_string(),
                hls_path,
                created_at: chrono::Local::now().to_rfc3339(),
                source_type,
                source_url,
            };

            if let Err(e) = state.db.add_stream(&stream) {
                eprintln!("Failed to add stream to DB: {}", e);
            }

            // Criar configuração de áudio padrão baseada nas configurações globais apenas se não existir
            if state
                .db
                .get_stream_audio_config(&stream_id)
                .ok()
                .flatten()
                .is_none()
            {
                let default_audio_config =
                    state.db.create_default_audio_config_from_global(&stream_id);
                if let Err(e) = state.db.save_stream_audio_config(&default_audio_config) {
                    eprintln!("Failed to save default audio config: {}", e);
                }
            }

            // Return updated status
            Ok(get_status(state.clone()))
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn get_status(state: State<'_, AppState>) -> StatusResponse {
    // Get streams from DB and join with port
    let settings = state.db.get_settings().unwrap_or_default();
    let port = settings.hls_server_port;

    let streams = match state.db.get_all_streams() {
        Ok(db_streams) => db_streams
            .into_iter()
            .map(|s| StreamStatus {
                id: s.id,
                title: s.title,
                status: s.status,
                hls_path: format!("http://localhost:{}{}", port, s.hls_path),
            })
            .collect(),
        Err(e) => {
            eprintln!("Failed to fetch streams: {}", e);
            Vec::new()
        }
    };

    let active = streams.iter().filter(|s| s.status == "running").count();

    StatusResponse { streams, active }
}

#[tauri::command]
fn get_stream_info(state: State<'_, AppState>, stream_id: String) -> Result<DbStream, String> {
    match state.db.get_all_streams() {
        Ok(streams) => streams
            .into_iter()
            .find(|s| s.id == stream_id)
            .ok_or_else(|| "Stream not found".to_string()),
        Err(e) => Err(format!("Database error: {}", e)),
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.db.get_settings().unwrap_or_default()
}

#[tauri::command]
fn update_settings(state: State<'_, AppState>, new_settings: AppSettings) {
    if let Err(e) = state.db.update_settings(&new_settings) {
        eprintln!("Failed to update settings: {}", e);
    }
}

#[tauri::command]
fn stop_source_capture(state: State<'_, AppState>) -> StatusResponse {
    let mut handle = state.capture_handle.lock().unwrap();
    let mut active_id = state.active_stream_id.lock().unwrap();

    if let Some(id) = active_id.as_ref() {
        if let Err(e) = state.db.update_stream_status(id, "stopped") {
            eprintln!("Failed to update stream status: {}", e);
        }
    }

    // FORÇA a finalização do CaptureHandle (drop) e espera threads terminarem
    if handle.is_some() {
        println!("🛑 Stopping capture and waiting for threads to finish...");
        let start_stop = std::time::Instant::now();
        *handle = None; // Drop CaptureHandle, fecha channel e para threads
        drop(handle); // Libera o lock

        // Aguarda as threads finalizarem (FFmpeg e áudio)
        // Tempo maior para garantir flush de buffers
        println!("⏳ Waiting for FFmpeg and audio threads to finish...");
        std::thread::sleep(std::time::Duration::from_millis(2500));

        let elapsed = start_stop.elapsed();
        println!(
            "✅ Capture stopped in {:.2}s, threads finished",
            elapsed.as_secs_f64()
        );
    }

    *active_id = None;

    // NÃO fechar a janela do browser aqui - apenas ao remover ou iniciar nova stream
    // Isso permite retomar a mesma janela sem recarregar

    get_status(state.clone())
}

#[tauri::command]
fn remove_stream(state: State<'_, AppState>, id: String) -> StatusResponse {
    // If it's the active stream, stop it first
    {
        let mut active_id = state.active_stream_id.lock().unwrap();
        if let Some(active) = active_id.as_ref() {
            if active == &id {
                let mut handle = state.capture_handle.lock().unwrap();
                *handle = None;
                *active_id = None;
            }
        }
    } // Drop lock before IO

    // Fechar janela do browser específica desta stream (se existir)
    {
        let mut b_wins = state.browser_windows.lock().unwrap();
        if let Some(win) = b_wins.remove(&id) {
            let _ = win.close();
        }
    }

    // Remove from DB
    if let Err(e) = state.db.remove_stream(&id) {
        eprintln!("Failed to remove stream from DB: {}", e);
    }

    // Remove files from disk using GLOBAL path
    let stream_dir = state.streams_path.join(id.replace(":", "_"));

    if stream_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&stream_dir) {
            eprintln!("Failed to remove stream directory: {}", e);
        } else {
            println!("Removed stream files: {:?}", stream_dir);
        }
    }

    get_status(state.clone())
}

// Struct para informações de dispositivo de áudio
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct AudioDeviceInfo {
    name: String,
    device_type: String, // "loopback" ou "input"
}

// Implementação real: lista dispositivos de áudio via FFmpeg DirectShow
#[tauri::command]
fn list_audio_devices_ffmpeg(
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
                    println!(
                        "💾 Using cached audio devices ({}s old)",
                        timestamp.elapsed().as_secs()
                    );
                    return Ok(devices.clone());
                }
            }
        }
    }

    println!("🎧 Listando dispositivos de áudio via FFmpeg DirectShow...");

    // Caminho do FFmpeg (mesmo método usado em ffmpeg.rs)
    let exe_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let exe_dir = exe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let ffmpeg_path = exe_dir.join("binaries").join("ffmpeg.exe");

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
                    println!(
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

    println!("✅ Total de dispositivos encontrados: {}", devices.len());

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
fn get_stream_audio_config(
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
fn save_stream_audio_config(
    state: State<AppState>,
    config: stream_config::StreamAudioConfig,
) -> Result<(), String> {
    state
        .db
        .save_stream_audio_config(&config)
        .map_err(|e| format!("Failed to save config: {}", e))
}

#[tauri::command]
fn delete_stream_audio_config(state: State<AppState>, stream_id: String) -> Result<(), String> {
    state
        .db
        .delete_stream_audio_config(&stream_id)
        .map_err(|e| format!("Failed to delete config: {}", e))
}

#[tauri::command]
fn reset_stream_audio_config(
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Determine executable directory
    let exe_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let is_dev_env = exe_path.to_string_lossy().contains("target");

    // Database directory: same as executable (or project root in dev)
    let db_dir = if is_dev_env {
        // Dev Mode: go up from target/debug/rust-app.exe to project root
        exe_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()
    } else {
        // Production: same directory as executable
        exe_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()
    };

    println!("📂 Executável em: {}", exe_path.display());
    println!("📂 Diretório de dados: {}", db_dir.display());

    // Initialize database in the same location as executable (contains all tables now)
    let db_path = db_dir.join("hipocast.db");
    println!("🗄️ Banco de dados: {}", db_path.display());

    let db = Database::init(&db_path).expect("Failed to init database");
    let db = Arc::new(db);

    // Streams path: same as database (next to executable)
    let streams_path = db_dir.join("streams");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            capture_handle: Mutex::new(None),
            active_stream_id: Mutex::new(None),
            browser_windows: Mutex::new(std::collections::HashMap::new()),
            db,
            streams_path: streams_path.clone(),
            audio_devices_cache: Mutex::new(None),
        })
        .setup(move |app| {
            let _app_handle = app.handle().clone();

            // Ensure directory exists
            std::fs::create_dir_all(&streams_path).ok();
            let streams_dir_str = streams_path.to_string_lossy().into_owned();

            println!("✅ Global Stream Path: {}", streams_dir_str);

            // Fetch settings for port
            let state = app.state::<AppState>();
            let port = state
                .db
                .get_settings()
                .map(|s| s.hls_server_port)
                .unwrap_or(8080);

            tauri::async_runtime::spawn(async move {
                server::start_server(port, streams_dir_str).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_capture_sources,
            start_source_capture,
            start_browser_stream,
            stop_source_capture,
            get_status,
            get_stream_info,
            get_settings,
            update_settings,
            remove_stream,
            list_audio_devices_ffmpeg,
            get_stream_audio_config,
            save_stream_audio_config,
            delete_stream_audio_config,
            reset_stream_audio_config,
            stream_config::get_suggested_audio_preset,
            stream_config::get_audio_presets_info,
            stream_config::get_microphone_presets_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
