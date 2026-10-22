//! The overlay's material: frosted glass over a compositor-blurred backdrop, light grey and
//! white with dark text or (see `set_dark`) smoky with light text, and one orange accent. Turned
//! from one to the other while it shows (`turn_dark`), every colour fades across.
//!
//! Painting only covers what we draw; the blur behind it comes from the platform, which reads
//! the [`GlassShape`]s the overlay collected while painting.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::epaint::TextShape;
use egui::{
    Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Mesh, Painter, Pos2, Rect,
    Shape, Stroke, StrokeKind, Vec2, vec2,
};

pub const ACCENT: Color32 = Color32::from_rgb(244, 96, 18);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(255, 136, 56);
pub const GREEN: Color32 = Color32::from_rgb(52, 178, 84);
pub const RED: Color32 = Color32::from_rgb(222, 64, 52);

/// The overlay's colours in light or dark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub text: Color32,
    pub text_muted: Color32,
    /// The wheel's frosted disc, and how opaque it is.
    pub frame: Color32,
    pub frame_alpha: f32,
    /// The captions' and cards' frosted panel, and how opaque it is.
    pub panel: Color32,
    pub panel_alpha: f32,
    /// A wedge at rest, lit from above: `wedge` at the top, `wedge_shade` at the bottom.
    pub wedge: Color32,
    pub wedge_shade: Color32,
    /// The mode band's track, and the slice raised on it for the mode shown.
    pub track: Color32,
    pub raised: Color32,
    /// The hub's centre.
    pub hub: Color32,
    /// Hairline edges: dark on light glass, light on dark.
    pub edge: Color32,
    /// How bright the lit rim is.
    pub rim: f32,
}

pub const LIGHT_GLASS: Palette = Palette {
    text: Color32::from_rgb(29, 29, 31),
    text_muted: Color32::from_rgb(112, 112, 120),
    frame: Color32::from_rgb(222, 222, 227),
    frame_alpha: 0.82,
    panel: Color32::from_rgb(246, 246, 248),
    panel_alpha: 0.9,
    wedge: Color32::from_rgb(248, 248, 250),
    wedge_shade: Color32::from_rgb(236, 236, 240),
    track: Color32::from_rgb(230, 230, 235),
    raised: Color32::WHITE,
    hub: Color32::from_rgb(252, 252, 253),
    edge: Color32::BLACK,
    rim: 0.8,
};

pub const DARK_GLASS: Palette = Palette {
    text: Color32::from_rgb(242, 242, 246),
    text_muted: Color32::from_rgb(160, 160, 170),
    frame: Color32::from_rgb(38, 38, 44),
    frame_alpha: 0.86,
    panel: Color32::from_rgb(42, 42, 48),
    panel_alpha: 0.92,
    wedge: Color32::from_rgb(66, 66, 74),
    wedge_shade: Color32::from_rgb(52, 52, 60),
    track: Color32::from_rgb(28, 28, 33),
    raised: Color32::from_rgb(78, 78, 88),
    hub: Color32::from_rgb(58, 58, 66),
    edge: Color32::WHITE,
    rim: 0.35,
};

/// How long the glass takes to turn from light to dark, or back.
pub const TURN_TIME: f32 = 0.45;

/// Dark or light, and the turn to it: how dark it was when the turn began, and when.
struct Look {
    dark: bool,
    from: f32,
    since: Option<Instant>,
}

static LOOK: Mutex<Look> = Mutex::new(Look {
    dark: false,
    from: 0.0,
    since: None,
});

fn look() -> std::sync::MutexGuard<'static, Look> {
    LOOK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Draws everything dark, or light, from now on, at once: for what isn't showing yet. A turn
/// already on its way there goes on, for what is showing.
pub fn set_dark(dark: bool) {
    let mut look = look();
    if look.dark == dark {
        return;
    }
    *look = Look {
        dark,
        from: 0.0,
        since: None,
    };
}

/// Turns everything dark, or light, over `TURN_TIME`, from however dark it is now.
pub fn turn_dark(dark: bool) {
    let mut look = look();
    if look.dark == dark {
        return;
    }
    let now = Instant::now();
    let from = darkness_of(&look, now);
    *look = Look {
        dark,
        from,
        since: Some(now),
    };
}

/// Whether everything is dark, or turning dark.
pub fn dark() -> bool {
    look().dark
}

/// Whether the glass is turning: what shows it has to be drawn again.
pub fn turning() -> bool {
    look()
        .since
        .is_some_and(|at| at.elapsed().as_secs_f32() < TURN_TIME)
}

/// How dark everything is now, 0 (light) → 1 (dark).
pub fn darkness() -> f32 {
    darkness_of(&look(), Instant::now())
}

fn darkness_of(look: &Look, now: Instant) -> f32 {
    let to = if look.dark { 1.0 } else { 0.0 };
    let Some(at) = look.since else {
        return to;
    };
    let t = (now.saturating_duration_since(at).as_secs_f32() / TURN_TIME).min(1.0);
    // Slow out of one, slow into the other.
    let eased = t * t * (3.0 - 2.0 * t);
    look.from + (to - look.from) * eased
}

/// The colours to draw with now: light's or dark's, or on the way between.
pub fn palette() -> Palette {
    blend(&LIGHT_GLASS, &DARK_GLASS, darkness())
}

/// Text `t` of the way from light's to dark's. Faded across, it would be as grey as the glass
/// halfway; instead it fades out just before the middle and back in as the other just after.
pub fn swap_text(light: Color32, dark: Color32, t: f32) -> Color32 {
    let (color, away) = if t < 0.5 {
        (light, 1.0 - 2.0 * t)
    } else {
        (dark, 2.0 * t - 1.0)
    };
    color.gamma_multiply((away / 0.3).min(1.0))
}

/// `light`'s colours `t` of the way to `dark`'s.
fn blend(light: &Palette, dark: &Palette, t: f32) -> Palette {
    if t <= 0.0 {
        return *light;
    }
    if t >= 1.0 {
        return *dark;
    }
    let c = |a: Color32, b: Color32| a.lerp_to_gamma(b, t);
    let f = |a: f32, b: f32| a + (b - a) * t;
    Palette {
        text: swap_text(light.text, dark.text, t),
        text_muted: swap_text(light.text_muted, dark.text_muted, t),
        frame: c(light.frame, dark.frame),
        frame_alpha: f(light.frame_alpha, dark.frame_alpha),
        panel: c(light.panel, dark.panel),
        panel_alpha: f(light.panel_alpha, dark.panel_alpha),
        wedge: c(light.wedge, dark.wedge),
        wedge_shade: c(light.wedge_shade, dark.wedge_shade),
        track: c(light.track, dark.track),
        raised: c(light.raised, dark.raised),
        hub: c(light.hub, dark.hub),
        edge: c(light.edge, dark.edge),
        rim: f(light.rim, dark.rim),
    }
}

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
    let glass = palette();
    p.circle_filled(c, r, glass.frame.gamma_multiply(glass.frame_alpha * alpha));
    p.circle_stroke(
        c,
        r,
        Stroke::new(1.0, glass.edge.gamma_multiply(0.10 * alpha)),
    );
    rim(p, c, |_| r - 1.5, 1.5, glass.rim * alpha);
}

/// The frame with its edge at `radius(angle)` instead of round, as where it swells toward the
/// pointer (angles as `Vec2::angled` takes them).
pub fn frame_shaped(p: &Painter, c: Pos2, radius: impl Fn(f32) -> f32, alpha: f32) {
    const STEPS: usize = 180;
    let edge = |grow: f32| -> Vec<Pos2> {
        (0..STEPS)
            .map(|i| {
                let a = i as f32 / STEPS as f32 * TAU;
                c + Vec2::angled(a) * (radius(a) + grow)
            })
            .collect()
    };
    let drop = vec2(0.0, 10.0);
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let shadow: Vec<Pos2> = edge(2.0 + 22.0 * t).iter().map(|p| *p + drop).collect();
        p.add(fan(
            c + drop,
            &shadow,
            Color32::BLACK.gamma_multiply(0.018 * alpha),
        ));
    }
    let glass = palette();
    let outline = edge(0.0);
    p.add(fan(
        c,
        &outline,
        glass.frame.gamma_multiply(glass.frame_alpha * alpha),
    ));
    p.add(Shape::closed_line(
        outline,
        Stroke::new(1.0, glass.edge.gamma_multiply(0.10 * alpha)),
    ));
    rim(p, c, |a| radius(a) - 1.5, 1.5, glass.rim * alpha);
}

/// A filled shape that every point of `ring` sees `center` from (as a swollen disc does).
fn fan(center: Pos2, ring: &[Pos2], color: Color32) -> Shape {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, color);
    for p in ring {
        mesh.colored_vertex(*p, color);
    }
    let n = ring.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    Shape::mesh(mesh)
}

/// A white highlight along the edge at `radius(angle)`, strongest where it faces the light.
fn rim(p: &Painter, c: Pos2, radius: impl Fn(f32) -> f32, width: f32, alpha: f32) {
    let steps = ((radius(0.0) * TAU / 5.0) as usize).clamp(24, 160);
    for i in 0..steps {
        let a0 = i as f32 / steps as f32 * TAU;
        let a1 = (i + 1) as f32 / steps as f32 * TAU;
        let facing = (((a0 + a1) / 2.0 - LIGHT).cos() + 1.0) / 2.0;
        let strength = 0.1 + 0.9 * facing.powi(2);
        p.line_segment(
            [
                c + Vec2::angled(a0) * radius(a0),
                c + Vec2::angled(a1) * radius(a1),
            ],
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
    let glass = palette();
    p.rect_filled(
        rect,
        radius,
        glass.panel.gamma_multiply(glass.panel_alpha * alpha),
    );
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, glass.edge.gamma_multiply(0.09 * alpha)),
        StrokeKind::Inside,
    );
}

/// Centred text. Needs [`install_fonts`] for `bold`.
pub fn text(p: &Painter, pos: Pos2, s: &str, size: f32, color: Color32, bold: bool) {
    p.text(pos, Align2::CENTER_CENTER, s, font(size, bold), color);
}

/// Text bent along a circle of `radius` about `center`, centred on the angle `mid` (radians,
/// clockwise from three o'clock). Above the centre it runs clockwise and below it the other way,
/// so it reads left to right with its letters upright. Letters are spaced by their advances
/// (kerning included) rather than the laid-out positions, which are rounded to whole pixels:
/// at this size that rounding shows once the letters are turned.
#[allow(clippy::too_many_arguments)]
pub fn text_on_arc(
    p: &Painter,
    center: Pos2,
    radius: f32,
    mid: f32,
    s: &str,
    size: f32,
    color: Color32,
    bold: bool,
) {
    let font = font(size, bold);
    let galley = p.layout_no_wrap(s.to_owned(), font.clone(), color);
    let Some(row) = galley.rows.first() else {
        return;
    };
    let width: f32 = row.row.glyphs.iter().map(|g| g.advance_width).sum();
    let below = mid.sin() > 0.0;
    let mut x = 0.0;
    for glyph in &row.row.glyphs {
        let left = x;
        x += glyph.advance_width;
        if glyph.chr.is_whitespace() {
            continue;
        }
        let letter = p.layout_no_wrap(glyph.chr.to_string(), font.clone(), color);
        let along = left + letter.size().x / 2.0 - width / 2.0;
        let (angle, turn) = if below {
            let angle = mid - along / radius;
            (angle, angle - FRAC_PI_2)
        } else {
            let angle = mid + along / radius;
            (angle, angle + FRAC_PI_2)
        };
        let top_left = center + Vec2::angled(angle) * radius - letter.size() / 2.0;
        p.add(
            TextShape::new(top_left, letter, color)
                .with_angle_and_anchor(turn, Align2::CENTER_CENTER),
        );
    }
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
    fn the_glass_turns_across_and_settles() {
        assert_eq!(blend(&LIGHT_GLASS, &DARK_GLASS, 0.0), LIGHT_GLASS);
        assert_eq!(blend(&LIGHT_GLASS, &DARK_GLASS, 1.0), DARK_GLASS);
        let half = blend(&LIGHT_GLASS, &DARK_GLASS, 0.5);
        assert!(half.frame != LIGHT_GLASS.frame && half.frame != DARK_GLASS.frame);
        // Text is never grey on grey: light's or dark's, faded out only near the middle.
        assert_eq!(half.text.a(), 0);
        assert_eq!(blend(&LIGHT_GLASS, &DARK_GLASS, 0.3).text, LIGHT_GLASS.text);
        assert_eq!(blend(&LIGHT_GLASS, &DARK_GLASS, 0.7).text, DARK_GLASS.text);
        assert!((half.rim - (LIGHT_GLASS.rim + DARK_GLASS.rim) / 2.0).abs() < 1e-6);
        let at = Instant::now();
        let look = Look {
            dark: true,
            from: 0.25,
            since: Some(at),
        };
        let at_t = |s: f32| darkness_of(&look, at + std::time::Duration::from_secs_f32(s));
        assert!((at_t(0.0) - 0.25).abs() < 1e-6);
        assert!(at_t(TURN_TIME / 2.0) > 0.25 && at_t(TURN_TIME / 2.0) < 1.0);
        assert_eq!(at_t(TURN_TIME + 0.01), 1.0);
    }

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
