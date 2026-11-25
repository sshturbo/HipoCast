//! FFmpeg Encoder - Orquestrador de Streaming
//!
//! Este módulo orquestra captura → encode → output HLS.
//! Responsabilidades de encoding e output foram extraídas para módulos separados.

use crate::encoder::{
    build_audio_encoding_args, build_audio_input_args, build_audio_mix_filter,
    build_single_audio_filter, build_stream_mapping, build_video_encoding_args, get_encoder_config,
    get_ffmpeg_path, AudioConfig, VideoEncoderConfig,
};
use crate::output::{HlsConfig, HlsGeneration};
use crate::AppSettings;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};

/// FFmpeg process wrapper para streaming HLS
pub struct FfmpegEncoder {
    process: Child,
    hls: HlsGeneration,
}

impl FfmpegEncoder {
    pub fn new(
        settings: &AppSettings,
        audio_config: AudioConfig,
        hls_config: &HlsConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (codec, preset_args) = get_encoder_config(
            settings.enable_hw_accel,
            &settings.ffmpeg_preset,
            &settings.gpu_preset,
        );
        let video_config = VideoEncoderConfig {
            codec,
            preset_args,
            width: settings.width,
            height: settings.height,
            fps: settings.framerate,
            bitrate: settings.bitrate,
        };

        Self::spawn_process(video_config, audio_config, hls_config)
    }

    /// Spawn do processo FFmpeg com configurações modulares
    fn spawn_process(
        video_config: VideoEncoderConfig,
        audio_config: AudioConfig,
        hls_config: &HlsConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let ffmpeg_path = get_ffmpeg_path();

        let hls = hls_config.prepare_generation()?;

        // Inicia comando FFmpeg
        let mut command = Command::new(&ffmpeg_path);
        let filter_threads = if video_config.width * video_config.height > 1920 * 1080 {
            "4"
        } else {
            "2"
        };
        // Configuração mínima - sem flags de sincronização
        command.args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-nostats",
            "-nostdin",
            "-filter_threads",
            filter_threads,
            "-filter_complex_threads",
            "2",
            "-sws_flags",
            "fast_bilinear",
            "-y",
        ]);

        // ===== INPUTS =====

        // Input de áudio (sempre primeiro)
        let audio_args = build_audio_input_args(&audio_config);
        command.args(&audio_args);

        // Raw video does not carry timing; use its arrival time and retain DirectShow's audio timestamps.
        command.args([
            "-probesize",
            "32",
            "-analyzeduration",
            "1",
            "-use_wallclock_as_timestamps",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &format!("{}x{}", video_config.width, video_config.height),
            "-framerate",
            &video_config.fps.to_string(),
            "-i",
            "pipe:0",
        ]);

        // ===== FILTER COMPLEX (se necessário) =====

        if let Some(mix_filter) = build_audio_mix_filter(&audio_config) {
            command.args(["-filter_complex", &mix_filter]);
        }

        // ===== STREAM MAPPING =====

        let mapping = build_stream_mapping(&audio_config);
        command.args(&mapping);

        // ===== VIDEO ENCODING =====

        let video_args = build_video_encoding_args(&video_config);
        command.args(&video_args);
        let keyframe_interval = hls_config
            .segment_duration()
            .max(1.0 / f64::from(video_config.fps));
        command.args([
            "-fps_mode:v",
            "vfr",
            "-force_key_frames:v",
            &format!("expr:gte(t,n_forced*{keyframe_interval})"),
        ]);

        // ===== AUDIO ENCODING =====

        let audio_encoding_args = build_audio_encoding_args(&audio_config);
        command.args(&audio_encoding_args);

        // Filtro de áudio para stream única (não mixada)
        if let Some(af) = build_single_audio_filter(&audio_config) {
            command.args(["-af", &af]);
            tracing::info!("🎛️ Aplicando filtros de áudio: {}", af);
        }

        // ===== OUTPUT HLS =====

        // Argumentos de formato HLS
        let format_args = hls.to_ffmpeg_args();
        command.args(&format_args);

        // -shortest se áudio desabilitado
        if !audio_config.enabled && !audio_config.microphone_enabled {
            command.arg("-shortest");
        }

        // Argumentos finais de output
        let output_args = hls.to_ffmpeg_output_args();
        command.args(&output_args);

        // ===== PROCESS CONFIG =====

        let stderr_log = hls
            .create_log_file()
            .map_err(|e| format!("Falha ao criar arquivo de log: {}", e))?;

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_log));

        // Esconder janela do console no Windows
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let process = command.spawn()?;

        tracing::info!(
            "FFmpeg iniciado: {} ({}x{} @ {} fps) [Encoder: {}]",
            hls_config.stream_id(),
            video_config.width,
            video_config.height,
            video_config.fps,
            video_config.codec
        );

        Ok(Self { process, hls })
    }

    pub fn publish_playlist(&mut self) -> std::io::Result<bool> {
        self.hls.publish_playlist()
    }

    pub fn log_path(&self) -> &Path {
        self.hls.log_path()
    }

    pub fn take_input(&mut self) -> Option<ChildStdin> {
        self.process.stdin.take()
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.process.try_wait()
    }

    pub fn stop(&mut self) -> std::io::Result<()> {
        if self.process.try_wait()?.is_none() {
            if let Err(error) = self.process.kill() {
                if self.process.try_wait()?.is_none() {
                    return Err(error);
                }
            } else {
                self.process.wait()?;
            }
        }
        Ok(())
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            tracing::warn!(%error, "Falha ao encerrar o FFmpeg");
        }
    }
}
