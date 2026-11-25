use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const GENERATION_PREFIX: &str = "run-";
const RETENTION_MARKER: &str = ".retain-until";

#[derive(Debug, Clone)]
pub struct HlsConfig {
    output_dir: String,
    stream_id: String,
    list_size: u32,
    hls_time: f64,
}

impl HlsConfig {
    pub fn new(output_dir: &str, stream_id: &str, list_size: u32, hls_time: f64) -> Self {
        Self {
            output_dir: output_dir.to_string(),
            stream_id: stream_id.to_string(),
            list_size: if list_size == 0 { 0 } else { list_size.max(3) },
            hls_time,
        }
    }

    pub fn stream_dir(&self) -> String {
        format!("{}/{}", self.output_dir, self.stream_id)
    }

    pub fn playlist_path(&self) -> String {
        format!("{}/{}.m3u8", self.stream_dir(), self.stream_id)
    }

    pub fn playlist_route(&self) -> String {
        format!("/streams/{0}/{0}.m3u8", self.stream_id)
    }

    pub fn segment_duration(&self) -> f64 {
        self.hls_time
    }

    fn encoder_window(&self, previous_target: u64) -> u32 {
        let target = self
            .hls_time
            .ceil()
            .max(previous_target as f64)
            .clamp(1.0, 60.0);
        let minimum = (3.0 * target / self.hls_time.max(0.01)).ceil() as u32;
        self.list_size.max(minimum).saturating_add(3)
    }

    pub fn prepare_generation(&self) -> io::Result<HlsGeneration> {
        let previous = read_playlist(Path::new(&self.playlist_path()))?;
        let next_sequence = previous.as_ref().map_or(0, MediaPlaylist::next_sequence);
        let name = format!("{GENERATION_PREFIX}{}", uuid::Uuid::new_v4());
        // Use forward slashes for FFmpeg's segment and init paths on Windows.
        let directory = format!("{}/{name}", self.stream_dir());
        fs::create_dir_all(&directory)?;
        let log = Path::new(&directory).join("ffmpeg.log");
        Ok(HlsGeneration {
            config: self.clone(),
            name,
            directory,
            log,
            next_sequence,
            published: previous.clone(),
            previous,
            last_manifest: String::new(),
        })
    }

    #[allow(dead_code)]
    pub fn playlist_is_ready(&self) -> bool {
        read_playlist(Path::new(&self.playlist_path())).is_ok_and(|manifest| {
            manifest.is_some_and(|manifest| manifest.files_exist(Path::new(&self.stream_dir())))
        })
    }

    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
}

pub struct HlsGeneration {
    config: HlsConfig,
    name: String,
    directory: String,
    log: PathBuf,
    next_sequence: u64,
    previous: Option<MediaPlaylist>,
    published: Option<MediaPlaylist>,
    last_manifest: String,
}

impl HlsGeneration {
    pub fn to_ffmpeg_args(&self) -> Vec<String> {
        let mut args = vec![
            "-f".into(),
            "hls".into(),
            "-hls_time".into(),
            self.config.hls_time.to_string(),
            "-hls_segment_type".into(),
            "fmp4".into(),
            "-hls_fmp4_init_filename".into(),
            "init.mp4".into(),
            "-start_number".into(),
            self.next_sequence.to_string(),
        ];
        if self.config.list_size == 0 {
            args.extend([
                "-hls_flags".into(),
                "independent_segments+program_date_time+temp_file".into(),
                "-hls_playlist_type".into(),
                "event".into(),
            ]);
        } else {
            let previous_target = self
                .previous
                .as_ref()
                .map_or(1, |playlist| playlist.target_duration);
            let window = self.config.encoder_window(previous_target).to_string();
            args.extend([
                "-hls_flags".into(),
                "delete_segments+independent_segments+program_date_time+temp_file".into(),
                "-hls_list_size".into(),
                window.clone(),
                "-hls_delete_threshold".into(),
                window,
            ]);
        }
        args
    }

    pub fn to_ffmpeg_output_args(&self) -> Vec<String> {
        vec![
            "-hls_segment_filename".to_string(),
            format!("{}/segment_%d.m4s", self.directory),
            format!("{}/pending.m3u8", self.directory),
        ]
    }

    pub fn log_path(&self) -> &Path {
        &self.log
    }

    pub fn create_log_file(&self) -> io::Result<File> {
        File::create(self.log_path())
    }

    pub fn publish_playlist(&mut self) -> io::Result<bool> {
        let raw = match fs::read_to_string(Path::new(&self.directory).join("pending.m3u8")) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if raw == self.last_manifest {
            return Ok(false);
        }
        let Some(mut current) = MediaPlaylist::parse(&raw) else {
            return Ok(false);
        };
        if current.sequence < self.next_sequence || !current.files_exist(Path::new(&self.directory))
        {
            return Ok(false);
        }
        for segment in &mut current.segments {
            segment.uri = format!("{}/{}", self.name, segment.uri);
            segment.init = format!("{}/{}", self.name, segment.init);
        }

        if let Some(previous) = &self.previous {
            let previous_discontinuities = previous.discontinuities();
            if current.sequence == self.next_sequence {
                current.segments[0].discontinuity = true;
                let mut prior = previous.clone();
                prior.segments.append(&mut current.segments);
                current.segments = prior.segments;
                current.sequence = prior.sequence;
                current.discontinuity_sequence = prior.discontinuity_sequence;
            } else {
                current.discontinuity_sequence = previous_discontinuities + 1;
            }
        }
        if let Some(published) = &self.published {
            current.target_duration = current.target_duration.max(published.target_duration);
        }
        if self.config.list_size > 0 {
            current.trim_to(self.config.list_size as usize);
        }

        if self.published.as_ref().is_some_and(|published| {
            current.sequence < published.sequence
                || current.next_sequence() <= published.next_sequence()
        }) {
            return Ok(false);
        }
        // If the original files were removed outside HipoCast, do not advertise them again.
        let stream_directory = self.config.stream_dir();
        current.drop_missing_prefix(Path::new(&stream_directory));
        if current.segments.is_empty() {
            return Ok(false);
        }
        let temporary = Path::new(&self.directory).join("publication.tmp");
        fs::write(&temporary, current.render(self.config.list_size == 0))?;
        self.mark_retired(Path::new(&stream_directory), &current)?;
        match fs::rename(&temporary, self.config.playlist_path()) {
            Ok(()) => {}
            // Some Windows readers briefly hold the public file without delete sharing.
            Err(error) if error.raw_os_error() == Some(32) => return Ok(false),
            Err(error) => return Err(error),
        }
        if let Err(error) = self.prune_retired(Path::new(&stream_directory), &current) {
            tracing::warn!(%error, "Falha ao remover segmentos HLS expirados");
        }
        self.published = Some(current);
        self.last_manifest = raw;
        Ok(true)
    }

    fn mark_retired(&self, root: &Path, current: &MediaPlaylist) -> io::Result<()> {
        let Some(previous) = &self.published else {
            return Ok(());
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let expiration = now
            .as_secs()
            .saturating_add(previous.retention().as_secs())
            .saturating_add(1);
        let retained = current.generations();
        for generation in previous.generations().difference(&retained) {
            let marker = root.join(generation).join(RETENTION_MARKER);
            match fs::write(marker, expiration.to_string()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn prune_retired(&self, root: &Path, current: &MediaPlaylist) -> io::Result<()> {
        let protected = current.generations();
        let previous = self.published.as_ref();
        let grace = previous
            .map(MediaPlaylist::retention)
            .unwrap_or_default()
            .max(current.retention());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !is_generation_name(name) || name == self.name || !entry.file_type()?.is_dir() {
                continue;
            }
            if protected.contains(name) {
                continue;
            }
            let path = entry.path();
            let marker = path.join(RETENTION_MARKER);
            let deadline = fs::read_to_string(marker)
                .ok()
                .and_then(|value| value.parse().ok())
                .map(Duration::from_secs)
                .or_else(|| {
                    // Recover abandoned generations when a previous process exited before cleanup.
                    let modified = entry.metadata().ok()?.modified().ok()?;
                    let modified = modified.duration_since(UNIX_EPOCH).ok()?;
                    Some(modified.saturating_add(grace))
                });
            if deadline.is_some_and(|deadline| now >= deadline)
                && path.canonicalize()?.parent() == Some(root.canonicalize()?.as_path())
            {
                fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct MediaSegment {
    duration: f64,
    uri: String,
    init: String,
    date_time: Option<String>,
    discontinuity: bool,
}

#[derive(Clone)]
struct MediaPlaylist {
    target_duration: u64,
    sequence: u64,
    discontinuity_sequence: u64,
    segments: Vec<MediaSegment>,
}

impl MediaPlaylist {
    fn parse(raw: &str) -> Option<Self> {
        let mut lines = raw.lines();
        if lines.next()? != "#EXTM3U" {
            return None;
        }
        let (mut target_duration, mut sequence, mut discontinuity_sequence) = (0, 0u64, 0);
        let (mut duration, mut date_time, mut init) = (None::<f64>, None, None::<String>);
        let mut discontinuity = false;
        let mut segments = Vec::new();
        for line in lines {
            if let Some(value) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
                target_duration = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
                sequence = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:") {
                discontinuity_sequence = value.parse().ok()?;
            } else if let Some(value) = line.strip_prefix("#EXTINF:") {
                let value: f64 = value.split_once(',')?.0.parse().ok()?;
                if !value.is_finite() || value <= 0.0 {
                    return None;
                }
                duration = Some(value);
            } else if let Some(value) = line.strip_prefix("#EXT-X-MAP:URI=\"") {
                init = Some(value.split_once('"')?.0.to_owned());
            } else if let Some(value) = line.strip_prefix("#EXT-X-PROGRAM-DATE-TIME:") {
                date_time = Some(value.to_owned());
            } else if line == "#EXT-X-DISCONTINUITY" {
                discontinuity = true;
            } else if !line.is_empty() && !line.starts_with('#') {
                let init = init.clone()?;
                if !is_media_path(&init, ".mp4") || !is_media_path(line, ".m4s") {
                    return None;
                }
                segments.push(MediaSegment {
                    duration: duration.take()?,
                    uri: line.to_owned(),
                    init,
                    date_time: date_time.take(),
                    discontinuity: std::mem::take(&mut discontinuity),
                });
            }
        }
        if segments.is_empty() {
            return None;
        }
        sequence.checked_add(segments.len() as u64)?;
        Some(Self {
            target_duration: target_duration.max(1),
            sequence,
            discontinuity_sequence,
            segments,
        })
    }

    fn next_sequence(&self) -> u64 {
        self.sequence + self.segments.len() as u64
    }

    fn discontinuities(&self) -> u64 {
        self.discontinuity_sequence
            + self
                .segments
                .iter()
                .filter(|segment| segment.discontinuity)
                .count() as u64
    }

    fn files_exist(&self, root: &Path) -> bool {
        self.segments
            .iter()
            .all(|segment| root.join(&segment.uri).is_file() && root.join(&segment.init).is_file())
    }

    fn trim_to(&mut self, maximum: usize) {
        let mut retained = 0;
        let mut duration = 0.0;
        for segment in self.segments.iter().rev() {
            retained += 1;
            duration += segment.duration;
            if retained >= maximum && duration + 0.001 >= self.target_duration as f64 * 3.0 {
                break;
            }
        }
        self.drop_prefix(self.segments.len() - retained);
    }

    fn drop_missing_prefix(&mut self, root: &Path) {
        if let Some(last_missing) = self.segments.iter().rposition(|segment| {
            !root.join(&segment.uri).is_file() || !root.join(&segment.init).is_file()
        }) {
            self.drop_prefix(last_missing + 1);
        }
    }

    fn drop_prefix(&mut self, count: usize) {
        self.sequence += count as u64;
        self.discontinuity_sequence += self
            .segments
            .drain(..count)
            .filter(|segment| segment.discontinuity)
            .count() as u64;
    }

    fn generations(&self) -> HashSet<&str> {
        self.segments
            .iter()
            .filter_map(|segment| segment.uri.split_once('/').map(|(dir, _)| dir))
            .collect()
    }

    fn retention(&self) -> Duration {
        let seconds = self
            .segments
            .iter()
            .map(|segment| segment.duration)
            .sum::<f64>();
        let largest = self
            .segments
            .iter()
            .map(|segment| segment.duration)
            .fold(0.0, f64::max);
        Duration::try_from_secs_f64((seconds + largest).ceil()).unwrap_or(Duration::MAX)
    }

    fn render(&self, event: bool) -> String {
        let mut body = format!(
            "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:{}\n#EXT-X-DISCONTINUITY-SEQUENCE:{}\n#EXT-X-INDEPENDENT-SEGMENTS\n",
            self.target_duration, self.sequence, self.discontinuity_sequence,
        );
        if event {
            body.push_str("#EXT-X-PLAYLIST-TYPE:EVENT\n");
        }
        let mut last_init = "";
        for segment in &self.segments {
            if segment.discontinuity {
                body.push_str("#EXT-X-DISCONTINUITY\n");
            }
            if segment.init != last_init {
                let _ = writeln!(body, "#EXT-X-MAP:URI=\"{}\"", segment.init);
                last_init = &segment.init;
            }
            let _ = writeln!(body, "#EXTINF:{:.6},", segment.duration);
            if let Some(date_time) = &segment.date_time {
                let _ = writeln!(body, "#EXT-X-PROGRAM-DATE-TIME:{date_time}");
            }
            let _ = writeln!(body, "{}", segment.uri);
        }
        body
    }
}

fn read_playlist(path: &Path) -> io::Result<Option<MediaPlaylist>> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(MediaPlaylist::parse(&raw)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn is_generation_name(name: &str) -> bool {
    name.strip_prefix(GENERATION_PREFIX)
        .filter(|id| id.len() == 36)
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .is_some()
}

pub(crate) fn is_media_path(path: &str, extension: &str) -> bool {
    let filename = if let Some((directory, filename)) = path.split_once('/') {
        if !is_generation_name(directory) {
            return false;
        }
        filename
    } else {
        path
    };
    !filename.starts_with('.')
        && filename.ends_with(extension)
        && filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::get_ffmpeg_path;
    use crate::server::{playlist_is_served, start_server};
    use std::process::Command;

    #[test]
    fn playback_url_and_legacy_retention_are_preserved() {
        let config = HlsConfig::new("/tmp/streams", "test_stream", 1, 2.0);
        assert_eq!(config.stream_dir(), "/tmp/streams/test_stream");
        assert_eq!(
            config.playlist_path(),
            "/tmp/streams/test_stream/test_stream.m3u8"
        );
        assert_eq!(
            config.playlist_route(),
            "/streams/test_stream/test_stream.m3u8"
        );
        assert_eq!(config.list_size, 3);
        assert_eq!(HlsConfig::new("/tmp", "test_stream", 0, 2.0).list_size, 0);
        assert_eq!(
            HlsConfig::new("/tmp", "test_stream", 3, 0.35).encoder_window(1),
            12
        );
    }

    fn generate(generation: &HlsGeneration, seconds: &str) {
        let output = Command::new(get_ffmpeg_path())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:size=160x90:rate=30",
                "-t",
                seconds,
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-g",
                "30",
                "-sc_threshold",
                "0",
            ])
            .args(generation.to_ffmpeg_args())
            .args(generation.to_ffmpeg_output_args())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn synthetic_restart_keeps_the_url_sequence_and_recent_media() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("hipocast-hls-test-{}", uuid::Uuid::new_v4()));
        let config = HlsConfig::new(&root.to_string_lossy(), "synthetic", 3, 1.0);
        let path = config.playlist_path();
        let mut first = config.prepare_generation()?;
        let port = start_server(0, root.clone()).map_err(io::Error::other)?;
        generate(&first, "10");
        assert!(first.publish_playlist()?);
        let before = fs::read_to_string(&path)?;
        let before_media = MediaPlaylist::parse(&before).unwrap();
        assert!(before_media.sequence > 0);
        assert!(playlist_is_served(port, &config.playlist_route()));
        assert!(!playlist_is_served(
            port,
            &format!("/streams/synthetic/{}/pending.m3u8", first.name)
        ));

        let mut second = config.prepare_generation()?;
        assert_eq!(fs::read_to_string(&path)?, before);
        assert!(!second.publish_playlist()?);
        generate(&second, "1");
        assert!(second.publish_playlist()?);
        let second_media = read_playlist(Path::new(&path))?.unwrap();
        assert_eq!(
            second_media.next_sequence(),
            before_media.next_sequence() + 1
        );
        assert!(second_media.sequence >= before_media.sequence);
        assert_eq!(second_media.generations().len(), 2);
        assert!(second_media.segments.last().unwrap().discontinuity);
        assert!(second_media
            .segments
            .last()
            .unwrap()
            .init
            .starts_with(&second.name));
        assert!(playlist_is_served(
            port,
            &format!("/streams/synthetic/{}", before_media.segments[0].uri)
        ));

        let mut third = config.prepare_generation()?;
        generate(&third, "7");
        assert!(third.publish_playlist()?);
        let current_media = read_playlist(Path::new(&path))?.unwrap();
        assert!(current_media.sequence > second_media.sequence);
        assert_eq!(current_media.discontinuity_sequence, 2);
        assert!(config.playlist_is_ready());
        assert!(Path::new(&first.directory).join(RETENTION_MARKER).is_file());
        assert!(Path::new(&first.directory).is_dir());
        fs::write(Path::new(&first.directory).join(RETENTION_MARKER), "0")?;
        third.prune_retired(Path::new(&config.stream_dir()), &current_media)?;
        assert!(!Path::new(&first.directory).is_dir());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
