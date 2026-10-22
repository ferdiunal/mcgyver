//! Audio and video conversions and tools through the user's FFmpeg installation.

use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use crate::archive::Progress;

/// How often a running FFmpeg is checked on, to stop it once the job is cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(100);
/// The tags and chapters go along with the sound and the picture.
const KEEP_INFO: [&str; 4] = ["-map_metadata", "0", "-map_chapters", "0"];
/// Scales a video down to 1920 pixels on its long side, keeping both sides even.
const FIT_HD: &str = "scale='if(gte(iw,ih),min(trunc(iw/2)*2,1920),-2)':\
                      'if(gte(iw,ih),-2,min(trunc(ih/2)*2,1920))'";
/// H.264 in 4:2:0 wants both sides even: an odd one loses a pixel.
const EVEN: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";
/// Frames takes one still a second, or this many spread over a longer video.
const MOST_FRAMES: f64 = 100.0;
/// A silence at most this far from the start or the end counts as being at it, in seconds.
const NEAR_SILENCE: f64 = 0.1;
/// The same for black: blackdetect ends black at the last frame, which may be a frame or two
/// short of the end, or further where the sound runs on.
const NEAR_BLACK: f64 = 0.5;

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

    fn is_video(self) -> bool {
        matches!(
            self,
            Self::Mp4 | Self::Mov | Self::Mkv | Self::Webm | Self::Gif
        )
    }

    /// The encoders and their settings for this format, for a file of the same size or, when
    /// `smaller`, lower bit rates and a picture no larger than 1080p. (Compress saves
    /// uncompressed and lossless sound as M4A instead: see [`tool_target`].) `filter` goes
    /// first among the picture's filters (Rotate's turn).
    fn codecs(self, smaller: bool, probe: &Probe, filter: Option<&str>) -> Vec<String> {
        let sound: &[&str] = match (self, smaller) {
            (Self::Mp3, false) => &["-c:a", "libmp3lame", "-q:a", "2"],
            (Self::Mp3, true) => &["-c:a", "libmp3lame", "-b:a", "96k"],
            (Self::M4a | Self::Mp4 | Self::Mov | Self::Mkv, false) => {
                &["-c:a", "aac", "-b:a", "192k"]
            }
            (Self::M4a, true) => &["-c:a", "aac", "-b:a", "96k"],
            (Self::Mp4 | Self::Mov | Self::Mkv, true) => &["-c:a", "aac", "-b:a", "128k"],
            (Self::Wav, _) => &["-c:a", "pcm_s16le"],
            (Self::Flac, _) => &["-c:a", "flac"],
            (Self::Aiff, _) => &["-c:a", "pcm_s16be"],
            (Self::Ogg, false) => &["-c:a", "libvorbis", "-q:a", "5"],
            (Self::Ogg, true) => &["-c:a", "libvorbis", "-q:a", "2"],
            (Self::Opus, false) => &["-c:a", "libopus", "-b:a", "128k"],
            (Self::Opus, true) => &["-c:a", "libopus", "-b:a", "64k"],
            (Self::Webm, false) => &["-c:a", "libopus", "-b:a", "128k"],
            (Self::Webm, true) => &["-c:a", "libopus", "-b:a", "96k"],
            (Self::Gif, _) => &["-an"],
        };
        let (mut args, size) = match self {
            Self::Mp4 | Self::Mov | Self::Mkv => (
                h264(smaller, probe),
                Some(if smaller { FIT_HD } else { EVEN }),
            ),
            Self::Webm => (
                strings(&[
                    "-c:v",
                    "libvpx-vp9",
                    "-crf",
                    if smaller { "40" } else { "32" },
                    "-b:v",
                    "0",
                ]),
                smaller.then_some(FIT_HD),
            ),
            Self::Gif => (Vec::new(), Some("fps=15,scale=640:-1:flags=lanczos")),
            _ => (strings(&["-vn"]), None),
        };
        // One chain: FFmpeg keeps only the last of several -vf.
        let filters: Vec<&str> = filter.into_iter().chain(size).collect();
        if self.is_video() && !filters.is_empty() {
            args.extend(["-vf".to_owned(), filters.join(",")]);
        }
        args.extend(strings(sound));
        args
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|&arg| arg.to_owned()).collect()
}

/// H.264 with the encoder this FFmpeg has: x264 at a constant quality; else OpenH264 (Fedora's
/// ffmpeg-free has only that), which knows only bit rates, at one for the picture's size; else
/// FFmpeg's own MPEG-4, which every build has.
fn h264(smaller: bool, probe: &Probe) -> Vec<String> {
    if has_encoder("libx264") {
        let quality = if smaller { "28" } else { "23" };
        return strings(&[
            "-c:v", "libx264", "-preset", "medium", "-crf", quality, "-pix_fmt", "yuv420p",
        ]);
    }
    if has_encoder("libopenh264") {
        let (width, height) = probe.size.unwrap_or((1920, 1080));
        let long = width.max(height).max(1) as f64;
        // Compress fits the picture into 1080p first.
        let fit = if smaller {
            (1920.0 / long).min(1.0)
        } else {
            1.0
        };
        let pixels = width as f64 * height as f64 * fit * fit;
        // Bits a pixel takes at 30 frames a second.
        let bits = pixels * 30.0 * if smaller { 0.05 } else { 0.1 };
        let rate = format!("{}k", (bits / 1000.0).clamp(300.0, 20_000.0) as u32);
        return strings(&["-c:v", "libopenh264", "-b:v", &rate, "-pix_fmt", "yuv420p"]);
    }
    strings(&["-c:v", "mpeg4", "-q:v", if smaller { "8" } else { "4" }])
}

/// Whether this FFmpeg has the encoder `name`.
fn has_encoder(name: &str) -> bool {
    static ENCODERS: OnceLock<String> = OnceLock::new();
    let encoders = ENCODERS.get_or_init(|| {
        let mut command = Command::new("ffmpeg");
        crate::actions::quiet(&mut command)
            .args(["-hide_banner", "-encoders"])
            .stdin(Stdio::null())
            .output()
            .map(|said| String::from_utf8_lossy(&said.stdout).into_owned())
            .unwrap_or_default()
    });
    encoders
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(name))
}

/// What the Tools wheel does to audio and video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Cuts the silence at the start and the end of a recording.
    TrimSilence,
    /// Evens the loudness out (EBU R128, to -16 LUFS).
    Normalize,
    /// A smaller file: lower bit rates, and a video no larger than 1080p.
    Compress,
    /// Cuts the black at the start and the end of a video.
    TrimBlack,
    /// Drops the sound, copying the picture as it is.
    Mute,
    /// Turns a video a quarter turn clockwise.
    Rotate,
    /// Stills from a video, into a folder: one a second, or 100 spread over a longer one.
    Frames,
}

impl Tool {
    pub fn from_label(label: &str, video: bool) -> Option<Tool> {
        Some(match (label, video) {
            ("Trim", false) => Tool::TrimSilence,
            ("Normalize", false) => Tool::Normalize,
            ("Compress", _) => Tool::Compress,
            ("Trim", true) => Tool::TrimBlack,
            ("Mute", true) => Tool::Mute,
            ("Rotate", true) => Tool::Rotate,
            ("Frames", true) => Tool::Frames,
            _ => return None,
        })
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Tool::TrimSilence | Tool::TrimBlack => "trimmed",
            Tool::Normalize => "normalized",
            Tool::Compress => "compressed",
            Tool::Mute => "muted",
            Tool::Rotate => "rotated",
            Tool::Frames => "frames",
        }
    }
}

/// The format a file of `ext` is saved in by a tool or a merge: its own where FFmpeg writes
/// it, else MP4 or M4A; Compress makes uncompressed and lossless sound M4A.
pub fn tool_target(ext: &str, video: bool, tool: Option<Tool>) -> Target {
    let same = match Target::from_label(&ext.to_uppercase()) {
        Some(target) if target != Target::Gif && target.is_video() == video => target,
        _ if video => Target::Mp4,
        _ => Target::M4a,
    };
    match (tool, same) {
        (Some(Tool::Compress), Target::Wav | Target::Flac | Target::Aiff) => Target::M4a,
        _ => same,
    }
}

/// The extension of what `tool` makes of a file of `ext`; `None` for a folder of frames.
pub fn tool_extension(ext: &str, video: bool, tool: Tool) -> Option<String> {
    match tool {
        Tool::Frames => None,
        // The streams are copied as they are, into the same kind of file.
        Tool::Mute => Some(ext.to_owned()),
        _ => Some(tool_target(ext, video, Some(tool)).extension().to_owned()),
    }
}

pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| command_on_path("ffmpeg"))
}

/// What a media file holds, as FFmpeg reads it from its header.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Probe {
    /// How long it plays, in seconds, where the file says.
    pub seconds: Option<f64>,
    pub video: bool,
    pub audio: bool,
    /// The sound's sample rate.
    pub rate: Option<u32>,
    /// The picture's width and height.
    pub size: Option<(u32, u32)>,
}

pub fn probe(input: &Path) -> io::Result<Probe> {
    let mut command = Command::new("ffmpeg");
    let said = crate::actions::quiet(&mut command)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(input)
        .stdin(Stdio::null())
        .output()?;
    Ok(read_probe(&String::from_utf8_lossy(&said.stderr)))
}

/// The probe in what `ffmpeg -i` says about its input.
fn read_probe(said: &str) -> Probe {
    let mut probe = Probe::default();
    for line in said.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Duration: ") {
            probe.seconds = rest.split(',').next().and_then(clock);
        } else if line.starts_with("Stream #") {
            // A cover picture is a video stream to FFmpeg, but not a video.
            if line.contains(": Video: ") && !line.contains("(attached pic)") {
                probe.video = true;
                probe.size = probe.size.or_else(|| {
                    line.split(", ").find_map(|part| {
                        let (width, height) = part.split_whitespace().next()?.split_once('x')?;
                        Some((width.parse().ok()?, height.parse().ok()?))
                            .filter(|&(width, height)| width > 0 && height > 0)
                    })
                });
            }
            if line.contains(": Audio: ") {
                probe.audio = true;
                probe.rate = probe.rate.or_else(|| {
                    line.split(", ")
                        .find_map(|part| part.strip_suffix(" Hz")?.trim().parse().ok())
                });
            }
        }
    }
    probe
}

/// "01:02:03.45" in seconds.
fn clock(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    for part in text.trim().split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(seconds)
}

/// FFmpeg logging at `level`, writing over what is there, and saying how far it has got on its
/// standard output.
fn ffmpeg(level: &str) -> Command {
    let mut command = Command::new("ffmpeg");
    crate::actions::quiet(&mut command).args([
        "-hide_banner",
        "-loglevel",
        level,
        "-nostdin",
        "-y",
        "-progress",
        "pipe:1",
        "-nostats",
    ]);
    command
}

/// Converts one supported input to `output`, which must end in the target extension. FFmpeg is
/// stopped if the job is cancelled.
pub fn convert(input: &Path, target: Target, output: &Path, progress: &Progress) -> io::Result<()> {
    let probe = probe(input)?;
    let mut command = ffmpeg("error");
    command
        .arg("-i")
        .arg(input)
        .args(KEEP_INFO)
        .args(target.codecs(false, &probe, None))
        .arg(output);
    run(
        &mut command,
        probe.seconds,
        progress,
        "FFmpeg could not convert this file",
    )?;
    made(output)
}

/// Applies `tool` to `input`, saving what it makes in the folder `dir`: a file "output.ext" in
/// the format [`tool_extension`] names for what the file holds, or for Frames the stills in `dir`
/// itself. Returns where it is.
pub fn apply(input: &Path, tool: Tool, dir: &Path, progress: &Progress) -> io::Result<PathBuf> {
    let probe = probe(input)?;
    // What the file holds decides, not its name: a .webm may be sound alone.
    let tool = match tool {
        Tool::TrimBlack if !probe.video => Tool::TrimSilence,
        tool => tool,
    };
    if matches!(tool, Tool::Rotate | Tool::Frames) && !probe.video {
        return Err(io::Error::other("this file has no picture"));
    }
    let ext = crate::catalog::ext_of(input);
    let target = tool_target(&ext, probe.video, Some(tool));
    let output = match tool_extension(&ext, probe.video, tool) {
        Some(ext) => dir.join(format!("output.{ext}")),
        None => dir.to_owned(),
    };
    let failed = "FFmpeg could not change this file";
    let mut command = ffmpeg("error");
    match tool {
        Tool::TrimSilence | Tool::TrimBlack => {
            progress.set_steps(2);
            let mut look = ffmpeg("info");
            look.arg("-i").arg(input);
            let (start, end, near) = if tool == Tool::TrimSilence {
                look.args(["-vn", "-af", "silencedetect=noise=-50dB:d=0.5"]);
                ("silence_start:", "silence_end:", NEAR_SILENCE)
            } else {
                look.args(["-an", "-vf", "blackdetect=d=0.1:pix_th=0.10"]);
                ("black_start:", "black_end:", NEAR_BLACK)
            };
            look.args(["-f", "null", "-"]);
            let said = run(&mut look, probe.seconds, progress, failed)?;
            let (from, to) = kept(&spans(&said, start, end), probe.seconds, near);
            let length = to.or(probe.seconds).map(|to| to - from);
            if from <= 0.0 && to.is_none() {
                return Err(io::Error::other(if tool == Tool::TrimSilence {
                    "there is no silence at the start or the end"
                } else {
                    "there is no black at the start or the end"
                }));
            }
            if length.is_some_and(|length| length <= 0.0) {
                return Err(io::Error::other("nothing would be left"));
            }
            progress.next_step();
            command
                .args(["-ss", &format!("{from:.3}")])
                .arg("-i")
                .arg(input);
            if let Some(length) = length {
                command.args(["-t", &format!("{length:.3}")]);
            }
            command
                .args(KEEP_INFO)
                .args(target.codecs(false, &probe, None))
                .arg(&output);
            run(&mut command, length, progress, failed)?;
        }
        Tool::Normalize => {
            // Loudnorm works at 192 kHz; the file keeps its own rate (Opus only knows 48 kHz).
            let rate = match target {
                Target::Opus => 48_000,
                _ => probe.rate.unwrap_or(48_000),
            };
            command
                .arg("-i")
                .arg(input)
                .args(KEEP_INFO)
                .args([
                    "-af",
                    "loudnorm=I=-16:TP=-1.5:LRA=11",
                    "-ar",
                    &rate.to_string(),
                ])
                .args(target.codecs(false, &probe, None))
                .arg(&output);
            run(&mut command, probe.seconds, progress, failed)?;
        }
        Tool::Compress => {
            command
                .arg("-i")
                .arg(input)
                .args(KEEP_INFO)
                .args(target.codecs(true, &probe, None))
                .arg(&output);
            run(&mut command, probe.seconds, progress, failed)?;
            if std::fs::metadata(&output)?.len() >= std::fs::metadata(input)?.len() {
                return Err(io::Error::other("this file is already as small as it gets"));
            }
        }
        Tool::Mute => {
            if !probe.audio {
                return Err(io::Error::other("this video has no sound"));
            }
            // Data tracks (a phone's or an action camera's readings) don't go into every file.
            command
                .arg("-i")
                .arg(input)
                .args(["-map", "0", "-map", "-0:a", "-map", "-0:d", "-c", "copy"])
                .args(KEEP_INFO)
                .arg(&output);
            run(&mut command, probe.seconds, progress, failed)?;
        }
        Tool::Rotate => {
            command
                .arg("-i")
                .arg(input)
                .args(KEEP_INFO)
                .args(target.codecs(false, &probe, Some("transpose=clock")));
            // The sound goes along as it is, where it stays in the same kind of file.
            if target.extension() == ext {
                command.args(["-c:a", "copy"]);
            }
            command.arg(&output);
            run(&mut command, probe.seconds, progress, failed)?;
        }
        Tool::Frames => {
            let rate = match probe.seconds {
                Some(seconds) if seconds > MOST_FRAMES => MOST_FRAMES / seconds,
                _ => 1.0,
            };
            // FFmpeg numbers the stills in place of %03d and takes any other % in the path for
            // a mistake: the stills are named from inside the folder, a % in the name doubled.
            let stem = crate::catalog::stem_of(input).replace('%', "%%");
            command
                .current_dir(&output)
                .arg("-i")
                .arg(std::path::absolute(input)?)
                .args(["-an", "-vf", &format!("fps={rate:.6}"), "-q:v", "2"])
                .args(["-frames:v", &(MOST_FRAMES as u32).to_string()])
                .arg(format!("{stem} %03d.jpg"));
            run(&mut command, probe.seconds, progress, failed)?;
            if std::fs::read_dir(&output)?.next().is_none() {
                return Err(io::Error::other("FFmpeg produced no frames"));
            }
            return Ok(output);
        }
    }
    made(&output)?;
    Ok(output)
}

/// The recordings `inputs`, one after another, saved to `output` as `target`.
pub fn merge(
    inputs: &[PathBuf],
    target: Target,
    output: &Path,
    progress: &Progress,
) -> io::Result<()> {
    let mut length = Some(0.0);
    let mut command = ffmpeg("error");
    for input in inputs {
        let seconds = probe(input)?.seconds;
        length = length.zip(seconds).map(|(a, b)| a + b);
        command.arg("-i").arg(input);
    }
    let streams: String = (0..inputs.len()).map(|i| format!("[{i}:a:0]")).collect();
    let join = format!("{streams}concat=n={}:v=0:a=1[joined]", inputs.len());
    command
        .args([
            "-filter_complex",
            &join,
            "-map",
            "[joined]",
            "-map_metadata",
            "0",
        ])
        .args(target.codecs(false, &Probe::default(), None))
        .arg(output);
    run(
        &mut command,
        length,
        progress,
        "FFmpeg could not join these files",
    )?;
    made(output)
}

/// The spans FFmpeg reported between `start` and `end` keys ("silence_start: 0",
/// "black_end:2.0"), in order; the last may run to the end.
fn spans(said: &str, start: &str, end: &str) -> Vec<(f64, Option<f64>)> {
    let number = |line: &str, key: &str| -> Option<f64> {
        let rest = line[line.find(key)? + key.len()..].trim_start();
        let digits = rest
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .unwrap_or(rest.len());
        rest[..digits].parse().ok()
    };
    let mut spans: Vec<(f64, Option<f64>)> = Vec::new();
    for line in said.lines() {
        if let Some(at) = number(line, start) {
            spans.push((at, None));
        }
        if let Some(at) = number(line, end)
            && let Some(last) = spans.last_mut().filter(|span| span.1.is_none())
        {
            last.1 = Some(at);
        }
    }
    spans
}

/// What is left between a span at the very start and one running to the very end (within
/// `near` seconds of either): from where, and to where (`None`: to the end).
fn kept(spans: &[(f64, Option<f64>)], length: Option<f64>, near: f64) -> (f64, Option<f64>) {
    let from = spans
        .first()
        .filter(|(start, _)| *start <= near)
        .map_or(0.0, |(_, end)| end.or(length).unwrap_or(0.0));
    let to = spans
        .last()
        .filter(|(start, end)| {
            *start > near
                && match (end, length) {
                    (None, _) => true,
                    (Some(end), Some(length)) => *end >= length - near,
                    (Some(_), None) => false,
                }
        })
        .map(|(start, _)| *start);
    (from.max(0.0), to)
}

/// Runs `command` to its end, stopping it if the job is cancelled, and moving `progress` along
/// as FFmpeg gets through `seconds` of media. Returns what FFmpeg said on its error output.
fn run(
    command: &mut Command,
    seconds: Option<f64>,
    progress: &Progress,
    failed: &str,
) -> io::Result<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let total = seconds
        .filter(|s| s.is_finite() && *s > 0.0)
        .map(|s| (s * 1e6) as u64);
    progress.begin(total.unwrap_or(0));
    std::thread::scope(|scope| {
        // Read as it comes, so a chatty FFmpeg never waits on a full pipe.
        let said = scope.spawn(move || {
            let mut text = Vec::new();
            let _ = stderr.read_to_end(&mut text);
            String::from_utf8_lossy(&text).into_owned()
        });
        scope.spawn(move || {
            let mut reached = 0;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Some(at) = line
                    .strip_prefix("out_time_us=")
                    .and_then(|at| at.trim().parse::<u64>().ok())
                else {
                    continue;
                };
                let at = total.map_or(at, |total| at.min(total));
                if at > reached {
                    progress.add(at - reached);
                    reached = at;
                }
            }
        });
        let status = loop {
            let waited = child.try_wait();
            if let Ok(Some(status)) = waited {
                break status;
            }
            // Stopped when cancelled, or when it can't be waited for (the readers wait for it).
            if progress.is_cancelled() || waited.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                progress.check()?;
                waited?;
            }
            std::thread::sleep(CHECK_EVERY);
        };
        let said = said.join().unwrap_or_default();
        if status.success() {
            Ok(said)
        } else {
            Err(io::Error::other(command_error(&said, failed)))
        }
    })
}

/// Whether FFmpeg left a file there with something in it.
fn made(output: &Path) -> io::Result<()> {
    let metadata = std::fs::metadata(output)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(io::Error::other("FFmpeg produced an empty file"));
    }
    Ok(())
}

fn command_on_path(name: &str) -> bool {
    crate::actions::has(name)
}

fn command_error(said: &str, fallback: &str) -> String {
    said.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `seconds` of 16-bit mono sound at 8 kHz: silent but for a tone in `loud` (in seconds).
    fn wav(seconds: f32, loud: std::ops::Range<f32>) -> Vec<u8> {
        let rate = 8000u32;
        let samples = (seconds * rate as f32) as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        for i in 0..samples {
            let t = i as f32 / rate as f32;
            let sample = if loud.contains(&t) {
                ((t * 440.0 * std::f32::consts::TAU).sin() * 12_000.0) as i16
            } else {
                0
            };
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }

    #[test]
    fn a_cancelled_conversion_stops_ffmpeg() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("quiet.wav");
        std::fs::write(&input, wav(1.0, 0.0..0.0)).unwrap();

        let progress = Progress::default();
        let output = dir.path().join("quiet.flac");
        convert(&input, Target::Flac, &output, &progress).unwrap();
        assert!(output.is_file());
        assert!(progress.fraction() > 0.9, "{}", progress.fraction());

        progress.cancel();
        let again = dir.path().join("again.flac");
        let error = convert(&input, Target::Flac, &again, &progress).unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
    }

    #[test]
    fn ffmpeg_says_what_a_file_holds() {
        let said = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'clip.mp4':\n  Duration: 00:01:02.50, \
                    start: 0.000000, bitrate: 1205 kb/s\n  Stream #0:0[0x1](und): Video: h264 \
                    (High), yuv420p, 1280x720, 1070 kb/s, 30 fps\n  Stream #0:1[0x2](und): \
                    Audio: aac (LC), 44100 Hz, stereo, fltp, 128 kb/s (default)\n";
        assert_eq!(
            read_probe(said),
            Probe {
                seconds: Some(62.5),
                video: true,
                audio: true,
                rate: Some(44_100),
                size: Some((1280, 720)),
            }
        );
        let song = "  Duration: N/A, bitrate: N/A\n  Stream #0:0: Audio: mp3, 48000 Hz, stereo\n  \
                    Stream #0:1: Video: mjpeg, 600x600 (attached pic)\n";
        let song = read_probe(song);
        assert!(song.audio && !song.video && song.seconds.is_none());
    }

    #[test]
    fn the_ends_are_cut_where_the_spans_touch_them() {
        let said = "[silencedetect @ 0x1] silence_start: 0\n[silencedetect @ 0x1] silence_end: \
                    1.5 | silence_duration: 1.5\n[silencedetect @ 0x1] silence_start: 4.25\n\
                    [silencedetect @ 0x1] silence_start: 8\n";
        let found = spans(said, "silence_start:", "silence_end:");
        assert_eq!(found, [(0.0, Some(1.5)), (4.25, None), (8.0, None)]);
        // Only the span running to the end counts; one in the middle is left alone.
        assert_eq!(kept(&found, Some(10.0), NEAR_SILENCE), (1.5, Some(8.0)));
        let black = "[blackdetect @ 0x2] black_start:2 black_end:3 black_duration:1\n";
        let found = spans(black, "black_start:", "black_end:");
        assert_eq!(kept(&found, Some(10.0), NEAR_SILENCE), (0.0, None));
        assert_eq!(
            kept(&[(0.0, Some(10.0))], Some(10.0), NEAR_SILENCE),
            (10.0, None)
        );
        // FFmpeg writes tiny times with an exponent.
        let tiny = "[silencedetect @ 0x1] silence_start: 2.26757e-05\n";
        assert_eq!(
            spans(tiny, "silence_start:", "silence_end:"),
            [(2.26757e-05, None)]
        );
    }

    #[test]
    fn trim_cuts_the_silence_off_both_ends() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("talk.wav");
        std::fs::write(&input, wav(4.0, 1.0..3.0)).unwrap();
        let progress = Progress::default();
        let output = apply(&input, Tool::TrimSilence, dir.path(), &progress).unwrap();
        assert_eq!(output, dir.path().join("output.wav"));
        let left = probe(&output).unwrap().seconds.unwrap();
        assert!((1.8..2.4).contains(&left), "{left}");

        let quiet = dir.path().join("loud.wav");
        std::fs::write(&quiet, wav(1.0, 0.0..1.0)).unwrap();
        let error = apply(&quiet, Tool::TrimSilence, dir.path(), &progress).unwrap_err();
        assert_eq!(
            error.to_string(),
            "there is no silence at the start or the end"
        );
    }

    #[test]
    fn tracks_join_into_one() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wav");
        let b = dir.path().join("b.wav");
        std::fs::write(&a, wav(1.0, 0.0..1.0)).unwrap();
        std::fs::write(&b, wav(2.0, 0.0..2.0)).unwrap();
        let output = dir.path().join("joined.wav");
        merge(&[a, b], Target::Wav, &output, &Progress::default()).unwrap();
        let length = probe(&output).unwrap().seconds.unwrap();
        assert!((2.9..3.1).contains(&length), "{length}");
    }

    /// A video of black, a test picture and black again (a second, two, a second), with a tone.
    fn clip(dir: &Path) -> PathBuf {
        let clip = dir.join("clip.mp4");
        let made = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "color=black:size=320x240:rate=25:duration=1",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=25:duration=2",
            ])
            .args([
                "-f",
                "lavfi",
                "-i",
                "color=black:size=320x240:rate=25:duration=1",
            ])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=4"])
            .args(["-filter_complex", "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]"])
            .args([
                "-map", "[v]", "-map", "3:a", "-c:v", "mpeg4", "-pix_fmt", "yuv420p",
            ])
            .args(["-c:a", "aac", "-shortest"])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(made.success());
        clip
    }

    #[test]
    fn video_tools_do_what_they_say() {
        if !available() {
            return;
        }
        // FFmpeg reads a % in a path as the start of a frame number.
        let dir = tempfile::tempdir().unwrap();
        let videos = dir.path().join("100% done");
        std::fs::create_dir(&videos).unwrap();
        let clip = clip(&videos);
        let progress = Progress::default();
        let into = |name: &str| {
            let folder = dir.path().join(name);
            std::fs::create_dir(&folder).unwrap();
            folder
        };

        let trimmed = apply(&clip, Tool::TrimBlack, &into("trimmed"), &progress).unwrap();
        let left = probe(&trimmed).unwrap().seconds.unwrap();
        assert!((1.8..2.3).contains(&left), "{left}");

        let muted = apply(&clip, Tool::Mute, &into("muted"), &progress).unwrap();
        let muted = probe(&muted).unwrap();
        assert!(muted.video && !muted.audio);

        let rotated = apply(&clip, Tool::Rotate, &into("rotated"), &progress).unwrap();
        let said = Command::new("ffmpeg")
            .arg("-i")
            .arg(&rotated)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&said.stderr).contains("240x320"));

        let frames = apply(&clip, Tool::Frames, &into("frames"), &progress).unwrap();
        let mut stills: Vec<String> = std::fs::read_dir(&frames)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        stills.sort();
        assert_eq!(stills.first().map(String::as_str), Some("clip 001.jpg"));
        assert!((4..=5).contains(&stills.len()), "{stills:?}");
    }

    #[test]
    fn what_a_file_holds_decides_and_odd_sizes_encode() {
        if !available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let make = |name: &str, args: &[&str]| {
            let path = dir.path().join(name);
            let made = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
                .args(args)
                .arg(&path)
                .status()
                .unwrap();
            assert!(made.success());
            path
        };
        // Sound alone in a .webm, as downloads often are: compressed as sound.
        let song = make(
            "song.webm",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-c:a",
                "libopus",
                "-b:a",
                "160k",
            ],
        );
        let out = dir.path().join("song");
        std::fs::create_dir(&out).unwrap();
        let smaller = apply(&song, Tool::Compress, &out, &Progress::default()).unwrap();
        assert_eq!(smaller.extension().unwrap(), "m4a");
        // A picture 321 pixels wide becomes 320 for H.264.
        let odd = make(
            "odd.mkv",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=321x241:rate=10:duration=1",
                "-c:v",
                "ffv1",
            ],
        );
        let mp4 = dir.path().join("odd.mp4");
        convert(&odd, Target::Mp4, &mp4, &Progress::default()).unwrap();
        assert_eq!(probe(&mp4).unwrap().size, Some((320, 240)));
    }

    #[test]
    fn tools_keep_the_format_where_they_can() {
        assert_eq!(
            tool_target("mp3", false, Some(Tool::Normalize)),
            Target::Mp3
        );
        assert_eq!(tool_target("wav", false, Some(Tool::Compress)), Target::M4a);
        assert_eq!(tool_target("wma", false, None), Target::M4a);
        assert_eq!(tool_target("avi", true, Some(Tool::Rotate)), Target::Mp4);
        assert_eq!(
            tool_extension("avi", true, Tool::Mute).as_deref(),
            Some("avi")
        );
        assert_eq!(tool_extension("mkv", true, Tool::Frames), None);
    }
}
