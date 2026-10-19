//! Audio and video conversions through the user's FFmpeg installation.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use crate::archive::Progress;

/// How often a running FFmpeg is checked on, to stop it once the job is cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Mp3,
    M4a,
    Wav,
    Flac,
    Ogg,
    Opus,
    Aiff,
    Mp4,
    Mov,
    Mkv,
    Webm,
    Gif,
}

impl Target {
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "MP3" => Self::Mp3,
            "M4A" => Self::M4a,
            "WAV" => Self::Wav,
            "FLAC" => Self::Flac,
            "OGG" => Self::Ogg,
            "OPUS" => Self::Opus,
            "AIFF" => Self::Aiff,
            "MP4" => Self::Mp4,
            "MOV" => Self::Mov,
            "MKV" => Self::Mkv,
            "WEBM" => Self::Webm,
            "GIF" => Self::Gif,
            _ => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Aiff => "aiff",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Gif => "gif",
        }
    }
}

pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| command_on_path("ffmpeg"))
}

/// Converts one supported input to `output`, which must end in the target extension. FFmpeg is
/// stopped if the job is cancelled.
pub fn convert(input: &Path, target: Target, output: &Path, progress: &Progress) -> io::Result<()> {
    let mut command = Command::new("ffmpeg");
    crate::actions::quiet(&mut command)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(input)
        .args(["-map_metadata", "0", "-map_chapters", "0"]);

    match target {
        Target::Mp3 => {
            command.args(["-vn", "-c:a", "libmp3lame", "-q:a", "2"]);
        }
        Target::M4a => {
            command.args(["-vn", "-c:a", "aac", "-b:a", "192k"]);
        }
        Target::Wav => {
            command.args(["-vn", "-c:a", "pcm_s16le"]);
        }
        Target::Flac => {
            command.args(["-vn", "-c:a", "flac"]);
        }
        Target::Ogg => {
            command.args(["-vn", "-c:a", "libvorbis", "-q:a", "5"]);
        }
        Target::Opus => {
            command.args(["-vn", "-c:a", "libopus", "-b:a", "128k"]);
        }
        Target::Aiff => {
            command.args(["-vn", "-c:a", "pcm_s16be"]);
        }
        Target::Mp4 | Target::Mov => {
            command.args([
                "-c:v", "libx264", "-preset", "medium", "-crf", "23", "-c:a", "aac", "-b:a", "192k",
            ]);
        }
        Target::Mkv => {
            command.args([
                "-c:v", "libx264", "-preset", "medium", "-crf", "23", "-c:a", "aac", "-b:a", "192k",
            ]);
        }
        Target::Webm => {
            command.args([
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "32",
                "-b:v",
                "0",
                "-c:a",
                "libopus",
                "-b:a",
                "128k",
            ]);
        }
        Target::Gif => {
            command.args(["-an", "-vf", "fps=15,scale=640:-1:flags=lanczos"]);
        }
    }

    let mut child = command
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    // Read as it comes, so a chatty FFmpeg never waits on a full pipe.
    let mut stderr = child.stderr.take().expect("piped");
    let said = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stderr.read_to_end(&mut text);
        text
    });
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if progress.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return progress.check();
        }
        std::thread::sleep(CHECK_EVERY);
    };
    let said = said.join().unwrap_or_default();
    if !status.success() {
        return Err(io::Error::other(command_error(
            &said,
            "FFmpeg could not convert this file",
        )));
    }
    let metadata = std::fs::metadata(output)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(io::Error::other("FFmpeg produced an empty file"));
    }
    Ok(())
}

fn command_on_path(name: &str) -> bool {
    crate::actions::has(name)
}

fn command_error(stderr: &[u8], fallback: &str) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_conversion_stops_ffmpeg() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // A second of silence: 16-bit mono at 8 kHz.
        let samples = 8000u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        wav.resize(wav.len() + samples as usize * 2, 0);
        let input = dir.path().join("quiet.wav");
        std::fs::write(&input, wav).unwrap();

        let progress = Progress::default();
        let output = dir.path().join("quiet.flac");
        convert(&input, Target::Flac, &output, &progress).unwrap();
        assert!(output.is_file());

        progress.cancel();
        let again = dir.path().join("again.flac");
        let error = convert(&input, Target::Flac, &again, &progress).unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
    }
}
