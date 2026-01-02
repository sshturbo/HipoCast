use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::io::Write;
use std::fs::OpenOptions;

/// FFmpeg process wrapper for HLS streaming
pub struct FfmpegEncoder {
    process: Child,
    width: u32,
    height: u32,
}

impl FfmpegEncoder {
    /// Get the path to FFmpeg executable
    fn get_ffmpeg_path() -> PathBuf {
        // First check local binaries folder (works in both dev and bundled)
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        
        // Try bundled path (for release)
        let bundled_path = exe_dir.join("binaries").join("ffmpeg.exe");
        if bundled_path.exists() {
            return bundled_path;
        }
        
        // Try src-tauri/binaries for development
        let dev_path = PathBuf::from("src-tauri/binaries/ffmpeg.exe");
        if dev_path.exists() {
            return dev_path;
        }

        // Try binaries folder relative to current dir
        let local_path = PathBuf::from("binaries/ffmpeg.exe");
        if local_path.exists() {
            return local_path;
        }
        
        // Fallback to system PATH
        PathBuf::from("ffmpeg")
    }

    /// Start FFmpeg process for HLS streaming
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32, list_size: u32, hls_time: f64, output_dir: &str, stream_id: &str, enable_hw_accel: bool, ffmpeg_preset: String, enable_audio: bool, enable_microphone: bool, audio_bitrate: u32, audio_buffer_size: u32, audio_offset: i32, audio_device: String, microphone_device: String, audio_filters: String, microphone_filters: String) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let ffmpeg_path = Self::get_ffmpeg_path();
        
        let (codec, preset_args) = if enable_hw_accel {
            if !ffmpeg_path.exists() {
                // Try system FFmpeg
                let system_ffmpeg = PathBuf::from("ffmpeg");
                let mut cmd = Command::new(&system_ffmpeg);
                cmd.arg("-version");
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
                if cmd.output().is_ok() {
                    Self::detect_encoder(&system_ffmpeg)
                } else {
                     return Err(format!("FFmpeg not found at {:?}", ffmpeg_path).into());
                }
            } else {
                 Self::detect_encoder(&ffmpeg_path)
            }
        } else {
            println!("🛑 Hardware Acceleration DISABLED by user. Using CPU.");
             ("libx264".to_string(), vec!["-preset".to_string(), ffmpeg_preset, "-tune".to_string(), "zerolatency".to_string()])
        };

        Self::spawn_process(
            if ffmpeg_path.exists() { ffmpeg_path } else { PathBuf::from("ffmpeg") }, 
                width, height, fps, bitrate, list_size, hls_time, output_dir, stream_id, codec, preset_args, enable_audio, enable_microphone, audio_bitrate, audio_buffer_size, audio_offset, audio_device, microphone_device, audio_filters, microphone_filters
        )
    }

    fn detect_encoder(ffmpeg_path: &PathBuf) -> (String, Vec<String>) {
        let log_msg = format!("🕵️ Detecting Hardware Encoders...\n   FFmpeg path: {:?}\n", ffmpeg_path);
        println!("{}", log_msg);
        Self::write_log(&log_msg);

        // List of candidate encoders in priority order (AMD first for Ryzen APUs)
        let candidates = [
            ("h264_amf", vec!["-usage", "lowlatency", "-quality", "speed"]),   // AMD Radeon (Simplificado para integrado)
            ("h264_nvenc", vec!["-preset", "p1", "-tune", "ll"]), // NVIDIA
            ("h264_qsv", vec!["-preset", "veryfast"]),    // Intel
        ];

        for (codec, preset) in candidates {
            if Self::test_encoder(ffmpeg_path, codec) {
                let success_msg = format!("🚀 GPU Encoder Confirmed: Using {} with preset {:?}\n", codec, preset);
                println!("{}", success_msg);
                Self::write_log(&success_msg);
                
                let mut args = vec![];
                for p in preset {
                    args.push(p.to_string());
                }
                return (codec.to_string(), args);
            }
        }

        let fallback_msg = "⚠️ No GPU Encoder found. Falling back to CPU encoding\n   Isso causará alta CPU e possíveis travamentos\n";
        println!("{}", fallback_msg);
        Self::write_log(fallback_msg);
        
        ("libx264".to_string(), vec![
            "-preset".to_string(), "veryfast".to_string(),
            "-tune".to_string(), "zerolatency".to_string(),
            "-threads".to_string(), "0".to_string(),
        ])
    }
    
    fn write_log(msg: &str) {
        // Salva log no diretório do executável (mesma pasta do banco de dados)
        let exe_path = std::env::current_exe().ok();
        let log_dir = exe_path
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        
        let log_path = log_dir.join("hipocast_encoder.log");
        
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "[{}] {}", timestamp, msg.trim());
        }
    }

    /// Tests if an encoder is actually usable on the system
    fn test_encoder(ffmpeg_path: &PathBuf, codec: &str) -> bool {
        let test_msg = format!("   📍 Testing {}...", codec);
        println!("{}", test_msg);
        Self::write_log(&test_msg);
        
        // Run a tiny dummy encoding to see if hardware init succeeds
        // AMF requires minimum resolution (use 256x256 instead of 64x64)
        let mut cmd = Command::new(ffmpeg_path);
        cmd.args([
                "-hide_banner",
                "-v", "warning",
                "-f", "lavfi", "-i", "color=c=black:s=256x256:d=0.1:r=30",
                "-vframes", "1",
                "-an",
                "-c:v", codec,
                "-b:v", "1M",  // Add bitrate for AMF
                "-f", "null", "-",
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
                    let success = format!("   ✅ {} WORKS! (tested in {:?})", codec, elapsed);
                    println!("{}", success);
                    Self::write_log(&success);
                    true
                } else {
                    let err = String::from_utf8_lossy(&out.stderr);
                    let fail_msg = format!("   ❌ {} FAILED (exit code: {:?})", codec, out.status.code());
                    println!("{}", fail_msg);
                    Self::write_log(&fail_msg);
                    
                    if !err.is_empty() && !err.contains("deprecated") {
                        let reason = format!("      Reason: {}", err.trim().lines().next().unwrap_or(""));
                        println!("{}", reason);
                        Self::write_log(&reason);
                    }
                    false
                }
            },
            Err(e) => {
                let error = format!("   ❌ {} failed to execute: {}", codec, e);
                println!("{}", error);
                Self::write_log(&error);
                false
            }
        }
    }

    fn spawn_process(
        ffmpeg_path: PathBuf,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        list_size: u32,
        hls_time: f64,
        output_dir: &str,
        stream_id: &str,
        codec: String,
        preset_args: Vec<String>,
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
        // Create stream-specific subdirectory (e.g., streams/monitor_1)
        let stream_dir = format!("{}/{}", output_dir, stream_id);
        
        // FORCE CLEAN: Remove ALL old segments before starting
        if std::path::Path::new(&stream_dir).exists() {
            println!("🧹 Cleaning old stream directory: {}", stream_dir);
            let _ = std::fs::remove_dir_all(&stream_dir);
        }
        std::fs::create_dir_all(&stream_dir)?;
        
        let output_path = format!("{}/{}.m3u8", stream_dir, stream_id);
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let segment_filename = format!("{}/{}_{}_seg_%03d.m4s", stream_dir, stream_id, timestamp);
        
        println!("📹 HLS Config: list_size={}, hls_time={}, output={}", list_size, hls_time, output_path);
        
        // HLS Settings
        let mut hls_flags = "delete_segments+discont_start+independent_segments+program_date_time".to_string();
        let mut hls_args = vec![
            "-f".to_string(), "hls".to_string(),
            "-hls_time".to_string(), hls_time.to_string(),
            "-hls_list_size".to_string(), list_size.to_string(),
            "-hls_segment_type".to_string(), "fmp4".to_string(),
            "-hls_fmp4_init_filename".to_string(), "init.mp4".to_string(),
        ];

        if list_size == 0 {
            hls_flags = "discont_start+independent_segments+program_date_time".to_string();
            hls_args.extend_from_slice(&["-hls_playlist_type".to_string(), "event".to_string()]);
        }
        hls_args.extend_from_slice(&["-hls_flags".to_string(), hls_flags]);
        
        // Build Command
        let mut command = Command::new(ffmpeg_path);
        command.args(["-y", "-fflags", "+genpts"]);

        // Audio Input Logic - Native WASAPI/DirectShow (NO PIPES!)
        let video_input_index;

        if enable_audio && enable_microphone {
            // CASE 1: BOTH System Audio (Pipe) + Microphone (Pipe WASAPI) -> Mix them
            video_input_index = 2; // Input 0 (system audio), Input 1 (mic), Input 2 (video)

            // Apply offset if configured
            if audio_offset != 0 {
                let offset_sec = audio_offset as f64 / 1000.0;
                command.args(["-itsoffset", &format!("{:.3}", offset_sec)]);
            }

            // Input 0: System Audio via DirectShow (Direct!)
            command.args([
                "-f", "dshow",
                "-use_wallclock_as_timestamps", "1",  // Sincroniza com horário do sistema
                "-i", &format!("audio={}", audio_device),
                "-thread_queue_size", "512",
                "-audio_buffer_size", &audio_buffer_size.to_string(),
            ]);

            // Input 1: Microphone via DirectShow (Direct!)
            command.args([
                "-f", "dshow",
                "-use_wallclock_as_timestamps", "1",  // Sincroniza com horário do sistema
                "-i", &format!("audio={}", microphone_device),
                "-thread_queue_size", "512",
                "-audio_buffer_size", &audio_buffer_size.to_string(),
            ]);

            // Complex Filter to mix them + apply audio effects
            let mix_filter = match (!audio_filters.is_empty(), !microphone_filters.is_empty()) {
                (true, true) => {
                    // Both have filters: apply filters to each input, then mix
                    format!("[0:a]aresample=48000,aformat=channel_layouts=stereo,{}[a0];[1:a]aresample=48000,aformat=channel_layouts=stereo,{}[a1];[a0][a1]amerge=inputs=2,pan=stereo|c0<c0+c2|c1<c1+c3[outa]", 
                            audio_filters, microphone_filters)
                },
                (true, false) => {
                    // Only system audio has filters
                    format!("[0:a]aresample=48000,aformat=channel_layouts=stereo,{}[a0];[1:a]aresample=48000,aformat=channel_layouts=stereo[a1];[a0][a1]amerge=inputs=2,pan=stereo|c0<c0+c2|c1<c1+c3[outa]", 
                            audio_filters)
                },
                (false, true) => {
                    // Only microphone has filters
                    format!("[0:a]aresample=48000,aformat=channel_layouts=stereo[a0];[1:a]aresample=48000,aformat=channel_layouts=stereo,{}[a1];[a0][a1]amerge=inputs=2,pan=stereo|c0<c0+c2|c1<c1+c3[outa]", 
                            microphone_filters)
                },
                (false, false) => {
                    // No filters for either
                    "[0:a]aresample=48000,aformat=channel_layouts=stereo[a0];[1:a]aresample=48000,aformat=channel_layouts=stereo[a1];[a0][a1]amerge=inputs=2,pan=stereo|c0<c0+c2|c1<c1+c3[outa]".to_string()
                }
            };
            command.args(["-filter_complex", &mix_filter]);
            
             println!("🎤🔊 Mixing Audio: System='{}' + Mic='{}' (offset: {}ms, sys_filters: {}, mic_filters: {})",
                      audio_device, microphone_device, audio_offset, 
                      if audio_filters.is_empty() { "none" } else { &audio_filters },
                      if microphone_filters.is_empty() { "none" } else { &microphone_filters });

        } else if enable_audio {
            // CASE 2: System Audio ONLY (DirectShow)
            video_input_index = 1;
            if audio_offset != 0 {
                let offset_sec = audio_offset as f64 / 1000.0;
                command.args(["-itsoffset", &format!("{:.3}", offset_sec)]);
            }
            
            command.args([
                "-f", "dshow",
                "-use_wallclock_as_timestamps", "1",  // Sincroniza com horário do sistema
                "-i", &format!("audio={}", audio_device),
                "-thread_queue_size", "512",
                "-audio_buffer_size", &audio_buffer_size.to_string(),
            ]);
            
            println!("🔊 System Audio Only: device='{}' (offset: {}ms)", audio_device, audio_offset);

        } else if enable_microphone {
            // CASE 3: Microphone ONLY (DirectShow)
            video_input_index = 1;
            if audio_offset != 0 {
                 let offset_sec = audio_offset as f64 / 1000.0;
                 command.args(["-itsoffset", &format!("{:.3}", offset_sec)]);
            }
            
            command.args([
                "-f", "dshow",
                "-use_wallclock_as_timestamps", "1",  // Sincroniza com horário do sistema
                "-i", &format!("audio={}", microphone_device),
                "-thread_queue_size", "512",
                "-audio_buffer_size", &audio_buffer_size.to_string(),
            ]);
            
            println!("🎤 Microphone Only: device='{}' (offset: {}ms)", microphone_device, audio_offset);

        } else {
            // CASE 4: No Audio (Silent Fallback)
            command.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]);
            video_input_index = 1;
            
            println!("🔇 Audio disabled: Using silent audio track");
        }
        
        // Video Input from pipe (último input)
        command.args([
            "-f", "rawvideo",
            "-pix_fmt", "rgba",
            "-s", &format!("{}x{}", width, height),
            "-r", &fps.to_string(),
            "-i", "pipe:0",
        ]);
        
        // Apply audio filters for single input cases (cases 2 and 3)
        // Filters must be applied AFTER all inputs are defined
        if enable_audio && !enable_microphone {
            // CASE 2: System audio only - apply audio_filters
            if !audio_filters.is_empty() {
                let audio_filter = format!("aresample=48000,{}", audio_filters);
                command.args(["-filter:a", &audio_filter]);
                println!("   Filters: {}", audio_filters);
            } else {
                command.args(["-filter:a", "aresample=48000"]);
                println!("   Filters: none (only resample)");
            }
        } else if !enable_audio && enable_microphone {
            // CASE 3: Microphone only - apply microphone_filters
            if !microphone_filters.is_empty() {
                let mic_filter = format!("aresample=48000,{}", microphone_filters);
                command.args(["-filter:a", &mic_filter]);
                println!("   Filters: {}", microphone_filters);
            } else {
                command.args(["-filter:a", "aresample=48000"]);
                println!("   Filters: none (only resample)");
            }
        }
        
        // Map streams: audio from appropriate input, video from last input
        if enable_audio && enable_microphone {
            // Mixed audio from filter_complex output
            command.args(["-map", "[outa]"]);
        } else if enable_audio || enable_microphone {
            // Audio from input 0
            command.args(["-map", "0:a"]);
        } else {
            // Silent audio from input 0
            command.args(["-map", "0:a"]);
        }
        // Video from last input (pipe:0)
        command.args(["-map", &format!("{}:v", video_input_index)]);

        // Video encoding
        command.args(["-c:v", &codec]);

        // Add codec-specific presets
        command.args(&preset_args);

        // Video settings
        command.args([
            "-b:v", &format!("{}k", bitrate / 1000),
            "-maxrate", &format!("{}k", bitrate / 1000),
            "-bufsize", &format!("{}k", bitrate / 500),
            "-g", &fps.to_string(), // GOP size = 1s
            "-color_range", "tv",
            "-colorspace", "bt709",
            "-color_primaries", "bt709",
            "-color_trc", "bt709",
            "-vf", "scale=w=trunc(iw/2)*2:h=trunc(ih/2)*2,format=yuv420p",
        ]);
        
        // Audio encoding (using configurable bitrate and buffer size)
        let audio_bitrate_str = format!("{}k", audio_bitrate);
        
        // Audio encoding with optional filters
        // For single audio input (not mixed), apply filters via -af
        let needs_af_filter = (enable_audio || enable_microphone) && !(enable_audio && enable_microphone) && !audio_filters.is_empty();
        
        command.args([
            "-c:a", "aac",
            "-b:a", &audio_bitrate_str,
            "-ar", "48000",
            "-ac", "2",  // Stereo
            "-aac_coder", "fast", // 'fast' é mais estável que 'twoloop' para sinais complexos
        ]);
        
        // Apply audio filters for single input streams (system OR mic, not both)
        if needs_af_filter {
            // Adiciona aresample para garantir consistência de 48kHz vindo do WASAPI
            let final_filters = format!("aresample=48000,{}", audio_filters);
            command.args(["-af", &final_filters]);
            println!("🎛️ Applying audio filters: {}", final_filters);
        } else if (enable_audio || enable_microphone) && !(enable_audio && enable_microphone) {
            // Mesmo sem filtros de efeito, garante o resample
            command.args(["-af", "aresample=48000"]);
        }
        
        // Add HLS specific args
        command.args(&hls_args);
        
        // If audio is disabled, use -shortest to match video length
        if !enable_audio {
            command.arg("-shortest");
        }
        
        // Output path and flags
        command.args([
            "-hls_segment_filename", &segment_filename,
            "-threads", "0",
            &output_path,
        ]);

        // Redirect stderr to file for debugging
        let stderr_log = std::fs::File::create(format!("{}/ffmpeg.log", stream_dir))?;

        let mut process_command = command;
        process_command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_log));

        // Hide console window on Windows
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            process_command.creation_flags(CREATE_NO_WINDOW);
        }

        let process = process_command.spawn()?;

        println!("FFmpeg started: {} ({}x{} @ {} fps) [Encoder: {}]", stream_id, width, height, fps, codec);

        Ok(Self {
            process,
            width,
            height,
        })
    }

    /// Write a raw BGRA frame to FFmpeg
    pub fn write_frame(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let expected_size = (self.width * self.height * 4) as usize;
        if data.len() != expected_size {
            return Err(format!(
                "Frame size mismatch: expected {}, got {}",
                expected_size,
                data.len()
            ).into());
        }

        if let Some(stdin) = self.process.stdin.as_mut() {
            stdin.write_all(data)?;
        }
        Ok(())
    }

    /// Stop FFmpeg process immediately
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
