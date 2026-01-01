use windows::Win32::Media::Audio::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Pipes::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Threading::*;
use windows::core::{Result, PCSTR, PWSTR, Interface};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::ffi::CString;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSession {
    pub process_id: u32,
    pub process_name: String,
    pub display_name: String,
    pub icon_path: String,
}

pub struct AudioCapture {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

pub struct MicrophoneCapture {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl AudioCapture {
    pub fn new(stream_id: String, target_pid: Option<u32>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            unsafe {
                // Initialize COM
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

                // Create Named Pipe
                let pipe_name = format!("\\\\.\\pipe\\live_go_audio_{}", stream_id);
                let c_pipe_name = CString::new(pipe_name.clone()).unwrap();
                let pc_pipe_name = PCSTR(c_pipe_name.as_ptr() as *const u8);

                println!("🎧 Creating Audio Pipe: {}", pipe_name);

                let pipe_handle = CreateNamedPipeA(
                    pc_pipe_name,
                    PIPE_ACCESS_OUTBOUND,
                    PIPE_TYPE_BYTE | PIPE_WAIT,
                    1, // Max instances
                    262144, // Out buffer (256KB - aumentado de 64KB)
                    262144, // In buffer (256KB - aumentado de 64KB)
                    0, // Default timeout
                    None,
                ).expect("Failed to create named pipe");

                if pipe_handle == INVALID_HANDLE_VALUE {
                    eprintln!("❌ Failed to create named pipe");
                    return;
                }

                // Wait for FFmpeg to connect
                println!("⏳ Waiting for FFmpeg to connect to audio pipe...");
                if ConnectNamedPipe(pipe_handle, None).is_err() {
                     let err = windows::core::Error::from_win32();
                     // ERROR_PIPE_CONNECTED is actually success for existing connection
                     if err.code() != windows::Win32::Foundation::ERROR_PIPE_CONNECTED.into() {
                         eprintln!("❌ ConnectNamedPipe failed: {:?}", err);
                         let _ = CloseHandle(pipe_handle);
                         return;
                     }
                }
                println!("✅ FFmpeg connected to audio pipe!");

                // WASAPI Loopback Setup
                let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).unwrap();
                
                // Activate Audio Client (IAudioClient2 for process-specific capture if needed)
                let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None).unwrap();
                
                // If target_pid is specified, try to use process loopback (Windows 10+)
                if let Some(pid) = target_pid {
                    // Note: Process-specific loopback requires IAudioClient2 and SetClientProperties
                    // For now, we'll use system loopback and filter in post-processing
                    // Full implementation would require AudioClientProperties with process ID
                    println!("🎯 Target PID: {} (system-wide loopback for now)", pid);
                }

                // Check mix format
                let format_ptr = audio_client.GetMixFormat().unwrap();
                let format = *format_ptr;
                
                // Initialize WASAPI in Loopback Mode
                audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK, // Loopback flag
                    20_000_000, // 2 second buffer (increased for stability)
                    0,
                    format_ptr,
                    None,
                ).unwrap();

                let capture_client: IAudioCaptureClient = audio_client.GetService().unwrap();
                audio_client.Start().unwrap();
                
                let channels = format.nChannels;
                let sample_rate = format.nSamplesPerSec;
                println!("🎙️ WASAPI Loopback Started. Fmt: {}ch {}Hz", channels, sample_rate);

                // Calculate silence buffer size (10ms worth of audio)
                let silence_frames = (sample_rate / 100) as usize; // 10ms
                let bytes_per_frame = (channels * format.wBitsPerSample / 8) as usize;
                let _silence_buffer = vec![0u8; silence_frames * bytes_per_frame];

                while running_clone.load(Ordering::SeqCst) {
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let mut frames_available = 0;
                    let mut flags = 0;

                    if let Ok(next_packet_size) = capture_client.GetNextPacketSize() {
                         if next_packet_size > 0 {
                            if capture_client.GetBuffer(&mut data, &mut frames_available, &mut flags, None, None).is_ok() {
                                if frames_available > 0 {
                                    let total_bytes = frames_available as usize * bytes_per_frame;
                                    let mut write_success = false;
                                    
                                    // Check if buffer contains silence (AUDCLNT_BUFFERFLAGS_SILENT = 0x2)
                                    if flags & 0x2 != 0 {
                                        // Write silence
                                        let silence = vec![0u8; total_bytes];
                                        let mut written = 0;
                                        if WriteFile(pipe_handle, Some(&silence), Some(&mut written), None).is_ok() {
                                            if written as usize == total_bytes {
                                                let _ = FlushFileBuffers(pipe_handle);
                                                write_success = true;
                                            }
                                        }
                                    } else {
                                        // Write actual audio data
                                        let slice = std::slice::from_raw_parts(data, total_bytes);
                                        let mut written = 0;
                                        if WriteFile(pipe_handle, Some(slice), Some(&mut written), None).is_ok() {
                                            if written as usize == total_bytes {
                                                let _ = FlushFileBuffers(pipe_handle);
                                                write_success = true;
                                            }
                                        }
                                    }
                                    
                                    // Sempre liberar o buffer WASAPI mesmo se a escrita falhou
                                    // para evitar acúmulo de buffers e travamentos
                                    let _ = capture_client.ReleaseBuffer(frames_available);
                                    
                                    if !write_success {
                                        // Se falhou, pequeno sleep para evitar loop tight
                                        thread::sleep(std::time::Duration::from_millis(1));
                                    }
                                } else {
                                    let _ = capture_client.ReleaseBuffer(frames_available);
                                }
                            }
                         } else {
                             // No packet available, wait for data
                             thread::sleep(std::time::Duration::from_millis(1));
                         }
                    } else {
                        // Error getting packet size
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                }

                audio_client.Stop().unwrap();
                let _ = CloseHandle(pipe_handle);
                CoUninitialize();
                println!("🛑 Audio Capture Stopped");
            }
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl MicrophoneCapture {
    pub fn new(stream_id: String) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

                // Create Named Pipe for Microphone
                let pipe_name = format!("\\\\.\\pipe\\live_go_mic_{}", stream_id);
                let c_pipe_name = CString::new(pipe_name.clone()).unwrap();
                let pc_pipe_name = PCSTR(c_pipe_name.as_ptr() as *const u8);

                println!("🎤 Creating Microphone Pipe: {}", pipe_name);

                let pipe_handle = CreateNamedPipeA(
                    pc_pipe_name,
                    PIPE_ACCESS_OUTBOUND,
                    PIPE_TYPE_BYTE | PIPE_WAIT,
                    1,
                    262144, // Out buffer (256KB - aumentado de 64KB)
                    262144, // In buffer (256KB - aumentado de 64KB)
                    0,
                    None,
                ).expect("Failed to create microphone named pipe");

                if pipe_handle == INVALID_HANDLE_VALUE {
                    eprintln!("❌ Failed to create microphone named pipe");
                    return;
                }

                println!("⏳ Waiting for FFmpeg to connect to microphone pipe...");
                if ConnectNamedPipe(pipe_handle, None).is_err() {
                    let err = windows::core::Error::from_win32();
                    if err.code() != windows::Win32::Foundation::ERROR_PIPE_CONNECTED.into() {
                        eprintln!("❌ ConnectNamedPipe failed: {:?}", err);
                        let _ = CloseHandle(pipe_handle);
                        return;
                    }
                }
                println!("✅ FFmpeg connected to microphone pipe!");

                // WASAPI Capture Setup (Microphone Input)
                let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
                let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole).unwrap(); // eCapture for input!
                let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None).unwrap();

                let format_ptr = audio_client.GetMixFormat().unwrap();
                let format = *format_ptr;

                // Initialize WASAPI in Capture Mode (not loopback)
                audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    0, // No loopback flag
                    20_000_000, // 2 second buffer (increased for stability)
                    0,
                    format_ptr,
                    None,
                ).unwrap();

                let capture_client: IAudioCaptureClient = audio_client.GetService().unwrap();
                audio_client.Start().unwrap();

                let channels = format.nChannels;
                let sample_rate = format.nSamplesPerSec;
                println!("🎤 WASAPI Microphone Started. Fmt: {}ch {}Hz", channels, sample_rate);

                // Calculate silence buffer size (10ms worth of audio)
                let silence_frames = (sample_rate / 100) as usize; // 10ms
                let bytes_per_frame = (channels * format.wBitsPerSample / 8) as usize;
                let _silence_buffer = vec![0u8; silence_frames * bytes_per_frame];

                while running_clone.load(Ordering::SeqCst) {
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let mut frames_available = 0;
                    let mut flags = 0;

                    if let Ok(next_packet_size) = capture_client.GetNextPacketSize() {
                        if next_packet_size > 0 {
                            if capture_client.GetBuffer(&mut data, &mut frames_available, &mut flags, None, None).is_ok() {
                                if frames_available > 0 {
                                    let total_bytes = frames_available as usize * bytes_per_frame;
                                    let mut write_success = false;
                                    
                                    // Check if buffer contains silence (AUDCLNT_BUFFERFLAGS_SILENT = 0x2)
                                    if flags & 0x2 != 0 {
                                        // Write silence
                                        let silence = vec![0u8; total_bytes];
                                        let mut written = 0;
                                        if WriteFile(pipe_handle, Some(&silence), Some(&mut written), None).is_ok() {
                                            if written as usize == total_bytes {
                                                let _ = FlushFileBuffers(pipe_handle);
                                                write_success = true;
                                            }
                                        }
                                    } else {
                                        // Write actual audio data
                                        let slice = std::slice::from_raw_parts(data, total_bytes);
                                        let mut written = 0;
                                        if WriteFile(pipe_handle, Some(slice), Some(&mut written), None).is_ok() {
                                            if written as usize == total_bytes {
                                                let _ = FlushFileBuffers(pipe_handle);
                                                write_success = true;
                                            }
                                        }
                                    }
                                    
                                    // Sempre liberar o buffer WASAPI mesmo se a escrita falhou
                                    let _ = capture_client.ReleaseBuffer(frames_available);
                                    
                                    if !write_success {
                                        thread::sleep(std::time::Duration::from_millis(1));
                                    }
                                } else {
                                    let _ = capture_client.ReleaseBuffer(frames_available);
                                }
                            }
                        } else {
                            // No packet available, wait for data
                            thread::sleep(std::time::Duration::from_millis(1));
                        }
                    } else {
                        // Error getting packet size
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                }

                audio_client.Stop().unwrap();
                let _ = CloseHandle(pipe_handle);
                CoUninitialize();
                println!("🛑 Microphone Capture Stopped");
            }
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// List all active audio sessions (applications playing audio)
pub fn list_audio_sessions() -> Result<Vec<AudioSession>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
        
        let mut sessions = Vec::new();
        
        // Get default audio endpoint
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        
        // Get session manager
        let session_manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let session_enum = session_manager.GetSessionEnumerator()?;
        
        let count = session_enum.GetCount()?;
        
        for i in 0..count {
            if let Ok(session_control) = session_enum.GetSession(i) {
                let session2: IAudioSessionControl2 = session_control.cast()?;
                
                // Get process ID
                let process_id = session2.GetProcessId().unwrap_or(0);
                if process_id == 0 {
                    continue;
                }
                
                // Get display name
                let display_pwstr = session2.GetDisplayName().unwrap_or(PWSTR::null());
                let display_name = if !display_pwstr.is_null() {
                    display_pwstr.to_string().unwrap_or_default()
                } else {
                    String::new()
                };
                
                // Get icon path
                let icon_pwstr = session2.GetIconPath().unwrap_or(PWSTR::null());
                let icon_path = if !icon_pwstr.is_null() {
                    icon_pwstr.to_string().unwrap_or_default()
                } else {
                    String::new()
                };
                
                // Get process name from PID
                let process_name = get_process_name(process_id).unwrap_or_else(|| "Unknown".to_string());
                
                // Check if session has audio state
                let state = session2.GetState().unwrap_or(AudioSessionStateExpired);
                if state == AudioSessionStateActive || state == AudioSessionStateInactive {
                    sessions.push(AudioSession {
                        process_id,
                        process_name: process_name.clone(),
                        display_name: if display_name.is_empty() { process_name } else { display_name },
                        icon_path,
                    });
                }
            }
        }
        
        CoUninitialize();
        Ok(sessions)
    }
}

/// Get process name from PID
fn get_process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        
        let mut buffer = vec![0u16; 260];
        let mut size = buffer.len() as u32;
        
        if QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut size).is_ok() {
            let _ = CloseHandle(handle);
            let path = String::from_utf16_lossy(&buffer[..size as usize]);
            // Extract just the filename
            if let Some(name) = path.split('\\').last() {
                return Some(name.to_string());
            }
        }
        
        let _ = CloseHandle(handle);
        None
    }
}

pub fn list_input_devices() -> Result<Vec<String>> {
    // Stubbed for debugging build issues.
    // Native listing logic temporarily removed to isolate build errors.
    Ok(vec![
        "Mixagem estéreo (Realtek Audio)".to_string(), 
        "Microfone (Realtek Audio)".to_string(),
        "Default Input".to_string(),
    ])
}
