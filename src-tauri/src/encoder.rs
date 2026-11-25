//! Módulo de Encoding de Vídeo/Áudio
//!
//! Responsável APENAS por encoding - não conhece HLS, SRT ou qualquer formato de saída.
//! Segue o princípio de responsabilidade única (SRP).

use crate::state::AudioDeviceInfo;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x08000000;

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
static DETECTED_ENCODER: OnceLock<Option<&str>> = OnceLock::new();

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

    #[cfg(debug_assertions)]
    {
        let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/ffmpeg.exe");
        if dev_path.is_file() {
            return dev_path;
        }
    }

    // Fallback para PATH do sistema
    PathBuf::from("ffmpeg")
}

pub fn enumerate_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let mut process = Command::new(get_ffmpeg_path())
        .args([
            "-hide_banner",
            "-list_devices",
            "true",
            "-f",
            "dshow",
            "-i",
            "dummy",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("Falha ao executar FFmpeg: {error}"))?;
    let mut stderr = process.stderr.take().expect("FFmpeg stderr is piped");
    let reader = std::thread::spawn(move || {
        let mut result = Vec::new();
        stderr.read_to_end(&mut result)?;
        Ok::<_, std::io::Error>(result)
    });
    let status = wait_for_probe(&mut process);
    let stderr = reader
        .join()
        .map_err(|_| "Falha ao ler a lista de áudio.")?
        .map_err(|error| error.to_string())?;
    status.map_err(|error| error.to_string())?;
    let stderr = String::from_utf8_lossy(&stderr);
    let devices = stderr
        .lines()
        .filter_map(|line| {
            if !line.starts_with("[dshow") || !line.ends_with("(audio)") {
                return None;
            }
            let name = line.split('"').nth(1)?.to_string();
            let normalized = name.to_lowercase();
            let loopback = [
                "stereo mix",
                "mixagem",
                "wave out",
                "what you hear",
                "virtual-audio-capturer",
            ]
            .iter()
            .any(|pattern| normalized.contains(pattern));
            Some(AudioDeviceInfo {
                name,
                device_type: if loopback { "loopback" } else { "input" }.to_string(),
            })
        })
        .collect();
    Ok(devices)
}

fn wait_for_probe(process: &mut std::process::Child) -> std::io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = process.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "O FFmpeg demorou para responder.",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub fn get_encoder_config(
    enable_hw_accel: bool,
    cpu_preset: &str,
    gpu_preset: &str,
) -> (String, Vec<String>) {
    if enable_hw_accel {
        let encoder = DETECTED_ENCODER.get_or_init(|| {
            let path = get_ffmpeg_path();
            ["h264_amf", "h264_nvenc", "h264_qsv"]
                .into_iter()
                .find(|codec| test_encoder(&path, codec))
        });
        if let Some(codec) = encoder {
            return (codec.to_string(), get_gpu_preset_args(codec, gpu_preset));
        }
    }

    (
        "libx264".to_string(),
        vec![
            "-preset".into(),
            cpu_preset.into(),
            "-tune".into(),
            "zerolatency".into(),
        ],
    )
}

/// Mapeia gpu_preset (performance/balanced/quality) para flags específicas de cada encoder
fn get_gpu_preset_args(codec: &str, gpu_preset: &str) -> Vec<String> {
    match codec {
        "h264_nvenc" => {
            // NVIDIA NVENC: P1-P7, ll (low latency), hq (high quality)
            match gpu_preset {
                "quality" => vec!["-preset", "p6", "-tune", "hq"],
                "balanced" => vec!["-preset", "p4", "-tune", "ll"],
                _ => vec!["-preset", "p1", "-tune", "ll"], // performance (default)
            }
        }
        "h264_amf" => {
            // AMD AMF: speed, balanced, quality - sempre lowlatency para streaming
            match gpu_preset {
                "quality" => vec!["-usage", "lowlatency", "-quality", "quality"],
                "balanced" => vec!["-usage", "lowlatency", "-quality", "balanced"],
                _ => vec!["-usage", "lowlatency", "-quality", "speed"], // performance
            }
        }
        "h264_qsv" => {
            // Intel QSV: veryfast, faster, fast, medium
            match gpu_preset {
                "quality" => vec!["-preset", "medium"],
                "balanced" => vec!["-preset", "fast"],
                _ => vec!["-preset", "veryfast"], // performance
            }
        }
        _ => vec![], // Fallback vazio
    }
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Testa se um encoder está disponível e funcional no sistema
fn test_encoder(ffmpeg_path: &PathBuf, codec: &str) -> bool {
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

    let result = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .and_then(|mut process| wait_for_probe(&mut process));
    match result {
        Ok(status) => {
            tracing::debug!(codec, %status, "Teste do encoder de hardware");
            status.success()
        }
        Err(error) => {
            tracing::debug!(codec, %error, "Falha no teste do encoder de hardware");
            false
        }
    }
}

pub fn build_audio_input_args(config: &AudioConfig) -> Vec<String> {
    let mut args = Vec::new();
    for (enabled, device) in [
        (config.enabled, &config.device),
        (config.microphone_enabled, &config.microphone_device),
    ] {
        if !enabled {
            continue;
        }
        if config.offset_ms != 0 {
            args.extend([
                "-itsoffset".into(),
                format!("{:.3}", f64::from(config.offset_ms) / 1000.0),
            ]);
        }
        args.extend([
            "-probesize".into(),
            "32".into(),
            "-analyzeduration".into(),
            "1".into(),
            "-f".into(),
            "dshow".into(),
            "-thread_queue_size".into(),
            "8192".into(),
            "-rtbufsize".into(),
            "150M".into(),
            "-audio_buffer_size".into(),
            config.buffer_size.max(10).to_string(),
            "-i".into(),
            format!("audio={device}"),
        ]);
    }
    if args.is_empty() {
        args.extend(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"].map(str::to_string));
    }
    args
}

// Ignore large or backward driver clock jumps before aresample can synthesize silence.
const LIVE_AUDIO_SYNC: &str = "asetpts='if(isnan(PREV_INPTS),PTS,PREV_OUTPTS+if(between(PTS-PREV_INPTS,0,2/TB),PTS-PREV_INPTS,NB_SAMPLES/(SR*TB)))',aresample=async=1000";

/// Gera o filtro de mixagem de áudio para FFmpeg
pub fn build_audio_mix_filter(config: &AudioConfig) -> Option<String> {
    if config.enabled && config.microphone_enabled {
        let sys_filter_str = if !config.audio_filters.is_empty() {
            format!("{LIVE_AUDIO_SYNC},{}", config.audio_filters)
        } else {
            LIVE_AUDIO_SYNC.to_string()
        };

        let mic_filter_str = if !config.microphone_filters.is_empty() {
            format!("{LIVE_AUDIO_SYNC},{}", config.microphone_filters)
        } else {
            LIVE_AUDIO_SYNC.to_string()
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
    if config.codec == "libx264" {
        let pixels_per_second =
            u64::from(config.width) * u64::from(config.height) * u64::from(config.fps);
        let budget = if pixels_per_second <= 1920 * 1080 * 30 {
            4
        } else if pixels_per_second <= 1920 * 1080 * 60 {
            6
        } else {
            8
        };
        args.extend(["-threads:v".into(), budget.min(num_cpus::get()).to_string()]);
    }

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
        "44100".to_string(), // Match input sample rate para evitar resample
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
        let filters = if config.enabled && !config.audio_filters.is_empty() {
            format!("{LIVE_AUDIO_SYNC},{}", config.audio_filters)
        } else if config.microphone_enabled && !config.microphone_filters.is_empty() {
            format!("{LIVE_AUDIO_SYNC},{}", config.microphone_filters)
        } else {
            LIVE_AUDIO_SYNC.to_string()
        };

        Some(filters)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_keeps_short_gaps_and_ignores_large_clock_jumps() -> Result<(), std::io::Error> {
        let filter = build_single_audio_filter(&AudioConfig {
            enabled: true,
            ..AudioConfig::default()
        })
        .unwrap();
        for (gap, expected_bytes) in [(0.5, 240_000), (30.0, 192_000)] {
            let source = format!("sine=frequency=440:sample_rate=48000:duration=2,asetpts='PTS+if(gte(T,1),{gap}/TB,0)'");
            let output = Command::new(get_ffmpeg_path())
                .args([
                    "-hide_banner",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    &source,
                    "-af",
                    &filter,
                    "-c:a",
                    "pcm_s16le",
                    "-f",
                    "s16le",
                    "-",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout.len(), expected_bytes);
        }
        Ok(())
    }
}
