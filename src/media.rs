//! Audio and video conversions through the user's FFmpeg installation.

use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

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

/// Converts one supported input to `output`, which must end in the target extension.
pub fn convert(input: &Path, target: Target, output: &Path) -> io::Result<()> {
    let mut command = Command::new("ffmpeg");
    command
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

    let output_result = command.arg(output).output()?;
    if !output_result.status.success() {
        return Err(io::Error::other(command_error(
            &output_result.stderr,
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
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
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
