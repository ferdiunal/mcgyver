//! The Permissions card on macOS: Input Monitoring, which lets the app hear the trigger key and
//! the mouse as they change. Without it the app still works, reading their state every 30 ms,
//! so the card asks for it first but nothing waits on it. Allowed while the app runs, it takes
//! a restart, which the card offers.

use std::fs::File;
use std::time::{Duration, Instant};

use crate::glass::{GREEN, RED};
use crate::i18n::tr;
use crate::settings::{APP_ID, Trigger, config_home};

use super::{AMBER, Row, row};

pub(super) const TRIGGER_HELP: &str = "Hold it while dragging files to open the wheel.";
const INPUT_HELP: &str = "Lets McGyver notice the trigger key the moment you press it, and saves \
                          battery. McGyver works without it too.";
const ALREADY_ON: &str = "If it is already switched on there, remove McGyver with − and allow it \
                          again.";
const RESTART_HELP: &str = "Allowed. McGyver starts using it once it restarts.";
/// System Settings → Privacy & Security → Input Monitoring.
const INPUT_MONITORING: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
/// How often the permission is looked at again while the window is open.
pub(super) const CHECK_EVERY: Duration = Duration::from_secs(1);
/// How long a restart may take before the button comes back.
const RESTART_WAIT: Duration = Duration::from_secs(10);

/// One settings window at a time, however it was opened: the guard holds a lock on a file in
/// the settings folder. Without the folder, the window opens anyway.
pub(super) fn claim() -> Option<Option<File>> {
    let Some(dir) = config_home().map(|d| d.join(APP_ID)) else {
        return Some(None);
    };
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = File::create(dir.join("settings.lock")) else {
        return Some(None);
    };
    match file.try_lock() {
        Ok(()) => Some(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => None,
        Err(_) => Some(Some(file)),
    }
}

/// The running app reads the settings file again by itself when it changes; this only says
/// whether it is running.
pub(super) fn tell_app() -> bool {
    crate::mac::running()
}

/// The bundle starts as a menu bar app, whose windows open behind the app in front and can't be
/// switched to: make this one a regular app and bring its window forward.
pub(super) fn bring_forward(ctx: &egui::Context) {
    crate::mac::regular_app();
    crate::mac::activate();
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Where Input Monitoring stands for the running app.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    NotAllowed,
    /// Allowed after the app started: it still reads the keys until it restarts.
    NeedsRestart,
    On,
}

pub(super) struct Permissions {
    state: State,
    checked_at: Instant,
    /// When Restart was pressed, while waiting for the app to come back hearing the keys.
    restarting: Option<Instant>,
}

impl Permissions {
    pub(super) fn new() -> Self {
        Permissions {
            state: state(),
            checked_at: Instant::now(),
            restarting: None,
        }
    }

    pub(super) fn update(&mut self) {
        if self.checked_at.elapsed() >= CHECK_EVERY {
            self.state = state();
            self.checked_at = Instant::now();
            if self.state != State::NeedsRestart
                || self
                    .restarting
                    .is_some_and(|at| at.elapsed() >= RESTART_WAIT)
            {
                self.restarting = None;
            }
        }
    }

    pub(super) fn take_note(&mut self) -> Option<String> {
        None
    }

    /// Until the app hears the keys, the card comes first.
    pub(super) fn all_granted(&self) -> bool {
        self.state == State::On
    }

    pub(super) fn card(&mut self, ui: &mut egui::Ui, _trigger: Trigger) {
        if self.state == State::On {
            ui.label(egui::RichText::new(tr("Everything is ready")).color(GREEN));
        }
        let asking = [tr("Ask"), tr("Open System Settings")];
        let restarting = [tr("Restart McGyver")];
        let help = match self.state {
            State::NotAllowed => format!("{} {}", tr(INPUT_HELP), tr(ALREADY_ON)),
            State::NeedsRestart => tr(RESTART_HELP).to_owned(),
            State::On => tr(INPUT_HELP).to_owned(),
        };
        let shown = Row {
            title: tr("Input Monitoring"),
            help: &help,
            color: match self.state {
                State::NotAllowed => RED,
                State::NeedsRestart => AMBER,
                State::On => GREEN,
            },
            status: match self.state {
                State::NotAllowed => tr("Not allowed"),
                State::NeedsRestart => tr("Needs a restart"),
                State::On => tr("On"),
            },
            buttons: match self.state {
                State::NotAllowed => &asking,
                State::NeedsRestart => &restarting,
                State::On => &[],
            },
            busy: self.restarting.is_some(),
        };
        match (self.state, row(ui, &shown)) {
            // The first time, macOS shows its own prompt; after that only System Settings can.
            (State::NotAllowed, Some(0)) => {
                crate::mac::request_input_monitoring();
            }
            (State::NotAllowed, Some(_)) => {
                let _ = std::process::Command::new("open")
                    .arg(INPUT_MONITORING)
                    .status();
            }
            (State::NeedsRestart, Some(_)) if crate::mac::ask_restart() => {
                self.restarting = Some(Instant::now());
            }
            _ => {}
        }
    }
}

fn state() -> State {
    if !crate::mac::input_monitoring() {
        State::NotAllowed
    } else if crate::mac::keys_heard() == Some(false) {
        State::NeedsRestart
    } else {
        State::On
    }
}
