//! Renders the overlay in an ordinary window, for working on the UI without dragging files.
//!
//!     cargo run --example preview -- [wheel|hover|swap|tools|actions|mixed|json|document|card|cards|archive|folder|failed]
//!     cargo run --example preview -- [desk|desk-volume|desk-keyboard|desk-apps|desk-empty]
//!
//! With `EFRAME_SCREENSHOT_TO=out.png` it saves the second frame and exits; `MCGYVER_LANG=tr` shows
//! it in Turkish. The `settings` scene opens the settings window instead. `MCGYVER_AT=0.1` stops
//! the clock that many seconds after the scene's last step (the wheel opening, the wedge aimed
//! at, the mode switched), to see a frame of the motion.

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
    let size = if scene.starts_with("desk") {
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
            scene(&self.scene, screen, then)
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

/// What a desktop might have, for the desk scenes.
fn menu(name: &str) -> Menu {
    let layout = |id: &str, short: &str, name: &str| desk::Layout {
        id: id.into(),
        short: short.into(),
        name: name.into(),
    };
    let app = |name: &str| desk::App {
        id: format!("{}.desktop", name.to_lowercase()),
        name: name.into(),
    };
    Menu {
        player: (name == "desk").then(|| desk::Player {
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
        volume: Some(desk::Volume {
            level: 0.45,
            muted: false,
        }),
        brightness: Some(0.8),
        apps: if name == "desk-empty" {
            Vec::new()
        } else {
            ["Firefox", "Dolphin", "Konsole", "Telegram", "Kate"]
                .map(app)
                .to_vec()
        },
    }
}

/// The desk, opened at `then` less a second, with a wedge aimed at (and one of its bubbles)
/// at `then`.
fn desk_scene(name: &str, screen: Rect, then: Instant) -> Overlay {
    let center = screen.center();
    let mut overlay = Overlay::default();
    overlay.open_desk(menu(name), center, screen, then - Duration::from_secs(1));
    // The mode slice to switch to, and the wedge aimed at (of how many) with which bubble (of
    // how many).
    type Aim = Option<(usize, usize, Option<(usize, usize)>)>;
    let (mode_at, wedge): (Option<f32>, Aim) = match name {
        // Volume, lower right, and its 50 % bubble.
        "desk-volume" => (None, Some((1, 3, Some((2, 5))))),
        // The keyboard on top, and its third bubble.
        "desk-keyboard" => (None, Some((0, 3, Some((2, 4))))),
        // To Apps, lower left on the ring.
        "desk-apps" => (Some(150.0), Some((0, 5, None))),
        "desk-empty" => (Some(150.0), None),
        _ => (None, None),
    };
    if let Some(degrees) = mode_at {
        overlay.drag_moved(
            center + Vec2::angled(degrees.to_radians()) * 48.0,
            then - Duration::from_millis(900),
        );
        overlay.click(then - Duration::from_millis(900));
    }
    if let Some((i, n, bubble)) = wedge {
        let toward = Vec2::angled(-FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32);
        overlay.drag_moved(center + toward * 118.0, then - Duration::from_millis(600));
        if let Some((k, count)) = bubble {
            overlay.drag_moved(mcgyver::wheel::bubble_spots(center, i, n, count)[k], then);
        }
    }
    overlay
}

/// Sets up the overlay as a drag would have: the scene's last step (the wheel opening, the drag
/// aiming at a wedge, the mode switching) at `then`, the steps before it a second earlier.
fn scene(name: &str, screen: Rect, then: Instant) -> Overlay {
    if name.starts_with("desk") {
        return desk_scene(name, screen, then);
    }
    let center = screen.center();
    let (files, mode, aim): (&[&str], Mode, Option<f32>) = match name {
        "hover" => (&["holiday.jpg"], Mode::Convert, Some(60.0)),
        "swap" => (&["holiday.jpg"], Mode::Convert, None),
        "tools" => (&["trip.mov"], Mode::Tools, Some(0.0)),
        "actions" => (&["report.pdf"], Mode::Actions, Some(120.0)),
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
    let opened = if aim.is_some() || name == "swap" {
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
