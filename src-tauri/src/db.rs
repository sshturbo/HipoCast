use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbSettings {
    pub id: Option<i32>,
    pub framerate: u32,
    pub hls_time: f64,
    pub hls_server_port: u16,
    pub bitrate: u32,
    pub enable_audio: bool,
    pub enable_microphone: bool,
    pub audio_bitrate: u32,
    pub audio_buffer_size: u32,
    pub audio_offset: i32,
    pub audio_device: String,
    pub microphone_device: String,
    pub hls_list_size: u32,
    pub width: u32,
    pub height: u32,
    pub enable_hw_accel: bool,
    pub ffmpeg_preset: String,
    // Global Audio Effects Configuration
    pub global_audio_preset: String,
    pub global_audio_volume: f32,
    pub global_audio_custom_filters: Option<String>,
    // Global Microphone Effects Configuration
    pub global_microphone_preset: String,
    pub global_microphone_volume: f32,
    pub global_microphone_custom_filters: Option<String>,
}

impl Default for DbSettings {
    fn default() -> Self {
        Self {
            id: None,
            framerate: 60,
            hls_time: 2.0,
            hls_server_port: 8080,
            bitrate: 6_000_000,
            enable_audio: true,
            enable_microphone: false,
            audio_bitrate: 128,
            audio_buffer_size: 50,
            audio_offset: 0, 
            audio_device: "".to_string(),
            microphone_device: "".to_string(),
            hls_list_size: 5,
            width: 1280,
            height: 720,
            enable_hw_accel: true,
            ffmpeg_preset: "ultrafast".to_string(),
            global_audio_preset: "none".to_string(),
            global_audio_volume: 1.0,
            global_audio_custom_filters: None,
            global_microphone_preset: "none".to_string(),
            global_microphone_volume: 1.0,
            global_microphone_custom_filters: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbStream {
    pub id: String,
    pub title: String,
    pub status: String,
    pub hls_path: String,
    pub created_at: String,
    pub source_type: String,     
    pub source_url: Option<String>, 
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn init(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Create settings table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY,
                framerate INTEGER NOT NULL,
                hls_time REAL NOT NULL,
                hls_server_port INTEGER NOT NULL,
                bitrate INTEGER NOT NULL,
                enable_audio BOOLEAN NOT NULL,
                hls_list_size INTEGER NOT NULL,
                width INTEGER NOT NULL DEFAULT 1280,
                height INTEGER NOT NULL DEFAULT 720,
                enable_hw_accel BOOLEAN NOT NULL DEFAULT 1,
                ffmpeg_preset TEXT NOT NULL DEFAULT 'ultrafast',
                global_audio_preset TEXT NOT NULL DEFAULT 'none',
                global_audio_volume REAL NOT NULL DEFAULT 0.7,
                global_audio_custom_filters TEXT,
                global_microphone_preset TEXT NOT NULL DEFAULT 'balanced',
                global_microphone_volume REAL NOT NULL DEFAULT 1.0,
                global_microphone_custom_filters TEXT
            )",
            [],
        )?;

        // Migration: Add columns if they don't exist
        // Note: rusqlite doesn't support easy "ADD COLUMN IF NOT EXISTS" in one go for SQLite < 3.35 in all contexts,
        // but we can just try to add them and ignore errors, or check pragma.
        // For simplicity in this app, checking PRAGMA is safer to avoid errors in logs.

        let (
            has_hls_list_size,
            has_width,
            has_height,
            has_hw_accel,
            has_preset,
            has_mic,
            has_audio_br,
            has_audio_buf,
            has_audio_off,
            has_audio_dev,
            has_mic_dev,
            has_global_preset,
            has_global_vol,
            has_global_filters,
            has_global_mic_preset,
            has_global_mic_vol,
            has_global_mic_filters,
        ) = {
            let mut stmt = conn.prepare("PRAGMA table_info(settings)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;

            let mut has_hls_list_size = false;
            let mut has_width = false;
            let mut has_height = false;
            let mut has_hw_accel = false;
            let mut has_preset = false;
            let mut has_mic = false;
            let mut has_audio_br = false;
            let mut has_audio_buf = false;
            let mut has_audio_off = false;
            let mut has_audio_dev = false;
            let mut has_mic_dev = false;
            let mut has_global_preset = false;
            let mut has_global_vol = false;
            let mut has_global_filters = false;
            let mut has_global_mic_preset = false;
            let mut has_global_mic_vol = false;
            let mut has_global_mic_filters = false;

            for name_result in rows {
                if let Ok(name) = name_result {
                    if name == "hls_list_size" {
                        has_hls_list_size = true;
                    }
                    if name == "width" {
                        has_width = true;
                    }
                    if name == "height" {
                        has_height = true;
                    }
                    if name == "enable_hw_accel" {
                        has_hw_accel = true;
                    }
                    if name == "ffmpeg_preset" {
                        has_preset = true;
                    }
                    if name == "enable_microphone" {
                        has_mic = true;
                    }
                    if name == "audio_bitrate" {
                        has_audio_br = true;
                    }
                    if name == "audio_buffer_size" {
                        has_audio_buf = true;
                    }
                    if name == "audio_offset" {
                        has_audio_off = true;
                    }
                    if name == "audio_device" {
                        has_audio_dev = true;
                    }
                    if name == "microphone_device" {
                        has_mic_dev = true;
                    }
                    if name == "global_audio_preset" {
                        has_global_preset = true;
                    }
                    if name == "global_audio_volume" {
                        has_global_vol = true;
                    }
                    if name == "global_audio_custom_filters" {
                        has_global_filters = true;
                    }
                    if name == "global_microphone_preset" {
                        has_global_mic_preset = true;
                    }
                    if name == "global_microphone_volume" {
                        has_global_mic_vol = true;
                    }
                    if name == "global_microphone_custom_filters" {
                        has_global_mic_filters = true;
                    }
                }
            }
            (
                has_hls_list_size,
                has_width,
                has_height,
                has_hw_accel,
                has_preset,
                has_mic,
                has_audio_br,
                has_audio_buf,
                has_audio_off,
                has_audio_dev,
                has_mic_dev,
                has_global_preset,
                has_global_vol,
                has_global_filters,
                has_global_mic_preset,
                has_global_mic_vol,
                has_global_mic_filters,
            )
        };

        if !has_hls_list_size {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN hls_list_size INTEGER NOT NULL DEFAULT 5",
                [],
            );
        }
        if !has_width {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN width INTEGER NOT NULL DEFAULT 1280",
                [],
            );
        }
        if !has_height {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN height INTEGER NOT NULL DEFAULT 720",
                [],
            );
        }
        if !has_hw_accel {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN enable_hw_accel BOOLEAN NOT NULL DEFAULT 1",
                [],
            );
        }
        if !has_preset {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN ffmpeg_preset TEXT NOT NULL DEFAULT 'ultrafast'",
                [],
            );
        }
        if !has_mic {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN enable_microphone BOOLEAN NOT NULL DEFAULT 0",
                [],
            );
        }
        if !has_audio_br {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN audio_bitrate INTEGER NOT NULL DEFAULT 128",
                [],
            );
        }
        if !has_audio_buf {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN audio_buffer_size INTEGER NOT NULL DEFAULT 50",
                [],
            );
        }
        if !has_audio_off {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN audio_offset INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }
        if !has_audio_dev {
            let _ = conn.execute("ALTER TABLE settings ADD COLUMN audio_device TEXT NOT NULL DEFAULT 'virtual-audio-capturer'", []);
        }
        if !has_mic_dev {
            let _ = conn.execute("ALTER TABLE settings ADD COLUMN microphone_device TEXT NOT NULL DEFAULT 'virtual-audio-capturer'", []);
        }
        if !has_global_preset {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN global_audio_preset TEXT NOT NULL DEFAULT 'none'",
                [],
            );
        }
        if !has_global_vol {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN global_audio_volume REAL NOT NULL DEFAULT 0.7",
                [],
            );
        }
        if !has_global_filters {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN global_audio_custom_filters TEXT",
                [],
            );
        }
        if !has_global_mic_preset {
            let _ = conn.execute("ALTER TABLE settings ADD COLUMN global_microphone_preset TEXT NOT NULL DEFAULT 'balanced'", []);
        }
        if !has_global_mic_vol {
            let _ = conn.execute("ALTER TABLE settings ADD COLUMN global_microphone_volume REAL NOT NULL DEFAULT 1.0", []);
        }
        if !has_global_mic_filters {
            let _ = conn.execute(
                "ALTER TABLE settings ADD COLUMN global_microphone_custom_filters TEXT",
                [],
            );
        }

        // Ensure default settings exist
        let count: i32 = conn.query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))?;
        if count == 0 {
            conn.execute(
                "INSERT INTO settings (framerate, hls_time, hls_server_port, bitrate, enable_audio, hls_list_size, width, height, enable_hw_accel, ffmpeg_preset)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![60, 2.0, 8080, 6_000_000, true, 5, 1280, 720, true, "ultrafast"],
            )?;
        }

        // Create streams table with source_type and source_url
        conn.execute(
            "CREATE TABLE IF NOT EXISTS streams (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                hls_path TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                source_type TEXT NOT NULL DEFAULT 'window',
                source_url TEXT
            )",
            [],
        )?;

        // Migration: Add source_type and source_url if they don't exist
        let (has_source_type, has_source_url) = {
            let mut stmt = conn.prepare("PRAGMA table_info(streams)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut has_source_type = false;
            let mut has_source_url = false;
            for name_result in rows {
                if let Ok(name) = name_result {
                    if name == "source_type" {
                        has_source_type = true;
                    }
                    if name == "source_url" {
                        has_source_url = true;
                    }
                }
            }
            (has_source_type, has_source_url)
        };

        if !has_source_type {
            let _ = conn.execute(
                "ALTER TABLE streams ADD COLUMN source_type TEXT NOT NULL DEFAULT 'window'",
                [],
            );
        }
        if !has_source_url {
            let _ = conn.execute("ALTER TABLE streams ADD COLUMN source_url TEXT", []);
        }

        // Initialize stream audio config table
        Self::init_stream_audio_config_table(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // Settings Operations
    pub fn get_settings(&self) -> Result<DbSettings> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, framerate, hls_time, hls_server_port, bitrate, enable_audio, enable_microphone, audio_bitrate, audio_buffer_size, audio_offset, audio_device, hls_list_size, width, height, enable_hw_accel, ffmpeg_preset, microphone_device, global_audio_preset, global_audio_volume, global_audio_custom_filters, global_microphone_preset, global_microphone_volume, global_microphone_custom_filters FROM settings LIMIT 1",
            [],
            |row| {
                Ok(DbSettings {
                    id: Some(row.get(0)?),
                    framerate: row.get(1)?,
                    hls_time: row.get(2)?,
                    hls_server_port: row.get(3)?,
                    bitrate: row.get(4)?,
                    enable_audio: row.get(5)?,
                    enable_microphone: row.get(6).unwrap_or(false),
                    audio_bitrate: row.get(7).unwrap_or(128),
                    audio_buffer_size: row.get(8).unwrap_or(50),
                    audio_offset: row.get(9).unwrap_or(0),
                    audio_device: row.get(10).unwrap_or("".to_string()),
                    hls_list_size: row.get(11).unwrap_or(5),
                    width: row.get(12).unwrap_or(1280),
                    height: row.get(13).unwrap_or(720),
                    enable_hw_accel: row.get(14).unwrap_or(true),
                    ffmpeg_preset: row.get(15).unwrap_or("ultrafast".to_string()),
                    microphone_device: row.get(16).unwrap_or("".to_string()),
                    global_audio_preset: row.get(17).unwrap_or("none".to_string()),
                    global_audio_volume: row.get(18).unwrap_or(0.7),
                    global_audio_custom_filters: row.get(19).ok(),
                    global_microphone_preset: row.get(20).unwrap_or("balanced".to_string()),
                    global_microphone_volume: row.get(21).unwrap_or(1.0),
                    global_microphone_custom_filters: row.get(22).ok(),
                })
            },
        )
    }

    pub fn update_settings(&self, settings: &DbSettings) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE settings SET framerate = ?1, hls_time = ?2, hls_server_port = ?3, bitrate = ?4, enable_audio = ?5, enable_microphone = ?6, audio_bitrate = ?7, audio_buffer_size = ?8, audio_offset = ?9, audio_device = ?10, hls_list_size = ?11, width = ?12, height = ?13, enable_hw_accel = ?14, ffmpeg_preset = ?15, microphone_device = ?16, global_audio_preset = ?17, global_audio_volume = ?18, global_audio_custom_filters = ?19, global_microphone_preset = ?20, global_microphone_volume = ?21, global_microphone_custom_filters = ?22 WHERE id = (SELECT id FROM settings LIMIT 1)",
            params![
                settings.framerate,
                settings.hls_time,
                settings.hls_server_port,
                settings.bitrate,
                settings.enable_audio,
                settings.enable_microphone,
                settings.audio_bitrate,
                settings.audio_buffer_size,
                settings.audio_offset,
                settings.audio_device,
                settings.hls_list_size,
                settings.width,
                settings.height,
                settings.enable_hw_accel,
                settings.ffmpeg_preset,
                settings.microphone_device,
                settings.global_audio_preset,
                settings.global_audio_volume,
                settings.global_audio_custom_filters,
                settings.global_microphone_preset,
                settings.global_microphone_volume,
                settings.global_microphone_custom_filters,
            ],
        )?;
        Ok(())
    }

    // Stream Operations
    pub fn add_stream(&self, stream: &DbStream) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO streams (id, title, status, hls_path, source_type, source_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![stream.id, stream.title, stream.status, stream.hls_path, stream.source_type, stream.source_url],
        )?;
        Ok(())
    }

    pub fn update_stream_status(&self, id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE streams SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn get_all_streams(&self) -> Result<Vec<DbStream>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, title, status, hls_path, created_at, source_type, source_url FROM streams ORDER BY created_at DESC")?;

        let streams = stmt.query_map([], |row| {
            Ok(DbStream {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                hls_path: row.get(3)?,
                created_at: row.get(4)?,
                source_type: row
                    .get::<_, Option<String>>(5)?
                    .unwrap_or("window".to_string()),
                source_url: row.get(6)?,
            })
        })?;

        let mut result = Vec::new();
        for stream in streams {
            result.push(stream?);
        }
        Ok(result)
    }

    pub fn remove_stream(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM streams WHERE id = ?1", params![id])?;
        // Also remove stream audio config if exists
        let _ = conn.execute(
            "DELETE FROM stream_audio_config WHERE stream_id = ?1",
            params![id],
        );
        Ok(())
    }
}

// Stream Audio Configuration structures (moved from stream_config.rs)
use crate::stream_config::{AudioEffects, MicrophoneEffects, StreamAudioConfig};

impl Database {
    /// Initialize stream audio config table (called during init)
    fn init_stream_audio_config_table(conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS stream_audio_config (
                stream_id TEXT PRIMARY KEY,
                audio_mode TEXT NOT NULL DEFAULT 'system',
                target_pid INTEGER,
                target_process_name TEXT,
                enable_microphone INTEGER NOT NULL DEFAULT 0,
                microphone_device TEXT NOT NULL DEFAULT '',
                audio_effects_enabled INTEGER NOT NULL DEFAULT 0,
                audio_effects_preset TEXT NOT NULL DEFAULT 'cleanbalanced',
                audio_effects_custom_filters TEXT,
                audio_effects_master_volume REAL NOT NULL DEFAULT 0.7,
                microphone_effects_enabled INTEGER NOT NULL DEFAULT 0,
                microphone_effects_preset TEXT NOT NULL DEFAULT 'balanced',
                microphone_effects_custom_filters TEXT,
                microphone_effects_volume REAL NOT NULL DEFAULT 1.0
            )",
            [],
        )?;
        Ok(())
    }

    /// Get stream-specific audio configuration
    pub fn get_stream_audio_config(&self, stream_id: &str) -> Result<Option<StreamAudioConfig>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT stream_id, audio_mode, target_pid, target_process_name, enable_microphone, microphone_device,
                    audio_effects_enabled, audio_effects_preset, audio_effects_custom_filters, audio_effects_master_volume,
                    microphone_effects_enabled, microphone_effects_preset, microphone_effects_custom_filters, microphone_effects_volume
             FROM stream_audio_config WHERE stream_id = ?1",
            params![stream_id],
            |row| {
                Ok(StreamAudioConfig {
                    stream_id: row.get(0)?,
                    audio_mode: row.get(1)?,
                    target_pid: row.get::<_, Option<i64>>(2)?.map(|p| p as u32),
                    target_process_name: row.get(3)?,
                    enable_microphone: row.get::<_, i32>(4)? != 0,
                    microphone_device: row.get(5)?,
                    audio_effects: AudioEffects {
                        enabled: row.get::<_, i32>(6)? != 0,
                        preset: serde_json::from_str(&format!("\"{}\"", row.get::<_, String>(7)?)).unwrap_or_default(),
                        custom_filters: row.get(8)?,
                        master_volume: row.get(9)?,
                    },
                    microphone_effects: MicrophoneEffects {
                        enabled: row.get::<_, i32>(10)? != 0,
                        preset: serde_json::from_str(&format!("\"{}\"", row.get::<_, String>(11)?)).unwrap_or_default(),
                        custom_filters: row.get(12)?,
                        volume: row.get(13)?,
                    },
                })
            },
        ) {
            Ok(config) => Ok(Some(config)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Save or update stream audio configuration
    pub fn save_stream_audio_config(&self, config: &StreamAudioConfig) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let audio_preset_str = format!("{:?}", config.audio_effects.preset).to_lowercase();
        let mic_preset_str = format!("{:?}", config.microphone_effects.preset).to_lowercase();

        conn.execute(
            "INSERT OR REPLACE INTO stream_audio_config 
             (stream_id, audio_mode, target_pid, target_process_name, enable_microphone, microphone_device,
              audio_effects_enabled, audio_effects_preset, audio_effects_custom_filters, audio_effects_master_volume,
              microphone_effects_enabled, microphone_effects_preset, microphone_effects_custom_filters, microphone_effects_volume)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                config.stream_id,
                config.audio_mode,
                config.target_pid.map(|p| p as i64),
                config.target_process_name,
                config.enable_microphone as i32,
                config.microphone_device,
                config.audio_effects.enabled as i32,
                audio_preset_str,
                config.audio_effects.custom_filters,
                config.audio_effects.master_volume,
                config.microphone_effects.enabled as i32,
                mic_preset_str,
                config.microphone_effects.custom_filters,
                config.microphone_effects.volume,
            ],
        )?;
        Ok(())
    }

    /// Delete stream audio configuration
    pub fn delete_stream_audio_config(&self, stream_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM stream_audio_config WHERE stream_id = ?1",
            params![stream_id],
        )?;
        Ok(())
    }

    /// Create default audio config from global settings
    pub fn create_default_audio_config_from_global(&self, stream_id: &str) -> StreamAudioConfig {
        let settings = self.get_settings().unwrap_or_default();

        StreamAudioConfig {
            stream_id: stream_id.to_string(),
            audio_mode: if settings.enable_audio {
                "system"
            } else {
                "muted"
            }
            .to_string(),
            target_pid: None,
            target_process_name: None,
            enable_microphone: settings.enable_microphone,
            microphone_device: settings.microphone_device,
            audio_effects: AudioEffects {
                enabled: settings.global_audio_preset != "none",
                preset: serde_json::from_str(&format!("\"{}\"", settings.global_audio_preset))
                    .unwrap_or_default(),
                custom_filters: settings.global_audio_custom_filters,
                master_volume: settings.global_audio_volume,
            },
            microphone_effects: MicrophoneEffects {
                enabled: settings.global_microphone_preset != "none",
                preset: serde_json::from_str(&format!("\"{}\"", settings.global_microphone_preset))
                    .unwrap_or_default(),
                custom_filters: settings.global_microphone_custom_filters,
                volume: settings.global_microphone_volume,
            },
        }
    }
}
