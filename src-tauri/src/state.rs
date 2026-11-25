use crate::{capture::CaptureHandle, db::Database};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Url, WebviewWindow};

// Re-export settings from DB module to keep API consistent
pub use crate::db::DbSettings as AppSettings;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamStatus {
    pub id: String,
    pub title: String,
    pub status: String,
    pub hls_path: String,
    pub message: Option<String>,
    pub playback_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub streams: Vec<StreamStatus>,
    pub active: usize,
    pub server_error: Option<String>,
}

// Struct para informações de dispositivo de áudio (usado no cache)
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub name: String,
    pub device_type: String, // "loopback" ou "input"
}

pub struct AppState {
    pub capture: tokio::sync::Mutex<Option<ActiveCapture>>,
    pub runtime: Arc<Mutex<HashMap<String, StreamRuntime>>>,
    pub browser_windows: Mutex<HashMap<String, (Url, WebviewWindow)>>,
    pub db: Arc<Database>,
    pub streams_path: std::path::PathBuf,
    pub hls_server: Result<u16, String>,
    pub audio_devices_cache: Mutex<Option<(std::time::Instant, Vec<AudioDeviceInfo>)>>,
}

pub struct ActiveCapture {
    pub id: String,
    pub handle: CaptureHandle,
}

pub struct StreamRuntime {
    pub status: &'static str,
    pub message: Option<String>,
    pub playback_id: String,
}
