use crate::capture::{self, CaptureSource};
use crate::commands::stream::{find_stream, get_status};
use crate::db::DbStream;
use crate::output::HlsConfig;
use crate::state::{ActiveCapture, AppState, StatusResponse, StreamRuntime};
use tauri::{AppHandle, Emitter, LogicalSize, State, Url, WebviewUrl, WebviewWindowBuilder};

#[tauri::command]
pub fn get_capture_sources() -> Vec<CaptureSource> {
    capture::list_windows()
}

#[tauri::command]
pub async fn start_source_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    title: String,
    custom_id: Option<String>,
) -> Result<StatusResponse, String> {
    let source_type = source_kind(&id).ok_or("Fonte de captura inválida.")?;
    let stream_id = match custom_id {
        Some(custom_id) => validate_custom_id(&custom_id)?.to_string(),
        None => id.clone(),
    };
    let stream = new_stream(stream_id, title, source_type, Some(id), None)?;
    start_saved(&app, &state, &mut *state.capture.lock().await, stream)?;
    Ok(get_status(state.clone()))
}

#[tauri::command]
pub async fn start_browser_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    title: String,
    custom_id: Option<String>,
) -> Result<StatusResponse, String> {
    let url = parse_web_url(&url)?;
    let stream_id = match custom_id {
        Some(custom_id) => validate_custom_id(&custom_id)?.to_string(),
        None => format!("browser_{}", uuid::Uuid::new_v4()),
    };
    let stream = new_stream(stream_id, title, "browser", None, Some(url.to_string()))?;
    start_saved(&app, &state, &mut *state.capture.lock().await, stream)?;
    Ok(get_status(state.clone()))
}

#[tauri::command]
pub async fn resume_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<StatusResponse, String> {
    let mut capture = state.capture.lock().await;
    let stream = find_stream(&state, &id)?;
    start_saved(&app, &state, &mut capture, stream)?;
    Ok(get_status(state.clone()))
}

#[tauri::command]
pub async fn stop_source_capture(
    state: State<'_, AppState>,
    id: Option<String>,
) -> Result<StatusResponse, String> {
    let mut capture = state.capture.lock().await;
    if capture
        .as_ref()
        .is_some_and(|active| id.as_ref().is_none_or(|id| id == &active.id))
    {
        stop_capture(&state, &mut capture).await?;
    }
    Ok(get_status(state.clone()))
}

pub(crate) async fn stop_capture(
    state: &AppState,
    capture: &mut Option<ActiveCapture>,
) -> Result<(), String> {
    if let Some(ActiveCapture { id, mut handle }) = capture.take() {
        tauri::async_runtime::spawn_blocking(move || handle.stop())
            .await
            .map_err(|error| format!("Falha ao aguardar a captura: {error}"))?;
        state.runtime.lock().unwrap().remove(&id);
    }
    Ok(())
}

pub(crate) async fn restart_if_active(
    app: &AppHandle,
    state: &AppState,
    capture: &mut Option<ActiveCapture>,
    id: Option<&str>,
) -> Result<bool, String> {
    let Some(active) = capture
        .as_ref()
        .filter(|active| !active.handle.is_finished() && id.is_none_or(|id| id == active.id))
    else {
        return Ok(false);
    };
    let stream = find_stream(state, &active.id)?;
    stop_capture(state, capture).await?;
    start_saved(app, state, capture, stream).map_err(|error| {
        format!("Configurações salvas. Falha ao reiniciar a transmissão: {error}")
    })?;
    Ok(true)
}

fn start_saved(
    app: &AppHandle,
    state: &AppState,
    capture: &mut Option<ActiveCapture>,
    mut stream: DbStream,
) -> Result<(), String> {
    if capture
        .as_ref()
        .is_some_and(|active| !active.handle.is_finished())
    {
        return Err("Já existe uma captura ativa. Pause-a antes de iniciar outra.".into());
    }
    let port = *state.hls_server.as_ref().map_err(Clone::clone)?;
    let name = output_name(&stream.id)?;
    let path = format!("/streams/{name}/{name}.m3u8");
    if state
        .db
        .stream_path_in_use(&stream.id, &path)
        .map_err(|error| error.to_string())?
    {
        return Err("Esse identificador já está associado a outra transmissão.".into());
    }
    let settings = state.db.get_settings().map_err(|error| error.to_string())?;
    crate::commands::settings::validate_settings(&settings)?;
    let audio = state
        .db
        .get_stream_audio_config(&stream.id)
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| state.db.create_default_audio_config_from_global(&stream.id));
    let source_id = if stream.source_type == "browser" {
        let url = parse_web_url(stream.source_url.as_deref().unwrap_or_default())?;
        browser_source(app, state, &stream.id, url, &stream.title, &settings)?
    } else {
        let source = stream.source_id.take().unwrap_or_else(|| stream.id.clone());
        stream.source_type = source_kind(&source)
            .ok_or("Fonte antiga inválida. Selecione uma nova fonte.")?
            .into();
        stream.source_id = Some(source.clone());
        source
    };
    stream.status = "stopped".to_string();
    stream.hls_path = path;
    state
        .db
        .add_stream(&stream)
        .map_err(|error| error.to_string())?;

    let hls = HlsConfig::new(
        &state.streams_path.to_string_lossy(),
        &name,
        settings.hls_list_size,
        settings.hls_time,
    );
    let runtime = state.runtime.clone();
    let app = app.clone();
    let id = stream.id.clone();
    let update = move |status, message| {
        {
            let mut streams = runtime.lock().unwrap();
            let playback_id = if status == "starting" {
                uuid::Uuid::new_v4().to_string()
            } else {
                streams
                    .get(&id)
                    .map(|stream| stream.playback_id.clone())
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
            };
            streams.insert(
                id.clone(),
                StreamRuntime {
                    status,
                    message,
                    playback_id,
                },
            );
        }
        let _ = app.emit_to("main", "hipocast:stream-status", ());
    };
    update("starting", None);
    let handle =
        capture::start_capture(source_id, settings, audio, hls, port, update).map_err(|error| {
            state.runtime.lock().unwrap().remove(&stream.id);
            format!("Não foi possível iniciar a captura: {error}")
        })?;
    *capture = Some(ActiveCapture {
        id: stream.id,
        handle,
    });
    Ok(())
}

fn browser_source(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    url: Url,
    title: &str,
    settings: &crate::AppSettings,
) -> Result<String, String> {
    let existing = state.browser_windows.lock().unwrap().get(id).cloned();
    if let Some((previous_url, window)) = existing {
        if previous_url == url {
            if let Ok(hwnd) = window.hwnd() {
                let _ = window.show();
                let _ = window.set_skip_taskbar(false);
                let _ = window.unminimize();
                window
                    .set_size(LogicalSize::new(settings.width, settings.height))
                    .map_err(|error| {
                        format!("Não foi possível ajustar a janela da página: {error}")
                    })?;
                let _ = window.set_title(title);
                return Ok(format!("window:{}", hwnd.0 as usize));
            }
        }
        state.browser_windows.lock().unwrap().remove(id);
        let _ = window.destroy();
    }

    let label = format!("source_{}", uuid::Uuid::new_v4());
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url.clone()))
        .title(title)
        .inner_size(f64::from(settings.width), f64::from(settings.height))
        .center()
        .build()
        .map_err(|error| format!("Não foi possível abrir a página: {error}"))?;
    let hwnd = window.hwnd().map_err(|error| {
        let _ = window.destroy();
        format!("Não foi possível localizar a janela da página: {error}")
    })?;
    state
        .browser_windows
        .lock()
        .unwrap()
        .insert(id.to_string(), (url, window));
    Ok(format!("window:{}", hwnd.0 as usize))
}

fn new_stream(
    id: String,
    title: String,
    source_type: &str,
    source_id: Option<String>,
    source_url: Option<String>,
) -> Result<DbStream, String> {
    let name = output_name(&id)?;
    Ok(DbStream {
        id,
        title,
        status: "stopped".to_string(),
        hls_path: format!("/streams/{name}/{name}.m3u8"),
        created_at: chrono::Local::now().to_rfc3339(),
        source_type: source_type.to_string(),
        source_id,
        source_url,
    })
}

fn source_kind(id: &str) -> Option<&'static str> {
    let (kind, number) = id.split_once(':')?;
    if number.is_empty()
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || number.parse::<usize>().is_err()
    {
        return None;
    }
    match kind {
        "window" => Some("window"),
        "monitor" => Some("monitor"),
        _ => None,
    }
}

fn validate_custom_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err("Use até 64 letras, números, hífens ou sublinhados no identificador.".into());
    }
    let upper = id.to_ascii_uppercase();
    let reserved_number = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"));
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved_number.is_some_and(|number| {
            number.len() == 1 && number != "0" && number.as_bytes()[0].is_ascii_digit()
        })
    {
        return Err("Esse identificador é reservado pelo Windows.".into());
    }
    Ok(id)
}

pub(crate) fn output_name(id: &str) -> Result<String, String> {
    if source_kind(id).is_some() {
        return Ok(id.replace(':', "_"));
    }
    validate_custom_id(id).map(str::to_string)
}

fn parse_web_url(input: &str) -> Result<Url, String> {
    let url =
        Url::parse(input.trim()).map_err(|_| "Informe um link válido com http:// ou https://.")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Informe um link válido com http:// ou https://.".into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_id_preserves_the_selected_capture_source() {
        let source = String::from("monitor:1");
        let stream = new_stream(
            "sala-1".into(),
            "Sala".into(),
            source_kind(&source).unwrap(),
            Some(source),
            None,
        )
        .unwrap();
        assert_eq!(stream.hls_path, "/streams/sala-1/sala-1.m3u8");
        assert_eq!(stream.source_id.as_deref(), Some("monitor:1"));
        assert_eq!(output_name("window:123").as_deref(), Ok("window_123"));
        assert!(output_name("COM1").is_err());
        assert!(output_name("canal/extra").is_err());
    }
}
