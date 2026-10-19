//! The click on every transition of the wheel: opening, switching modes, moving onto another
//! wedge, picking one. PipeWire's `pw-play` (or PulseAudio's `paplay`) plays it, so nothing
//! links against the audio stack. `MCGYVER_SOUND=0` turns it off.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CLICK: &[u8] = include_bytes!("../assets/sounds/click.mp3");
/// Clicks closer together than this, as when sweeping across wedges, play once.
const MIN_GAP: Duration = Duration::from_millis(40);

pub struct Clicker {
    /// The player and the click written out for it; `None` keeps quiet.
    player: Option<(&'static str, PathBuf)>,
    last: Option<Instant>,
}

impl Clicker {
    pub fn new() -> Self {
        let wanted = std::env::var_os("MCGYVER_SOUND").is_none_or(|v| v != "0");
        let player = ["pw-play", "paplay"]
            .into_iter()
            .find(|p| crate::actions::has(p))
            .filter(|_| wanted)
            .and_then(|player| write_click().map(|path| (player, path)));
        Self { player, last: None }
    }

    pub fn play(&mut self) {
        let Some((player, path)) = &self.player else {
            return;
        };
        let now = Instant::now();
        if self.last.is_some_and(|last| now < last + MIN_GAP) {
            return;
        }
        self.last = Some(now);
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
}

/// The click as a file the player can open, in the user's private runtime directory.
fn write_click() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())?;
    let path = dir.join("mcgyver-click.mp3");
    std::fs::write(&path, CLICK).ok()?;
    Some(path)
}
