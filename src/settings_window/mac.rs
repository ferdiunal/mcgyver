//! The Permissions card on macOS. Holding a modifier is read from its state, which macOS may
//! give without asking; if it doesn't, Input Monitoring is what lets the app read keys while
//! another app is in front, so the card offers it rather than insisting on it.

use std::fs::File;
use std::time::{Duration, Instant};

use crate::glass::{GREEN, TEXT_MUTED};
use crate::i18n::tr;
use crate::settings::{APP_ID, Trigger, config_home};

use super::{Row, row};

pub(super) const TRIGGER_HELP: &str = "Hold it while dragging files to open the wheel.";
const INPUT_HELP: &str = "Only needed if holding the key doesn't open the wheel. Reopen McGyver \
                          after allowing it.";
/// System Settings → Privacy & Security → Input Monitoring.
const INPUT_MONITORING: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
/// How often the permission is looked at again while the window is open.
pub(super) const CHECK_EVERY: Duration = Duration::from_secs(1);

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

pub(super) struct Permissions {
    allowed: bool,
    checked_at: Instant,
}

impl Permissions {
    pub(super) fn new() -> Self {
        Permissions {
            allowed: crate::mac::input_monitoring(),
            checked_at: Instant::now(),
        }
    }

    pub(super) fn update(&mut self) {
        if self.checked_at.elapsed() >= CHECK_EVERY {
            self.allowed = crate::mac::input_monitoring();
            self.checked_at = Instant::now();
        }
    }

    pub(super) fn take_note(&mut self) -> Option<String> {
        None
    }

    /// Input Monitoring is optional, so the card never comes first.
    pub(super) fn all_granted(&self) -> bool {
        true
    }

    pub(super) fn card(&mut self, ui: &mut egui::Ui, _trigger: Trigger) {
        if self.allowed {
            ui.label(egui::RichText::new(tr("Everything is ready")).color(GREEN));
        }
        let buttons = [tr("Ask"), tr("Open System Settings")];
        let shown = Row {
            title: tr("Input Monitoring"),
            help: tr(INPUT_HELP),
            color: if self.allowed { GREEN } else { TEXT_MUTED },
            status: if self.allowed {
                tr("On")
            } else {
                tr("Not allowed")
            },
            buttons: if self.allowed { &[] } else { &buttons },
            busy: false,
        };
        match row(ui, &shown) {
            // The first time, macOS shows its own prompt; after that only System Settings can.
            Some(0) => {
                crate::mac::request_input_monitoring();
            }
            Some(_) => {
                let _ = std::process::Command::new("open")
                    .arg(INPUT_MONITORING)
                    .status();
            }
            None => {}
        }
    }
}
