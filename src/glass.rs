//! The overlay's material: light grey and white frosted glass over a compositor-blurred
//! backdrop, with dark text and one orange accent.
//!
//! Painting only covers what we draw; the blur behind it comes from the platform, which reads
//! the [`GlassShape`]s the overlay collected while painting.

use std::f32::consts::TAU;
use std::sync::Arc;

use egui::epaint::TextShape;
use egui::{
    Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Stroke,
    StrokeKind, Vec2, vec2,
};

pub const TEXT: Color32 = Color32::from_rgb(29, 29, 31);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(112, 112, 120);
pub const ACCENT: Color32 = Color32::from_rgb(244, 96, 18);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(255, 136, 56);
pub const GREEN: Color32 = Color32::from_rgb(52, 178, 84);
pub const RED: Color32 = Color32::from_rgb(222, 64, 52);

const FRAME: Color32 = Color32::from_rgb(222, 222, 227);
const FRAME_ALPHA: f32 = 0.82;
const PANEL: Color32 = Color32::from_rgb(246, 246, 248);
const PANEL_ALPHA: f32 = 0.9;
/// Light comes from the top left.
const LIGHT: f32 = -2.3;

const BOLD: &str = "bold";

/// Noto Sans for all text, with its bold face as the `bold` family. Call before the first frame.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, data) in [
        (
            "noto-sans",
            &include_bytes!("../assets/fonts/NotoSans-Regular.ttf")[..],
        ),
        (
            "noto-sans-bold",
            &include_bytes!("../assets/fonts/NotoSans-Bold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(data)));
    }
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "noto-sans".into());
    let mut bold = vec!["noto-sans-bold".to_owned()];
    bold.extend(fallbacks);
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);
    // Glyphs on whole pixels: sharper labels than egui's quarter-pixel binning.
    ctx.all_styles_mut(|style| style.visuals.text_options.subpixel_binning = false);
}

/// Where the platform should blur the backdrop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlassShape {
    Circle { center: Pos2, radius: f32 },
    Capsule(Rect),
}

impl GlassShape {
    /// The shape as horizontal bands `[x, y, width, height]` in whole points, since
    /// compositors take blur regions as a union of rectangles.
    pub fn bands(&self, step: i32) -> Vec<[i32; 4]> {
        let (top, bottom) = match *self {
            GlassShape::Circle { center, radius } => (center.y - radius, center.y + radius),
            GlassShape::Capsule(rect) => (rect.top(), rect.bottom()),
        };
        let span_at = |y: f32| match *self {
            GlassShape::Circle { center, radius } => {
                let hw = (radius * radius - (y - center.y).powi(2)).max(0.0).sqrt();
                (center.x - hw, center.x + hw)
            }
            GlassShape::Capsule(rect) => {
                let r = rect.height() / 2.0;
                let inset = r - (r * r - (y - rect.center().y).powi(2)).max(0.0).sqrt();
                (rect.left() + inset, rect.right() - inset)
            }
        };
        let mut bands = Vec::new();
        let mut y = top.floor() as i32;
        while (y as f32) < bottom {
            let (x0, x1) = span_at(y as f32 + step as f32 / 2.0);
            if x1 > x0 {
                bands.push([x0.round() as i32, y, (x1 - x0).round() as i32, step]);
            }
            y += step;
        }
        bands
    }
}

/// The wheel's frame: a frosted grey disc with a soft shadow and a lit edge.
pub fn frame(p: &Painter, c: Pos2, r: f32, alpha: f32) {
    for i in 0..8 {
        let t = i as f32 / 7.0;
        p.circle_filled(
            c + vec2(0.0, 10.0),
            r + 2.0 + 22.0 * t,
            Color32::BLACK.gamma_multiply(0.018 * alpha),
        );
    }
    p.circle_filled(c, r, FRAME.gamma_multiply(FRAME_ALPHA * alpha));
    p.circle_stroke(
        c,
        r,
        Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.10 * alpha)),
    );
    rim(p, c, r - 1.5, 1.5, 0.8 * alpha);
}

/// A white highlight along the edge, strongest where it faces the light.
fn rim(p: &Painter, c: Pos2, r: f32, width: f32, alpha: f32) {
    let steps = ((r * TAU / 5.0) as usize).clamp(24, 160);
    for i in 0..steps {
        let a0 = i as f32 / steps as f32 * TAU;
        let a1 = (i + 1) as f32 / steps as f32 * TAU;
        let facing = (((a0 + a1) / 2.0 - LIGHT).cos() + 1.0) / 2.0;
        let strength = 0.1 + 0.9 * facing.powi(2);
        p.line_segment(
            [c + Vec2::angled(a0) * r, c + Vec2::angled(a1) * r],
            Stroke::new(width, Color32::WHITE.gamma_multiply(alpha * strength)),
        );
    }
}

/// A frosted white capsule (pill) for captions and cards.
pub fn capsule(p: &Painter, rect: Rect, alpha: f32) {
    let radius = rect.height() / 2.0;
    for i in 0..5 {
        let spread = 1.0 + 9.0 * i as f32 / 4.0;
        p.rect_filled(
            rect.expand(spread).translate(vec2(0.0, 4.0)),
            radius + spread,
            Color32::BLACK.gamma_multiply(0.02 * alpha),
        );
    }
    p.rect_filled(rect, radius, PANEL.gamma_multiply(PANEL_ALPHA * alpha));
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.09 * alpha)),
        StrokeKind::Inside,
    );
}

/// Centred text. Needs [`install_fonts`] for `bold`.
pub fn text(p: &Painter, pos: Pos2, s: &str, size: f32, color: Color32, bold: bool) {
    p.text(pos, Align2::CENTER_CENTER, s, font(size, bold), color);
}

/// Centred text turned `angle` radians clockwise about its centre, to follow a curve.
pub fn text_turned(
    p: &Painter,
    pos: Pos2,
    s: &str,
    size: f32,
    color: Color32,
    bold: bool,
    angle: f32,
) {
    let galley = p.layout_no_wrap(s.to_owned(), font(size, bold), color);
    let top_left = pos - galley.size() / 2.0;
    p.add(
        TextShape::new(top_left, galley, color).with_angle_and_anchor(angle, Align2::CENTER_CENTER),
    );
}

/// How wide `s` is set at `size`.
pub fn text_width(p: &Painter, s: &str, size: f32, bold: bool) -> f32 {
    p.layout_no_wrap(s.to_owned(), font(size, bold), Color32::WHITE)
        .size()
        .x
}

fn font(size: f32, bold: bool) -> FontId {
    let family = if bold {
        FontFamily::Name(BOLD.into())
    } else {
        FontFamily::Proportional
    };
    FontId::new(size, family)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_bands_cover_the_disc() {
        let bands = GlassShape::Circle {
            center: Pos2::new(100.0, 100.0),
            radius: 50.0,
        }
        .bands(2);
        assert_eq!(bands.len(), 50);
        let widest = bands.iter().map(|b| b[2]).max().unwrap();
        assert!((98..=100).contains(&widest), "{widest}");
        assert!(bands.iter().all(|b| b[0] >= 50 && b[0] + b[2] <= 150));
    }

    #[test]
    fn capsule_bands_round_the_ends() {
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), vec2(200.0, 40.0));
        let bands = GlassShape::Capsule(rect).bands(2);
        assert_eq!(bands[10][2], 200);
        assert!(bands[0][2] < 180, "{:?}", bands[0]);
    }
}
