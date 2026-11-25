use crate::commands::capture_cmd::{output_name, stop_capture};
use crate::db::DbStream;
use crate::state::{AppState, StatusResponse, StreamStatus};
use tauri::State;

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> StatusResponse {
    let port = state
        .hls_server
        .as_ref()
        .copied()
        .unwrap_or_else(|_| state.db.get_settings().unwrap_or_default().hls_server_port);
    let saved = state.db.get_all_streams().unwrap_or_else(|error| {
        tracing::error!(%error, "Falha ao carregar as transmissões");
        Vec::new()
    });
    let runtime = state.runtime.lock().unwrap();
    let streams: Vec<_> = saved
        .into_iter()
        .map(|stream| {
            let live = runtime.get(&stream.id);
            StreamStatus {
                status: live.map_or("stopped", |entry| entry.status).to_string(),
                message: live.and_then(|entry| entry.message.clone()),
                playback_id: live.map(|entry| entry.playback_id.clone()),
                hls_path: format!("http://127.0.0.1:{port}{}", stream.hls_path),
                id: stream.id,
                title: stream.title,
            }
        })
        .collect();
    let active = streams
        .iter()
        .filter(|stream| stream.status == "running")
        .count();
    StatusResponse {
        streams,
        active,
        server_error: state.hls_server.as_ref().err().cloned(),
    }
}

pub(crate) fn find_stream(state: &AppState, id: &str) -> Result<DbStream, String> {
    state
        .db
        .get_all_streams()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|stream| stream.id == id)
        .ok_or_else(|| "Transmissão não encontrada.".into())
}

#[tauri::command]
pub fn get_stream_info(state: State<'_, AppState>, stream_id: String) -> Result<DbStream, String> {
    let mut stream = find_stream(&state, &stream_id)?;
    stream.status = state
        .runtime
        .lock()
        .unwrap()
        .get(&stream_id)
        .map_or("stopped", |entry| entry.status)
        .into();
    Ok(stream)
}

#[tauri::command]
pub async fn remove_stream(
    state: State<'_, AppState>,
    id: String,
) -> Result<StatusResponse, String> {
    let mut capture = state.capture.lock().await;
    let stream = find_stream(&state, &id)?;
    if capture.as_ref().is_some_and(|active| active.id == id) {
        stop_capture(&state, &mut capture).await?;
    }

    if let Ok(name) = output_name(&id) {
        let expected_path = format!("/streams/{name}/{name}.m3u8");
        let shared = state
            .db
            .stream_path_in_use(&id, &expected_path)
            .map_err(|error| error.to_string())?;
        if !shared && stream.hls_path == expected_path {
            let directory = state.streams_path.join(name);
            tauri::async_runtime::spawn_blocking(move || {
                match std::fs::remove_dir_all(directory) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(format!("Falha ao remover os arquivos HLS: {error}")),
                }
            })
            .await
            .map_err(|error| error.to_string())??;
        }
    }

    state
        .db
        .remove_stream(&id)
        .map_err(|error| error.to_string())?;
    state.runtime.lock().unwrap().remove(&id);
    let window = state.browser_windows.lock().unwrap().remove(&id);
    if let Some((_, window)) = window {
        let _ = window.destroy();
    }
    Ok(get_status(state.clone()))
}
