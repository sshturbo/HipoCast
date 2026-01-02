use serde::{Deserialize, Serialize};

// Removed: StreamConfigDb - now consolidated into db.rs Database
// All database operations for stream audio configs are in db.rs

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioPreset {
    None,
    CleanLight,      // CPU: ~2% - Para PCs fracos
    #[default]
    CleanBalanced,   // CPU: ~5% - Padrão
    Professional,    // CPU: ~15% - Para PCs potentes
    Gaming,          // CPU: ~4%
    Podcast,         // CPU: ~18% - Máxima qualidade
    Custom,
}

impl AudioPreset {
    /// Retorna o impacto estimado de CPU
    pub fn cpu_impact(&self) -> &str {
        match self {
            Self::None => "Nenhum",
            Self::CleanLight => "Baixo (~2%)",
            Self::CleanBalanced => "Médio (~5%)",
            Self::Professional => "Alto (~15%)",
            Self::Gaming => "Baixo-Médio (~4%)",
            Self::Podcast => "Muito Alto (~18%)",
            Self::Custom => "Variável",
        }
    }
    
    /// Retorna a descrição do preset
    pub fn description(&self) -> &str {
        match self {
            Self::None => "Sem processamento de áudio",
            Self::CleanLight => "Filtro básico de ruído - Ideal para PCs fracos",
            Self::CleanBalanced => "Filtro balanceado com gate - Recomendado para maioria",
            Self::Professional => "Processamento completo com compressor - Requer CPU potente",
            Self::Gaming => "Otimizado para jogos com realce de graves",
            Self::Podcast => "Máxima qualidade vocal com deesser - CPU muito potente",
            Self::Custom => "Filtros personalizados pelo usuário",
        }
    }
    
    /// Retorna o filtro FFmpeg correspondente
    pub fn to_ffmpeg_filter(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::CleanLight => 
                "highpass=f=50,lowpass=f=18000".to_string(),
            Self::CleanBalanced => 
                "highpass=f=50,lowpass=f=17000,agate=threshold=0.03:ratio=2:attack=20:release=250".to_string(),
            Self::Professional => 
                "highpass=f=60,lowpass=f=15000,acompressor=threshold=0.1:ratio=3:attack=20:release=250,loudnorm=I=-16:TP=-1.5:LRA=11".to_string(),
            Self::Gaming => 
                "highpass=f=40,lowpass=f=18000,agate=threshold=0.02:ratio=3:attack=10:release=200,bass=g=2".to_string(),
            Self::Podcast => 
                "highpass=f=80,lowpass=f=12000,acompressor=threshold=0.1:ratio=4:attack=20:release=250,deesser,loudnorm=I=-19:TP=-1.5".to_string(),
            Self::Custom => String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioEffects {
    pub enabled: bool,
    pub preset: AudioPreset,
    pub custom_filters: Option<String>,
    pub master_volume: f32,  // 0.0 - 2.0 (padrão 1.0)
}

impl Default for AudioEffects {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: AudioPreset::CleanBalanced,
            custom_filters: None,
            master_volume: 0.7,  // Reduzido para 0.7 para garantir headroom extra
        }
    }
}

impl AudioEffects {
    /// Converte para filtro FFmpeg completo
    pub fn to_ffmpeg_filter(&self) -> String {
        // Filtro de proteção contra clipping e distorção (Headroom de segurança)
        // 1. highpass=f=25: Remove frequências infra-sônicas que consomem energia sem som útil
        // 2. alimiter: Garante que o áudio nunca ultrapasse -1dB (previne clipping no encoder AAC)
        let protection = "highpass=f=25,alimiter=level_in=1:level_out=0.9:limit=0.95:attack=5:release=20";
        
        if !self.enabled {
            // Mesmo sem efeitos, aplica proteção + volume para prevenir distorção
            return format!("{},volume={}", protection, self.master_volume);
        }
        
        let base_filter = match self.preset {
            AudioPreset::Custom => {
                self.custom_filters.clone().unwrap_or_default()
            },
            _ => self.preset.to_ffmpeg_filter(),
        };
        
        // Aplica: [preset ou custom] -> proteção -> volume
        if base_filter.is_empty() {
            format!("{},volume={}", protection, self.master_volume)
        } else {
            format!("{},{},volume={}", base_filter, protection, self.master_volume)
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MicrophonePreset {
    None,          // Sem processamento
    Light,         // Leve: apenas noise gate e highpass
    #[default]
    Balanced,      // Balanceado: gate + compressor suave
    Podcast,       // Podcast: processamento completo para voz
    Professional,  // Profissional: máxima qualidade
    Custom,        // Filtros personalizados
}

impl MicrophonePreset {
    pub fn to_ffmpeg_filter(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Light => 
                "highpass=f=80,agate=threshold=0.02:ratio=4:attack=10:release=100".to_string(),
            Self::Balanced => 
                "highpass=f=100,agate=threshold=0.03:ratio=4:attack=10:release=100,acompressor=threshold=0.1:ratio=3:attack=20:release=200".to_string(),
            Self::Podcast => 
                "highpass=f=100,agate=threshold=0.03:ratio=6:attack=10:release=100,acompressor=threshold=0.1:ratio=4:attack=20:release=200,deesser".to_string(),
            Self::Professional => 
                "highpass=f=80,lowpass=f=12000,agate=threshold=0.02:ratio=8:attack=5:release=100,acompressor=threshold=0.09:ratio=5:attack=15:release=250,deesser,loudnorm=I=-20:TP=-1.5".to_string(),
            Self::Custom => String::new(),
        }
    }
    
    pub fn description(&self) -> &str {
        match self {
            Self::None => "Sem processamento",
            Self::Light => "Leve: Redução básica de ruído",
            Self::Balanced => "Balanceado: Gate + Compressor suave (Recomendado)",
            Self::Podcast => "Podcast: Voz profissional com de-esser",
            Self::Professional => "Profissional: Máxima qualidade vocal",
            Self::Custom => "Filtros personalizados",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneEffects {
    pub enabled: bool,
    pub preset: MicrophonePreset,
    pub custom_filters: Option<String>,
    pub volume: f32,  // 0.0 - 2.0 (padrão 1.0)
}

impl Default for MicrophoneEffects {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: MicrophonePreset::Balanced,
            custom_filters: None,
            volume: 1.0,
        }
    }
}

impl MicrophoneEffects {
    pub fn to_ffmpeg_filter(&self) -> String {
        let protection = "alimiter=level_in=1:level_out=0.95:limit=0.98:attack=3:release=15";
        
        if !self.enabled {
            return format!("{},volume={}", protection, self.volume);
        }
        
        let base_filter = match self.preset {
            MicrophonePreset::Custom => {
                self.custom_filters.clone().unwrap_or_default()
            },
            _ => self.preset.to_ffmpeg_filter(),
        };
        
        if base_filter.is_empty() {
            format!("{},volume={}", protection, self.volume)
        } else {
            format!("{},{},volume={}", base_filter, protection, self.volume)
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StreamAudioConfig {
    pub stream_id: String,
    pub audio_mode: String,  // "system", "process", "muted"
    pub target_pid: Option<u32>,
    pub target_process_name: Option<String>,
    pub enable_microphone: bool,
    pub microphone_device: String,
    pub audio_effects: AudioEffects,
    pub microphone_effects: MicrophoneEffects,
}

impl Default for StreamAudioConfig {
    fn default() -> Self {
        Self {
            stream_id: String::new(),
            audio_mode: "system".to_string(),
            target_pid: None,
            target_process_name: None,
            enable_microphone: false,
            microphone_device: "".to_string(),
            audio_effects: AudioEffects::default(),
            microphone_effects: MicrophoneEffects::default(),
        }
    }
}

pub struct StreamConfigDb {
    conn: Mutex<Connection>,
}

impl StreamConfigDb {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        
        // Create table if not exists
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

        // Migration: Add audio_effects columns if they don't exist (for existing databases)
        let columns_to_add = vec![
            ("audio_effects_enabled", "INTEGER NOT NULL DEFAULT 0"),
            ("audio_effects_preset", "TEXT NOT NULL DEFAULT 'cleanbalanced'"),
            ("audio_effects_custom_filters", "TEXT"),
            ("audio_effects_master_volume", "REAL NOT NULL DEFAULT 0.7"),
            ("microphone_effects_enabled", "INTEGER NOT NULL DEFAULT 0"),
            ("microphone_effects_preset", "TEXT NOT NULL DEFAULT 'balanced'"),
            ("microphone_effects_custom_filters", "TEXT"),
            ("microphone_effects_volume", "REAL NOT NULL DEFAULT 1.0"),
        ];

        for (column_name, column_type) in columns_to_add {
            let _ = conn.execute(
                &format!("ALTER TABLE stream_audio_config ADD COLUMN {} {}", column_name, column_type),
                [],
            );
            // Ignora erro se a coluna já existir
        }

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_config(&self, stream_id: &str) -> Result<Option<StreamAudioConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT stream_id, audio_mode, target_pid, target_process_name, enable_microphone, microphone_device,
                    audio_effects_enabled, audio_effects_preset, audio_effects_custom_filters, audio_effects_master_volume,
                    microphone_effects_enabled, microphone_effects_preset, microphone_effects_custom_filters, microphone_effects_volume
             FROM stream_audio_config WHERE stream_id = ?1"
        )?;

        let result = stmt.query_row(params![stream_id], |row| {
            let audio_preset_str: String = row.get(7)?;
            let audio_preset = match audio_preset_str.as_str() {
                "none" => AudioPreset::None,
                "cleanlight" => AudioPreset::CleanLight,
                "cleanbalanced" => AudioPreset::CleanBalanced,
                "professional" => AudioPreset::Professional,
                "gaming" => AudioPreset::Gaming,
                "podcast" => AudioPreset::Podcast,
                "custom" => AudioPreset::Custom,
                _ => AudioPreset::CleanBalanced,
            };
            
            let mic_preset_str: String = row.get(11).unwrap_or("balanced".to_string());
            let mic_preset = match mic_preset_str.as_str() {
                "none" => MicrophonePreset::None,
                "light" => MicrophonePreset::Light,
                "balanced" => MicrophonePreset::Balanced,
                "podcast" => MicrophonePreset::Podcast,
                "professional" => MicrophonePreset::Professional,
                "custom" => MicrophonePreset::Custom,
                _ => MicrophonePreset::Balanced,
            };
            
            Ok(StreamAudioConfig {
                stream_id: row.get(0)?,
                audio_mode: row.get(1)?,
                target_pid: row.get(2)?,
                target_process_name: row.get(3)?,
                enable_microphone: row.get::<_, i32>(4)? == 1,
                microphone_device: row.get(5)?,
                audio_effects: AudioEffects {
                    enabled: row.get::<_, i32>(6)? == 1,
                    preset: audio_preset,
                    custom_filters: row.get(8)?,
                    master_volume: row.get(9)?,
                },
                microphone_effects: MicrophoneEffects {
                    enabled: row.get::<_, i32>(10).unwrap_or(0) == 1,
                    preset: mic_preset,
                    custom_filters: row.get(12).ok(),
                    volume: row.get(13).unwrap_or(1.0),
                },
            })
        });

        match result {
            Ok(config) => Ok(Some(config)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn save_config(&self, config: &StreamAudioConfig) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        
        let audio_preset_str = match config.audio_effects.preset {
            AudioPreset::None => "none",
            AudioPreset::CleanLight => "cleanlight",
            AudioPreset::CleanBalanced => "cleanbalanced",
            AudioPreset::Professional => "professional",
            AudioPreset::Gaming => "gaming",
            AudioPreset::Podcast => "podcast",
            AudioPreset::Custom => "custom",
        };
        
        let mic_preset_str = match config.microphone_effects.preset {
            MicrophonePreset::None => "none",
            MicrophonePreset::Light => "light",
            MicrophonePreset::Balanced => "balanced",
            MicrophonePreset::Podcast => "podcast",
            MicrophonePreset::Professional => "professional",
            MicrophonePreset::Custom => "custom",
        };
        
        conn.execute(
            "INSERT OR REPLACE INTO stream_audio_config 
             (stream_id, audio_mode, target_pid, target_process_name, enable_microphone, microphone_device,
              audio_effects_enabled, audio_effects_preset, audio_effects_custom_filters, audio_effects_master_volume,
              microphone_effects_enabled, microphone_effects_preset, microphone_effects_custom_filters, microphone_effects_volume)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                &config.stream_id,
                &config.audio_mode,
                &config.target_pid,
                &config.target_process_name,
                if config.enable_microphone { 1 } else { 0 },
                &config.microphone_device,
                if config.audio_effects.enabled { 1 } else { 0 },
                audio_preset_str,
                &config.audio_effects.custom_filters,
                config.audio_effects.master_volume,
                if config.microphone_effects.enabled { 1 } else { 0 },
                mic_preset_str,
                &config.microphone_effects.custom_filters,
                config.microphone_effects.volume,
            ],
        )?;
        Ok(())
    }

    pub fn delete_config(&self, stream_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM stream_audio_config WHERE stream_id = ?1", params![stream_id])?;
        Ok(())
    }

    /// Cria uma configuração de áudio padrão usando as configurações globais
    pub fn create_default_config_from_global(&self, stream_id: &str, db: &crate::db::Database) -> StreamAudioConfig {
        // Tenta obter configurações globais
        let global_settings = db.get_settings().ok();
        
        let (preset, volume, custom_filters) = if let Some(settings) = global_settings {
            let preset = match settings.global_audio_preset.as_str() {
                "none" => AudioPreset::None,
                "clean_light" => AudioPreset::CleanLight,
                "cleanlight" => AudioPreset::CleanLight,
                "clean_balanced" => AudioPreset::CleanBalanced,
                "cleanbalanced" => AudioPreset::CleanBalanced,
                "professional" => AudioPreset::Professional,
                "gaming" => AudioPreset::Gaming,
                "podcast" => AudioPreset::Podcast,
                "custom" => AudioPreset::Custom,
                _ => AudioPreset::None,
            };
            (preset, settings.global_audio_volume, settings.global_audio_custom_filters)
        } else {
            (AudioPreset::None, 0.8, None)
        };

        StreamAudioConfig {
            stream_id: stream_id.to_string(),
            audio_mode: "system".to_string(),
            target_pid: None,
            target_process_name: None,
            enable_microphone: false,
            microphone_device: "".to_string(),
            audio_effects: AudioEffects {
                enabled: preset != AudioPreset::None,
                preset,
                custom_filters,
                master_volume: volume,
            },
            microphone_effects: MicrophoneEffects::default(),
        }
    }
}

/// Detecta o número de cores da CPU e sugere o preset ideal
#[tauri::command]
pub fn get_suggested_audio_preset() -> (String, String, String) {
    let cores = num_cpus::get();
    
    let (preset, reason, cpu_impact) = if cores >= 8 {
        ("professional", "CPU potente detectada (8+ cores)", "Alto (~15%)")
    } else if cores >= 4 {
        ("cleanbalanced", "CPU média detectada (4-7 cores)", "Médio (~5%)")
    } else {
        ("cleanlight", "CPU fraca detectada (2-3 cores)", "Baixo (~2%)")
    };
    
    (preset.to_string(), reason.to_string(), cpu_impact.to_string())
}

/// Retorna informações de todos os presets disponíveis
#[derive(Serialize)]
pub struct PresetInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub cpu_impact: String,
}

#[tauri::command]
pub fn get_audio_presets_info() -> Vec<PresetInfo> {
    vec![
        PresetInfo {
            id: "none".to_string(),
            name: "Nenhum".to_string(),
            description: AudioPreset::None.description().to_string(),
            cpu_impact: AudioPreset::None.cpu_impact().to_string(),
        },
        PresetInfo {
            id: "cleanlight".to_string(),
            name: "Limpo (Leve)".to_string(),
            description: AudioPreset::CleanLight.description().to_string(),
            cpu_impact: AudioPreset::CleanLight.cpu_impact().to_string(),
        },
        PresetInfo {
            id: "cleanbalanced".to_string(),
            name: "Limpo (Balanceado)".to_string(),
            description: AudioPreset::CleanBalanced.description().to_string(),
            cpu_impact: AudioPreset::CleanBalanced.cpu_impact().to_string(),
        },
        PresetInfo {
            id: "professional".to_string(),
            name: "Profissional".to_string(),
            description: AudioPreset::Professional.description().to_string(),
            cpu_impact: AudioPreset::Professional.cpu_impact().to_string(),
        },
        PresetInfo {
            id: "gaming".to_string(),
            name: "Gaming".to_string(),
            description: AudioPreset::Gaming.description().to_string(),
            cpu_impact: AudioPreset::Gaming.cpu_impact().to_string(),
        },
        PresetInfo {
            id: "podcast".to_string(),
            name: "Podcast".to_string(),
            description: AudioPreset::Podcast.description().to_string(),
            cpu_impact: AudioPreset::Podcast.cpu_impact().to_string(),
        },
        PresetInfo {
            id: "custom".to_string(),
            name: "Personalizado".to_string(),
            description: AudioPreset::Custom.description().to_string(),
            cpu_impact: AudioPreset::Custom.cpu_impact().to_string(),
        },
    ]
}

/// Retorna informações de todos os presets de microfone disponíveis
#[tauri::command]
pub fn get_microphone_presets_info() -> Vec<PresetInfo> {
    vec![
        PresetInfo {
            id: "none".to_string(),
            name: "Nenhum".to_string(),
            description: MicrophonePreset::None.description().to_string(),
            cpu_impact: "Nenhum".to_string(),
        },
        PresetInfo {
            id: "light".to_string(),
            name: "Leve".to_string(),
            description: MicrophonePreset::Light.description().to_string(),
            cpu_impact: "Baixo (~1%)".to_string(),
        },
        PresetInfo {
            id: "balanced".to_string(),
            name: "Balanceado".to_string(),
            description: MicrophonePreset::Balanced.description().to_string(),
            cpu_impact: "Médio (~3%)".to_string(),
        },
        PresetInfo {
            id: "podcast".to_string(),
            name: "Podcast".to_string(),
            description: MicrophonePreset::Podcast.description().to_string(),
            cpu_impact: "Alto (~8%)".to_string(),
        },
        PresetInfo {
            id: "professional".to_string(),
            name: "Profissional".to_string(),
            description: MicrophonePreset::Professional.description().to_string(),
            cpu_impact: "Muito Alto (~12%)".to_string(),
        },
        PresetInfo {
            id: "custom".to_string(),
            name: "Personalizado".to_string(),
            description: MicrophonePreset::Custom.description().to_string(),
            cpu_impact: "Variável".to_string(),
        },
    ]
}
