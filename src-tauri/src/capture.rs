use crate::db::Database;
use fast_image_resize::{images::Image, PixelType, ResizeAlg, ResizeOptions, Resizer};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CaptureSource {
    pub id: String,
    pub title: String,
    pub source_type: String,
}

pub fn list_windows() -> Vec<CaptureSource> {
    let mut sources = Vec::new();

    // List Monitors
    for monitor in Monitor::enumerate().unwrap_or_default() {
        if let Ok(index) = monitor.index() {
            sources.push(CaptureSource {
                id: format!("monitor:{}", index),
                title: format!("Monitor {}", index),
                source_type: "monitor".to_string(),
            });
        }
    }

    // List Windows
    for window in Window::enumerate().unwrap_or_default() {
        if let Ok(title) = window.title() {
            if !title.is_empty() {
                sources.push(CaptureSource {
                    id: format!("window:{}", window.as_raw_hwnd() as usize),
                    title,
                    source_type: "window".to_string(),
                });
            }
        }
    }

    sources
}

use std::sync::mpsc::{channel, Sender};
use std::thread;

#[derive(Clone)]
pub struct FrameData {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

// Wrapper to hold capture control + stop signal
pub struct CaptureHandle {
    _control: CaptureControl<CaptureHandler, Box<dyn std::error::Error + Send + Sync>>,
    pub stop_signal: Arc<AtomicBool>,
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        tracing::info!("🛑 CaptureHandle dropped, sending stop signal...");
        self.stop_signal.store(true, Ordering::Relaxed);
    }
}

pub struct CaptureHandler {
    sender: Sender<FrameData>,
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = Sender<FrameData>;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Log only first frame to avoid flooding
        static LOGGED_FIRST_FRAME: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !LOGGED_FIRST_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("📸 First frame arrived from Windows Capture API");
        }

        let width = frame.width();
        let height = frame.height();
        let mut frame_buffer = frame.buffer()?;
        let data = frame_buffer.as_raw_buffer();

        // Handle Stride (Row Pitch): data from API might have padding at the end of each row.
        // We copy only the actual pixel data (width * 4) for each row.
        let row_size = (width * 4) as usize;
        let expected_total_len = row_size * (height as usize);

        // Defensive Check: Ensure data length is consistent with height and stride
        // prevents STATUS_STACK_BUFFER_OVERRUN during resolution transitions
        if data.len() < expected_total_len {
            tracing::warn!(
                "⚠️ Frame data too small! Expected {}, got {}. Skipping corrupt frame.",
                expected_total_len,
                data.len()
            );
            return Ok(());
        }

        let stride = data.len() / (height as usize);
        let mut buffer = Vec::with_capacity(expected_total_len);

        for y in 0..height as usize {
            let start = y * stride;
            let end = start + row_size;
            if end <= data.len() {
                buffer.extend_from_slice(&data[start..end]);
            }
        }

        let _ = self.sender.send(FrameData {
            data: buffer,
            width,
            height,
        });

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        tracing::warn!(
            "⚠️ CaptureHandler::on_closed called! Windows Capture session ended unexpectedly."
        );
        Ok(())
    }

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        tracing::info!("🆕 CaptureHandler initialized (Capture Session Started)");
        Ok(Self { sender: ctx.flags })
    }
}

use crate::ffmpeg::FfmpegEncoder;
use crate::AppSettings;
use std::sync::Arc;

pub fn start_capture(
    _app: tauri::AppHandle,
    id: String,
    output_dir: String,
    settings_config: AppSettings,
    db: Arc<Database>,
) -> Result<CaptureHandle, Box<dyn std::error::Error + Send + Sync>> {
    let (tx, rx) = channel::<FrameData>();
    let stream_id = id.clone().replace(":", "_");
    let output_dir_clone = output_dir.clone();

    // Flag to signal thread stop
    let should_stop = Arc::new(AtomicBool::new(false));
    let should_stop_clone = should_stop.clone();

    // Spawn thread to handle FFmpeg HLS encoding
    thread::spawn(move || {
        tracing::info!("🎬 FFmpeg thread started for stream: {}", stream_id);
        let mut ffmpeg: Option<FfmpegEncoder> = None;
        let mut last_frame_time = std::time::Instant::now();
        let frame_interval =
            std::time::Duration::from_secs_f64(1.0 / settings_config.framerate as f64);

        let mut last_valid_frame: Option<FrameData> = None;

        loop {
            // Check stop signal first
            if should_stop_clone.load(Ordering::Relaxed) {
                tracing::info!("🛑 Stop signal received. Stopping FFmpeg thread...");
                tracing::info!("✅ FFmpeg thread finished successfully (stop signal)");
                break;
            }

            // 1. DRAIN CHANNEL: Only process the LATEST frame to prevent memory backlog (OOM)
            let mut latest_frame = None;
            let mut disconnected = false;
            loop {
                match rx.try_recv() {
                    Ok(frame) => latest_frame = Some(frame),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                tracing::warn!("🔌 Channel disconnected. Stopping FFmpeg thread...");
                tracing::info!("✅ FFmpeg thread finished successfully (channel disconnect)");
                break; // FFmpeg será dropado automaticamente
            }

            if let Some(frame) = latest_frame {
                let start_process = std::time::Instant::now();
                // Initialize FFmpeg with Dynamic Canvas Resolution from Settings
                let canvas_width = settings_config.width;
                let canvas_height = settings_config.height;

                if ffmpeg.is_none() {
                    // Load stream-specific audio config
                    let stream_audio_config = db.get_stream_audio_config(&stream_id).ok().flatten();

                    let (
                        enable_audio,
                        enable_microphone,
                        microphone_device,
                        _target_pid,
                        audio_filters,
                        microphone_filters,
                    ) = if let Some(config) = stream_audio_config {
                        // Use stream-specific config
                        let enable_audio = config.audio_mode != "muted";
                        let target_pid = if config.audio_mode == "process" {
                            config.target_pid
                        } else {
                            None
                        };
                        let audio_filters = config.audio_effects.to_ffmpeg_filter();
                        let microphone_filters = config.microphone_effects.to_ffmpeg_filter();
                        (
                            enable_audio,
                            config.enable_microphone,
                            config.microphone_device.clone(),
                            target_pid,
                            audio_filters,
                            microphone_filters,
                        )
                    } else {
                        // Fallback to global settings
                        (
                            settings_config.enable_audio,
                            settings_config.enable_microphone,
                            settings_config.microphone_device.clone(),
                            None,
                            String::new(),
                            String::new(),
                        )
                    };

                    match FfmpegEncoder::new(
                        canvas_width,
                        canvas_height,
                        settings_config.framerate,
                        settings_config.bitrate,
                        settings_config.hls_list_size,
                        settings_config.hls_time,
                        &output_dir_clone,
                        &stream_id,
                        settings_config.enable_hw_accel,
                        settings_config.ffmpeg_preset.clone(),
                        enable_audio,
                        enable_microphone,
                        settings_config.audio_bitrate,
                        settings_config.audio_buffer_size,
                        settings_config.audio_offset,
                        settings_config.audio_device.clone(),
                        microphone_device,
                        audio_filters,
                        microphone_filters,
                    ) {
                        Ok(enc) => {
                            ffmpeg = Some(enc);
                            last_frame_time = std::time::Instant::now(); // Reset on first frame
                            tracing::info!(
                                "Capture initialized with Canvas at {}x{}",
                                canvas_width,
                                canvas_height
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to start FFmpeg: {:?}", e);
                            continue;
                        }
                    }
                }

                // PROCESS FRAME: Scale if dimensions don't match Dynamic Canvas
                let canvas_width = settings_config.width;
                let canvas_height = settings_config.height;

                // Destructure to take ownership of fields to avoid partial move issues
                let FrameData {
                    data,
                    width,
                    height,
                } = frame;

                if width != canvas_width || height != canvas_height {
                    // Optimized scaling using fast_image_resize
                    let src_width = NonZeroU32::new(width).unwrap();
                    let src_height = NonZeroU32::new(height).unwrap();
                    let dst_width = NonZeroU32::new(canvas_width).unwrap();
                    let dst_height = NonZeroU32::new(canvas_height).unwrap();

                    let src_image = Image::from_vec_u8(
                        src_width.get(),
                        src_height.get(),
                        data,
                        PixelType::U8x4,
                    )
                    .unwrap();

                    let mut dst_image =
                        Image::new(dst_width.get(), dst_height.get(), PixelType::U8x4);

                    let mut resizer = Resizer::new();
                    let options = ResizeOptions::new().resize_alg(ResizeAlg::Nearest);

                    if let Err(e) = resizer.resize(&src_image, &mut dst_image, &options) {
                        tracing::warn!("Resize failed: {:?}", e);
                        continue;
                    }

                    last_valid_frame = Some(FrameData {
                        data: dst_image.into_vec(),
                        width: canvas_width,
                        height: canvas_height,
                    });
                } else {
                    last_valid_frame = Some(FrameData {
                        data,
                        width,
                        height,
                    });
                }

                // Performance Monitor: Alert if scaling is too slow for 60fps
                let elapsed = start_process.elapsed();
                if elapsed > frame_interval {
                    tracing::warn!(
                        "⚠️ Warning: Scaling lag detected! Processing took {:?}, limit is {:?}",
                        elapsed,
                        frame_interval
                    );
                }

                // If we were idle for too long, reset pacer to current time
                if last_frame_time.elapsed() > std::time::Duration::from_secs(1) {
                    last_frame_time = std::time::Instant::now();
                }
            }

            // 2. Check if it's time to send a frame (CFR Logic)
            if let Some(frame) = &last_valid_frame {
                let mut catchup_limit = 5; // Max 5 frames catch-up per loop to prevent huge bursts
                while last_frame_time.elapsed() >= frame_interval && catchup_limit > 0 {
                    if let Some(enc) = ffmpeg.as_mut() {
                        // Crucial: write_frame already checks for size mismatch and returns error
                        // We handle the error here to skip bad frames instead of crashing the thread
                        if let Err(e) = enc.write_frame(&frame.data) {
                            // Check for Broken Pipe (OS Error 232 on Windows) which means FFmpeg was killed/stopped
                            // We need to try to downcast to io::Error because write_frame returns Box<dyn Error>
                            let is_pipe_closed =
                                if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                                    if let Some(os_err) = io_err.raw_os_error() {
                                        os_err == 232 || os_err == 109
                                    } else {
                                        io_err.kind() == std::io::ErrorKind::BrokenPipe
                                    }
                                } else {
                                    false
                                };

                            if is_pipe_closed {
                                // Silent exit
                                break;
                            } else {
                                tracing::warn!("Frame skip: size mismatch or error: {}", e);
                            }
                        }
                    }

                    // Advance time by EXACTLY one frame interval to prevent drift
                    last_frame_time += frame_interval;
                    catchup_limit -= 1;
                }

                // If we are still too far behind, skip time to prevent permanent lag
                if last_frame_time.elapsed() > std::time::Duration::from_secs(1) {
                    last_frame_time = std::time::Instant::now();
                }
            }

            // Sleep tiny amount to prevent CPU spinning
            thread::sleep(std::time::Duration::from_micros(100)); // 0.1ms precision
        }

        // Cleanup is automatic via Drop
    });

    let control = if id.starts_with("monitor:") {
        let target_index = id.strip_prefix("monitor:").unwrap().parse::<usize>()?;
        let monitors = Monitor::enumerate()?;

        // Find monitor by its actual index, not position in array
        let monitor = monitors
            .iter()
            .find(|m| m.index().ok() == Some(target_index))
            .or_else(|| monitors.get(target_index))
            .ok_or("Monitor not found")?;

        let settings = Settings::new(
            *monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            tx,
        );
        CaptureHandler::start_free_threaded(settings)?
    } else if id.starts_with("window:") {
        let hwnd = id.strip_prefix("window:").unwrap().parse::<usize>()?;
        let window = Window::from_raw_hwnd(hwnd as _);

        let settings = Settings::new(
            window,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            tx,
        );
        CaptureHandler::start_free_threaded(settings)?
    } else {
        return Err("Invalid source ID".into());
    };

    Ok(CaptureHandle {
        _control: control,
        stop_signal: should_stop,
    })
}

// Version for browser streams that separates stream_id (for folders) from capture_id (for actual capture)
pub fn start_capture_with_ids(
    _app: tauri::AppHandle,
    stream_id: String,  // Custom ID for folders/files (what user sees)
    capture_id: String, // Real window ID for capture (window:HWND)
    output_dir: String,
    settings_config: AppSettings,
    db: Arc<Database>,
) -> Result<CaptureHandle, Box<dyn std::error::Error + Send + Sync>> {
    let (tx, rx) = channel::<FrameData>();
    let stream_id_sanitized = stream_id.clone().replace(":", "_");
    let output_dir_clone = output_dir.clone();

    // Flag to signal thread stop
    let should_stop = Arc::new(AtomicBool::new(false));
    let should_stop_clone = should_stop.clone();

    // Spawn thread to handle FFmpeg HLS encoding (same as start_capture but uses stream_id for folders)
    thread::spawn(move || {
        tracing::info!(
            "🎬 FFmpeg thread started for browser stream: {}",
            stream_id_sanitized
        );
        let mut ffmpeg: Option<FfmpegEncoder> = None;
        let mut last_frame_time = std::time::Instant::now();
        let frame_interval =
            std::time::Duration::from_secs_f64(1.0 / settings_config.framerate as f64);

        let mut last_valid_frame: Option<FrameData> = None;

        loop {
            // Check stop signal first
            if should_stop_clone.load(Ordering::Relaxed) {
                tracing::info!(
                    "🛑 Stop signal received. Stopping FFmpeg thread (browser stream)..."
                );
                tracing::info!("✅ FFmpeg thread finished successfully (stop signal)");
                break;
            }

            // 1. DRAIN CHANNEL: Only process the LATEST frame to prevent memory backlog (OOM)
            let mut latest_frame = None;
            let mut disconnected = false;
            loop {
                match rx.try_recv() {
                    Ok(frame) => latest_frame = Some(frame),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                tracing::warn!(
                    "🔌 Channel disconnected. Stopping FFmpeg thread (browser stream)..."
                );
                tracing::info!("✅ FFmpeg thread finished successfully (channel disconnect)");
                break; // FFmpeg será dropado automaticamente
            }

            if let Some(frame) = latest_frame {
                let start_process = std::time::Instant::now();
                // Initialize FFmpeg with Dynamic Canvas Resolution from Settings
                let canvas_width = settings_config.width;
                let canvas_height = settings_config.height;

                if ffmpeg.is_none() {
                    // Load stream-specific audio config (use stream_id_sanitized)
                    let stream_audio_config = db
                        .get_stream_audio_config(&stream_id_sanitized)
                        .ok()
                        .flatten();

                    let (
                        enable_audio,
                        enable_microphone,
                        microphone_device,
                        _target_pid,
                        audio_filters,
                        microphone_filters,
                    ) = if let Some(config) = stream_audio_config {
                        // Use stream-specific config
                        let enable_audio = config.audio_mode != "muted";
                        let target_pid = if config.audio_mode == "process" {
                            config.target_pid
                        } else {
                            None
                        };

                        let audio_filters = config.audio_effects.to_ffmpeg_filter();
                        let microphone_filters = config.microphone_effects.to_ffmpeg_filter();
                        (
                            enable_audio,
                            config.enable_microphone,
                            config.microphone_device.clone(),
                            target_pid,
                            audio_filters,
                            microphone_filters,
                        )
                    } else {
                        // Fallback to global settings
                        (
                            settings_config.enable_audio,
                            settings_config.enable_microphone,
                            settings_config.microphone_device.clone(),
                            None,
                            String::new(),
                            String::new(),
                        )
                    };

                    match FfmpegEncoder::new(
                        canvas_width,
                        canvas_height,
                        settings_config.framerate,
                        settings_config.bitrate,
                        settings_config.hls_list_size,
                        settings_config.hls_time,
                        &output_dir_clone,
                        &stream_id_sanitized, // Use custom stream_id for folders
                        settings_config.enable_hw_accel,
                        settings_config.ffmpeg_preset.clone(),
                        enable_audio,
                        enable_microphone,
                        settings_config.audio_bitrate,
                        settings_config.audio_buffer_size,
                        settings_config.audio_offset,
                        settings_config.audio_device.clone(),
                        microphone_device,
                        audio_filters,
                        microphone_filters,
                    ) {
                        Ok(enc) => {
                            ffmpeg = Some(enc);
                            last_frame_time = std::time::Instant::now(); // Reset on first frame
                            println!(
                                "Capture initialized with Canvas at {}x{} for stream '{}'",
                                canvas_width, canvas_height, stream_id_sanitized
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to start FFmpeg: {:?}", e);
                            continue;
                        }
                    }
                }

                // PROCESS FRAME: Scale if dimensions don't match Dynamic Canvas
                let canvas_width = settings_config.width;
                let canvas_height = settings_config.height;

                // Destructure to take ownership of fields to avoid partial move issues
                let FrameData {
                    data,
                    width,
                    height,
                } = frame;

                if width != canvas_width || height != canvas_height {
                    // Optimized scaling using fast_image_resize
                    let src_width = NonZeroU32::new(width).unwrap();
                    let src_height = NonZeroU32::new(height).unwrap();
                    let dst_width = NonZeroU32::new(canvas_width).unwrap();
                    let dst_height = NonZeroU32::new(canvas_height).unwrap();

                    let src_image = Image::from_vec_u8(
                        src_width.get(),
                        src_height.get(),
                        data,
                        PixelType::U8x4,
                    )
                    .unwrap();

                    let mut dst_image =
                        Image::new(dst_width.get(), dst_height.get(), PixelType::U8x4);

                    let mut resizer = Resizer::new();
                    let options = ResizeOptions::new().resize_alg(ResizeAlg::Nearest);

                    if let Err(e) = resizer.resize(&src_image, &mut dst_image, &options) {
                        tracing::warn!("Resize failed: {:?}", e);
                        continue;
                    }

                    last_valid_frame = Some(FrameData {
                        data: dst_image.into_vec(),
                        width: canvas_width,
                        height: canvas_height,
                    });
                } else {
                    last_valid_frame = Some(FrameData {
                        data,
                        width,
                        height,
                    });
                }

                // ENCODE FRAME if available
                if let (Some(enc), Some(valid_frame)) = (ffmpeg.as_mut(), &last_valid_frame) {
                    if let Err(e) = enc.write_frame(&valid_frame.data) {
                        tracing::error!("FFmpeg encode error: {:?}", e);
                    }
                }

                let elapsed_process = start_process.elapsed();

                if elapsed_process > frame_interval.mul_f32(0.8) {
                    tracing::warn!(
                        "⚠️ Frame processing took {:?} (limit: {:?}). Risk of frame drop.",
                        elapsed_process,
                        frame_interval
                    );
                }
            } else if last_valid_frame.is_some() {
                // No new frame but we have a valid one cached
                if let (Some(enc), Some(valid_frame)) = (ffmpeg.as_mut(), &last_valid_frame) {
                    if let Err(e) = enc.write_frame(&valid_frame.data) {
                        tracing::error!("FFmpeg encode error (cached): {:?}", e);
                    }
                }
            }

            // 2. SLEEP to target framerate (but also prevent CPU busy-wait)
            let elapsed_total = last_frame_time.elapsed();
            if elapsed_total < frame_interval {
                thread::sleep(frame_interval - elapsed_total);
            }
            last_frame_time = std::time::Instant::now();

            // Micro-sleep for responsiveness
            thread::sleep(std::time::Duration::from_micros(100)); // 0.1ms precision
        }

        // Cleanup is automatic via Drop
    });

    // Use capture_id for the actual capture (window:HWND)
    let control = if capture_id.starts_with("monitor:") {
        let target_index = capture_id
            .strip_prefix("monitor:")
            .unwrap()
            .parse::<usize>()?;
        let monitors = Monitor::enumerate()?;

        // Find monitor by its actual index, not position in array
        let monitor = monitors
            .iter()
            .find(|m| m.index().ok() == Some(target_index))
            .or_else(|| monitors.get(target_index))
            .ok_or("Monitor not found")?;

        let settings = Settings::new(
            *monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            tx,
        );
        CaptureHandler::start_free_threaded(settings)?
    } else if capture_id.starts_with("window:") {
        let hwnd = capture_id
            .strip_prefix("window:")
            .unwrap()
            .parse::<usize>()?;
        let window = Window::from_raw_hwnd(hwnd as _);

        let settings = Settings::new(
            window,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            tx,
        );
        CaptureHandler::start_free_threaded(settings)?
    } else {
        return Err("Invalid capture ID".into());
    };

    Ok(CaptureHandle {
        _control: control,
        stop_signal: should_stop,
    })
}
