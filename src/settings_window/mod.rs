//! The settings window (`--settings`), a process of its own so the overlay never waits on it:
//! language, trigger key, starting at login, and the permissions the desktop grants, each with
//! the button that fixes it (`linux`, `mac`, `windows`). Each change is saved at once and the
//! running app picks it up.

use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};

use crate::glass::{ACCENT, TEXT, TEXT_MUTED};
use crate::i18n::{self, Lang, tr, trf};
use crate::settings::{self, APP_ID, APP_NAME, Settings, Trigger};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

use platform::{CHECK_EVERY, Permissions, TRIGGER_HELP};

/// Something that needs one more step, as a restart.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const AMBER: Color32 = Color32::from_rgb(226, 146, 18);
/// How long "Saved" stays up.
const NOTE_FOR: Duration = Duration::from_secs(3);
const BACKGROUND: Color32 = Color32::from_rgb(242, 243, 245);
const CARD: Color32 = Color32::WHITE;

pub fn run() -> Result<(), String> {
    // One window at a time, however it was opened.
    let Some(_claim) = platform::claim() else {
        println!("The settings are open already.");
        return Ok(());
    };
    settings::migrate();
    let settings = Settings::load();
    i18n::set(settings.language);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title())
        .with_app_id(APP_ID)
        .with_inner_size([440.0, 680.0])
        .with_min_inner_size([380.0, 360.0]);
    if let Some(rgba) = crate::icon::rgba(64) {
        viewport = viewport.with_icon(egui::IconData {
            rgba,
            width: 64,
            height: 64,
        });
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            crate::glass::install_fonts(&cc.egui_ctx);
            style(&cc.egui_ctx);
            Ok(Box::new(Window::new(settings)))
        }),
    )
    .map_err(|e| e.to_string())
}

fn title() -> String {
    format!("{} — {APP_NAME}", tr("Settings"))
}

fn style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 6.0);
        let v = &mut style.visuals;
        v.panel_fill = BACKGROUND;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.inactive.corner_radius = CornerRadius::same(8);
        v.widgets.hovered.corner_radius = CornerRadius::same(8);
        v.widgets.active.corner_radius = CornerRadius::same(8);
    });
}

struct Window {
    settings: Settings,
    start_at_login: bool,
    permissions: Permissions,
    /// The first frame is still to come (a menu bar app's window must be brought forward).
    first_frame: bool,
    /// What the last change came to, and when.
    note: Option<(String, Instant)>,
}

impl Window {
    fn new(settings: Settings) -> Self {
        Window {
            settings,
            start_at_login: settings::starts_at_login(),
            permissions: Permissions::new(),
            first_frame: true,
            note: None,
        }
    }

    /// Saves, and has the running app pick the change up.
    fn changed(&mut self) {
        i18n::set(self.settings.language);
        let said = match self.settings.save() {
            Err(e) => trf("Couldn't save: {}", &[&e]),
            Ok(()) if platform::tell_app() => tr("Saved").to_owned(),
            Ok(()) => trf(
                "{} isn't running; it will use these when it starts.",
                &[&APP_NAME],
            ),
        };
        self.note(said);
    }

    fn note(&mut self, said: String) {
        self.note = Some((said, Instant::now()));
    }
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.first_frame) {
            platform::bring_forward(ui.ctx());
        }
        let before = self.settings;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BACKGROUND).inner_margin(Margin::same(18)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.cards(ui));
            });
        if self.settings != before {
            self.changed();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title()));
        }
    }
}

impl Window {
    fn cards(&mut self, ui: &mut egui::Ui) {
        self.permissions.update();
        if let Some(said) = self.permissions.take_note() {
            self.note(said);
        }
        ui.ctx().request_repaint_after(CHECK_EVERY);
        // Something missing comes first; once all is granted it moves to the bottom.
        let all_granted = self.permissions.all_granted();
        let trigger = self.settings.trigger;

        ui.label(RichText::new(tr("Settings")).size(22.0).strong());
        ui.add_space(6.0);
        if !all_granted {
            card(ui, tr("Permissions"), None, |ui| {
                self.permissions.card(ui, trigger)
            });
        }

        card(ui, tr("Language"), None, |ui| {
            ui.horizontal(|ui| {
                for lang in Lang::ALL {
                    segment(ui, &mut self.settings.language, lang, lang.name());
                }
            });
        });

        card(ui, tr("Trigger key"), Some(tr(TRIGGER_HELP)), |ui| {
            ui.horizontal_wrapped(|ui| {
                for &key in Trigger::ALL {
                    segment(ui, &mut self.settings.trigger, key, key.label());
                }
            });
        });

        card(ui, tr("Start at login"), None, |ui| {
            if toggle(ui, &mut self.start_at_login).changed() {
                match settings::set_start_at_login(self.start_at_login) {
                    Ok(()) => self.note(tr("Saved").to_owned()),
                    Err(e) => {
                        self.start_at_login = settings::starts_at_login();
                        self.note(trf("Couldn't save: {}", &[&e]));
                    }
                }
            }
        });

        if all_granted {
            card(ui, tr("Permissions"), None, |ui| {
                self.permissions.card(ui, trigger)
            });
        }

        if let Some((said, at)) = &self.note {
            let left = NOTE_FOR.saturating_sub(at.elapsed());
            if left.is_zero() {
                self.note = None;
            } else {
                ui.add_space(4.0);
                ui.label(RichText::new(said.as_str()).color(TEXT_MUTED));
                ui.ctx().request_repaint_after(left);
            }
        }
    }
}

/// One settings window at a time, however it was opened: the guard holds a lock on a file in
/// the settings folder. Without the folder, the window opens anyway. (On Linux, the session bus
/// does this.)
#[cfg(any(target_os = "macos", windows))]
fn claim_lock_file() -> Option<Option<std::fs::File>> {
    let Some(dir) = settings::config_home().map(|d| d.join(APP_ID)) else {
        return Some(None);
    };
    let _ = std::fs::create_dir_all(&dir);
    // Not emptied on opening: Windows refuses to empty a file another process has locked.
    let Ok(file) = std::fs::File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("settings.lock"))
    else {
        return Some(None);
    };
    match file.try_lock() {
        Ok(()) => Some(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => None,
        Err(_) => Some(Some(file)),
    }
}

/// One permission as the card shows it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
struct Row<'a> {
    title: &'a str,
    help: &'a str,
    color: Color32,
    status: &'a str,
    /// The buttons that would fix it, already in the current language.
    buttons: &'a [&'a str],
    /// Something is being done about it: a spinner instead of the buttons.
    busy: bool,
}

/// Draws a permission: a dot in its colour, its title, where it stands and why it matters, and
/// its buttons. Returns the index of the one clicked.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn row(ui: &mut egui::Ui, row: &Row) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 5.0, row.color);
        ui.label(RichText::new(row.title).strong());
    });
    ui.indent(row.title, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(row.status).size(12.5).color(row.color));
            ui.label(RichText::new(row.help).size(12.5).color(TEXT_MUTED));
        });
        if row.buttons.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            if row.busy {
                ui.spinner();
                return;
            }
            for (i, label) in row.buttons.iter().enumerate() {
                if ui.button(*label).clicked() {
                    clicked = Some(i);
                }
            }
        });
    });
    clicked
}

/// A white panel with a heading, an optional line under it, and the controls.
fn card(ui: &mut egui::Ui, heading: &str, help: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(12))
        .stroke(Stroke::new(1.0, Color32::from_black_alpha(18)))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(heading).size(15.0).strong());
            if let Some(help) = help {
                ui.label(RichText::new(help).size(12.5).color(TEXT_MUTED));
            }
            ui.add_space(2.0);
            add(ui);
        });
    ui.add_space(4.0);
}

/// One button of a row where exactly one is on.
fn segment<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, this: T, text: &str) {
    let on = *value == this;
    let label = RichText::new(text).color(if on { Color32::WHITE } else { TEXT });
    let button = egui::Button::new(label)
        .fill(if on { ACCENT } else { BACKGROUND })
        .min_size(Vec2::new(84.0, 32.0));
    if ui.add(button).clicked() {
        *value = this;
    }
}

/// An on/off switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(42.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let track = BACKGROUND.lerp_to_gamma(ACCENT, t);
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(12),
        track,
        Stroke::new(1.0, Color32::from_black_alpha(25)),
        egui::StrokeKind::Inside,
    );
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    painter.circle(
        egui::pos2(x, rect.center().y),
        9.0,
        Color32::WHITE,
        Stroke::new(1.0, Color32::from_black_alpha(30)),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::translate;

    #[test]
    fn everything_the_window_says_has_turkish() {
        for text in [
            "Settings",
            "Language",
            "Trigger key",
            TRIGGER_HELP,
            "Start at login",
            "Permissions",
            "Saved",
            "Couldn't save: {}",
            "{} isn't running; it will use these when it starts.",
        ] {
            assert_ne!(translate(Lang::Turkish, text), text, "{text}");
        }
    }
}
