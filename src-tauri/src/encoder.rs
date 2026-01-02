//! Módulo de Encoding de Vídeo/Áudio
//!
//! Responsável APENAS por encoding - não conhece HLS, SRT ou qualquer formato de saída.
//! Segue o princípio de responsabilidade única (SRP).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// Configuração do encoder de vídeo
#[derive(Debug, Clone)]
pub struct VideoEncoderConfig {
    pub codec: String,
    pub preset_args: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
}

/// Configuração de áudio simplificada
#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub enabled: bool,
    pub microphone_enabled: bool,
    pub device: String,
    pub microphone_device: String,
    pub bitrate: u32,
    pub buffer_size: u32,
    pub offset_ms: i32,
    pub audio_filters: String,
    pub microphone_filters: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            microphone_enabled: false,
            device: String::new(),
            microphone_device: String::new(),
            bitrate: 128,
            buffer_size: 40,
            offset_ms: 0,
            audio_filters: String::new(),
            microphone_filters: String::new(),
        }
    }
}

/// Cache estático para o encoder detectado (detecta UMA vez)
static DETECTED_ENCODER: OnceLock<(String, Vec<String>)> = OnceLock::new();

/// Obtém o caminho do FFmpeg
pub fn get_ffmpeg_path() -> PathBuf {
    // Primeiro verifica na pasta de binários local
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();

    // Tenta caminho empacotado (release)
    let bundled_path = exe_dir.join("binaries").join("ffmpeg.exe");
    if bundled_path.exists() {
        return bundled_path;
    }

    // Tenta src-tauri/binaries para desenvolvimento
    let dev_path = PathBuf::from("src-tauri/binaries/ffmpeg.exe");
    if dev_path.exists() {
        return dev_path;
    }

    // Tenta pasta binaries relativa ao diretório atual
    let local_path = PathBuf::from("binaries/ffmpeg.exe");
    if local_path.exists() {
        return local_path;
    }

    // Fallback para PATH do sistema
    PathBuf::from("ffmpeg")
}

/// Obtém o encoder detectado (cached)
pub fn get_cached_encoder() -> &'static (String, Vec<String>) {
    DETECTED_ENCODER.get_or_init(|| {
        let ffmpeg_path = get_ffmpeg_path();
        detect_encoder(&ffmpeg_path)
    })
}

/// Detecta o melhor encoder disponível no sistema
pub fn detect_encoder(ffmpeg_path: &PathBuf) -> (String, Vec<String>) {
    let log_msg = format!(
        "🕵️ Detectando Encoders de Hardware...\n   Caminho FFmpeg: {:?}\n",
        ffmpeg_path
    );
    tracing::info!(
        "🕵️ Detectando Encoders de Hardware...\n   Caminho FFmpeg: {:?}",
        ffmpeg_path
    );
    write_encoder_log(&log_msg);

    // Lista de encoders candidatos em ordem de prioridade (AMD primeiro para APUs Ryzen)
    let candidates = [
        (
            "h264_amf",
            vec!["-usage", "lowlatency", "-quality", "speed"],
        ), // AMD Radeon
        ("h264_nvenc", vec!["-preset", "p1", "-tune", "ll"]), // NVIDIA
        ("h264_qsv", vec!["-preset", "veryfast"]),            // Intel
    ];

    for (codec, preset) in candidates {
        if test_encoder(ffmpeg_path, codec) {
            let success_msg = format!(
                "🚀 Encoder GPU Confirmado: Usando {} com preset {:?}\n",
                codec, preset
            );
            tracing::info!(
                "🚀 Encoder GPU Confirmado: Usando {} com preset {:?}",
                codec,
                preset
            );
            write_encoder_log(&success_msg);

            let args: Vec<String> = preset.iter().map(|p| p.to_string()).collect();
            return (codec.to_string(), args);
        }
    }

    let fallback_msg = "⚠️ Nenhum Encoder GPU encontrado. Usando encoding por CPU\n   Isso causará alta CPU e possíveis travamentos\n";
    tracing::warn!("{}", fallback_msg);
    write_encoder_log(fallback_msg);

    (
        "libx264".to_string(),
        vec![
            "-preset".to_string(),
            "veryfast".to_string(),
            "-tune".to_string(),
            "zerolatency".to_string(),
            "-threads".to_string(),
            "0".to_string(),
        ],
    )
}

/// Obtém configuração de encoder com override de preset para CPU
pub fn get_encoder_config(enable_hw_accel: bool, cpu_preset: &str) -> (String, Vec<String>) {
    if enable_hw_accel {
        let (codec, args) = get_cached_encoder();
        (codec.clone(), args.clone())
    } else {
        tracing::warn!("🛑 Aceleração de Hardware DESABILITADA pelo usuário. Usando CPU.");
        (
            "libx264".to_string(),
            vec![
                "-preset".to_string(),
                cpu_preset.to_string(),
                "-tune".to_string(),
                "zerolatency".to_string(),
            ],
        )
    }
}

/// Testa se um encoder está disponível e funcional no sistema
fn test_encoder(ffmpeg_path: &PathBuf, codec: &str) -> bool {
    let test_msg = format!("   📍 Testando {}...", codec);
    tracing::info!("{}", test_msg);
    write_encoder_log(&test_msg);

    // Executa uma codificação mínima para verificar se o hardware funciona
    // AMF requer resolução mínima (256x256 ao invés de 64x64)
    let mut cmd = Command::new(ffmpeg_path);
    cmd.args([
        "-hide_banner",
        "-v",
        "warning",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=256x256:d=0.1:r=30",
        "-vframes",
        "1",
        "-an",
        "-c:v",
        codec,
        "-b:v",
        "1M", // Adiciona bitrate para AMF
        "-f",
        "null",
        "-",
    ]);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let start = std::time::Instant::now();
    let output = cmd.output();
    let elapsed = start.elapsed();

    match output {
        Ok(out) => {
            if out.status.success() {
                let success = format!("   ✅ {} FUNCIONA! (testado em {:?})", codec, elapsed);
                tracing::info!("{}", success);
                write_encoder_log(&success);
                true
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                let fail_msg = format!(
                    "   ❌ {} FALHOU (código de saída: {:?})",
                    codec,
                    out.status.code()
                );
                tracing::warn!("{}", fail_msg);
                write_encoder_log(&fail_msg);

                if !err.is_empty() && !err.contains("deprecated") {
                    let reason =
                        format!("      Razão: {}", err.trim().lines().next().unwrap_or(""));
                    tracing::warn!("{}", reason);
                    write_encoder_log(&reason);
                }
                false
            }
        }
        Err(e) => {
            let error = format!("   ❌ {} falhou ao executar: {}", codec, e);
            println!("{}", error);
            write_encoder_log(&error);
            false
        }
    }
}

/// Escreve log do encoder no arquivo
fn write_encoder_log(msg: &str) {
    let exe_path = std::env::current_exe().ok();
    let log_dir = exe_path
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let log_path = log_dir.join("hipocast_encoder.log");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] {}", timestamp, msg.trim());
    }
}

/// Índices fixos para inputs do FFmpeg (usado em fases futuras)
#[allow(dead_code)]
pub mod input_index {
    /// Áudio sempre é o primeiro input (0)
    pub const AUDIO: u32 = 0;
    /// Vídeo sempre é o segundo input (1)  
    pub const VIDEO: u32 = 1;
}

/// Gera argumentos de input de áudio para FFmpeg
pub fn build_audio_input_args(config: &AudioConfig) -> Vec<String> {
    let mut args = Vec::new();

    if config.enabled && config.microphone_enabled {
        // CASO 1: Sistema + Microfone -> Mix
        // Offset aplicado se configurado
        // CASO 1: Sistema + Microfone -> Mix
        // Input 0: Áudio do Sistema via DirectShow
        args.extend([
            "-f".to_string(),
            "dshow".to_string(),
            "-thread_queue_size".to_string(),
            "4096".to_string(),
            "-rtbufsize".to_string(),
            "30M".to_string(),
            "-audio_buffer_size".to_string(),
            config.buffer_size.to_string(),
            "-i".to_string(),
            format!("audio={}", config.device),
        ]);

        // Input 1: Microfone via DirectShow
        args.extend([
            "-f".to_string(),
            "dshow".to_string(),
            "-thread_queue_size".to_string(),
            "4096".to_string(),
            "-rtbufsize".to_string(),
            "30M".to_string(),
            "-audio_buffer_size".to_string(),
            config.buffer_size.to_string(),
            "-i".to_string(),
            format!("audio={}", config.microphone_device),
        ]);

        tracing::info!("🎤🔊 Mixando Áudio: Sistema + Microfone");
    } else if config.enabled {
        // CASO 2: Apenas Áudio do Sistema
        // CASO 2: Apenas Áudio do Sistema

        args.extend([
            "-f".to_string(),
            "dshow".to_string(),
            "-thread_queue_size".to_string(),
            "4096".to_string(),
            "-rtbufsize".to_string(),
            "30M".to_string(),
            "-audio_buffer_size".to_string(),
            config.buffer_size.to_string(),
            "-i".to_string(),
            format!("audio={}", config.device),
        ]);

        tracing::info!(
            "🔊 Apenas Áudio do Sistema: device='{}' (offset: {}ms)",
            config.device,
            config.offset_ms
        );
    } else if config.microphone_enabled {
        // CASO 3: Apenas Microfone
        // CASO 3: Apenas Microfone

        args.extend([
            "-f".to_string(),
            "dshow".to_string(),
            "-use_wallclock_as_timestamps".to_string(),
            "1".to_string(),
            "-thread_queue_size".to_string(),
            "4096".to_string(),
            "-rtbufsize".to_string(),
            "150M".to_string(),
            "-audio_buffer_size".to_string(),
            config.buffer_size.to_string(),
            "-i".to_string(),
            format!("audio={}", config.microphone_device),
        ]);

        tracing::info!(
            "🎤 Apenas Microfone: device='{}' (offset: {}ms)",
            config.microphone_device,
            config.offset_ms
        );
    } else {
        // CASO 4: Sem Áudio (Fallback Silencioso)
        args.extend([
            "-f".to_string(),
            "lavfi".to_string(),
            "-i".to_string(),
            "anullsrc=r=48000:cl=stereo".to_string(),
        ]);

        tracing::info!("🔇 Áudio desabilitado: Usando trilha silenciosa");
    }

    args
}

/// Gera o filtro de mixagem de áudio para FFmpeg
pub fn build_audio_mix_filter(config: &AudioConfig) -> Option<String> {
    if config.enabled && config.microphone_enabled {
        let offset_sec = config.offset_ms as f64 / 1000.0;

        let sys_filter_str = if !config.audio_filters.is_empty() {
            format!(
                "aresample=48000:async=1,asetpts=PTS-STARTPTS+{:.3}/TB,{}",
                offset_sec, config.audio_filters
            )
        } else {
            format!(
                "aresample=48000:async=1,asetpts=PTS-STARTPTS+{:.3}/TB",
                offset_sec
            )
        };

        let mic_filter_str = if !config.microphone_filters.is_empty() {
            format!(
                "aresample=48000:async=1,asetpts=PTS-STARTPTS+{:.3}/TB,{}",
                offset_sec, config.microphone_filters
            )
        } else {
            format!(
                "aresample=48000:async=1,asetpts=PTS-STARTPTS+{:.3}/TB",
                offset_sec
            )
        };

        let mix_filter = format!(
            "[0:a]{}[a0];[1:a]{}[a1];[a0][a1]amix=inputs=2:duration=longest[outa]",
            sys_filter_str, mic_filter_str
        );

        Some(mix_filter)
    } else {
        None
    }
}

/// Gera argumentos de mapeamento de streams para FFmpeg
pub fn build_stream_mapping(config: &AudioConfig) -> Vec<String> {
    if config.enabled && config.microphone_enabled {
        // Áudio mixado do filter_complex
        vec![
            "-map".to_string(),
            "[outa]".to_string(),
            "-map".to_string(),
            "2:v".to_string(),
        ]
    } else {
        // Áudio do input 0, vídeo do input 1
        vec![
            "-map".to_string(),
            "0:a".to_string(),
            "-map".to_string(),
            "1:v".to_string(),
        ]
    }
}

/// Gera argumentos de encoding de vídeo
pub fn build_video_encoding_args(config: &VideoEncoderConfig) -> Vec<String> {
    let mut args = vec!["-c:v".to_string(), config.codec.clone()];

    // Adiciona preset do codec
    args.extend(config.preset_args.clone());

    // Configurações de vídeo
    args.extend([
        "-b:v".to_string(),
        format!("{}k", config.bitrate / 1000),
        "-maxrate".to_string(),
        format!("{}k", config.bitrate / 1000),
        "-bufsize".to_string(),
        format!("{}k", config.bitrate / 500),
        "-g".to_string(),
        config.fps.to_string(), // GOP size = 1s
        "-color_range".to_string(),
        "tv".to_string(),
        "-colorspace".to_string(),
        "bt709".to_string(),
        "-color_primaries".to_string(),
        "bt709".to_string(),
        "-color_trc".to_string(),
        "bt709".to_string(),
        "-vf".to_string(),
        "scale=w=trunc(iw/2)*2:h=trunc(ih/2)*2,format=yuv420p".to_string(),
    ]);

    args
}

/// Gera argumentos de encoding de áudio
pub fn build_audio_encoding_args(config: &AudioConfig) -> Vec<String> {
    let audio_bitrate_str = format!("{}k", config.bitrate);

    vec![
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        audio_bitrate_str,
        "-ar".to_string(),
        "48000".to_string(),
        "-ac".to_string(),
        "2".to_string(), // Stereo
        "-aac_coder".to_string(),
        "fast".to_string(), // 'fast' é mais estável que 'twoloop'
    ]
}

/// Gera filtro de áudio para stream única (não mixada)
pub fn build_single_audio_filter(config: &AudioConfig) -> Option<String> {
    let is_single_audio = (config.enabled || config.microphone_enabled)
        && !(config.enabled && config.microphone_enabled);

    if is_single_audio {
        let offset_sec = config.offset_ms as f64 / 1000.0;

        let filters = if config.enabled && !config.audio_filters.is_empty() {
            format!(
                "aresample=48000:async=1,asetpts=PTS-STARTPTS+{:.3}/TB,{}",
                offset_sec, config.audio_filters
            )
        } else if config.microphone_enabled && !config.microphone_filters.is_empty() {
            format!(
                "aresample=48000:async=1,asetpts=PTS-STARTPTS+{:.3}/TB,{}",
                offset_sec, config.microphone_filters
            )
        } else {
            format!(
                "aresample=48000:async=1,asetpts=PTS-STARTPTS+{:.3}/TB",
                offset_sec
            )
        };

        Some(filters)
    } else {
        None
    }
}
