//! Renders the overlay in an ordinary window, for working on the UI without dragging files.
//!
//!     cargo run --example preview -- [wheel|hover|swap|tools|actions|share|mixed|json|document|card|cards|archive|folder|failed]
//!     cargo run --example preview -- [desk|desk-apps|desk-media|desk-volume|desk-keyboard|desk-monitor|desk-system|desk-empty|welcome]
//!
//! With `EFRAME_SCREENSHOT_TO=out.png` it saves the second frame and exits; `MCGYVER_LANG=tr` shows
//! it in Turkish, `MCGYVER_DARK=1` dark, `MCGYVER_TURN=1` (not for `settings`) halfway through
//! turning dark. The `settings` scene opens the settings window instead. `MCGYVER_AT=0.1` stops the clock that many
//! seconds after the scene's last step (the wheel opening, the wedge aimed at, the mode switched),
//! to see a frame of the motion.

use std::f32::consts::FRAC_PI_2;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Rect, Vec2};
use mcgyver::catalog::Mode;
use mcgyver::desk::{self, Menu};
use mcgyver::overlay::Overlay;

fn main() -> eframe::Result {
    let scene = std::env::args().nth(1).unwrap_or_else(|| "wheel".into());
    if let Some(lang) = std::env::var("MCGYVER_LANG")
        .ok()
        .and_then(|code| mcgyver::i18n::Lang::from_code(&code))
    {
        mcgyver::i18n::set(lang);
    }
    if std::env::var_os("MCGYVER_DARK").is_some() {
        mcgyver::glass::set_dark(true);
    }
    // The wheel offers Copy and Cut where the platform can put files on the clipboard; here,
    // nothing is put anywhere.
    mcgyver::actions::serve_files_with(mcgyver::actions::FileClipboard {
        put: Box::new(|_, _| Ok(())),
        can_cut: true,
    });
    #[cfg(target_os = "linux")]
    if scene == "settings" {
        return mcgyver::settings_window::run().map_err(|e| eframe::Error::AppCreation(e.into()));
    }
    // The desk's bubbles reach further out.
    let size = if scene.starts_with("desk") || scene == "welcome" {
        620.0
    } else {
        520.0
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([size, size]),
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        "mcgyver preview",
        options,
        Box::new(move |cc| {
            mcgyver::glass::install_fonts(&cc.egui_ctx);
            Ok(Box::new(Preview {
                scene,
                overlay: None,
                frozen: None,
            }))
        }),
    )
}

struct Preview {
    scene: String,
    overlay: Option<Overlay>,
    /// With `MCGYVER_AT`: the moment every frame shows.
    frozen: Option<Instant>,
}

impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let screen = ui.max_rect();
        backdrop(ui.painter(), screen);
        // Setting up a scene may wait for a job, so read the clock after it.
        let overlay = self.overlay.get_or_insert_with(|| {
            let at = std::env::var("MCGYVER_AT")
                .ok()
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|at| at.is_finite() && *at >= 0.0);
            let now = Instant::now();
            // The scene's last step two seconds ago, everything settled; or at the frozen time.
            let then = now - Duration::from_secs(2);
            self.frozen = at.map(|at| then + Duration::from_secs_f32(at));
            let overlay = scene(&self.scene, screen, then);
            // Halfway through turning dark, once the scene is set up (the glass keeps its own
            // time, not the frozen one).
            if std::env::var_os("MCGYVER_TURN").is_some() {
                mcgyver::glass::set_dark(false);
                mcgyver::glass::turn_dark(true);
                std::thread::sleep(Duration::from_secs_f32(mcgyver::glass::TURN_TIME * 0.45));
            }
            overlay
        });
        let now = self.frozen.unwrap_or_else(Instant::now);
        // A frozen clock shows the same frame for good: no need to draw it again.
        if let Some(wait) = overlay.paint(ui, now)
            && self.frozen.is_none()
        {
            ui.ctx().request_repaint_after(wait);
        }
    }
}

/// A colourful desktop stand-in. There is no compositor blur here, so the glass looks clearer
/// than on KDE.
fn backdrop(p: &egui::Painter, screen: Rect) {
    let corners = [
        egui::Color32::from_rgb(255, 95, 109),
        egui::Color32::from_rgb(255, 195, 113),
        egui::Color32::from_rgb(71, 202, 204),
        egui::Color32::from_rgb(91, 75, 219),
    ];
    let mut mesh = egui::Mesh::default();
    for (pos, color) in [
        screen.left_top(),
        screen.right_top(),
        screen.right_bottom(),
        screen.left_bottom(),
    ]
    .into_iter()
    .zip([corners[0], corners[1], corners[3], corners[2]])
    {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}

/// The apps on the desk scenes' desktop, by the ids their icons are found by.
const APPS: [(&str, &str); 5] = [
    ("org.mozilla.firefox", "Firefox"),
    ("org.kde.konsole", "Konsole"),
    ("org.kde.dolphin", "Dolphin"),
    ("org.kde.kate", "Kate"),
    ("org.telegram.desktop", "Telegram"),
];

/// What a desktop might have, for the desk scenes.
fn menu(name: &str) -> Menu {
    let layout = |id: &str, short: &str, name: &str| desk::Layout {
        id: id.into(),
        short: short.into(),
        name: name.into(),
    };
    let language = |id: &str, short: &str, name: &str| desk::Language {
        id: id.into(),
        short: short.into(),
        name: name.into(),
    };
    let window = |id: &str, (app_id, app): (&str, &str), title: &str, active: bool| {
        mcgyver::switcher::Window {
            id: id.into(),
            title: title.into(),
            app: app.into(),
            app_id: app_id.into(),
            active,
        }
    };
    let empty = name == "desk-empty";
    Menu {
        player: matches!(name, "desk" | "desk-media").then(|| desk::Player {
            id: "org.mpris.MediaPlayer2.spotify".into(),
            name: "Spotify".into(),
            title: "Teardrop".into(),
            artist: "Massive Attack".into(),
            playing: true,
            can_next: true,
            can_previous: true,
            shuffle: Some(false),
            repeat: Some(desk::Repeat::Off),
        }),
        layouts: vec![
            layout("0", "EN", "English (US)"),
            layout("1", "TR", "Turkish"),
            layout("2", "DE", "German"),
            layout("3", "JA", "Japanese"),
        ],
        layout: Some(1),
        languages: vec![
            language("tr_TR", "TR", "Türkçe"),
            language("en_US", "EN", "English"),
        ],
        language: Some(0),
        volume: Some(desk::Volume {
            level: 0.45,
            muted: false,
        }),
        brightness: Some(0.8),
        mic_muted: Some(false),
        dark: Some(true),
        awake: Some(false),
        shots: vec![desk::Shot::Region, desk::Shot::Screen, desk::Shot::Window],
        records: vec![desk::Shot::Region, desk::Shot::Screen],
        usage: Some(desk::Usage {
            processor: 0.23,
            memory: 0.61,
            disk: 0.91,
        }),
        windows: if empty {
            Vec::new()
        } else {
            vec![
                window("a", APPS[3], "notes.md — Kate", true),
                window("b", APPS[0], "McGyver — Mozilla Firefox", false),
                window("c", APPS[1], "~ : bash — Konsole", false),
                window("d", APPS[2], "Downloads — Dolphin", false),
                window("e", APPS[1], "cargo test — Konsole", false),
            ]
        },
        apps: if empty {
            Vec::new()
        } else {
            APPS.iter()
                .map(|(id, name)| desk::App {
                    id: format!("{id}.desktop"),
                    name: (*name).into(),
                })
                .collect()
        },
        ..Menu::default()
    }
}

/// The desk, opened at `then` less a second and switched to a mode, with a wedge aimed at (and
/// one of its bubbles) at `then`. `welcome` is the first start's instead.
fn desk_scene(name: &str, screen: Rect, then: Instant) -> Overlay {
    let center = screen.center();
    let menu = menu(name);
    let mut overlay = Overlay::default();
    // Found before the first frame asks for them.
    for (id, _) in APPS {
        let _ = mcgyver::icons::app(id, 64);
    }
    if name == "welcome" {
        let hint = mcgyver::i18n::trf(
            "Hold {} for your apps and settings, or while dragging files",
            &[&"Shift"],
        );
        overlay.welcome(menu, screen, hint, then);
        return overlay;
    }
    overlay.open_desk(menu.clone(), center, screen, then - Duration::from_secs(1));
    // The mode, and the wedge aimed at with which of its bubbles.
    let (mode, aim): (Mode, Option<(usize, Option<usize>)>) = match name {
        // Konsole, with its two windows.
        "desk-apps" => (Mode::Apps, Some((1, Some(1)))),
        "desk-media" => (Mode::Media, Some((1, None))),
        // Volume and its 50 % bubble.
        "desk-volume" => (Mode::Everyday, Some((2, Some(2)))),
        "desk-keyboard" => (Mode::Everyday, Some((0, Some(2)))),
        "desk-monitor" => (Mode::Everyday, Some((6, Some(1)))),
        // Dark Mode.
        "desk-system" => (Mode::System, Some((1, None))),
        _ => (Mode::Apps, None),
    };
    let modes = menu.modes();
    if let Some(i) = modes.iter().position(|&m| m == mode).filter(|&i| i > 0) {
        let degrees = -90.0 + 360.0 * i as f32 / modes.len() as f32;
        overlay.drag_moved(
            center + Vec2::angled(degrees.to_radians()) * 48.0,
            then - Duration::from_millis(900),
        );
        overlay.click(then - Duration::from_millis(900));
    }
    if let Some((i, bubble)) = aim {
        let n = menu.items(mode).len();
        let subs = menu.items(mode)[i].subs.len();
        let toward = Vec2::angled(-FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32);
        overlay.drag_moved(center + toward * 118.0, then - Duration::from_millis(600));
        if let Some(k) = bubble {
            overlay.drag_moved(mcgyver::wheel::bubble_spots(center, i, n, subs)[k], then);
        }
    }
    overlay
}

/// Sets up the overlay as a drag would have: the scene's last step (the wheel opening, the drag
/// aiming at a wedge, the mode switching) at `then`, the steps before it a second earlier.
fn scene(name: &str, screen: Rect, then: Instant) -> Overlay {
    if name.starts_with("desk") || name == "welcome" {
        return desk_scene(name, screen, then);
    }
    let center = screen.center();
    let (files, mode, aim): (&[&str], Mode, Option<f32>) = match name {
        "hover" => (&["holiday.jpg"], Mode::Convert, Some(60.0)),
        "swap" => (&["holiday.jpg"], Mode::Convert, None),
        "tools" => (&["trip.mov"], Mode::Tools, Some(0.0)),
        "actions" => (&["report.pdf"], Mode::Actions, Some(120.0)),
        "share" => (&["report.pdf"], Mode::Actions, None),
        "mixed" => (
            &["IMG_2041.heic", "banner.png", "scan.pdf"],
            Mode::Convert,
            None,
        ),
        "card" | "cards" => (
            &["IMG_2041.heic", "IMG_2042.heic", "IMG_2043.heic"],
            Mode::Convert,
            Some(0.0),
        ),
        "archive" => (&["backup.tar.gz"], Mode::Convert, Some(180.0)),
        // Nothing converts or takes a JSON file: Shift opens straight on Actions.
        "json" => (&["trace.json"], Mode::Convert, None),
        // A document converts and has actions but no tools: the ring is in halves.
        "document" => (&["Kapsam 3.docx"], Mode::Convert, None),
        // A real folder (the repository's own src), packed for real when dropped.
        "folder" => (&["src"], Mode::Convert, Some(0.0)),
        // A real conversion of a file that isn't there, to show a failure.
        "failed" => (&["missing.zip"], Mode::Convert, Some(0.0)),
        _ => (&["holiday.jpg"], Mode::Convert, None),
    };
    let mut overlay = Overlay::default();
    let opened = if aim.is_some() || name == "swap" || name == "share" {
        then - Duration::from_secs(1)
    } else {
        then
    };
    overlay.open(
        files.iter().map(PathBuf::from).collect(),
        center,
        screen,
        mode,
        opened,
    );
    if name == "swap" {
        // Onto the Tools slice of the ring, lower right, and switched there.
        overlay.drag_moved(
            center + Vec2::angled(FRAC_PI_2 / 3.0) * 48.0,
            then - Duration::from_millis(100),
        );
        overlay.click(then);
    }
    if let Some(degrees) = aim {
        overlay.drag_moved(
            center + Vec2::angled(degrees.to_radians() - FRAC_PI_2) * 118.0,
            then,
        );
    }
    if name == "share" {
        // The Share wedge, and its second bubble (the first messaging app).
        let choices = mcgyver::catalog::choices(&[PathBuf::from("report.pdf")], Mode::Actions);
        let n = choices.len();
        if let Some(i) = choices.iter().position(|c| c.label == "Share") {
            let subs = mcgyver::catalog::subs(choices[i]).len();
            let toward = Vec2::angled(-FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32);
            overlay.drag_moved(center + toward * 118.0, then - Duration::from_millis(600));
            if subs > 1 {
                overlay.drag_moved(mcgyver::wheel::bubble_spots(center, i, n, subs)[1], then);
            }
        }
    }
    if name == "cards" {
        // An earlier drop at the same spot, still working: its card moves up.
        overlay.drop(then - Duration::from_secs(1));
        overlay.open(
            vec![PathBuf::from("missing.zip")],
            center,
            screen,
            mode,
            then,
        );
        overlay.drag_moved(center + Vec2::angled(-FRAC_PI_2) * 118.0, then);
    }
    if matches!(name, "card" | "cards" | "failed") {
        overlay.drop(then);
        // Give the job's thread time to fail before the first frame.
        std::thread::sleep(Duration::from_millis(400));
    }
    overlay
}
