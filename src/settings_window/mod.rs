//! The settings window (`--settings`), a process of its own so the overlay never waits on it:
//! language, trigger key, the wheel without files and its quick apps, starting at login, the
//! permissions the desktop grants, each with the button that fixes it (`linux`, `mac`,
//! `windows`), and who made the app. Each change is saved at once and the running app picks
//! it up.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Vec2};

use crate::desk::{self, App};
use crate::glass::ACCENT;
use crate::i18n::{self, Lang, tr, trf};
use crate::settings::{self, APP_ID, APP_NAME, Appearance, HOLD_RANGE, Settings, Trigger};
use crate::textures::Textures;

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
/// More quick apps than this crowd the wheel.
const MOST_QUICK_APPS: usize = 8;
/// The quick apps' icons, and the tiles to add one from.
const ROW_ICON: f32 = 28.0;
const TILE: Vec2 = Vec2::new(88.0, 84.0);
const TILE_ICON: f32 = 36.0;
/// Room on the right for the scroll bar, which floats over what is under it.
const SCROLL_GUTTER: i8 = 12;
/// Who made the app, and where to find them.
const AUTHOR: &str = "Ferdi ÜNAL";
const EMAIL: &str = "mail@ferdiunal.tr";
/// Tagged with `utm_source`, so the sites' analytics can tell visits from the app.
const WEB: [(LinkIcon, &str, &str); 2] = [
    (
        LinkIcon::Code,
        "github.com/ferdiunal",
        "https://github.com/ferdiunal?utm_source=mcgyver",
    ),
    (
        LinkIcon::Globe,
        "ferdiunal.tr",
        "https://ferdiunal.tr/?utm_source=mcgyver",
    ),
];
const SPONSOR: &str = "https://github.com/sponsors/ferdiunal";
/// The sponsor banner's colour.
const PINK: Color32 = Color32::from_rgb(219, 97, 162);

/// The window's colours in light or dark.
#[derive(Clone, Copy)]
struct Look {
    background: Color32,
    card: Color32,
    /// Hairlines around cards and controls.
    edge: Color32,
    text: Color32,
    muted: Color32,
    /// Buttons, fields and rows at rest, and under the pointer.
    control: Color32,
    hover: Color32,
}

const LIGHT: Look = Look {
    background: Color32::from_rgb(242, 243, 245),
    card: Color32::WHITE,
    edge: Color32::from_rgba_premultiplied(0, 0, 0, 20),
    text: Color32::from_rgb(29, 29, 31),
    muted: Color32::from_rgb(112, 112, 120),
    control: Color32::from_rgb(242, 243, 245),
    hover: Color32::from_rgb(232, 233, 237),
};

const DARK: Look = Look {
    background: Color32::from_rgb(24, 24, 28),
    card: Color32::from_rgb(36, 36, 42),
    edge: Color32::from_rgba_premultiplied(18, 18, 18, 18),
    text: Color32::from_rgb(238, 238, 242),
    muted: Color32::from_rgb(150, 150, 160),
    control: Color32::from_rgb(48, 48, 56),
    hover: Color32::from_rgb(60, 60, 70),
};

/// The window's colours now: light's or dark's, or on the way between as the glass turns.
fn look() -> Look {
    let t = crate::glass::darkness();
    if t <= 0.0 {
        return LIGHT;
    }
    if t >= 1.0 {
        return DARK;
    }
    let c = |a: Color32, b: Color32| a.lerp_to_gamma(b, t);
    Look {
        background: c(LIGHT.background, DARK.background),
        card: c(LIGHT.card, DARK.card),
        edge: c(LIGHT.edge, DARK.edge),
        text: crate::glass::swap_text(LIGHT.text, DARK.text, t),
        muted: crate::glass::swap_text(LIGHT.muted, DARK.muted, t),
        control: c(LIGHT.control, DARK.control),
        hover: c(LIGHT.hover, DARK.hover),
    }
}

/// Muted text, for the platforms' permission rows.
#[cfg(not(target_os = "macos"))]
fn muted() -> Color32 {
    look().muted
}

pub fn run() -> Result<(), String> {
    run_sized([470.0, 720.0])
}

/// The window at `size` points, as `run` opens it: taller shows more of it at once (for the
/// preview's screenshots).
pub fn run_sized(size: [f32; 2]) -> Result<(), String> {
    // Before the lock: a file held open in the app's folder would keep it from being renamed.
    settings::migrate();
    // One window at a time, however it was opened.
    let Some(_claim) = platform::claim() else {
        println!("The settings are open already.");
        return Ok(());
    };
    let settings = Settings::load();
    i18n::set(settings.language);
    crate::glass::set_dark(settings.appearance.dark());
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title())
        .with_app_id(platform::app_id())
        .with_inner_size(size)
        .with_min_inner_size([400.0, 360.0]);
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
            Ok(Box::new(Window::new(settings, &cc.egui_ctx)))
        }),
    )
    .map_err(|e| e.to_string())
}

fn title() -> String {
    format!("{} — {APP_NAME}", tr("Settings"))
}

/// The look of every widget, light or dark as the glass is.
fn style(ctx: &egui::Context) {
    let look = look();
    let dark = crate::glass::darkness() >= 0.5;
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 6.0);
        style.spacing.slider_width = 220.0;
        let v = &mut style.visuals;
        v.dark_mode = dark;
        v.panel_fill = look.background;
        v.window_fill = look.card;
        v.extreme_bg_color = look.control;
        v.override_text_color = Some(look.text);
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.slider_trailing_fill = true;
        v.hyperlink_color = ACCENT;
        for (widgets, fill) in [
            (&mut v.widgets.inactive, look.control),
            (&mut v.widgets.hovered, look.hover),
            (&mut v.widgets.active, look.hover),
        ] {
            widgets.corner_radius = CornerRadius::same(8);
            widgets.bg_fill = fill;
            widgets.weak_bg_fill = fill;
            widgets.bg_stroke = Stroke::new(1.0, look.edge);
            widgets.fg_stroke = Stroke::new(1.5, look.text);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, look.edge);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, look.text);
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
    /// The installed apps, once a thread has listed them (that can take a moment on Windows).
    apps: Arc<Mutex<Option<Vec<App>>>>,
    /// What the apps to add are narrowed to.
    filter: String,
    icons: Textures,
    /// The app's own icon, for the header.
    logo: Option<egui::TextureHandle>,
    /// Whether the window is dark now, to restyle it when the system's look changes.
    dark: bool,
    /// The glass turned last frame: the window is styled once more, at the turn's end.
    turned: bool,
}

impl Window {
    fn new(settings: Settings, ctx: &egui::Context) -> Self {
        let apps = Arc::new(Mutex::new(None));
        let listed = apps.clone();
        std::thread::spawn(move || {
            let found = desk::installed_apps();
            *listed.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
        });
        let logo = crate::icon::rgba(96).map(|rgba| {
            let image = egui::ColorImage::from_rgba_unmultiplied([96, 96], &rgba);
            ctx.load_texture("logo", image, egui::TextureOptions::LINEAR)
        });
        Window {
            dark: settings.appearance.dark(),
            turned: false,
            settings,
            start_at_login: settings::starts_at_login(),
            permissions: Permissions::new(),
            first_frame: true,
            note: None,
            apps,
            filter: String::new(),
            icons: Textures::new(64),
            logo,
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

    /// Follows the appearance chosen, or the system's as it changes, fading across.
    fn restyle(&mut self, ctx: &egui::Context) {
        let dark = self.settings.appearance.dark();
        if dark != self.dark {
            self.dark = dark;
            crate::glass::turn_dark(dark);
        }
        let turning = crate::glass::turning();
        if turning || std::mem::take(&mut self.turned) {
            style(ctx);
            ctx.request_repaint();
            // Once more after the turn, at its end.
            self.turned = turning;
        }
    }
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.first_frame) {
            platform::bring_forward(ui.ctx());
        }
        self.restyle(ui.ctx());
        // System: look again every second, so a switch elsewhere shows here too.
        if self.settings.appearance == Appearance::System {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
        }
        let before = self.settings.clone();
        egui::CentralPanel::default()
            .frame(Frame::new().fill(look().background).inner_margin(Margin {
                right: 18 - SCROLL_GUTTER,
                ..Margin::same(18)
            }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin {
                            right: SCROLL_GUTTER,
                            ..Margin::ZERO
                        })
                        .show(ui, |ui| self.cards(ui));
                });
            });
        if self.settings != before {
            self.changed();
            self.restyle(ui.ctx());
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

        self.header(ui);
        if !all_granted {
            card(ui, tr("Permissions"), None, |ui| {
                self.permissions.card(ui, trigger)
            });
        }

        card(ui, tr("Appearance"), None, |ui| {
            labelled(ui, tr("Look"), |ui| {
                for appearance in Appearance::ALL {
                    segment(
                        ui,
                        &mut self.settings.appearance,
                        appearance,
                        tr(appearance.label()),
                    );
                }
            });
            labelled(ui, tr("Language"), |ui| {
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

        let held = trf(
            "Hold {} for a moment without dragging files: the wheel opens with your apps, what \
             is playing, the keyboard, the volume, the brightness and the system.",
            &[&trigger.label()],
        );
        card(ui, tr("Without files"), Some(&held), |ui| {
            toggle(ui, &mut self.settings.hold);
            if self.settings.hold {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Opens after")).color(look().muted));
                    let seconds = |ms: f64| match i18n::current() {
                        Lang::Turkish => format!("{:.2} sn", ms / 1000.0).replace('.', ","),
                        Lang::English => format!("{:.2} s", ms / 1000.0),
                    };
                    ui.add(
                        egui::Slider::new(&mut self.settings.hold_ms, HOLD_RANGE)
                            .step_by(50.0)
                            .custom_formatter(move |ms, _| seconds(ms))
                            .custom_parser(|text| {
                                text.trim_end_matches(|c: char| c.is_alphabetic() || c == ' ')
                                    .replace(',', ".")
                                    .parse::<f64>()
                                    .ok()
                                    .map(|s| s * 1000.0)
                            }),
                    );
                });
            }
        });

        if self.settings.hold {
            card(
                ui,
                tr("Quick Apps"),
                Some(tr("On the wheel's Apps, in this order.")),
                |ui| self.quick_apps(ui),
            );
        }

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

        // Under the settings, not under About, where it would be out of sight.
        if let Some((said, at)) = &self.note {
            let left = NOTE_FOR.saturating_sub(at.elapsed());
            if left.is_zero() {
                self.note = None;
            } else {
                ui.add_space(4.0);
                ui.label(RichText::new(said.as_str()).color(look().muted));
                ui.ctx().request_repaint_after(left);
            }
        }

        self.about(ui);
    }

    /// The app's icon and name over the cards.
    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), Vec2::splat(40.0))));
            }
            ui.vertical(|ui| {
                ui.add_space(1.0);
                ui.label(RichText::new(APP_NAME).size(20.0).strong());
                ui.label(RichText::new(tr("Settings")).size(13.0).color(look().muted));
            });
        });
        ui.add_space(8.0);
    }

    /// The app, its version and who made it, where to find them, and a way to sponsor it.
    fn about(&self, ui: &mut egui::Ui) {
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new((logo.id(), Vec2::splat(48.0))));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(APP_NAME).size(17.0).strong());
                        pill(ui, &trf("Version {}", &[&env!("CARGO_PKG_VERSION")]));
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        let small = |text| RichText::new(text).size(12.5).color(look().muted);
                        ui.label(small(AUTHOR));
                        ui.label(small("·"));
                        let mail = format!("mailto:{EMAIL}");
                        if ui
                            .link(RichText::new(EMAIL).size(12.5).color(ACCENT))
                            .on_hover_text(&mail)
                            .clicked()
                        {
                            desk::open_link(&mail);
                        }
                    });
                });
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if let Some(version) = crate::update::available() {
                    let text = trf("Get version {}", &[&version]);
                    chip(ui, LinkIcon::Download, &text, crate::update::DOWNLOAD);
                }
                for (icon, text, url) in WEB {
                    chip(ui, icon, text, url);
                }
            });
            ui.add_space(8.0);
            sponsor(ui);
        });
        ui.add_space(6.0);
    }

    /// The quick apps in order, each with its icon and buttons to move it or take it off, then
    /// the installed apps as tiles to add one from, narrowed as you type.
    fn quick_apps(&mut self, ui: &mut egui::Ui) {
        let apps = self.apps.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let name_of = |id: &str| {
            apps.as_ref()
                .and_then(|apps| apps.iter().find(|a| a.id == id))
                .map_or_else(|| pretty_id(id), |a| a.name.clone())
        };
        let count = self.settings.quick_apps.len();
        let mut change = None;
        for i in 0..count {
            let id = self.settings.quick_apps[i].clone();
            let name = name_of(&id);
            let icon = self.icons.get(ui.ctx(), &id);
            Frame::new()
                .fill(look().control)
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), ROW_ICON + 4.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            app_icon(ui, icon, &name, ROW_ICON);
                            ui.add_space(4.0);
                            ui.label(RichText::new(name).size(14.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if small_button(ui, Glyph::Remove, tr("Remove"), true).clicked() {
                                    change = Some(Change::Remove(i));
                                }
                                if small_button(ui, Glyph::Down, tr("Move down"), i + 1 < count)
                                    .clicked()
                                {
                                    change = Some(Change::Swap(i, i + 1));
                                }
                                if small_button(ui, Glyph::Up, tr("Move up"), i > 0).clicked() {
                                    change = Some(Change::Swap(i - 1, i));
                                }
                            });
                        },
                    );
                });
        }
        let picked = &mut self.settings.quick_apps;
        match change {
            Some(Change::Remove(i)) => {
                picked.remove(i);
            }
            Some(Change::Swap(a, b)) => picked.swap(a, b),
            None => {}
        }
        if picked.len() >= MOST_QUICK_APPS {
            ui.label(
                RichText::new(trf("The wheel holds {} apps.", &[&MOST_QUICK_APPS]))
                    .size(12.5)
                    .color(look().muted),
            );
            return;
        }

        ui.add_space(10.0);
        let Some(apps) = apps else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr("Looking for apps…")).color(look().muted));
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        };
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(tr("Add an app…"))
                .margin(Margin::symmetric(12, 8))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        let words = self.filter.to_lowercase();
        let offered: Vec<&App> = apps
            .iter()
            .filter(|a| !self.settings.quick_apps.contains(&a.id))
            .filter(|a| words.is_empty() || a.name.to_lowercase().contains(&words))
            .collect();
        if offered.is_empty() {
            ui.label(RichText::new(tr("No app has that name")).color(look().muted));
            return;
        }
        let mut added = None;
        // Two rows and a bit of the third, wherever the card is: inside the window's own scroll
        // area, a scroll area only gets what is left of the window's height.
        let height = TILE.y * 2.6;
        egui::ScrollArea::vertical()
            .id_salt("apps to add")
            .max_height(height)
            .min_scrolled_height(height)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    for app in offered {
                        let (rect, response) = ui.allocate_exact_size(TILE, Sense::click());
                        // Only the tiles in view look their icons up.
                        if !ui.is_rect_visible(rect) {
                            continue;
                        }
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(10), look().hover);
                        }
                        let icon = self.icons.get(ui.ctx(), &app.id);
                        let at = rect.center_top() + Vec2::new(0.0, 8.0 + TILE_ICON / 2.0);
                        draw_icon(ui, icon, &app.name, at, TILE_ICON);
                        let text = ui.painter().layout(
                            app.name.clone(),
                            egui::FontId::proportional(11.5),
                            look().text,
                            TILE.x - 8.0,
                        );
                        let top = rect.top() + 14.0 + TILE_ICON;
                        let x = rect.center().x - text.size().x / 2.0;
                        ui.painter().with_clip_rect(rect).galley(
                            egui::pos2(x, top),
                            text,
                            look().text,
                        );
                        if response.on_hover_text(&app.name).clicked() {
                            added = Some(app.id.clone());
                        }
                    }
                });
            });
        if let Some(id) = added {
            self.settings.quick_apps.push(id);
        }
    }
}

/// An app id as a name, when the app's own name isn't known (yet): "org.kde.dolphin.desktop"
/// → "dolphin".
fn pretty_id(id: &str) -> String {
    let base = std::path::Path::new(id)
        .file_stem()
        .map_or_else(|| id.to_owned(), |s| s.to_string_lossy().into_owned());
    base.rsplit('.').next().unwrap_or(&base).to_owned()
}

/// An app's icon of `size` points in the layout, or its initial on a tile while there is none.
fn app_icon(ui: &mut egui::Ui, icon: Option<egui::TextureId>, name: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    draw_icon(ui, icon, name, rect.center(), size);
}

fn draw_icon(ui: &egui::Ui, icon: Option<egui::TextureId>, name: &str, at: egui::Pos2, size: f32) {
    let rect = egui::Rect::from_center_size(at, Vec2::splat(size));
    match icon {
        Some(texture) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture, rect, uv, Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same((size / 4.0) as u8),
                ACCENT.gamma_multiply(0.85),
            );
            let initial: String = name
                .chars()
                .next()
                .into_iter()
                .flat_map(char::to_uppercase)
                .collect();
            ui.painter().text(
                at,
                egui::Align2::CENTER_CENTER,
                initial,
                egui::FontId::proportional(size * 0.5),
                Color32::WHITE,
            );
        }
    }
}

enum Change {
    Remove(usize),
    Swap(usize, usize),
}

/// What a small button shows, drawn rather than typed: Noto Sans has no arrows.
#[derive(Clone, Copy)]
enum Glyph {
    Up,
    Down,
    Remove,
}

/// A small round button with a drawn glyph, only a circle under the pointer.
fn small_button(ui: &mut egui::Ui, glyph: Glyph, hint: &str, enabled: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    let response = if enabled {
        response.on_hover_text(hint)
    } else {
        response
    };
    let p = ui.painter();
    if enabled && response.hovered() {
        p.circle_filled(rect.center(), 13.0, look().hover);
    }
    let color = if !enabled {
        look().muted.gamma_multiply(0.4)
    } else if matches!(glyph, Glyph::Remove) && response.hovered() {
        crate::glass::RED
    } else {
        look().text
    };
    let stroke = Stroke::new(1.6, color);
    let at = |x: f32, y: f32| rect.center() + Vec2::new(x, y);
    match glyph {
        Glyph::Up | Glyph::Down => {
            let k = if matches!(glyph, Glyph::Up) {
                1.0
            } else {
                -1.0
            };
            p.line_segment([at(0.0, 5.0 * k), at(0.0, -5.0 * k)], stroke);
            p.line_segment([at(-4.0, -k), at(0.0, -5.0 * k)], stroke);
            p.line_segment([at(4.0, -k), at(0.0, -5.0 * k)], stroke);
        }
        Glyph::Remove => {
            p.line_segment([at(-4.0, -4.0), at(4.0, 4.0)], stroke);
            p.line_segment([at(-4.0, 4.0), at(4.0, -4.0)], stroke);
        }
    }
    if enabled {
        response
    } else {
        response.on_hover_cursor(egui::CursorIcon::Default)
    }
}

/// A small rounded label in the accent colour.
fn pill(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(ACCENT.gamma_multiply(0.16))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(ACCENT));
        });
}

/// What a link chip shows before its text, drawn like the small buttons' glyphs.
#[derive(Clone, Copy)]
enum LinkIcon {
    /// `</>`, for code.
    Code,
    /// A globe, for a website.
    Globe,
    /// An arrow down to a line, for a download.
    Download,
}

/// A link as a rounded chip with an icon, opening in the browser.
fn chip(ui: &mut egui::Ui, icon: LinkIcon, text: &str, url: &str) {
    let icon_id = egui::IdSalt::new(url);
    let shown = egui::Button::new((
        egui::Atom::custom(icon_id, Vec2::splat(14.0)),
        RichText::new(text).size(13.0),
    ))
    .corner_radius(CornerRadius::same(15))
    .min_size(Vec2::new(0.0, 30.0))
    .atom_ui(ui);
    if let Some(rect) = shown.rect(icon_id) {
        let color = if shown.response.hovered() {
            ACCENT
        } else {
            look().muted
        };
        let stroke = Stroke::new(1.4, color);
        let at = |x: f32, y: f32| rect.center() + Vec2::new(x, y);
        let p = ui.painter();
        match icon {
            LinkIcon::Code => {
                p.add(egui::Shape::line(
                    vec![at(-2.5, -4.0), at(-6.5, 0.0), at(-2.5, 4.0)],
                    stroke,
                ));
                p.add(egui::Shape::line(
                    vec![at(2.5, -4.0), at(6.5, 0.0), at(2.5, 4.0)],
                    stroke,
                ));
                p.line_segment([at(1.2, -5.5), at(-1.2, 5.5)], stroke);
            }
            LinkIcon::Globe => {
                p.circle_stroke(rect.center(), 6.0, stroke);
                p.add(egui::Shape::ellipse_stroke(
                    rect.center(),
                    Vec2::new(2.6, 6.0),
                    stroke,
                ));
                p.line_segment([at(-6.0, 0.0), at(6.0, 0.0)], stroke);
            }
            LinkIcon::Download => {
                p.line_segment([at(0.0, -6.0), at(0.0, 2.5)], stroke);
                p.add(egui::Shape::line(
                    vec![at(-3.5, -1.0), at(0.0, 2.5), at(3.5, -1.0)],
                    stroke,
                ));
                p.line_segment([at(-5.5, 6.0), at(5.5, 6.0)], stroke);
            }
        }
    }
    if shown
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(url)
        .clicked()
    {
        desk::open_link(url);
    }
}

/// A banner across the card to sponsor the app on GitHub, pinker under the pointer.
fn sponsor(ui: &mut egui::Ui) {
    // The stroke is set before `begin`, which leaves room for it.
    let mut banner = Frame::new()
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(12, 10))
        .stroke(Stroke::new(1.0, PINK.gamma_multiply(0.35)))
        .begin(ui);
    let content = &mut banner.content_ui;
    content.set_min_width(content.available_width());
    content.horizontal(|ui| {
        let (badge, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
        // Drawn once the text is laid out, in the middle of its height.
        let badge = badge.center().x;
        ui.add_space(4.0);
        // Room on the right for the arrow.
        let width = (ui.available_width() - 24.0).max(0.0);
        ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(tr("Sponsor on GitHub")).size(14.0).strong());
            ui.label(
                RichText::new(trf(
                    "If {} saves you time, you can support its development.",
                    &[&APP_NAME],
                ))
                .size(12.0)
                .color(look().muted),
            );
        });
        let middle = egui::pos2(badge, ui.min_rect().center().y);
        ui.painter().circle_filled(middle, 17.0, PINK);
        heart(
            ui.painter(),
            egui::Rect::from_center_size(middle + Vec2::new(0.0, 0.5), Vec2::splat(16.0)),
            Color32::WHITE,
        );
    });
    let rect = banner.allocate_space(ui).rect;
    let response = ui
        .interact(rect, ui.scope_id().with("sponsor"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(SPONSOR);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, tr("Sponsor on GitHub")));
    let hovered = response.hovered();
    banner.frame.fill = PINK.gamma_multiply(if hovered { 0.24 } else { 0.14 });
    // Reached with Tab, it shows.
    if response.has_focus() {
        banner.frame.stroke.color = ACCENT;
    }
    banner.paint(ui);
    let arrow = egui::pos2(rect.right() - 20.0, rect.center().y);
    let stroke = Stroke::new(1.6, if hovered { PINK } else { look().muted });
    ui.painter().add(egui::Shape::line(
        vec![
            arrow + Vec2::new(-2.5, -5.0),
            arrow + Vec2::new(2.5, 0.0),
            arrow + Vec2::new(-2.5, 5.0),
        ],
        stroke,
    ));
    if response.clicked() {
        desk::open_link(SPONSOR);
    }
}

/// A heart filling the square `rect`, drawn: no font here has one. Two circles over a wedge.
fn heart(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let s = rect.width() / 14.0;
    let at = |x: f32, y: f32| rect.center() + Vec2::new(x, y) * s;
    painter.circle_filled(at(-3.22, -1.68), 3.78 * s, color);
    painter.circle_filled(at(3.22, -1.68), 3.78 * s, color);
    // From the point at the bottom to where its sides touch the circles, and their centres.
    painter.add(egui::Shape::convex_polygon(
        vec![
            at(-5.54, 1.32),
            at(-3.22, -1.68),
            at(3.22, -1.68),
            at(5.54, 1.32),
            at(0.0, 5.6),
        ],
        color,
        Stroke::NONE,
    ));
}

/// A row with a label on the left and its controls.
fn labelled(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.add_sized(
            Vec2::new(70.0, 32.0),
            egui::Label::new(RichText::new(label).color(look().muted)),
        );
        add(ui);
    });
}

/// One settings window at a time, however it was opened: the guard holds a lock on a file in
/// the settings folder. Without the folder, the window opens anyway. (On Linux, the session bus
/// does this.)
#[cfg(any(target_os = "macos", windows))]
fn claim_lock_file() -> Option<Option<std::fs::File>> {
    let Some(dir) = settings::app_dir() else {
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
            ui.label(RichText::new(row.help).size(12.5).color(look().muted));
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

/// A card's white (or dark) rounded panel.
fn panel() -> Frame {
    Frame::new()
        .fill(look().card)
        .corner_radius(CornerRadius::same(14))
        .stroke(Stroke::new(1.0, look().edge))
        .inner_margin(Margin::same(16))
}

/// A panel with a heading, an optional line under it, and the controls.
fn card(ui: &mut egui::Ui, heading: &str, help: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    panel().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(heading).size(15.0).strong());
        if let Some(help) = help {
            ui.label(RichText::new(help).size(12.5).color(look().muted));
        }
        ui.add_space(2.0);
        add(ui);
    });
    ui.add_space(6.0);
}

/// One button of a row where exactly one is on.
fn segment<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, this: T, text: &str) {
    let on = *value == this;
    let label = RichText::new(text).color(if on { Color32::WHITE } else { look().text });
    let button = egui::Button::new(label)
        .fill(if on { ACCENT } else { look().control })
        .min_size(Vec2::new(78.0, 32.0));
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
    let track = look().control.lerp_to_gamma(ACCENT, t);
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(12),
        track,
        Stroke::new(1.0, look().edge),
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
            "Get version {}",
            "Without files",
            "Hold {} for a moment without dragging files: the wheel opens with your apps, what \
             is playing, the keyboard, the volume, the brightness and the system.",
            "Opens after",
            "Appearance",
            "Look",
            "System",
            "Light",
            "Dark",
            "Quick Apps",
            "On the wheel's Apps, in this order.",
            "Move up",
            "Move down",
            "Remove",
            "The wheel holds {} apps.",
            "Looking for apps…",
            "Add an app…",
            "No app has that name",
            "Permissions",
            "Saved",
            "Couldn't save: {}",
            "{} isn't running; it will use these when it starts.",
            "Version {}",
            "Sponsor on GitHub",
            "If {} saves you time, you can support its development.",
        ] {
            assert_ne!(translate(Lang::Turkish, text), text, "{text}");
        }
    }
}
