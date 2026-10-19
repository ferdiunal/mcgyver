//! The click on every transition of the wheel: opening, switching modes, moving onto another
//! wedge, picking one. PipeWire's `pw-play` (or PulseAudio's `paplay`, or macOS's `afplay`)
//! plays it, so nothing links against the audio stack; Windows plays it from memory.
//! `MCGYVER_SOUND=0` turns it off.

#[cfg(not(windows))]
use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(not(windows))]
const CLICK: &[u8] = include_bytes!("../assets/sounds/click.mp3");
/// PlaySound takes WAV only.
#[cfg(windows)]
const CLICK: &[u8] = include_bytes!("../assets/sounds/click.wav");
/// Clicks closer together than this, as when sweeping across wedges, play once.
const MIN_GAP: Duration = Duration::from_millis(40);

pub struct Clicker {
    /// The player and the click written out for it; `None` keeps quiet.
    #[cfg(not(windows))]
    player: Option<(&'static str, PathBuf)>,
    /// Whether to play; Windows plays the click from memory.
    #[cfg(windows)]
    player: Option<()>,
    last: Option<Instant>,
}

impl Clicker {
    pub fn new() -> Self {
        let wanted = std::env::var_os("MCGYVER_SOUND").is_none_or(|v| v != "0");
        #[cfg(not(windows))]
        let player = ["pw-play", "paplay", "afplay"]
            .into_iter()
            .find(|p| crate::actions::has(p))
            .filter(|_| wanted)
            .and_then(|player| write_click().map(|path| (player, path)));
        #[cfg(windows)]
        let player = wanted.then_some(());
        Self { player, last: None }
    }

    pub fn play(&mut self) {
        let Some(player) = &self.player else {
            return;
        };
        let now = Instant::now();
        if self.last.is_some_and(|last| now < last + MIN_GAP) {
            return;
        }
        self.last = Some(now);
        play(player);
    }
}

/// Plays the click without waiting for it; a click still playing is cut short.
#[cfg(windows)]
fn play(_: &()) {
    use windows_sys::Win32::Media::Audio::{SND_ASYNC, SND_MEMORY, SND_NODEFAULT, PlaySoundW};
    // SAFETY: with SND_MEMORY the "name" is the sound itself, which lives as long as the app.
    unsafe {
        PlaySoundW(
            CLICK.as_ptr().cast(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        )
    };
}

/// Has the player play the click, and reaps it when it is done.
#[cfg(not(windows))]
fn play((player, path): &(&'static str, PathBuf)) {
    let mut command = Command::new(player);
    if *player == "pw-play" {
        command.args(["--latency", "20ms", "--media-role", "Notification"]);
    }
    let child = command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || child.wait());
    }
}

/// The click as a file the player can open, in the user's private runtime directory.
#[cfg(not(any(target_os = "macos", windows)))]
fn write_click() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())?;
    let path = dir.join("mcgyver-click.mp3");
    std::fs::write(&path, CLICK).ok()?;
    Some(path)
}

/// The click as a file the player can open, in the temporary folder under a fresh name (made
/// new, so nothing someone else put there is written through). macOS clears it out in time.
#[cfg(target_os = "macos")]
fn write_click() -> Option<PathBuf> {
    use std::io::Write;
    let mut file = tempfile::Builder::new()
        .prefix("mcgyver-click-")
        .suffix(".mp3")
        .tempfile()
        .ok()?;
    file.write_all(CLICK).ok()?;
    file.into_temp_path().keep().ok()
}
