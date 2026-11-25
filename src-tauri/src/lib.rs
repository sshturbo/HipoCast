mod capture;
mod commands;
mod db;
mod encoder;
mod ffmpeg;
mod gpu_scale;
mod logger;
mod output;
mod server;
mod state;
mod stream_config;

use commands::*;
use db::Database;
use state::AppState;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::Manager;

pub use db::DbSettings as AppSettings;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(error) = logger::init_logger() {
        eprintln!("Failed to initialize logger: {error}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let mut data_dir = app.path().app_local_data_dir()?;
            if cfg!(debug_assertions) {
                data_dir.push("dev");
            }
            let streams_path = data_dir.join("streams");
            std::fs::create_dir_all(&streams_path)?;

            let db_path = data_dir.join("hipocast.db");
            migrate_database(&db_path)?;
            let db = Arc::new(Database::init(&db_path)?);
            tracing::info!(path = %data_dir.display(), "Diretório de dados");

            let hls_server =
                server::start_server(db.get_settings()?.hls_server_port, streams_path.clone());
            if let Err(error) = &hls_server {
                tracing::error!(%error);
            }
            app.manage(AppState {
                capture: tokio::sync::Mutex::new(None),
                runtime: Arc::default(),
                browser_windows: Mutex::default(),
                db,
                streams_path,
                hls_server,
                audio_devices_cache: Mutex::new(None),
            });

            use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
            let mut tray = TrayIconBuilder::with_id("tray").on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    ..
                } = event
                {
                    for window in tray.app_handle().webview_windows().values() {
                        let _ = window.show();
                        let _ = window.set_skip_taskbar(false);
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            let app = window.app_handle();
            if window.label() == "main" {
                api.prevent_close();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let mut capture = state.capture.lock().await;
                    if let Err(error) = commands::stop_capture(&state, &mut capture).await {
                        tracing::error!(%error, "Falha ao encerrar a captura");
                    }
                    drop(capture);
                    app.exit(0);
                });
                return;
            }

            let state = app.state::<AppState>();
            let is_source_capturing = state.capture.try_lock().ok().is_some_and(|capture| {
                capture.as_ref().is_some_and(|active| {
                    !active.handle.is_finished()
                        && state
                            .browser_windows
                            .lock()
                            .unwrap()
                            .get(&active.id)
                            .is_some_and(|(_, source)| source.label() == window.label())
                })
            });
            if is_source_capturing {
                api.prevent_close();
                let _ = window.hide();
                let _ = window.set_skip_taskbar(true);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_capture_sources,
            start_source_capture,
            start_browser_stream,
            resume_stream,
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

fn migrate_database(destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let old_directory = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    } else {
        let executable = std::env::current_exe()?;
        let Some(directory) = executable.parent() else {
            return Ok(());
        };
        directory.to_path_buf()
    };
    let old_database = old_directory.join("hipocast.db");
    copy_legacy_database(&old_database, destination)
}

fn copy_legacy_database(
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if destination.exists() || !source.is_file() {
        return Ok(());
    }

    let temporary = destination.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let source =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        source.execute("VACUUM INTO ?1", [temporary.to_string_lossy().as_ref()])?;
        match std::fs::hard_link(&temporary, destination) {
            Ok(()) => tracing::info!("Banco de dados migrado"),
            Err(_) if destination.exists() => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_copy_preserves_legacy_and_existing_destination(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let directory =
            std::env::temp_dir().join(format!("hipocast-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory)?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let legacy = directory.join("legacy.db");
            let local = directory.join("local.db");
            let source = rusqlite::Connection::open(&legacy)?;
            source.execute_batch(
                "CREATE TABLE marker (value INTEGER); INSERT INTO marker VALUES (1);",
            )?;
            copy_legacy_database(&legacy, &local)?;
            let destination = rusqlite::Connection::open(&local)?;
            destination.execute("UPDATE marker SET value = 2", [])?;
            copy_legacy_database(&legacy, &local)?;
            let read_marker = |db: &rusqlite::Connection| {
                db.query_row("SELECT value FROM marker", [], |row| row.get::<_, i32>(0))
            };
            assert_eq!(read_marker(&source)?, 1);
            assert_eq!(read_marker(&destination)?, 2);
            Ok(())
        })();
        std::fs::remove_dir_all(directory)?;
        result
    }
}
