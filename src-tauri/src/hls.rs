#![allow(dead_code)]

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

pub struct HlsManager {
    output_dir: PathBuf,
    stream_id: String,
    segment_duration: f64,
    max_segments: usize,
    segment_index: u32,
    segments: Vec<SegmentInfo>,
    current_segment: Option<TsSegment>,
}

struct SegmentInfo {
    filename: String,
    duration: f64,
}

struct TsSegment {
    file: File,
    start_time_pts: u64,
    video_cc: u8,
    audio_cc: u8,
    pat_cc: u8,
    pmt_cc: u8,
}

impl HlsManager {
    pub fn new(output_dir: &str, stream_id: &str, segment_duration: f64, max_segments: usize) -> Self {
        let path = PathBuf::from(output_dir);
        if !path.exists() {
            std::fs::create_dir_all(&path).unwrap();
        }
        Self {
            output_dir: path,
            stream_id: stream_id.to_string(),
            segment_duration,
            max_segments,
            segment_index: 0,
            segments: Vec::new(),
            current_segment: None,
        }
    }

    pub fn write_video_frame(&mut self, data: &[u8], is_keyframe: bool, pts: u64) {
        if self.current_segment.is_none() {
            self.start_new_segment(pts);
        }

        let current = self.current_segment.as_mut().unwrap();
        let duration = (pts - current.start_time_pts) as f64 / 90000.0;

        if is_keyframe && duration >= self.segment_duration {
            self.close_current_segment(duration);
            self.start_new_segment(pts);
        }

        self.write_pes(data, pts, true, is_keyframe);
    }

    pub fn write_audio_frame(&mut self, data: &[u8], pts: u64) {
        if self.current_segment.is_some() {
            self.write_pes(data, pts, false, false);
        }
    }

    fn start_new_segment(&mut self, pts: u64) {
        let filename = format!("{}_{}.ts", self.stream_id, self.segment_index);
        let path = self.output_dir.join(&filename);
        let file = File::create(path).unwrap();

        let mut segment = TsSegment {
            file,
            start_time_pts: pts,
            video_cc: 0,
            audio_cc: 0,
            pat_cc: 0,
            pmt_cc: 0,
        };

        self.write_pat(&mut segment);
        self.write_pmt(&mut segment);

        self.current_segment = Some(segment);
    }

    fn close_current_segment(&mut self, duration: f64) {
        if let Some(_segment) = self.current_segment.take() {
            let filename = format!("{}_{}.ts", self.stream_id, self.segment_index);
            self.segments.push(SegmentInfo {
                filename,
                duration,
            });

            if self.segments.len() > self.max_segments {
                let old = self.segments.remove(0);
                let old_path = self.output_dir.join(old.filename);
                let _ = std::fs::remove_file(old_path);
            }

            self.segment_index += 1;
            self.write_playlist();
        }
    }

    fn write_pes(&mut self, data: &[u8], pts: u64, is_video: bool, is_keyframe: bool) {
        let segment = self.current_segment.as_mut().unwrap();
        let pid = if is_video { 256u16 } else { 257u16 };
        let stream_id = if is_video { 0xE0u8 } else { 0xC0u8 };

        let mut pes_header = vec![0u8; 14];
        pes_header[0] = 0x00;
        pes_header[1] = 0x00;
        pes_header[2] = 0x01;
        pes_header[3] = stream_id;

        let pes_len = (data.len() + 8) as u16;
        pes_header[4] = (pes_len >> 8) as u8;
        pes_header[5] = pes_len as u8;
        pes_header[6] = 0x80;
        pes_header[7] = 0x80;
        pes_header[8] = 5;
        // PTS
        pes_header[9] = 0x21 | (((pts >> 29) & 0x0E) as u8);
        pes_header[10] = ((pts >> 22) & 0xFF) as u8;
        pes_header[11] = 0x01 | (((pts >> 14) & 0xFE) as u8);
        pes_header[12] = ((pts >> 7) & 0xFF) as u8;
        pes_header[13] = 0x01 | (((pts << 1) & 0xFE) as u8);

        let mut pes_packet = pes_header;
        pes_packet.extend_from_slice(data);

        let mut offset = 0;
        let mut first = true;
        let cc = if is_video { &mut segment.video_cc } else { &mut segment.audio_cc };

        while offset < pes_packet.len() {
            let mut ts = vec![0u8; 188];
            ts[0] = 0x47;
            ts[1] = ((pid >> 8) & 0x1F) as u8;
            if first { ts[1] |= 0x40; }
            ts[2] = (pid & 0xFF) as u8;

            let mut adaptation_len = 0;
            if first && is_keyframe && is_video {
                ts[4] = 0x07;
                ts[5] = 0x50;
                // PCR = PTS (simplified)
                ts[6] = (pts >> 25) as u8;
                ts[7] = (pts >> 17) as u8;
                ts[8] = (pts >> 9) as u8;
                ts[9] = (pts >> 1) as u8;
                ts[10] = ((pts & 1) << 7) as u8 | 0x7E;
                ts[11] = 0x00;
                adaptation_len = 8;
            }

            let remaining = pes_packet.len() - offset;
            let mut payload_start = 4 + adaptation_len;
            let mut payload_size = 188 - payload_start;

            if remaining < payload_size {
                payload_size = remaining;
                let stuffing = 188 - payload_start - payload_size;
                if stuffing > 0 {
                    if adaptation_len == 0 {
                        ts[4] = (stuffing - 1) as u8;
                        if stuffing > 1 { ts[5] = 0x00; }
                        for i in 6..(4 + stuffing) { ts[i] = 0xFF; }
                        payload_start = 4 + stuffing;
                    } else {
                        let old_len = ts[4] as usize;
                        ts[4] = (old_len + stuffing) as u8;
                        for i in (5 + old_len)..(5 + old_len + stuffing) { ts[i] = 0xFF; }
                        payload_start = 4 + old_len + stuffing + 1;
                    }
                }
            }

            ts[3] = (if adaptation_len > 0 || (188 - payload_start - payload_size) > 0 { 0x30 } else { 0x10 }) | (*cc & 0x0F);
            
            let write_len = std::cmp::min(payload_size, pes_packet.len() - offset);
            ts[payload_start..(payload_start + write_len)].copy_from_slice(&pes_packet[offset..(offset + write_len)]);
            
            segment.file.write_all(&ts).unwrap();
            offset += write_len;
            *cc = (*cc + 1) & 0x0F;
            first = false;
        }
    }

    fn write_pat(&self, segment: &mut TsSegment) {
        let mut ts = vec![0xFFu8; 188];
        ts[0] = 0x47; ts[1] = 0x40; ts[2] = 0x00;
        ts[3] = 0x10 | (segment.pat_cc & 0x0F);
        segment.pat_cc = (segment.pat_cc + 1) & 0x0F;

        let mut pat = vec![0x00, 0x00, 0xB0, 0x0D, 0x00, 0x01, 0xC1, 0x00, 0x00, 0x00, 0x01, 0xE1, 0x00];
        let crc = self.crc32_mpeg2(&pat[1..]);
        pat.extend_from_slice(&crc.to_be_bytes());
        ts[4..(4 + pat.len())].copy_from_slice(&pat);
        segment.file.write_all(&ts).unwrap();
    }

    fn write_pmt(&self, segment: &mut TsSegment) {
        let mut ts = vec![0xFFu8; 188];
        ts[0] = 0x47; ts[1] = 0x50; ts[2] = 0x00;
        ts[3] = 0x10 | (segment.pmt_cc & 0x0F);
        segment.pmt_cc = (segment.pmt_cc + 1) & 0x0F;

        let mut pmt = vec![
            0x00, 0x02, 0xB0, 0x17, 0x00, 0x01, 0xC1, 0x00, 0x00, 0xE1, 0x00, 0xF0, 0x00,
            0x1B, 0xE1, 0x00, 0xF0, 0x00, 0x0F, 0xE1, 0x01, 0xF0, 0x00
        ];
        let crc = self.crc32_mpeg2(&pmt[1..]);
        pmt.extend_from_slice(&crc.to_be_bytes());
        ts[4..(4 + pmt.len())].copy_from_slice(&pmt);
        segment.file.write_all(&ts).unwrap();
    }

    fn write_playlist(&self) {
        let mut content = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
        let max_duration = self.segments.iter().map(|s| s.duration).fold(0.0, f64::max);
        content.push_str(&format!("#EXT-X-TARGETDURATION:{}\n", (max_duration as u32) + 1));
        content.push_str(&format!("#EXT-X-MEDIA-SEQUENCE:{}\n", self.segment_index.saturating_sub(self.segments.len() as u32)));

        for seg in &self.segments {
            content.push_str(&format!("#EXTINF:{:.3},\n{}\n", seg.duration, seg.filename));
        }

        let path = self.output_dir.join(format!("{}.m3u8", self.stream_id));
        std::fs::write(path, content).unwrap();
    }

    fn crc32_mpeg2(&self, data: &[u8]) -> u32 {
        let mut crc = 0xFFFFFFFFu32;
        for &b in data {
            crc ^= (b as u32) << 24;
            for _ in 0..8 {
                if crc & 0x80000000 != 0 {
                    crc = (crc << 1) ^ 0x04C11DB7;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }
}
