use std::collections::VecDeque;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING,
    ERROR_PIPE_NOT_CONNECTED, HANDLE,
};
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVE_FORMAT_PCM,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::Storage::FileSystem::{
    WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_OUTBOUND,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_NOWAIT, PIPE_REJECT_REMOTE_CLIENTS,
};

pub(crate) const DEFAULT_OUTPUT: &str = "wasapi:default";
const POLL: Duration = Duration::from_millis(4);
const PREBUFFER: Duration = Duration::from_millis(30);
const PIPE_BUFFER: u32 = 65_536;
type AudioError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone)]
pub(crate) struct LoopbackInput {
    pub(crate) path: String,
    pub(crate) demuxer: &'static str,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
    pub(crate) channel_mask: u32,
}

pub(crate) fn has_default_output() -> bool {
    let probe = || -> Result<(), AudioError> {
        let _apartment = Apartment::enter()?;
        default_output()?;
        Ok(())
    };
    probe().is_ok()
}

pub(crate) struct Loopback {
    input: LoopbackInput,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    failure: mpsc::Receiver<String>,
}

impl Loopback {
    pub(crate) fn start() -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (failure_tx, failure) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("hipocast-wasapi".into())
            .spawn(move || {
                let setup = (|| -> Result<_, AudioError> {
                    let receiver = Receiver::open()?;
                    let pipe = AudioPipe::create()?;
                    unsafe { receiver.client.Start()? };
                    Ok((receiver, pipe))
                })();
                match setup {
                    Ok((receiver, pipe)) => {
                        if ready_tx
                            .send(Ok(receiver.format.input(pipe.path.clone())))
                            .is_ok()
                        {
                            if let Err(error) = receiver.capture(&pipe, &worker_stop) {
                                let _ = failure_tx.send(error.to_string());
                            }
                        }
                        let _ = unsafe { receiver.client.Stop() };
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        let input = match ready_rx.recv_timeout(Duration::from_secs(4)) {
            Ok(Ok(input)) => input,
            Ok(Err(error)) => return Err(error),
            Err(error) => {
                stop.store(true, Ordering::Relaxed);
                return Err(format!("A saída padrão do Windows não respondeu: {error}"));
            }
        };
        Ok(Self {
            input,
            stop,
            worker: Some(worker),
            failure,
        })
    }

    pub(crate) fn input(&self) -> LoopbackInput {
        self.input.clone()
    }

    pub(crate) fn take_failure(&self) -> Option<String> {
        self.failure.try_recv().ok()
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Apartment;

impl Apartment {
    fn enter() -> windows::core::Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn default_output() -> windows::core::Result<IMMDevice> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)
    }
}

struct MixFormat {
    demuxer: &'static str,
    sample_rate: u32,
    channels: u16,
    channel_mask: u32,
    frame_bytes: usize,
    silence: u8,
}

impl MixFormat {
    fn from_wave_format(
        format: &WAVEFORMATEX,
        extension: Option<&WAVEFORMATEXTENSIBLE>,
    ) -> Result<Self, AudioError> {
        const PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
        const FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
        let tag = u32::from(format.wFormatTag);
        let (tag, mask) = match extension.map(|ext| (ext.SubFormat, ext.dwChannelMask)) {
            Some((PCM, mask)) => (WAVE_FORMAT_PCM, mask),
            Some((FLOAT, mask)) => (WAVE_FORMAT_IEEE_FLOAT, mask),
            _ => (tag, 0),
        };
        let (demuxer, bytes_per_sample, silence) = match (tag, format.wBitsPerSample) {
            (WAVE_FORMAT_PCM, 8) => ("u8", 1, 128),
            (WAVE_FORMAT_PCM, 16) => ("s16le", 2, 0),
            (WAVE_FORMAT_PCM, 24) => ("s24le", 3, 0),
            (WAVE_FORMAT_PCM, 32) => ("s32le", 4, 0),
            (WAVE_FORMAT_IEEE_FLOAT, 32) => ("f32le", 4, 0),
            (WAVE_FORMAT_IEEE_FLOAT, 64) => ("f64le", 8, 0),
            _ => {
                return Err(
                    "O formato de áudio da saída padrão do Windows não é compatível.".into(),
                )
            }
        };
        let channels = format.nChannels;
        let sample_rate = format.nSamplesPerSec;
        let frame_bytes = usize::from(format.nBlockAlign);
        if channels == 0
            || sample_rate == 0
            || frame_bytes != bytes_per_sample * usize::from(channels)
        {
            return Err("O Windows retornou um formato de áudio inválido.".into());
        }
        Ok(Self {
            demuxer,
            sample_rate,
            channels,
            channel_mask: if mask.count_ones() == u32::from(channels) {
                mask
            } else {
                0
            },
            frame_bytes,
            silence,
        })
    }

    fn input(&self, path: String) -> LoopbackInput {
        LoopbackInput {
            path,
            demuxer: self.demuxer,
            sample_rate: self.sample_rate,
            channels: self.channels,
            channel_mask: self.channel_mask,
        }
    }
}

struct Receiver {
    capture: IAudioCaptureClient,
    client: IAudioClient,
    format: MixFormat,
    _apartment: Apartment,
}

impl Receiver {
    fn open() -> Result<Self, AudioError> {
        let apartment = Apartment::enter()?;
        let device = default_output()?;
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
        let wave_ptr = unsafe { client.GetMixFormat()? };
        let Some(wave) = (unsafe { wave_ptr.as_ref() }) else {
            return Err("O Windows não informou o formato da saída padrão.".into());
        };
        let initialized = (|| -> Result<MixFormat, AudioError> {
            const EXTENSIBLE: u16 = 0xfffe;
            let extension = if wave.wFormatTag == EXTENSIBLE && wave.cbSize >= 22 {
                // The allocated format has the declared 22 extension bytes after WAVEFORMATEX.
                Some(unsafe { &*wave_ptr.cast::<WAVEFORMATEXTENSIBLE>() })
            } else {
                None
            };
            let format = MixFormat::from_wave_format(wave, extension)?;
            unsafe {
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    0,
                    0,
                    wave_ptr,
                    None,
                )?
            };
            Ok(format)
        })();
        unsafe { CoTaskMemFree(Some(wave_ptr.cast())) };
        let format = initialized?;
        let capture = unsafe { client.GetService()? };
        Ok(Self {
            capture,
            client,
            format,
            _apartment: apartment,
        })
    }

    fn capture(&self, pipe: &AudioPipe, stop: &AtomicBool) -> Result<(), AudioError> {
        if !pipe.wait_for_reader(stop)? {
            return Ok(());
        }
        let mut queue = VecDeque::new();
        // Discard packets captured while FFmpeg was opening the input.
        self.read_pending(&mut queue)?;
        queue.clear();
        let start = Instant::now();
        let rate = u128::from(self.format.sample_rate);
        let max_block = u128::from((self.format.sample_rate / 50).max(1));
        let max_queue = ((usize::try_from(self.format.sample_rate)? / 4) * self.format.frame_bytes)
            .max(self.format.frame_bytes);
        let mut sent = 0;
        let mut block = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            self.read_pending(&mut queue)?;
            let overdue = queue.len().saturating_sub(max_queue);
            if overdue > 0 {
                queue.drain(..overdue);
            }
            // Some drivers stop delivering packets while the endpoint is silent.
            // Pace PCM by elapsed time so the FFmpeg input clock continues to advance.
            let expected =
                start.elapsed().saturating_sub(PREBUFFER).as_nanos() * rate / 1_000_000_000;
            let due = expected.saturating_sub(sent).min(max_block);
            if due > 0 {
                let length = due as usize * self.format.frame_bytes;
                block.clear();
                block.extend(queue.drain(..length.min(queue.len())));
                block.resize(length, self.format.silence);
                if !pipe.write(&block, stop)? {
                    return Ok(());
                }
                sent += due;
            } else {
                thread::sleep(POLL);
            }
        }
        Ok(())
    }

    fn read_pending(&self, queue: &mut VecDeque<u8>) -> Result<(), AudioError> {
        while unsafe { self.capture.GetNextPacketSize()? } != 0 {
            let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
            unsafe {
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?
            };
            if frames == 0 {
                break;
            }
            let length = frames as usize * self.format.frame_bytes;
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
            if silent {
                queue.resize(queue.len() + length, self.format.silence);
            } else if !data.is_null() {
                // WASAPI keeps the whole packet valid until ReleaseBuffer.
                queue.extend(unsafe { std::slice::from_raw_parts(data, length) });
            }
            unsafe { self.capture.ReleaseBuffer(frames)? };
            if !silent && data.is_null() {
                return Err("O Windows retornou um pacote de áudio vazio.".into());
            }
        }
        Ok(())
    }
}

struct AudioPipe {
    handle: OwnedHandle,
    path: String,
}

impl AudioPipe {
    fn create() -> io::Result<Self> {
        let path = format!(r"\\.\pipe\hipocast-wasapi-{}", uuid::Uuid::new_v4());
        let handle = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(&path),
                PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PIPE_BUFFER,
                0,
                0,
                None,
            )
        };
        if handle.is_invalid() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: unsafe { OwnedHandle::from_raw_handle(handle.0) },
            path,
        })
    }

    fn wait_for_reader(&self, stop: &AtomicBool) -> windows::core::Result<bool> {
        while !stop.load(Ordering::Relaxed) {
            match unsafe { ConnectNamedPipe(HANDLE(self.handle.as_raw_handle()), None) } {
                Ok(()) => return Ok(true),
                Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(true),
                Err(error) if error.code() == ERROR_PIPE_LISTENING.to_hresult() => {
                    thread::sleep(POLL)
                }
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }

    fn write(&self, mut data: &[u8], stop: &AtomicBool) -> windows::core::Result<bool> {
        while !data.is_empty() && !stop.load(Ordering::Relaxed) {
            let mut written = 0;
            let result = unsafe {
                WriteFile(
                    HANDLE(self.handle.as_raw_handle()),
                    Some(data),
                    Some(&mut written),
                    None,
                )
            };
            if let Err(error) = result {
                if [ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED]
                    .iter()
                    .any(|code| error.code() == code.to_hresult())
                {
                    return Ok(false);
                }
                return Err(error);
            }
            if written == 0 {
                thread::sleep(POLL);
            } else {
                data = &data[written as usize..];
            }
        }
        Ok(data.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::{build_audio_input_args, get_ffmpeg_path, AudioConfig};
    use std::process::{Command, Stdio};

    #[test]
    fn ffmpeg_receives_pcm_from_the_loopback_pipe() -> Result<(), AudioError> {
        let pipe = AudioPipe::create()?;
        let config = AudioConfig {
            enabled: true,
            system_loopback: Some(LoopbackInput {
                path: pipe.path.clone(),
                demuxer: "s16le",
                sample_rate: 48_000,
                channels: 2,
                channel_mask: 3,
            }),
            ..AudioConfig::default()
        };
        let mut ffmpeg = Command::new(get_ffmpeg_path())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(build_audio_input_args(&config))
            .args(["-c:a", "copy", "-f", "s16le", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = stop.clone();
        let pcm = [0x34, 0x12, 0xcc, 0xed].repeat(480);
        let expected = pcm.clone();
        let writer = thread::spawn(move || -> windows::core::Result<bool> {
            Ok(pipe.wait_for_reader(&writer_stop)? && pipe.write(&pcm, &writer_stop)?)
        });
        let deadline = Instant::now() + Duration::from_secs(8);
        while ffmpeg.try_wait()?.is_none() && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        if ffmpeg.try_wait()?.is_none() {
            ffmpeg.kill()?;
        }
        stop.store(true, Ordering::Relaxed);
        let received = ffmpeg.wait_with_output()?;
        let sent = writer.join().expect("test pipe writer panicked")?;
        assert!(
            received.status.success(),
            "{}",
            String::from_utf8_lossy(&received.stderr)
        );
        assert!(sent);
        assert_eq!(received.stdout, expected);
        Ok(())
    }

    #[test]
    #[ignore = "requires an active Windows playback endpoint"]
    fn opens_default_windows_audio_output() -> Result<(), AudioError> {
        let receiver = Receiver::open()?;
        let format = receiver.format;
        println!(
            "{} Hz, {} canais, {}",
            format.sample_rate, format.channels, format.demuxer
        );
        Ok(())
    }
}
