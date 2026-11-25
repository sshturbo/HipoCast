use crate::encoder::{enumerate_audio_devices, AudioConfig};
use crate::ffmpeg::FfmpegEncoder;
use crate::output::HlsConfig;
use crate::stream_config::StreamAudioConfig;
use crate::AppSettings;
use fast_image_resize::{images::Image, PixelType, ResizeAlg, ResizeOptions, Resizer};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::ChildStdin;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Graphics::{
    Direct3D11::{
        ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    },
    Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
};
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl},
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

type CaptureError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Serialize)]
pub struct CaptureSource {
    pub id: String,
    pub title: String,
    pub source_type: &'static str,
}

pub fn list_windows() -> Vec<CaptureSource> {
    let mut sources = Vec::new();
    for monitor in Monitor::enumerate().unwrap_or_default() {
        if let Ok(index) = monitor.index() {
            sources.push(CaptureSource {
                id: format!("monitor:{index}"),
                title: format!("Monitor {index}"),
                source_type: "monitor",
            });
        }
    }
    for window in Window::enumerate().unwrap_or_default() {
        if let Ok(title) = window.title() {
            if !title.is_empty() {
                sources.push(CaptureSource {
                    id: format!("window:{}", window.as_raw_hwnd() as usize),
                    title,
                    source_type: "window",
                });
            }
        }
    }
    sources
}

struct FrameData {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Default)]
struct FrameState {
    latest: Option<FrameData>,
    spare_pixels: Vec<u8>,
    stopping: bool,
    source_closed: bool,
}

impl FrameState {
    fn recycle_pixels(&mut self, pixels: Vec<u8>) {
        if pixels.capacity() > self.spare_pixels.capacity() {
            self.spare_pixels = pixels;
        }
    }
}

#[derive(Default)]
struct Frames {
    state: Mutex<FrameState>,
    available: Condvar,
}

impl Frames {
    fn close(&self) {
        self.state.lock().unwrap().stopping = true;
        self.available.notify_all();
    }
}

pub struct CaptureHandle {
    frames: Arc<Frames>,
    worker: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn stop(&mut self) {
        self.frames.close();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                tracing::error!("A thread de captura terminou inesperadamente");
            }
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

struct CaptureFlags {
    frames: Arc<Frames>,
    interval: Duration,
}

struct CaptureHandler {
    frames: Arc<Frames>,
    interval: Duration,
    next_frame: Option<Instant>,
    readback: FrameReadback,
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = CaptureFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            frames: ctx.flags.frames,
            interval: ctx.flags.interval,
            next_frame: None,
            readback: FrameReadback {
                device: ctx.device,
                context: ctx.device_context,
                texture: None,
            },
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let (width, height) = (frame.width(), frame.height());
        let mut pixels = {
            let mut state = self.frames.state.lock().unwrap();
            if state.stopping {
                control.stop();
                return Ok(());
            }
            if width == 0 || height == 0 || !accept_frame(&mut self.next_frame, Instant::now(), self.interval) {
                return Ok(());
            }
            std::mem::take(&mut state.spare_pixels)
        };
        // The capture callback owns the frame for the duration of this copy.
        self.readback.copy(unsafe { frame.as_raw_texture() }, width, height, &mut pixels)?;
        let mut state = self.frames.state.lock().unwrap();
        if let Some(skipped) = state.latest.replace(FrameData { data: pixels, width, height }) {
            state.recycle_pixels(skipped.data);
        }
        self.frames.available.notify_one();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.frames.state.lock().unwrap().source_closed = true;
        self.frames.available.notify_all();
        Ok(())
    }
}

fn accept_frame(next_frame: &mut Option<Instant>, now: Instant, interval: Duration) -> bool {
    if let Some(deadline) = *next_frame {
        // Preserve the sampling cadence when the display frame arrives slightly early.
        if now + interval / 8 < deadline {
            return false;
        }
        *next_frame = Some((deadline + interval).max(now + interval / 2));
    } else {
        *next_frame = Some(now + interval);
    }
    true
}

struct FrameReadback {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    texture: Option<(u32, u32, ID3D11Texture2D)>,
}

impl FrameReadback {
    fn copy(
        &mut self,
        source: &ID3D11Texture2D,
        width: u32,
        height: u32,
        pixels: &mut Vec<u8>,
    ) -> Result<(), CaptureError> {
        if self.texture.as_ref().is_none_or(|(w, h, _)| (*w, *h) != (width, height)) {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            };
            let mut texture = None;
            unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture))? };
            self.texture = Some((width, height, texture.ok_or("Direct3D não criou a textura de leitura.")?));
        }
        let (_, _, texture) = self.texture.as_ref().unwrap();
        let mut mapped = MappedFrame {
            context: &self.context,
            texture,
            data: D3D11_MAPPED_SUBRESOURCE::default(),
        };
        // Both textures belong to the capture device and use the session's RGBA8 format.
        unsafe {
            self.context.CopyResource(texture, source);
            self.context.Map(texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped.data))?;
        }

        let row_size = width as usize * 4;
        let stride = mapped.data.RowPitch as usize;
        let data = mapped.data.pData.cast::<u8>();
        if data.is_null() || stride < row_size {
            return Err("O Direct3D retornou uma imagem inválida.".into());
        }
        let size = row_size * height as usize;
        pixels.clear();
        pixels.reserve(size);
        if stride == row_size {
            pixels.extend_from_slice(unsafe { std::slice::from_raw_parts(data, size) });
        } else {
            for row in 0..height as usize {
                pixels.extend_from_slice(unsafe { std::slice::from_raw_parts(data.add(row * stride), row_size) });
            }
        }
        Ok(())
    }
}

struct MappedFrame<'a> {
    context: &'a ID3D11DeviceContext,
    texture: &'a ID3D11Texture2D,
    data: D3D11_MAPPED_SUBRESOURCE,
}

impl Drop for MappedFrame<'_> {
    fn drop(&mut self) {
        if !self.data.pData.is_null() {
            unsafe { self.context.Unmap(self.texture, 0) };
        }
    }
}

pub fn start_capture(
    source_id: String,
    settings: AppSettings,
    stream_audio: StreamAudioConfig,
    hls: HlsConfig,
    hls_port: u16,
    update: impl Fn(&'static str, Option<String>) + Send + 'static,
) -> std::io::Result<CaptureHandle> {
    let frames = Arc::new(Frames::default());
    let worker_frames = frames.clone();
    let worker = thread::Builder::new()
        .name("hipocast-capture".into())
        .spawn(move || {
            let result = run_capture(
                &source_id,
                &settings,
                stream_audio,
                &hls,
                hls_port,
                &worker_frames,
                &update,
            );
            if let Err(error) = result {
                tracing::error!(stream = hls.stream_id(), %error, "Falha na captura");
                update("failed", Some(error));
            }
        })?;
    Ok(CaptureHandle {
        frames,
        worker: Some(worker),
    })
}

fn run_capture(
    source_id: &str,
    settings: &AppSettings,
    stream_audio: StreamAudioConfig,
    hls: &HlsConfig,
    hls_port: u16,
    frames: &Arc<Frames>,
    update: &impl Fn(&'static str, Option<String>),
) -> Result<(), String> {
    let (audio, notice) = resolve_audio(settings, stream_audio);
    if frames.state.lock().unwrap().stopping {
        return Ok(());
    }
    update("starting", notice.clone());

    let mut encoder = FfmpegEncoder::new(settings, audio, hls)
        .map_err(|error| format!("Não foi possível iniciar o FFmpeg: {error}"))?;
    let input = encoder
        .take_input()
        .ok_or("O FFmpeg não disponibilizou a entrada de vídeo.")?;
    if frames.state.lock().unwrap().stopping {
        return Ok(());
    }
    let capture = start_source(source_id, frames.clone(), settings.framerate)
        .map_err(|error| format!("Não foi possível abrir a fonte de captura: {error}"))?;

    let writer_frames = frames.clone();
    let (width, height, framerate) = (settings.width, settings.height, settings.framerate);
    let writer =
        thread::spawn(move || write_frames(input, &writer_frames, width, height, framerate));

    let playlist = hls.playlist_path();
    let route = hls.playlist_route();
    let timeout = Duration::from_secs_f64(12.0 + settings.hls_time * 2.0);
    let mut last_progress = Instant::now();
    let mut last_modified = None;
    let mut ready = false;

    let outcome = loop {
        let (stopping, source_closed) = {
            let state = frames.state.lock().unwrap();
            (state.stopping, state.source_closed)
        };
        if stopping {
            break Ok(());
        }
        if source_closed || capture.is_finished() {
            break Err(
                "A fonte de captura foi fechada. Selecione a janela ou monitor novamente.".into(),
            );
        }
        match encoder.try_wait() {
            Ok(Some(status)) => {
                break Err(encoder_error(
                    hls,
                    &format!("O FFmpeg encerrou ({status})."),
                ));
            }
            Err(error) => break Err(format!("Falha ao verificar o FFmpeg: {error}")),
            Ok(None) => {}
        }
        if writer.is_finished() {
            break Err(encoder_error(
                hls,
                "O envio de vídeo ao FFmpeg foi interrompido.",
            ));
        }
        if !ready && hls.playlist_is_ready() && crate::server::playlist_is_served(hls_port, &route)
        {
            ready = true;
            last_progress = Instant::now();
            update("running", notice.clone());
        }
        if ready {
            let modified = std::fs::metadata(&playlist)
                .and_then(|metadata| metadata.modified())
                .ok();
            if modified.is_some() && modified != last_modified {
                last_modified = modified;
                last_progress = Instant::now();
            }
        }
        if last_progress.elapsed() >= timeout {
            let message = if ready {
                "A playlist HLS parou de receber novos segmentos."
            } else {
                "O FFmpeg não gerou a playlist HLS a tempo."
            };
            break Err(encoder_error(hls, message));
        }
        thread::sleep(Duration::from_millis(100));
    };

    frames.close();
    let encoder_result = encoder
        .stop()
        .map_err(|error| format!("Falha ao encerrar o FFmpeg: {error}"));
    let capture_result = capture
        .stop()
        .map_err(|error| format!("Falha ao encerrar a fonte: {error}"));
    match writer.join() {
        Ok(Err(error)) => tracing::debug!(%error, "Envio de vídeo encerrado"),
        Err(_) => tracing::error!("A thread de envio de vídeo terminou inesperadamente"),
        Ok(Ok(())) => {}
    }
    outcome.and(encoder_result).and(capture_result)
}

fn start_source(
    source_id: &str,
    frames: Arc<Frames>,
    framerate: u32,
) -> Result<CaptureControl<CaptureHandler, CaptureError>, CaptureError> {
    let (kind, value) = source_id
        .split_once(':')
        .ok_or("Fonte de captura inválida.")?;
    let value = value.parse::<usize>()?;
    let interval = Duration::from_secs_f64(1.0 / f64::from(framerate));
    let native_interval = if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
        MinimumUpdateIntervalSettings::Custom(interval)
    } else {
        MinimumUpdateIntervalSettings::Default
    };
    macro_rules! capture {
        ($target:expr) => {
            CaptureHandler::start_free_threaded(Settings::new(
                $target,
                CursorCaptureSettings::Default,
                DrawBorderSettings::WithoutBorder,
                SecondaryWindowSettings::Default,
                native_interval,
                DirtyRegionSettings::Default,
                ColorFormat::Rgba8,
                CaptureFlags { frames, interval },
            ))?
        };
    }
    match kind {
        "window" => Ok(capture!(Window::from_raw_hwnd(value as _))),
        "monitor" => {
            let monitor = Monitor::enumerate()?
                .into_iter()
                .find(|monitor| monitor.index().ok() == Some(value))
                .ok_or("Monitor não encontrado.")?;
            Ok(capture!(monitor))
        }
        _ => Err("Fonte de captura inválida.".into()),
    }
}

fn write_frames(
    mut input: ChildStdin,
    frames: &Frames,
    width: u32,
    height: u32,
    framerate: u32,
) -> Result<(), CaptureError> {
    let interval = Duration::from_secs_f64(1.0 / f64::from(framerate));
    let mut next_frame = Instant::now();
    let mut previous = None;
    let mut resizer = Resizer::new();
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Nearest);

    loop {
        let frame = {
            let mut state = frames.state.lock().unwrap();
            if previous.is_none() {
                state = frames
                    .available
                    .wait_while(state, |state| {
                        state.latest.is_none() && !state.stopping && !state.source_closed
                    })
                    .unwrap();
                next_frame = Instant::now();
            } else {
                state = frames
                    .available
                    .wait_timeout_while(
                        state,
                        next_frame.saturating_duration_since(Instant::now()),
                        |state| !state.stopping && !state.source_closed,
                    )
                    .unwrap()
                    .0;
            }
            if state.stopping || state.source_closed {
                return Ok(());
            }
            state.latest.take()
        };
        if let Some(frame) = frame {
            let data = if (frame.width, frame.height) == (width, height) {
                if let Some(old) = previous.take() {
                    frames.state.lock().unwrap().recycle_pixels(old);
                }
                frame.data
            } else {
                let source =
                    Image::from_vec_u8(frame.width, frame.height, frame.data, PixelType::U8x4)?;
                let mut target = match previous.take() {
                    Some(pixels) => Image::from_vec_u8(width, height, pixels, PixelType::U8x4)?,
                    None => Image::new(width, height, PixelType::U8x4),
                };
                resizer.resize(&source, &mut target, &options)?;
                frames.state.lock().unwrap().recycle_pixels(source.into_vec());
                target.into_vec()
            };
            previous = Some(data);
        }
        if let Some(data) = &previous {
            input.write_all(data)?;
        }
        next_frame += interval;
        if next_frame + interval < Instant::now() {
            next_frame = Instant::now() + interval;
        }
    }
}

fn resolve_audio(
    settings: &AppSettings,
    stream: StreamAudioConfig,
) -> (AudioConfig, Option<String>) {
    let wants_system = stream.audio_mode != "muted";
    let system_selected = wants_system && !settings.audio_device.trim().is_empty();
    let microphone_selected =
        stream.enable_microphone && !stream.microphone_device.trim().is_empty();
    let devices = if system_selected || microphone_selected {
        enumerate_audio_devices().unwrap_or_else(|error| {
            tracing::warn!(%error, "Falha ao verificar os dispositivos de áudio");
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let available = |name: &str| devices.iter().any(|device| device.name == name);
    let enabled = system_selected && available(&settings.audio_device);
    let microphone_enabled = microphone_selected && available(&stream.microphone_device);
    let mut notices = Vec::new();
    if stream.audio_mode == "process" {
        notices.push(
            "O áudio por processo está indisponível. Usando o áudio do sistema quando disponível.",
        );
    }
    if wants_system && !enabled {
        notices.push("Dispositivo de áudio do sistema indisponível. Confira as configurações.");
    }
    if stream.enable_microphone && !microphone_enabled {
        notices.push("Microfone indisponível. Confira as configurações.");
    }
    let notice = (!notices.is_empty()).then(|| notices.join(" "));
    (
        AudioConfig {
            enabled,
            microphone_enabled,
            device: settings.audio_device.clone(),
            microphone_device: stream.microphone_device,
            bitrate: settings.audio_bitrate,
            buffer_size: settings.audio_buffer_size,
            offset_ms: settings.audio_offset,
            audio_filters: stream.audio_effects.to_ffmpeg_filter(),
            microphone_filters: stream.microphone_effects.to_ffmpeg_filter(),
        },
        notice,
    )
}

fn encoder_error(hls: &HlsConfig, message: &str) -> String {
    let details = (|| {
        let mut file = std::fs::File::open(format!("{}/ffmpeg.log", hls.stream_dir())).ok()?;
        let offset = file.metadata().ok()?.len().saturating_sub(4096);
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut bytes = Vec::new();
        file.take(4096).read_to_end(&mut bytes).ok()?;
        let log = String::from_utf8_lossy(&bytes);
        let detail = log.lines().rev().find(|line| !line.trim().is_empty())?;
        Some(detail.trim().to_string())
    })();
    details.map_or_else(
        || message.to_string(),
        |detail| format!("{message} {detail}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{playlist_is_served, start_server};

    #[test]
    fn synthetic_frames_generate_hls_over_http_and_stop() {
        let directory =
            std::env::temp_dir().join(format!("hipocast-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let result = (|| -> Result<(), CaptureError> {
            let settings = AppSettings {
                width: 320,
                height: 180,
                framerate: 15,
                enable_hw_accel: false,
                ..AppSettings::default()
            };
            let hls = HlsConfig::new(&directory.to_string_lossy(), "synthetic", 3, 1.0);
            let port = start_server(0, directory.clone())?;
            let route = hls.playlist_route();
            assert!(!playlist_is_served(port, &route));
            let stream_audio = StreamAudioConfig {
                enable_microphone: true,
                ..StreamAudioConfig::default()
            };
            let (audio, notice) = resolve_audio(&settings, stream_audio);
            assert!(!audio.enabled && !audio.microphone_enabled);
            assert!(notice.is_some());
            let mut encoder = FfmpegEncoder::new(&settings, audio, &hls)?;
            let input = encoder.take_input().ok_or("FFmpeg stdin unavailable")?;
            let frames = Arc::new(Frames::default());
            frames.state.lock().unwrap().latest = Some(FrameData {
                data: vec![127; (settings.width * settings.height * 4) as usize],
                width: settings.width,
                height: settings.height,
            });
            let test_frames = frames.clone();
            let writer = thread::spawn(move || {
                write_frames(
                    input,
                    &test_frames,
                    settings.width,
                    settings.height,
                    settings.framerate,
                )
            });
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut ready = false;
            while Instant::now() < deadline && !writer.is_finished() {
                if hls.playlist_is_ready() && playlist_is_served(port, &route) {
                    ready = true;
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }

            frames.close();
            encoder.stop()?;
            let _ = writer.join().map_err(|_| "Video writer panicked")?;
            if !ready {
                return Err(encoder_error(&hls, "HLS não ficou disponível no teste.").into());
            }
            assert!(encoder.try_wait()?.is_some());
            Ok(())
        })();
        std::fs::remove_dir_all(directory).unwrap();
        result.unwrap();
    }
}
