use crate::db::DbStream;
use crate::state::{AppState, StatusResponse, StreamStatus};
use tauri::State;

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> StatusResponse {
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
            tracing::error!("Failed to fetch streams: {}", e);
            Vec::new()
        }
    };

    let active = streams.iter().filter(|s| s.status == "running").count();

    StatusResponse { streams, active }
}

#[tauri::command]
pub fn get_stream_info(state: State<'_, AppState>, stream_id: String) -> Result<DbStream, String> {
    match state.db.get_all_streams() {
        Ok(streams) => streams
            .into_iter()
            .find(|s| s.id == stream_id)
            .ok_or_else(|| "Stream not found".to_string()),
        Err(e) => Err(format!("Database error: {}", e)),
    }
}

#[tauri::command]
pub fn remove_stream(state: State<'_, AppState>, id: String) -> StatusResponse {
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
        tracing::error!("Failed to remove stream from DB: {}", e);
    }

    // Remove files from disk using GLOBAL path
    let stream_dir = state.streams_path.join(id.replace(":", "_"));

    if stream_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&stream_dir) {
            tracing::error!("Failed to remove stream directory: {}", e);
        } else {
            tracing::info!("Removed stream files: {:?}", stream_dir);
        }
    }

    get_status(state)
}
