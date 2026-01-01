#![allow(dead_code)]

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::core::Result;
use std::ptr;
pub struct VideoEncoder {
    transform: IMFTransform,
    width: u32,
    height: u32,
    fps: u32,
    input_stream_id: u32,
    output_stream_id: u32,
}

impl VideoEncoder {
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self> {
        // Align to multiples of 16 for H.264 compatibility
        let aligned_width = (width + 15) & !15;
        let aligned_height = (height + 15) & !15;
        
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;

            // Find H.264 Encoder - try hardware first, then software
            let info = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_H264,
            };

            let mut pp_activate: *mut Option<IMFActivate> = ptr::null_mut();
            let mut count = 0;
            
            // Try synchronous encoders only (async encoders require different handling)
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
                None,
                Some(&info),
                &mut pp_activate,
                &mut count,
            )?;

            if count == 0 {
                return Err(windows::core::Error::new(E_FAIL, "No H.264 encoder found"));
            }

            let activate = (*pp_activate).as_ref().unwrap();
            let transform: IMFTransform = activate.ActivateObject()?;

            // Clean up activations
            let activations = std::slice::from_raw_parts_mut(pp_activate, count as usize);
            for act in activations {
                *act = None;
            }
            CoTaskMemFree(Some(pp_activate as *const _));

            // Set Output Type first (H.264)
            let out_type = MFCreateMediaType()?;
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            out_type.SetUINT64(&MF_MT_FRAME_SIZE, ((aligned_width as u64) << 32) | aligned_height as u64)?;
            out_type.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
            out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
            
            transform.SetOutputType(0, &out_type, 0)?;

            // Set Input Type - Use NV12 with aligned dimensions
            let in_type = MFCreateMediaType()?;
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            in_type.SetUINT64(&MF_MT_FRAME_SIZE, ((aligned_width as u64) << 32) | aligned_height as u64)?;
            in_type.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
            in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;

            transform.SetInputType(0, &in_type, 0)?;

            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            Ok(Self {
                transform,
                width: aligned_width,
                height: aligned_height,
                fps,
                input_stream_id: 0,
                output_stream_id: 0,
            })
        }
    }

    /// Convert BGRA frame to NV12 format for H.264 encoder
    fn bgra_to_nv12(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
        let w = width as usize;
        let h = height as usize;
        
        // NV12 size: Y plane (w*h) + UV plane (w*h/2)
        let mut nv12 = vec![0u8; w * h + w * h / 2];
        
        // Y plane
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) * 4;
                let b = bgra[idx] as i32;
                let g = bgra[idx + 1] as i32;
                let r = bgra[idx + 2] as i32;
                
                // Y = 0.299*R + 0.587*G + 0.114*B
                let luma = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
                nv12[y * w + x] = luma.clamp(0, 255) as u8;
            }
        }
        
        // UV plane (interleaved, subsampled 2x2)
        let uv_offset = w * h;
        for y in (0..h).step_by(2) {
            for x in (0..w).step_by(2) {
                let idx = (y * w + x) * 4;
                let b = bgra[idx] as i32;
                let g = bgra[idx + 1] as i32;
                let r = bgra[idx + 2] as i32;
                
                // U = -0.169*R - 0.331*G + 0.500*B + 128
                // V = 0.500*R - 0.419*G - 0.081*B + 128
                let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
                
                let uv_idx = uv_offset + (y / 2) * w + x;
                nv12[uv_idx] = u.clamp(0, 255) as u8;
                nv12[uv_idx + 1] = v.clamp(0, 255) as u8;
            }
        }
        
        nv12
    }

    pub fn encode(&mut self, data: &[u8], pts: u64) -> Result<Vec<EncodedPacket>> {
        let mut packets = Vec::new();
        unsafe {
            // Convert BGRA to NV12 format for the encoder
            let nv12_data = Self::bgra_to_nv12(data, self.width, self.height);
            
            // 1. Process Input
            let sample = MFCreateSample()?;
            let buffer = MFCreateMemoryBuffer(nv12_data.len() as u32)?;

            let mut ptr: *mut u8 = ptr::null_mut();
            buffer.Lock(&mut ptr, None, None)?;
            ptr::copy_nonoverlapping(nv12_data.as_ptr(), ptr, nv12_data.len());
            buffer.SetCurrentLength(nv12_data.len() as u32)?;
            buffer.Unlock()?;

            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(pts as i64)?;
            sample.SetSampleDuration((10_000_000 / self.fps) as i64)?;

            self.transform.ProcessInput(self.input_stream_id, &sample, 0)?;

            // 2. Process Output
            loop {
                let mut output_buffer = MFT_OUTPUT_DATA_BUFFER::default();
                output_buffer.dwStreamID = self.output_stream_id;
                
                let mut status = 0;
                let mut outputs = [output_buffer];
                let hr = self.transform.ProcessOutput(0, &mut outputs, &mut status);
                
                if let Err(e) = hr {
                    if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
                        break;
                    }
                    return Err(e);
                }

                if let Some(out_sample) = outputs[0].pSample.as_ref() {
                    let mut is_keyframe = false;
                    if out_sample.GetUINT32(&MFSampleExtension_CleanPoint).is_ok() {
                        is_keyframe = true;
                    }

                    let total_len = out_sample.GetTotalLength()?;
                    
                    let mut out_data = vec![0u8; total_len as usize];
                    let buffer = out_sample.ConvertToContiguousBuffer()?;
                    
                    let mut ptr: *mut u8 = ptr::null_mut();
                    buffer.Lock(&mut ptr, None, None)?;
                    ptr::copy_nonoverlapping(ptr, out_data.as_mut_ptr(), total_len as usize);
                    buffer.Unlock()?;

                    let timestamp = out_sample.GetSampleTime()?;

                    packets.push(EncodedPacket {
                        data: out_data,
                        is_keyframe,
                        pts: timestamp as u64,
                    });
                }
            }
        }
        Ok(packets)
    }
}

#[allow(dead_code)]
pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
    pub pts: u64,
}


pub struct AudioEncoder {
    transform: IMFTransform,
    channels: u32,
    sample_rate: u32,
    input_stream_id: u32,
    output_stream_id: u32,
}

impl AudioEncoder {
    pub fn new(channels: u32, sample_rate: u32, bitrate: u32) -> Result<Self> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;

            // Find AAC Encoder
            let info = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Audio,
                guidSubtype: MFAudioFormat_AAC,
            };

            let mut pp_activate: *mut Option<IMFActivate> = ptr::null_mut();
            let mut count = 0;
            MFTEnumEx(
                MFT_CATEGORY_AUDIO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
                None,
                Some(&info),
                &mut pp_activate,
                &mut count,
            )?;

            if count == 0 {
                // Try software encoder if hardware not available
                MFTEnumEx(
                    MFT_CATEGORY_AUDIO_ENCODER,
                    MFT_ENUM_FLAG_SYNCMFT,
                    None,
                    Some(&info),
                    &mut pp_activate,
                    &mut count,
                )?;
            }

            if count == 0 {
                return Err(windows::core::Error::new(E_FAIL, "No AAC encoder found"));
            }

            let activate = (*pp_activate).as_ref().unwrap();
            let transform: IMFTransform = activate.ActivateObject()?;

            // Clean up activations
            let activations = std::slice::from_raw_parts_mut(pp_activate, count as usize);
            for act in activations {
                *act = None;
            }
            CoTaskMemFree(Some(pp_activate as *const _));

            // Set Output Type
            let out_type = MFCreateMediaType()?;
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            out_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bitrate / 8)?;
            out_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)?;
            out_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels)?;
            out_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?; // AAC typically takes 16-bit PCM
            
            transform.SetOutputType(0, &out_type, 0)?;

            // Set Input Type (PCM)
            let in_type = MFCreateMediaType()?;
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            in_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            in_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)?;
            in_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels)?;
            in_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            in_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, channels * 2)?;
            in_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, sample_rate * channels * 2)?;
            
            transform.SetInputType(0, &in_type, 0)?;

            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            Ok(Self {
                transform,
                channels,
                sample_rate,
                input_stream_id: 0,
                output_stream_id: 0,
            })
        }
    }

    pub fn encode(&mut self, data: &[u8], pts: u64) -> Result<Vec<EncodedPacket>> {
        let mut packets = Vec::new();
        unsafe {
            // 1. Process Input
            let sample = MFCreateSample()?;
            let buffer = MFCreateMemoryBuffer(data.len() as u32)?;

            let mut ptr: *mut u8 = ptr::null_mut();
            buffer.Lock(&mut ptr, None, None)?;
            ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            buffer.SetCurrentLength(data.len() as u32)?;
            buffer.Unlock()?;

            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(pts as i64)?;
            
            let duration = (data.len() as u64 * 10_000_000) / (self.sample_rate as u64 * self.channels as u64 * 2);
            sample.SetSampleDuration(duration as i64)?;

            self.transform.ProcessInput(self.input_stream_id, &sample, 0)?;

            // 2. Process Output
            loop {
                let mut output_buffer = MFT_OUTPUT_DATA_BUFFER::default();
                output_buffer.dwStreamID = self.output_stream_id;
                
                let mut status = 0;
                let mut outputs = [output_buffer];
                let hr = self.transform.ProcessOutput(0, &mut outputs, &mut status);
                
                if let Err(e) = hr {
                    if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
                        break;
                    }
                    return Err(e);
                }

                if let Some(out_sample) = outputs[0].pSample.as_ref() {
                    let total_len = out_sample.GetTotalLength()?;
                    
                    let mut out_data = vec![0u8; total_len as usize];
                    let buffer = out_sample.ConvertToContiguousBuffer()?;
                    
                    let mut ptr: *mut u8 = ptr::null_mut();
                    buffer.Lock(&mut ptr, None, None)?;
                    ptr::copy_nonoverlapping(ptr, out_data.as_mut_ptr(), total_len as usize);
                    buffer.Unlock()?;

                    let timestamp = out_sample.GetSampleTime()?;

                    packets.push(EncodedPacket {
                        data: out_data,
                        is_keyframe: false, // Not used for audio in HLS logic usually
                        pts: timestamp as u64,
                    });
                }
            }
        }
        Ok(packets)
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

impl Drop for AudioEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}
