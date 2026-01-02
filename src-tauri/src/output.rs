//! Módulo de Output para Streaming HLS
//!
//! Responsável pela configuração e argumentos de saída HLS para FFmpeg.
//! Otimizado para baixa latência quando necessário.

/// Configuração de saída HLS
#[derive(Debug, Clone)]
pub struct HlsConfig {
    pub output_dir: String,
    pub stream_id: String,
    pub list_size: u32,
    pub hls_time: f64,
    /// Modo de baixa latência (reduz latência de ~10s para ~2s)
    pub low_latency: bool,
}

impl HlsConfig {
    /// Cria configuração HLS padrão
    pub fn new(output_dir: &str, stream_id: &str, list_size: u32, hls_time: f64) -> Self {
        Self {
            output_dir: output_dir.to_string(),
            stream_id: stream_id.to_string(),
            list_size,
            hls_time,
            low_latency: false,
        }
    }

    /// Cria configuração HLS otimizada para baixa latência (~2s ao invés de ~10s)
    /// Nota: Desativado temporariamente devido a problemas de sincronização de áudio
    #[allow(dead_code)]
    pub fn new_low_latency(output_dir: &str, stream_id: &str) -> Self {
        Self {
            output_dir: output_dir.to_string(),
            stream_id: stream_id.to_string(),
            list_size: 3,  // Menor playlist
            hls_time: 0.5, // Segmentos de 0.5s
            low_latency: true,
        }
    }

    /// Retorna o diretório específico do stream
    pub fn stream_dir(&self) -> String {
        format!("{}/{}", self.output_dir, self.stream_id)
    }

    /// Retorna o caminho completo do arquivo .m3u8
    pub fn playlist_path(&self) -> String {
        format!("{}/{}.m3u8", self.stream_dir(), self.stream_id)
    }

    /// Retorna o padrão de nome dos segmentos
    pub fn segment_pattern(&self) -> String {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        format!(
            "{}/{}_{}_seg_%03d.m4s",
            self.stream_dir(),
            self.stream_id,
            timestamp
        )
    }

    /// Prepara o diretório de stream (limpa antigos e cria novo)
    pub fn prepare_directory(&self) -> Result<(), std::io::Error> {
        let stream_dir = self.stream_dir();

        // Limpa diretório antigo se existir
        if std::path::Path::new(&stream_dir).exists() {
            println!("🧹 Limpando diretório de stream antigo: {}", stream_dir);
            std::fs::remove_dir_all(&stream_dir)?;
        }

        std::fs::create_dir_all(&stream_dir)?;
        Ok(())
    }

    /// Gera argumentos HLS para FFmpeg
    pub fn to_ffmpeg_args(&self) -> Vec<String> {
        // Flags base para HLS - NÃO usar split_by_time (causa problemas de áudio)
        let mut hls_flags =
            "delete_segments+discont_start+independent_segments+program_date_time".to_string();

        let mut args = vec![
            "-f".to_string(),
            "hls".to_string(),
            "-hls_time".to_string(),
            self.hls_time.to_string(),
            "-hls_list_size".to_string(),
            self.list_size.to_string(),
            "-hls_segment_type".to_string(),
            "fmp4".to_string(),
            "-hls_fmp4_init_filename".to_string(),
            "init.mp4".to_string(),
        ];

        // Low-latency: apenas log, sem flags problemáticas
        // As otimizações seguras são: hls_time menor e list_size menor
        if self.low_latency {
            println!(
                "⚡ HLS Low-Latency Mode: segments={}s, list_size={}",
                self.hls_time, self.list_size
            );
        }

        if self.list_size == 0 {
            hls_flags = "discont_start+independent_segments+program_date_time".to_string();
            args.extend(["-hls_playlist_type".to_string(), "event".to_string()]);
        }

        args.extend(["-hls_flags".to_string(), hls_flags]);

        println!(
            "📹 HLS Config: list_size={}, hls_time={}, low_latency={}, output={}",
            self.list_size,
            self.hls_time,
            self.low_latency,
            self.playlist_path()
        );

        args
    }

    /// Gera argumentos finais de output para FFmpeg
    pub fn to_ffmpeg_output_args(&self) -> Vec<String> {
        vec![
            "-hls_segment_filename".to_string(),
            self.segment_pattern(),
            "-threads".to_string(),
            "0".to_string(),
            self.playlist_path(),
        ]
    }

    /// Cria arquivo de log do FFmpeg no diretório do stream
    pub fn create_log_file(&self) -> Result<std::fs::File, std::io::Error> {
        let log_path = format!("{}/ffmpeg.log", self.stream_dir());
        std::fs::File::create(log_path)
    }

    /// Retorna o stream_id
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hls_config_paths() {
        let config = HlsConfig::new("/tmp/streams", "test_stream", 5, 2.0);

        assert_eq!(config.stream_dir(), "/tmp/streams/test_stream");
        assert_eq!(
            config.playlist_path(),
            "/tmp/streams/test_stream/test_stream.m3u8"
        );
        assert!(!config.low_latency);
    }

    #[test]
    fn test_hls_low_latency() {
        let config = HlsConfig::new_low_latency("/tmp/streams", "test_stream");

        assert_eq!(config.list_size, 3);
        assert_eq!(config.hls_time, 0.5);
        assert!(config.low_latency);
    }
}
