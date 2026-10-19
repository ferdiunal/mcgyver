//! Renders the overlay in an ordinary window, for working on the UI without dragging files.
//!
//!     cargo run --example preview -- [wheel|hover|tools|actions|mixed|json|document|card|cards|archive|folder|failed]
//!
//! With `EFRAME_SCREENSHOT_TO=out.png` it saves the second frame and exits; `MCGYVER_LANG=tr` shows
//! it in Turkish. The `settings` scene opens the settings window instead.

use std::f32::consts::FRAC_PI_2;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Rect, Vec2};
use mcgyver::catalog::Mode;
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 520.0]),
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
            }))
        }),
    )
}

struct Preview {
    scene: String,
    overlay: Option<Overlay>,
}

impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let screen = ui.max_rect();
        backdrop(ui.painter(), screen);
        // Setting up a scene may wait for a job, so read the clock after it.
        let overlay = self
            .overlay
            .get_or_insert_with(|| scene(&self.scene, screen, Instant::now()));
        if let Some(wait) = overlay.paint(ui, Instant::now()) {
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

/// Sets up the overlay as if a drag had arrived two seconds ago.
fn scene(name: &str, screen: Rect, now: Instant) -> Overlay {
    let then = now - Duration::from_secs(2);
    let center = screen.center();
    let (files, mode, aim): (&[&str], Mode, Option<f32>) = match name {
        "hover" => (&["holiday.jpg"], Mode::Convert, Some(60.0)),
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
    overlay.still = true;
    overlay.open(
        files.iter().map(PathBuf::from).collect(),
        center,
        screen,
        mode,
        then,
    );
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
