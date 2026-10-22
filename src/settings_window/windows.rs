//! The Permissions card on Windows, which has nothing to ask for: the hooks that hear the
//! trigger key and the mouse need no permission, so the card only says so.

use std::fs::File;
use std::time::Duration;

use crate::glass::GREEN;
use crate::i18n::tr;
use crate::settings::Trigger;

pub(super) const TRIGGER_HELP: &str = "Hold it while dragging files to open the wheel.";
const NOTHING_TO_ALLOW: &str =
    "Windows asks no permission for McGyver to notice the trigger key and the drag.";
/// Nothing changes by itself, so the card is seldom looked at again.
pub(super) const CHECK_EVERY: Duration = Duration::from_secs(60);

/// One settings window at a time.
pub(super) fn claim() -> Option<Option<File>> {
    super::claim_lock_file()
}

/// The running app reads the settings file again by itself when it changes; this only says
/// whether it is running.
pub(super) fn tell_app() -> bool {
    crate::desktop::running()
}

/// The window's app id, which only Linux uses.
pub(super) fn app_id() -> &'static str {
    crate::settings::APP_ID
}

/// A window opened from the tray comes up in front by itself.
pub(super) fn bring_forward(_: &egui::Context) {}

pub(super) struct Permissions;

impl Permissions {
    pub(super) fn new() -> Self {
        Permissions
    }

    pub(super) fn update(&mut self) {}

    pub(super) fn take_note(&mut self) -> Option<String> {
        None
    }

    pub(super) fn all_granted(&self) -> bool {
        true
    }

    pub(super) fn card(&mut self, ui: &mut egui::Ui, _trigger: Trigger) {
        ui.label(egui::RichText::new(tr("Everything is ready")).color(GREEN));
        ui.label(
            egui::RichText::new(tr(NOTHING_TO_ALLOW))
                .size(12.5)
                .color(super::muted()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, translate};

    #[test]
    fn the_card_has_turkish() {
        assert_ne!(translate(Lang::Turkish, NOTHING_TO_ALLOW), NOTHING_TO_ALLOW);
    }
}
