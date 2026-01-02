//! FFmpeg Encoder - Orquestrador de Streaming
//!
//! Este módulo orquestra captura → encode → output HLS.
//! Responsabilidades de encoding e output foram extraídas para módulos separados.

use crate::encoder::{
    build_audio_encoding_args, build_audio_input_args, build_audio_mix_filter,
    build_single_audio_filter, build_stream_mapping, build_video_encoding_args, get_encoder_config,
    get_ffmpeg_path, AudioConfig, VideoEncoderConfig,
};
use crate::output::HlsConfig;
use std::io::Write;
use std::process::{Child, Command, Stdio};

/// FFmpeg process wrapper para streaming HLS
pub struct FfmpegEncoder {
    process: Child,
    width: u32,
    height: u32,
}

impl FfmpegEncoder {
    /// Iniciar processo FFmpeg para streaming HLS
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        list_size: u32,
        hls_time: f64,
        output_dir: &str,
        stream_id: &str,
        enable_hw_accel: bool,
        ffmpeg_preset: String,
        enable_audio: bool,
        enable_microphone: bool,
        audio_bitrate: u32,
        audio_buffer_size: u32,
        audio_offset: i32,
        audio_device: String,
        microphone_device: String,
        audio_filters: String,
        microphone_filters: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Configuração de áudio
        let audio_config = AudioConfig {
            enabled: enable_audio,
            microphone_enabled: enable_microphone,
            device: audio_device,
            microphone_device,
            bitrate: audio_bitrate,
            buffer_size: audio_buffer_size,
            offset_ms: audio_offset,
            audio_filters,
            microphone_filters,
        };

        // Configuração de encoder
        let (codec, preset_args) = get_encoder_config(enable_hw_accel, &ffmpeg_preset);
        let video_config = VideoEncoderConfig {
            codec,
            preset_args,
            width,
            height,
            fps,
            bitrate,
        };

        // Configuração de output HLS (low_latency=false para estabilidade de áudio)
        let hls_config = HlsConfig::new(output_dir, stream_id, list_size, hls_time);

        Self::spawn_process(video_config, audio_config, hls_config)
    }

    /// Spawn do processo FFmpeg com configurações modulares
    fn spawn_process(
        video_config: VideoEncoderConfig,
        audio_config: AudioConfig,
        hls_config: HlsConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let ffmpeg_path = get_ffmpeg_path();

        // Prepara output (cria diretórios, etc)
        hls_config.prepare_directory()?;

        // Inicia comando FFmpeg
        let mut command = Command::new(&ffmpeg_path);
        command.args(["-y", "-fflags", "+genpts"]);

        // ===== INPUTS =====

        // Input de áudio (sempre primeiro)
        let audio_args = build_audio_input_args(&audio_config);
        command.args(&audio_args);

        // Input de vídeo (sempre segundo, exceto quando há mix de áudio)
        command.args([
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &format!("{}x{}", video_config.width, video_config.height),
            "-r",
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

        // ===== AUDIO ENCODING =====

        let audio_encoding_args = build_audio_encoding_args(&audio_config);
        command.args(&audio_encoding_args);

        // Filtro de áudio para stream única (não mixada)
        if let Some(af) = build_single_audio_filter(&audio_config) {
            command.args(["-af", &af]);
            println!("🎛️ Aplicando filtros de áudio: {}", af);
        }

        // ===== OUTPUT HLS =====

        // Argumentos de formato HLS
        let format_args = hls_config.to_ffmpeg_args();
        command.args(&format_args);

        // -shortest se áudio desabilitado
        if !audio_config.enabled && !audio_config.microphone_enabled {
            command.arg("-shortest");
        }

        // Argumentos finais de output
        let output_args = hls_config.to_ffmpeg_output_args();
        command.args(&output_args);

        // ===== PROCESS CONFIG =====

        let stderr_log = hls_config
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

        println!(
            "FFmpeg iniciado: {} ({}x{} @ {} fps) [Encoder: {}]",
            hls_config.stream_id(),
            video_config.width,
            video_config.height,
            video_config.fps,
            video_config.codec
        );

        Ok(Self {
            process,
            width: video_config.width,
            height: video_config.height,
        })
    }

    /// Escrever um frame BGRA raw para o FFmpeg
    pub fn write_frame(
        &mut self,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let expected_size = (self.width * self.height * 4) as usize;
        if data.len() != expected_size {
            return Err(format!(
                "Tamanho de frame incorreto: esperado {}, recebido {}",
                expected_size,
                data.len()
            )
            .into());
        }

        if let Some(stdin) = self.process.stdin.as_mut() {
            stdin.write_all(data)?;
        }
        Ok(())
    }

    /// Parar processo FFmpeg imediatamente
    pub fn stop(&mut self) {
        println!("🛑 Parando FFmpeg...");

        // Fechar stdin para sinalizar fim do stream
        drop(self.process.stdin.take());

        // Matar o processo imediatamente ao invés de esperar
        match self.process.kill() {
            Ok(_) => {
                println!("✅ Processo FFmpeg terminado");
                // Aguardar para garantir que recursos sejam liberados
                let _ = self.process.wait();
            }
            Err(e) => {
                eprintln!("⚠️ Erro ao matar processo FFmpeg: {}", e);
                // Tentar esperar mesmo assim
                let _ = self.process.wait();
            }
        }

        println!("✅ FFmpeg completamente parado");
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        self.stop();
    }
}
