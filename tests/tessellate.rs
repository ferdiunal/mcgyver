//! The overlay must produce visible geometry with the RawInput the KDE backend builds.
use std::path::PathBuf;
use std::time::Instant;

use egui::{Pos2, Rect, ViewportId, pos2, vec2};
use mcgyver::catalog::Mode;
use mcgyver::overlay::Overlay;

#[test]
fn wheel_tessellates_to_geometry() {
    let ctx = egui::Context::default();
    mcgyver::glass::install_fonts(&ctx);
    let mut overlay = Overlay::default();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0));
    let now = Instant::now();
    overlay.open(
        vec![PathBuf::from("holiday.jpg")],
        pos2(640.0, 400.0),
        screen,
        Mode::Convert,
        now,
    );
    let mut vertices = 0;
    for frame in 0..3 {
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(frame as f64 / 60.0),
            predicted_dt: 1.0 / 60.0,
            ..Default::default()
        };
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(1.0);
        let mut output = ctx.run_ui(input, |ui| {
            overlay.paint(ui, Instant::now());
        });
        output.textures_delta.clear();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        vertices = primitives
            .iter()
            .map(|p| match &p.primitive {
                egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
                _ => 0,
            })
            .sum::<usize>();
        eprintln!(
            "frame {frame}: {} primitives, {vertices} vertices, ppp {}",
            primitives.len(),
            output.pixels_per_point
        );
    }
    assert!(vertices > 1000, "only {vertices} vertices");
}
