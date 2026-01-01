mod capture;
mod hls;
mod encoder;
mod server;
mod audio;
mod ffmpeg;
mod db;
mod stream_config;

use std::sync::Mutex;
use std::sync::Arc;
use tauri::{State, Manager};
use capture::{CaptureSource, list_windows};
use db::{Database, DbStream};

use serde::{Serialize, Deserialize};

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
    stream_config_db: Arc<stream_config::StreamConfigDb>,
    streams_path: std::path::PathBuf,
}

#[tauri::command]
async fn start_browser_stream(
    app: tauri::AppHandle, 
    state: State<'_, AppState>, 
    url: String, 
    title: String,
    custom_id: Option<String>
) -> Result<StatusResponse, String> {
    // Determinar o stream_id primeiro
    let stream_id = custom_id.clone().unwrap_or_else(|| format!("browser_{}", uuid::Uuid::new_v4()));
    
    println!("🔍 Verificando janela existente para stream_id: '{}'", stream_id);
    
    // Verificar se já existe uma janela para este stream_id e obter o HWND
    let capture_source_id = {
        let b_wins = state.browser_windows.lock().unwrap();
        if let Some(existing_window) = b_wins.get(&stream_id) {
            // Tentar obter HWND da janela existente
            if let Ok(hwnd) = existing_window.hwnd() {
                let hwnd_id = format!("window:{}", hwnd.0 as usize);
                println!("♻️ Reutilizando janela existente para stream '{}' (HWND: {})", stream_id, hwnd.0 as usize);
                Some(hwnd_id)
            } else {
                println!("⚠️ Janela existe mas HWND inválido, será criada nova janela");
                None
            }
        } else {
            println!("🆕 Nenhuma janela existente para '{}', criando nova", stream_id);
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
            tauri::WebviewUrl::External(url.parse().map_err(|_| "URL inválida")?)
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

        let hwnd = new_window.hwnd().map_err(|_| "Falha ao obter HWND da janela")?;
        let hwnd_id = format!("window:{}", hwnd.0 as usize);
        
        println!("✅ Nova janela criada para stream '{}' (HWND: {})", stream_id, hwnd.0 as usize);
        
        // Salvar janela no HashMap
        {
            let mut b_wins = state.browser_windows.lock().unwrap();
            b_wins.insert(stream_id.clone(), new_window);
        }
        
        hwnd_id
    };

    // Delegate to existing capture logic with the custom ID
    // Pass both the stream_id (for folders/files) and capture_source_id (for actual capture)
    start_source_capture_browser(app, state, stream_id, capture_source_id, title, "browser".to_string(), Some(url))
}

#[tauri::command]
fn get_capture_sources() -> Vec<CaptureSource> {
    list_windows()
}

#[tauri::command]
fn start_source_capture(app: tauri::AppHandle, state: State<'_, AppState>, id: String, title: String) -> Result<StatusResponse, String> {
    start_source_capture_internal(app, state, id, title, "window".to_string(), None)
}

fn start_source_capture_internal(
    app: tauri::AppHandle, 
    state: State<'_, AppState>, 
    id: String, 
    title: String,
    source_type: String,
    source_url: Option<String>
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
    let stream_config_db = state.stream_config_db.clone();
    
    match capture::start_capture(app_handle, id.clone(), streams_dir_str.clone(), settings, stream_config_db) {
        Ok(control) => {
            *handle = Some(Box::new(control));
            *active_id = Some(id.clone());
            
            // Add to DB
            let hls_path = format!("/streams/{}/{}.m3u8", id.replace(":", "_"), id.replace(":", "_"));
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
            if state.stream_config_db.get_config(&id).ok().flatten().is_none() {
                let default_audio_config = state.stream_config_db.create_default_config_from_global(&id, &state.db);
                if let Err(e) = state.stream_config_db.save_config(&default_audio_config) {
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
    stream_id: String,      // Custom ID for folders/files/database
    capture_id: String,     // Real window ID for capture
    title: String,
    source_type: String,
    source_url: Option<String>
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
    let stream_config_db = state.stream_config_db.clone();
    
    // Use capture_id for the actual capture but stream_id for everything else
    match capture::start_capture_with_ids(app_handle, stream_id.clone(), capture_id, streams_dir_str.clone(), settings, stream_config_db) {
        Ok(control) => {
            *handle = Some(Box::new(control));
            *active_id = Some(stream_id.clone());
            
            // Add to DB using stream_id
            let hls_path = format!("/streams/{}/{}.m3u8", stream_id.replace(":", "_"), stream_id.replace(":", "_"));
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
            if state.stream_config_db.get_config(&stream_id).ok().flatten().is_none() {
                let default_audio_config = state.stream_config_db.create_default_config_from_global(&stream_id, &state.db);
                if let Err(e) = state.stream_config_db.save_config(&default_audio_config) {
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
        Ok(db_streams) => db_streams.into_iter().map(|s| StreamStatus {
            id: s.id,
            title: s.title,
            status: s.status,
            hls_path: format!("http://localhost:{}{}", port, s.hls_path),
        }).collect(),
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
        Ok(streams) => {
            streams.into_iter()
                .find(|s| s.id == stream_id)
                .ok_or_else(|| "Stream not found".to_string())
        }
        Err(e) => Err(format!("Database error: {}", e))
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
    
    *handle = None;
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

#[tauri::command]
fn list_audio_devices() -> Result<Vec<String>, String> {
    println!("🎧 Starting native audio device enumeration (Windows Core)...");
    match crate::audio::list_input_devices() {
        Ok(devices) => {
            for dev in &devices {
                println!("  - Found device: {}", dev);
            }
            println!("✅ Audio enumeration complete. Return {} unique devices.", devices.len());
            Ok(devices)
        },
        Err(e) => Err(format!("Failed to list devices: {}", e))
    }
}

#[tauri::command]
fn list_audio_sessions() -> Result<Vec<crate::audio::AudioSession>, String> {
    println!("🎧 Listing active audio sessions...");
    match crate::audio::list_audio_sessions() {
        Ok(sessions) => {
            println!("✅ Found {} active audio sessions", sessions.len());
            for session in &sessions {
                println!("  - {} (PID: {}, Display: {})", session.process_name, session.process_id, session.display_name);
            }
            Ok(sessions)
        },
        Err(e) => Err(format!("Failed to list audio sessions: {}", e))
    }
}

#[tauri::command]
fn get_stream_audio_config(state: State<AppState>, stream_id: String) -> Result<stream_config::StreamAudioConfig, String> {
    state.stream_config_db
        .get_config(&stream_id)
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "Config not found".to_string())
}

#[tauri::command]
fn save_stream_audio_config(state: State<AppState>, config: stream_config::StreamAudioConfig) -> Result<(), String> {
    state.stream_config_db
        .save_config(&config)
        .map_err(|e| format!("Failed to save config: {}", e))
}

#[tauri::command]
fn delete_stream_audio_config(state: State<AppState>, stream_id: String) -> Result<(), String> {
    state.stream_config_db
        .delete_config(&stream_id)
        .map_err(|e| format!("Failed to delete config: {}", e))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize DB in project root (outside src-tauri)
    let exe_path = std::env::current_exe().unwrap();
    // Go up from target/debug/deps/exe to project root (rust-app)
    let base_dir = exe_path.parent().unwrap().parent().unwrap().parent().unwrap().parent().unwrap();
    let db_path = base_dir.join("live-go.db");
    
    let db = Database::init(&db_path).expect("Failed to init database");
    let db = Arc::new(db);
    
    // Initialize Stream Config DB (same location)
    let stream_config_db_path = base_dir.join("stream_audio_config.db");
    let stream_config_db = Arc::new(stream_config::StreamConfigDb::new(
        &stream_config_db_path.to_string_lossy()
    ).expect("Failed to init stream config database"));

    // Resolve streams path ONCE at startup to ensure consistency across Server, Capture and Delete
    // Heuristic: If we are in a 'target' folder, we are likely in 'cargo run' (Dev).
    let exe_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let is_dev_env = exe_path.to_string_lossy().contains("target");

    // We can't access `app` here yet, so we have to resolve strict paths or defer
    // Actually we need to do this resolution INSIDE setup or pass it.
    // Ideally we verify the path in setup, but we need to pass it to Manage.
    // Let's rely on standard directories for the 'manage' call, but we can't easily access app_local_data_dir without AppHandle.
    // Workaround: We resolve it in `setup` and use a OnceLock or replace the state? 
    // Easier: We do the determination logic HERE but map AppData manually if needed?
    // Rust-Tauri: We can't access AppHandle before builder. 
    // Solution: We initialize with a default, and Setup replaces it? No, State is immutable usually.
    // Better: We calculate it robustly here.
    
    // For AppData on Windows we can use directories crate logic or env var if needed fallback.
    // But simplest is:
    
    let streams_path = if is_dev_env {
         // Dev Mode: up 4 levels
         exe_path.parent()
            .and_then(|p| p.parent()).and_then(|p| p.parent()).and_then(|p| p.parent()).and_then(|p| p.parent())
            .map(|p| p.join("streams"))
            .unwrap_or_else(|| std::path::PathBuf::from("streams")) // fallback to relative
    } else {
        // Production: Try Install Dir
         if let Some(parent) = exe_path.parent() {
            let local_streams = parent.join("streams");
            if std::fs::create_dir_all(&local_streams).is_ok() && std::fs::write(local_streams.join(".tmptest"), "").is_ok() {
                let _ = std::fs::remove_file(local_streams.join(".tmptest"));
                local_streams
            } else {
                // Fallback to %LOCALAPPDATA%/rust-app/streams
                dirs::data_local_dir().unwrap_or(std::path::PathBuf::from(".")).join("rust-app").join("streams")
            }
         } else {
             dirs::data_local_dir().unwrap_or(std::path::PathBuf::from(".")).join("rust-app").join("streams")
         }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            capture_handle: Mutex::new(None),
            active_stream_id: Mutex::new(None),
            browser_windows: Mutex::new(std::collections::HashMap::new()),
            db,
            stream_config_db,
            streams_path: streams_path.clone(),
        })
        .setup(move |app| {
            let _app_handle = app.handle().clone();
            
            // Ensure directory exists
            std::fs::create_dir_all(&streams_path).ok();
            let streams_dir_str = streams_path.to_string_lossy().into_owned();
            
            println!("✅ Global Stream Path: {}", streams_dir_str);
            
            // Fetch settings for port
            let state = app.state::<AppState>();
            let port = state.db.get_settings().map(|s| s.hls_server_port).unwrap_or(8080);

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
            list_audio_devices,
            list_audio_sessions,
            get_stream_audio_config,
            save_stream_audio_config,
            delete_stream_audio_config,
            stream_config::get_suggested_audio_preset,
            stream_config::get_audio_presets_info,
            stream_config::get_microphone_presets_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
