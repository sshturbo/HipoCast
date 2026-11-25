use crate::encoder::{enumerate_audio_devices, AudioConfig};
use crate::ffmpeg::FfmpegEncoder;
use crate::gpu_scale::GpuScaler;
use crate::output::HlsConfig;
use crate::stream_config::StreamAudioConfig;
use crate::AppSettings;
use fast_image_resize::{images::Image, PixelType, ResizeAlg, ResizeOptions, Resizer};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
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
    output_size: (u32, u32),
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
            readback: FrameReadback::new(ctx.device, ctx.device_context, ctx.flags.output_size),
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
            if width == 0
                || height == 0
                || !accept_frame(&mut self.next_frame, Instant::now(), self.interval)
            {
                return Ok(());
            }
            std::mem::take(&mut state.spare_pixels)
        };
        // The capture callback owns the frame for the duration of this copy.
        let (width, height) = self.readback.copy_for_output(
            unsafe { frame.as_raw_texture() },
            width,
            height,
            &mut pixels,
        )?;
        let mut state = self.frames.state.lock().unwrap();
        if let Some(skipped) = state.latest.replace(FrameData {
            data: pixels,
            width,
            height,
        }) {
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
    output_size: (u32, u32),
    gpu_scaler: Option<GpuScaler>,
    gpu_scaling_supported: bool,
}

impl FrameReadback {
    fn new(device: ID3D11Device, context: ID3D11DeviceContext, output_size: (u32, u32)) -> Self {
        Self {
            device,
            context,
            texture: None,
            output_size,
            gpu_scaler: None,
            gpu_scaling_supported: true,
        }
    }

    fn copy_for_output(
        &mut self,
        source: &ID3D11Texture2D,
        width: u32,
        height: u32,
        pixels: &mut Vec<u8>,
    ) -> Result<(u32, u32), CaptureError> {
        let (output_width, output_height) = self.output_size;
        let downscale =
            output_width <= width && output_height <= height && self.output_size != (width, height);
        if self.gpu_scaling_supported && downscale {
            let result = (|| {
                if self.gpu_scaler.is_none() {
                    self.gpu_scaler = Some(GpuScaler::new(&self.device)?);
                }
                let scaled = self.gpu_scaler.as_mut().unwrap().resize(
                    &self.device,
                    &self.context,
                    source,
                    (width, height),
                    self.output_size,
                )?;
                self.copy(&scaled, output_width, output_height, pixels)
            })();
            match result {
                Ok(()) => return Ok(self.output_size),
                Err(error) => {
                    tracing::warn!(%error, "Escala Direct3D indisponível, usando escala por CPU");
                    self.gpu_scaler = None;
                    self.gpu_scaling_supported = false;
                }
            }
        }
        self.copy(source, width, height, pixels)?;
        Ok((width, height))
    }

    fn copy(
        &mut self,
        source: &ID3D11Texture2D,
        width: u32,
        height: u32,
        pixels: &mut Vec<u8>,
    ) -> Result<(), CaptureError> {
        if self
            .texture
            .as_ref()
            .is_none_or(|(w, h, _)| (*w, *h) != (width, height))
        {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            };
            let mut texture = None;
            unsafe {
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut texture))?
            };
            self.texture = Some((
                width,
                height,
                texture.ok_or("Direct3D não criou a textura de leitura.")?,
            ));
        }
        let (_, _, texture) = self.texture.as_ref().unwrap();
        let mut mapped_resource = D3D11_MAPPED_SUBRESOURCE::default();
        // Both textures belong to the capture device and use the session's RGBA8 format.
        unsafe {
            self.context.CopyResource(texture, source);
            self.context
                .Map(texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped_resource))?;
        }
        let mapped = MappedFrame {
            context: &self.context,
            texture,
            data: mapped_resource,
        };

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
                pixels.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(data.add(row * stride), row_size)
                });
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
        unsafe { self.context.Unmap(self.texture, 0) };
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
    let (mut audio, notice) = resolve_audio(settings, stream_audio);
    let mut notice = notice;
    loop {
        if frames.state.lock().unwrap().stopping {
            return Ok(());
        }
        if notice.is_some() {
            update("starting", notice.clone());
        }
        let outcome = run_capture_attempt(
            source_id, settings, &audio, hls, hls_port, frames, &notice, update,
        );
        if let Err(error) = &outcome {
            if (audio.enabled || audio.microphone_enabled) && error.retry_without_audio {
                {
                    let mut state = frames.state.lock().unwrap();
                    if state.stopping {
                        return Ok(());
                    }
                    state.source_closed = false;
                    if let Some(frame) = state.latest.take() {
                        state.recycle_pixels(frame.data);
                    }
                }
                tracing::warn!(stream = hls.stream_id(), error = %error.message, "Reiniciando sem áudio após falha do FFmpeg");
                audio.enabled = false;
                audio.microphone_enabled = false;
                notice = Some(
                    "Falha no áudio do FFmpeg. A transmissão foi retomada em silêncio.".into(),
                );
                continue;
            }
        }
        return outcome.map_err(|error| error.message);
    }
}

struct AttemptError {
    message: String,
    retry_without_audio: bool,
}

impl From<String> for AttemptError {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_without_audio: false,
        }
    }
}

fn run_capture_attempt(
    source_id: &str,
    settings: &AppSettings,
    audio: &AudioConfig,
    hls: &HlsConfig,
    hls_port: u16,
    frames: &Arc<Frames>,
    notice: &Option<String>,
    update: &impl Fn(&'static str, Option<String>),
) -> Result<(), AttemptError> {
    if frames.state.lock().unwrap().stopping {
        return Ok(());
    }

    let mut encoder = FfmpegEncoder::new(settings, audio.clone(), hls)
        .map_err(|error| format!("Não foi possível iniciar o FFmpeg: {error}"))?;
    let input = encoder
        .take_input()
        .ok_or_else(|| "O FFmpeg não disponibilizou a entrada de vídeo.".to_string())?;
    if frames.state.lock().unwrap().stopping {
        return Ok(());
    }
    let capture = start_source(
        source_id,
        frames.clone(),
        settings.framerate,
        (settings.width, settings.height),
    )
    .map_err(|error| format!("Não foi possível abrir a fonte de captura: {error}"))?;

    let writer_frames = frames.clone();
    let (width, height, framerate) = (settings.width, settings.height, settings.framerate);
    let writer =
        thread::spawn(move || write_frames(input, &writer_frames, width, height, framerate));

    let route = hls.playlist_route();
    let timeout = Duration::from_secs_f64(12.0 + settings.hls_time * 2.0);
    let started = Instant::now();
    let mut last_progress = started;
    let mut published = false;
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
                    encoder.log_path(),
                    &format!("O FFmpeg encerrou ({status})."),
                ));
            }
            Err(error) => break Err(format!("Falha ao verificar o FFmpeg: {error}")),
            Ok(None) => {}
        }
        if writer.is_finished() {
            break Err(encoder_error(
                encoder.log_path(),
                "O envio de vídeo ao FFmpeg foi interrompido.",
            ));
        }
        match encoder.publish_playlist() {
            Ok(true) => {
                published = true;
                last_progress = Instant::now();
            }
            Ok(false) => {}
            Err(error) => break Err(format!("Falha ao publicar a playlist HLS: {error}")),
        }
        if !ready && published && crate::server::playlist_is_served(hls_port, &route) {
            ready = true;
            last_progress = Instant::now();
            update("running", notice.clone());
        }
        let elapsed = if ready {
            last_progress.elapsed()
        } else {
            started.elapsed()
        };
        if elapsed >= timeout {
            let message = if ready {
                "A playlist HLS parou de receber novos segmentos."
            } else {
                "O FFmpeg não gerou a playlist HLS a tempo."
            };
            break Err(encoder_error(encoder.log_path(), message));
        }
        thread::sleep(Duration::from_millis(100));
    };

    frames.state.lock().unwrap().source_closed = true;
    frames.available.notify_all();
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
    outcome
        .and(encoder_result)
        .and(capture_result)
        .map_err(|message| AttemptError {
            message,
            retry_without_audio: audio_buffer_failed(&read_encoder_log(encoder.log_path())),
        })
}

fn start_source(
    source_id: &str,
    frames: Arc<Frames>,
    framerate: u32,
    output_size: (u32, u32),
) -> Result<CaptureControl<CaptureHandler, CaptureError>, CaptureError> {
    let (kind, value) = source_id
        .split_once(':')
        .ok_or("Fonte de captura inválida.")?;
    let value = value.parse::<usize>()?;
    let interval = Duration::from_secs_f64(1.0 / f64::from(framerate));
    let native_interval =
        if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
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
                CaptureFlags {
                    frames,
                    interval,
                    output_size,
                },
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
                frames
                    .state
                    .lock()
                    .unwrap()
                    .recycle_pixels(source.into_vec());
                target.into_vec()
            };
            previous = Some(data);
        }
        if let Some(data) = &previous {
            input.write_all(data)?;
        }
        // Skip missed capture slots without changing the timestamps generated at pipe ingress.
        let now = Instant::now();
        next_frame = (next_frame + interval).max(now);
    }
}

fn resolve_audio(
    settings: &AppSettings,
    stream: StreamAudioConfig,
) -> (AudioConfig, Option<String>) {
    let wants_system = matches!(stream.audio_mode.as_str(), "system" | "process");
    let wants_microphone = stream.audio_mode != "silent" && stream.enable_microphone;
    let system_selected = wants_system && !settings.audio_device.trim().is_empty();
    let microphone_selected = wants_microphone && !stream.microphone_device.trim().is_empty();
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
    if wants_microphone && !microphone_enabled {
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

fn read_encoder_log(path: &Path) -> String {
    (|| {
        let mut file = std::fs::File::open(path).ok()?;
        let offset = file.metadata().ok()?.len().saturating_sub(4096);
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut bytes = Vec::new();
        file.take(4096).read_to_end(&mut bytes).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    })()
    .unwrap_or_default()
}

fn audio_buffer_failed(log: &str) -> bool {
    let audio_failure = log.lines().any(|line| {
        (line.starts_with("[af#") || line.starts_with("[fc#"))
            && line.contains("No space left on device")
    });
    let file_failure = log.lines().any(|line| {
        (line.starts_with("[hls ") || line.starts_with("[out#"))
            && (line.contains("No space left on device") || line.contains("Failed to open"))
    });
    audio_failure && !file_failure
}

fn encoder_error(path: &Path, message: &str) -> String {
    let log = read_encoder_log(path);
    let details = log
        .lines()
        .rev()
        .find(|line| {
            !line.trim().is_empty()
                && !line.contains("Terminating thread")
                && !line.contains("Task finished")
        })
        .or_else(|| log.lines().next_back());
    details.map_or_else(
        || message.to_string(),
        |detail| format!("{message} {}", detail.trim()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{playlist_is_served, start_server};
    use windows::Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_WARP,
        Direct3D11::{
            D3D11CreateDevice, D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
        },
        Dxgi::IDXGIAdapter,
    };

    #[test]
    fn one_hundred_twenty_hz_source_is_sampled_at_thirty_fps() {
        let start = Instant::now();
        let mut next_frame = None;
        let interval = Duration::from_secs_f64(1.0 / 30.0);
        let accepted = (0..120)
            .filter(|frame| {
                accept_frame(
                    &mut next_frame,
                    start + Duration::from_secs_f64(f64::from(*frame) / 120.0),
                    interval,
                )
            })
            .count();
        assert_eq!(accepted, 30);
    }

    fn warp_readback(output_size: (u32, u32)) -> Result<FrameReadback, CaptureError> {
        let (mut device, mut context) = (None, None);
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_WARP,
                Default::default(),
                D3D11_CREATE_DEVICE_FLAG::default(),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        Ok(FrameReadback::new(
            device.ok_or("No D3D11 device")?,
            context.ok_or("No D3D11 context")?,
            output_size,
        ))
    }

    fn source_texture(
        device: &ID3D11Device,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<ID3D11Texture2D, CaptureError> {
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: width * 4,
            SysMemSlicePitch: 0,
        };
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        let mut source = None;
        // The data has exactly width * height initialized RGBA pixels.
        unsafe { device.CreateTexture2D(&desc, Some(&data), Some(&mut source))? };
        source.ok_or_else(|| "No source texture".into())
    }

    #[test]
    fn d3d_readback_reuses_texture_and_packs_pixels_after_resize() -> Result<(), CaptureError> {
        let mut readback = warp_readback((64, 64))?;
        let mut pixels = Vec::new();
        let mut previous_texture = None;
        for (width, height, value) in [(13, 9, 64_u8), (13, 9, 123), (29, 7, 220)] {
            let expected: Vec<u8> = (0..width * height * 4)
                .map(|index| value.wrapping_add(index as u8))
                .collect();
            let source = source_texture(&readback.device, width, height, &expected)?;
            readback.copy(&source, width, height, &mut pixels)?;
            assert_eq!(pixels, expected);
            let (_, _, texture) = readback.texture.as_ref().unwrap();
            if value == 123 {
                assert_eq!(previous_texture.as_ref(), Some(texture));
            }
            if value == 220 {
                assert_ne!(previous_texture.as_ref(), Some(texture));
            }
            previous_texture = Some(texture.clone());
        }
        Ok(())
    }

    #[test]
    fn d3d_downscales_before_readback_and_matches_cpu_pixels() -> Result<(), CaptureError> {
        let (output_width, output_height) = (5, 4);
        let mut readback = warp_readback((output_width, output_height))?;
        let mut pixels = Vec::new();
        let mut staging_texture = None;

        for (width, height, value) in [(13, 9, 63_u8), (13, 9, 180), (29, 7, 236)] {
            let original: Vec<u8> = (0..width * height * 4)
                .map(|index| value.wrapping_add(index as u8))
                .collect();
            let source = source_texture(&readback.device, width, height, &original)?;
            assert_eq!(
                readback.copy_for_output(&source, width, height, &mut pixels)?,
                (output_width, output_height)
            );

            let mut expected = Image::new(output_width, output_height, PixelType::U8x4);
            let input = Image::from_vec_u8(width, height, original, PixelType::U8x4)?;
            let options = ResizeOptions::new().resize_alg(ResizeAlg::Nearest);
            Resizer::new().resize(&input, &mut expected, &options)?;
            assert_eq!(pixels, expected.buffer());

            let (staging_width, staging_height, texture) = readback.texture.as_ref().unwrap();
            assert_eq!(
                (*staging_width, *staging_height),
                (output_width, output_height)
            );
            if let Some(previous) = &staging_texture {
                assert_eq!(texture, previous);
            }
            staging_texture = Some(texture.clone());
        }

        let original = vec![127; 13 * 9 * 4];
        let source = source_texture(&readback.device, 13, 9, &original)?;
        readback.gpu_scaling_supported = false;
        assert_eq!(
            readback.copy_for_output(&source, 13, 9, &mut pixels)?,
            (13, 9)
        );
        assert_eq!(pixels, original);
        Ok(())
    }

    #[test]
    fn ffmpeg_audio_queue_error_can_retry_but_hls_write_error_cannot() {
        let audio_log = "[af#0:0 @ x] Error sending frames to consumers: No space left on device";
        let file_log = "[hls @ x] Failed to open file: No space left on device";
        assert!(audio_buffer_failed(audio_log));
        assert!(!audio_buffer_failed(file_log));
        assert!(!audio_buffer_failed(&format!("{file_log}\n{audio_log}")));
    }

    #[test]
    fn silent_capture_ignores_stale_microphone_flags() {
        let settings = AppSettings {
            audio_device: "unused loopback".into(),
            ..AppSettings::default()
        };
        let stream = StreamAudioConfig {
            audio_mode: "silent".into(),
            enable_microphone: true,
            microphone_device: "unused microphone".into(),
            ..StreamAudioConfig::default()
        };
        let (audio, notice) = resolve_audio(&settings, stream);
        assert!(!audio.enabled && !audio.microphone_enabled);
        assert!(notice.is_none());
    }

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
            let mut published = false;
            while Instant::now() < deadline && !writer.is_finished() {
                published |= encoder.publish_playlist()?;
                if published && playlist_is_served(port, &route) {
                    ready = true;
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }

            frames.close();
            encoder.stop()?;
            let _ = writer.join().map_err(|_| "Video writer panicked")?;
            if !ready {
                return Err(encoder_error(
                    encoder.log_path(),
                    "HLS não ficou disponível no teste.",
                )
                .into());
            }
            assert!(encoder.try_wait()?.is_some());
            Ok(())
        })();
        std::fs::remove_dir_all(directory).unwrap();
        result.unwrap();
    }

    #[test]
    fn slow_video_input_keeps_wall_clock_duration() -> Result<(), CaptureError> {
        let directory =
            std::env::temp_dir().join(format!("hipocast-clock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory)?;
        let result = (|| -> Result<bool, CaptureError> {
            let settings = AppSettings {
                width: 160,
                height: 90,
                framerate: 30,
                enable_hw_accel: false,
                ..AppSettings::default()
            };
            let hls = HlsConfig::new(&directory.to_string_lossy(), "slow", 3, 0.5);
            let mut encoder = FfmpegEncoder::new(&settings, AudioConfig::default(), &hls)?;
            let mut input = encoder.take_input().ok_or("FFmpeg stdin unavailable")?;
            let pixels = vec![127; (settings.width * settings.height * 4) as usize];
            let start = Instant::now();
            for index in 0..14 {
                let at = start + Duration::from_millis(100 * index);
                thread::sleep(at.saturating_duration_since(Instant::now()));
                input.write_all(&pixels)?;
            }
            drop(input);
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline && encoder.try_wait()?.is_none() {
                thread::sleep(Duration::from_millis(10));
            }
            encoder.publish_playlist()?;
            let playlist = std::fs::read_to_string(hls.playlist_path())?;
            let duration: f64 = playlist
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("#EXTINF:")?
                        .split(',')
                        .next()?
                        .parse::<f64>()
                        .ok()
                })
                .sum();
            Ok(duration > 0.9)
        })();
        std::fs::remove_dir_all(directory)?;
        assert!(
            result?,
            "14 frames at 10 FPS must not be compressed to the nominal 30 FPS timeline"
        );
        Ok(())
    }
}
