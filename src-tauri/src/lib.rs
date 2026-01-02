mod capture;
mod commands;
mod db;
mod encoder;
mod ffmpeg;
mod logger;
mod output;
mod server;
mod state;
mod stream_config;

use commands::*;
use db::Database;
use state::AppState;
use std::sync::{Arc, Mutex};
use tauri::Manager;

// Re-export settings from DB module to keep API consistent
pub use db::DbSettings as AppSettings;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = logger::init_logger() {
        // Fallback to stderr if logger fails
        eprintln!("Failed to initialize logger: {}", e);
    }

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

    tracing::info!("📂 Executável em: {}", exe_path.display());
    tracing::info!("📂 Diretório de dados: {}", db_dir.display());

    // Initialize database in the same location as executable (contains all tables now)
    let db_path = db_dir.join("hipocast.db");
    tracing::info!("🗄️ Banco de dados: {}", db_path.display());
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

            // === System Tray Setup ===
            use tauri::tray::TrayIconBuilder;

            let _tray = TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().unwrap().clone())
                .on_tray_icon_event(|tray, _event| {
                    // Clique no tray restaura todas as janelas
                    let app = tray.app_handle();
                    for (_, window) in app.webview_windows() {
                        let _ = window.show();
                        let _ = window.set_skip_taskbar(false);
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                })
                .build(app)?;
            // =========================
            // =========================

            // Ensure directory exists
            std::fs::create_dir_all(&streams_path).ok();
            let streams_dir_str = streams_path.to_string_lossy().into_owned();

            tracing::info!("✅ Global Stream Path: {}", streams_dir_str);

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
        .on_window_event(|window, event| {
            match event {
                // Implementando "Fechar para Bandeja"
                // Como não conseguimos interceptar 'Minimized' facilmente na v2,
                // interceptamos o 'Close' para manter o app rodando.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // Impede o fechamento real
                    api.prevent_close();
                    // Esconde a janela e remove da barra de tarefas
                    let _ = window.hide();
                    let _ = window.set_skip_taskbar(true);
                }
                _ => {}
            }
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
