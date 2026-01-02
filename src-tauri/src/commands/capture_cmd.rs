use crate::capture::{self, list_windows, CaptureSource};
use crate::commands::stream::get_status;
use crate::db::DbStream;
use crate::state::{AppState, StatusResponse};
use tauri::{AppHandle, State, WebviewUrl, WebviewWindowBuilder}; // Manager removido

#[tauri::command]
pub async fn start_browser_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    title: String,
    custom_id: Option<String>,
) -> Result<StatusResponse, String> {
    // Determinar o stream_id primeiro
    let stream_id = custom_id
        .clone()
        .unwrap_or_else(|| format!("browser_{}", uuid::Uuid::new_v4()));

    tracing::info!(
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
                tracing::info!(
                    "♻️ Reutilizando janela existente para stream '{}' (HWND: {})",
                    stream_id,
                    hwnd.0 as usize
                );
                Some(hwnd_id)
            } else {
                tracing::warn!("⚠️ Janela existe mas HWND inválido, será criada nova janela");
                None
            }
        } else {
            tracing::info!(
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

        tracing::info!(
            "📏 Creating browser window: {}x{}",
            settings.width,
            settings.height
        );

        let window =
            WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url.parse().unwrap()))
                .title(&title)
                .inner_size(win_width, win_height)
                .visible(true) // Necessário ser visível para API de captura do Windows funcionar
                .position(-3000.0, 0.0) // "Ocultar" movendo para fora da tela
                .build()
                .map_err(|e| format!("Failed to create off-screen window: {}", e))?;

        // Guardar referência da janela no mapa usando o stream_id
        {
            let mut b_wins = state.browser_windows.lock().unwrap();
            b_wins.insert(stream_id.clone(), window.clone());
        }

        // Aguardar carregamento (opcional, pode ajustar conforme necessidade)
        tracing::debug!("⏳ Waiting for window creation...");
        std::thread::sleep(std::time::Duration::from_millis(1000)); // Pequeno delay para garantir HWND

        let hwnd = window
            .hwnd()
            .map_err(|e| format!("Failed to get window HWND: {}", e))?;

        tracing::info!("✅ Browser window created. HWND: {:?}", hwnd);

        format!("window:{}", hwnd.0 as usize)
    };

    // Iniciar captura usando a janela (existente ou nova)
    // Passamos o stream_id (para persistência/arquivos) e o capture_source_id (janela real)
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
pub fn get_capture_sources() -> Vec<CaptureSource> {
    list_windows()
}

#[tauri::command]
pub fn start_source_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<StatusResponse, String> {
    start_source_capture_internal(app, state, id, title, "window".to_string(), None)
}

fn start_source_capture_internal(
    app: AppHandle,
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
                tracing::error!("Failed to add stream to DB: {}", e);
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
                    tracing::error!("Failed to save default audio config: {}", e);
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
    app: AppHandle,
    state: State<'_, AppState>,
    stream_id: String,  // Custom ID for folders/files/database
    capture_id: String, // Real window ID for capture
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
            *active_id = Some(stream_id.clone()); // Use stream_id as active ID

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
                tracing::error!("Failed to add stream to DB: {}", e);
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
                    tracing::error!("Failed to save default audio config: {}", e);
                }
            }

            // Return updated status
            Ok(get_status(state.clone()))
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn stop_source_capture(state: State<'_, AppState>) -> StatusResponse {
    let mut handle = state.capture_handle.lock().unwrap();
    let mut active_id = state.active_stream_id.lock().unwrap();

    if let Some(id) = active_id.as_ref() {
        if let Err(e) = state.db.update_stream_status(id, "stopped") {
            tracing::error!("Failed to update stream status: {}", e);
        }
    }

    // FORÇA a finalização do CaptureHandle (drop) e espera threads terminarem
    if handle.is_some() {
        tracing::info!("🛑 Stopping capture and waiting for threads to finish...");
        let start_stop = std::time::Instant::now();
        *handle = None; // Drop CaptureHandle, fecha channel e para threads
        drop(handle); // Libera o lock

        // Aguarda as threads finalizarem (FFmpeg e áudio)
        // Tempo maior para garantir flush de buffers
        tracing::debug!("⏳ Waiting for FFmpeg and audio threads to finish...");
        std::thread::sleep(std::time::Duration::from_millis(2500));

        let elapsed = start_stop.elapsed();
        tracing::info!(
            "✅ Capture stopped in {:.2}s, threads finished",
            elapsed.as_secs_f64()
        );
    }

    *active_id = None;

    // NÃO fechar a janela do browser aqui - apenas ao remover ou iniciar nova stream
    // Isso permite retomar a mesma janela sem recarregar

    get_status(state.clone())
}
