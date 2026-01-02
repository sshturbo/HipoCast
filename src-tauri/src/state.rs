use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::WebviewWindow;

// Re-export settings from DB module to keep API consistent
pub use crate::db::DbSettings as AppSettings;

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

// Struct para informações de dispositivo de áudio (usado no cache)
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub name: String,
    pub device_type: String, // "loopback" ou "input"
}

pub struct AppState {
    pub capture_handle: Mutex<Option<Box<dyn Send + Sync + 'static>>>,
    pub active_stream_id: Mutex<Option<String>>,
    pub browser_windows: Mutex<std::collections::HashMap<String, WebviewWindow>>,
    pub db: Arc<Database>,
    pub streams_path: std::path::PathBuf,
    pub audio_devices_cache: Mutex<Option<(std::time::Instant, Vec<AudioDeviceInfo>)>>,
}
