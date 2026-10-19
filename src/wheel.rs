//! Wheel geometry, hit testing and painting.
//!
//! A light frame holds a ring of rounded wedges around a hub. The hub's white centre shows what
//! is being dragged or, while aiming, the format it would become. Around it runs a ring with a
//! slice per mode that has something to offer, the first at the top: halves for two modes,
//! thirds for three, and no ring at all for one. Stopping the drag on another slice switches to
//! that mode. Wedge 0 is centred at twelve o'clock and wedges run clockwise.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Color32, FontId, Id, Mesh, Painter, Pos2, Rect, Shape, Stroke, Ui, Vec2, vec2};

use crate::catalog::{Choice, Mode};
use crate::glass::{self, ACCENT, ACCENT_LIGHT, TEXT, TEXT_MUTED};
use crate::i18n::tr;

pub const HUB_R: f32 = 62.0;
/// The hub's white centre; the mode slices fill the band from here to `HUB_R`.
pub const INFO_R: f32 = 34.0;
const MODE_GAP: f32 = 4.0;
const MODE_CORNER: f32 = 7.0;
pub const RING_R0: f32 = 74.0;
pub const RING_R1: f32 = 160.0;
pub const DISC_R: f32 = 172.0;
/// Width of the gap between neighbouring wedges.
const GAP: f32 = 7.0;
const CORNER: f32 = 14.0;
const GROW: f32 = 4.0;
/// Distance between the disc and the caption under it.
const CAPTION_GAP: f32 = 14.0;

const WEDGE: Color32 = Color32::from_rgb(248, 248, 250);
const WEDGE_SHADE: Color32 = Color32::from_rgb(236, 236, 240);
/// The mode band, like the track of a segmented control; the current mode is raised white.
const TRACK: Color32 = Color32::from_rgb(230, 230, 235);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The white centre.
    Hub,
    /// A mode slice around the centre.
    Mode(Mode),
    Choice(usize),
    /// On the frame between hub and wedges, or the wheel has no choices.
    Disc,
    Outside,
}

pub fn hit(center: Pos2, pos: Pos2, modes: &[Mode], choices: usize) -> Hit {
    hit_rotated(center, pos, modes, choices, 0.0)
}

/// Hit test against wedges drawn with the given angle offset, around a ring of `modes`.
pub fn hit_rotated(center: Pos2, pos: Pos2, modes: &[Mode], choices: usize, rotation: f32) -> Hit {
    let d = pos - center;
    let r = d.length();
    let ring = modes.len() > 1;
    if r <= INFO_R || (r <= HUB_R + 4.0 && !ring) {
        Hit::Hub
    } else if r <= HUB_R + 4.0 {
        Hit::Mode(modes[sector_at(d, modes.len(), 0.0)])
    } else if r > DISC_R + 10.0 {
        Hit::Outside
    } else if r < RING_R0 || choices == 0 {
        Hit::Disc
    } else {
        Hit::Choice(sector_at(d, choices, rotation))
    }
}

fn sector_at(d: Vec2, n: usize, rotation: f32) -> usize {
    let span = TAU / n as f32;
    let a = (d.y.atan2(d.x) - rotation + FRAC_PI_2 + span / 2.0).rem_euclid(TAU);
    ((a / span) as usize).min(n - 1)
}

fn sector_angles(i: usize, n: usize) -> (f32, f32) {
    let span = TAU / n as f32;
    let mid = -FRAC_PI_2 + i as f32 * span;
    (mid - span / 2.0, mid + span / 2.0)
}

pub struct View<'a> {
    pub choices: &'a [Choice],
    /// Modes on the ring, which shows only when there are two or more.
    pub modes: &'a [Mode],
    pub mode: Mode,
    pub hovered: Option<Hit>,
    pub chosen: Option<usize>,
    /// Shown in the hub when nothing is aimed at (size or count of the files).
    pub info: &'a str,
    /// 0 → 1 as the wheel opens.
    pub open_t: f32,
    /// 0 → 1 after the mode switches.
    pub swap_t: f32,
    pub epoch: u32,
    /// Distinguishes wheels so a new one doesn't inherit the last one's wedge animations.
    pub serial: u32,
    pub animate: bool,
}

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn scale(open_t: f32) -> f32 {
    0.6 + 0.4 * ease_out(open_t)
}

pub fn rotation(swap_t: f32) -> f32 {
    (1.0 - ease_out(swap_t)) * -0.35
}

pub fn paint(ui: &Ui, center: Pos2, v: &View) {
    let p = ui.painter();
    let s = scale(v.open_t);
    glass::frame(p, center, DISC_R * s, v.open_t);
    paint_wedges(ui, center, s, v);
    paint_hub(p, center, s, v);
}

fn paint_wedges(ui: &Ui, center: Pos2, s: f32, v: &View) {
    let p = ui.painter();
    let a = v.open_t * v.swap_t;
    if v.choices.is_empty() {
        let at = center + vec2(0.0, -(RING_R0 + RING_R1) / 2.0 * s);
        let hint = tr("Nothing to do with these");
        glass::text(p, at, hint, 13.0, TEXT_MUTED.gamma_multiply(a), false);
        return;
    }

    let n = v.choices.len();
    let spin = rotation(v.swap_t);
    for (i, choice) in v.choices.iter().enumerate() {
        let (a0, a1) = sector_angles(i, n);
        let (a0, a1) = (a0 + spin, a1 + spin);
        let chosen = v.chosen == Some(i);
        let lit = chosen || v.hovered == Some(Hit::Choice(i));
        let h = if v.animate {
            let id = Id::unique(("wedge", v.serial, v.epoch, i));
            ui.ctx().animate_bool_with_time(id, lit, 0.09)
        } else if lit {
            1.0
        } else {
            0.0
        };

        let r1 = (RING_R1 + GROW * h) * s;
        let (outer, inner) = rounded_sector(center, RING_R0 * s, r1, a0, a1, GAP * s, CORNER * s);
        let (light, shade) = if chosen {
            (
                ACCENT_LIGHT.lerp_to_gamma(Color32::WHITE, 0.5),
                ACCENT_LIGHT,
            )
        } else {
            (
                WEDGE.lerp_to_gamma(ACCENT_LIGHT, h),
                WEDGE_SHADE.lerp_to_gamma(ACCENT, h),
            )
        };
        let shade_at = |pos: Pos2| {
            // Lit from above: lighter toward the top of the wheel.
            let t = ((pos.y - center.y) / (DISC_R * s) * 0.5 + 0.5).clamp(0.0, 1.0);
            light.lerp_to_gamma(shade, t).gamma_multiply(0.94 * a)
        };
        p.add(strip(&outer, &inner, shade_at));
        let mut outline = outer.clone();
        outline.extend(inner.iter().rev());
        let edge = Color32::BLACK
            .lerp_to_gamma(ACCENT, h)
            .gamma_multiply((0.1 + 0.1 * h) * a);
        p.add(Shape::closed_line(outline, Stroke::new(1.0, edge)));

        let dir = Vec2::angled((a0 + a1) / 2.0);
        let at = center + dir * ((RING_R0 + RING_R1) / 2.0 * s + 2.0 * h);
        let color = TEXT.lerp_to_gamma(Color32::WHITE, h).gamma_multiply(a);
        glass::text(p, at, tr(choice.label), 15.0 + h, color, true);
    }
}

fn paint_hub(p: &Painter, center: Pos2, s: f32, v: &View) {
    let a = v.open_t;
    let ring = v.modes.len() > 1;
    p.circle_filled(
        center + vec2(0.0, 1.5),
        HUB_R * s + 1.0,
        Color32::BLACK.gamma_multiply(0.06 * a),
    );
    if ring {
        let r = HUB_R * s;
        p.circle_filled(center, r, TRACK.gamma_multiply(0.97 * a));
        p.circle_stroke(
            center,
            r,
            Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.07 * a)),
        );
    }

    // The mode slices.
    let (r0, r1) = ((INFO_R + 3.0) * s, (HUB_R - 3.0) * s);
    let n = v.modes.len();
    for (i, &mode) in v.modes.iter().enumerate().filter(|_| ring) {
        let (a0, a1) = sector_angles(i, n);
        let current = mode == v.mode;
        let pointed = v.hovered == Some(Hit::Mode(mode));
        if current || pointed {
            let (outer, inner) =
                rounded_sector(center, r0, r1, a0, a1, MODE_GAP * s, MODE_CORNER * s);
            let fill = if current {
                Color32::WHITE
            } else {
                TRACK.lerp_to_gamma(Color32::WHITE, 0.5)
            };
            if current {
                // A soft shadow under the raised slice.
                let drop = |chain: &[Pos2]| {
                    chain
                        .iter()
                        .map(|p| *p + vec2(0.0, 1.0))
                        .collect::<Vec<_>>()
                };
                let shadow = Color32::BLACK.gamma_multiply(0.08 * a);
                p.add(strip(&drop(&outer), &drop(&inner), |_| shadow));
            }
            p.add(strip(&outer, &inner, |_| fill.gamma_multiply(a)));
        }
        // Labels follow the ring, turned so they read left to right.
        let mid = (a0 + a1) / 2.0;
        let at = center + Vec2::angled(mid) * (r0 + r1) / 2.0;
        let mut turn = mid + FRAC_PI_2;
        if turn > FRAC_PI_2 && turn < 3.0 * FRAC_PI_2 {
            turn -= PI;
        }
        let color = if current { TEXT } else { TEXT_MUTED };
        glass::text_turned(
            p,
            at,
            tr(mode.title()),
            9.5 * s,
            color.gamma_multiply(a),
            true,
            turn,
        );
    }

    // The centre: what is aimed at, or what is being dragged.
    // Without a ring the centre takes the whole hub.
    let centre_r = if ring { INFO_R } else { HUB_R };
    p.circle_filled(
        center,
        centre_r * s,
        Color32::from_rgb(252, 252, 253).gamma_multiply(a),
    );
    p.circle_stroke(
        center,
        centre_r * s,
        Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.07 * a)),
    );
    let aimed = match v.hovered {
        Some(Hit::Choice(i)) => v.choices.get(i).map(|c| tr(c.label)),
        _ => None,
    };
    let text = aimed.unwrap_or(v.info);
    let bold = aimed.is_some();
    let room = (centre_r * 2.0 - 10.0) * s;
    let size = 13.0 * s;
    let size = size * (room / glass::text_width(p, text, size, bold)).min(1.0);
    glass::text(p, center, text, size, TEXT.gamma_multiply(a), bold);
}

/// Boundary of an annular sector between angles `a0..a1` and radii `r0..r1`, with straight
/// sides `gap / 2` inside the sector's radial edges and every corner rounded by `corner`.
/// Returns the outer chain and the inner chain, both running from the `a0` side to `a1`.
fn rounded_sector(
    c: Pos2,
    r0: f32,
    r1: f32,
    a0: f32,
    a1: f32,
    gap: f32,
    corner: f32,
) -> (Vec<Pos2>, Vec<Pos2>) {
    let g = gap / 2.0;
    // Keep the corners from overlapping on narrow or thin wedges.
    let inner_room = (r0 * (a1 - a0) - gap) / 2.0 - 1.0;
    let k = corner.min(inner_room).min((r1 - r0) / 2.0 - 1.0).max(0.5);

    // Each side is the line parallel to the radial edge at `theta`, `g` toward the inside;
    // `side` is +1 on the a0 edge (the inside lies at larger angles) and -1 on the a1 edge.
    // A corner circle of radius k touches that side and an arc at radius `rho ± k`.
    let corner_center = |theta: f32, side: f32, rho: f32| {
        let delta = ((k + g) / rho).clamp(-1.0, 1.0).asin();
        c + Vec2::angled(theta + side * delta) * rho
    };
    let into = |theta: f32, side: f32| Vec2::angled(theta + side * FRAC_PI_2);
    let angle_of = |p: Pos2| (p - c).angle();

    let o0 = corner_center(a0, 1.0, r1 - k);
    let o1 = corner_center(a1, -1.0, r1 - k);
    let i0 = corner_center(a0, 1.0, r0 + k);
    let i1 = corner_center(a1, -1.0, r0 + k);

    let mut outer = Vec::new();
    corner_arc(&mut outer, o0, k, -into(a0, 1.0), (o0 - c).normalized());
    arc_points(&mut outer, c, r1, angle_of(o0), angle_of(o1));
    corner_arc(&mut outer, o1, k, (o1 - c).normalized(), -into(a1, -1.0));

    let mut inner = Vec::new();
    corner_arc(&mut inner, i0, k, -into(a0, 1.0), -(i0 - c).normalized());
    arc_points(&mut inner, c, r0, angle_of(i0), angle_of(i1));
    corner_arc(&mut inner, i1, k, -(i1 - c).normalized(), -into(a1, -1.0));

    (outer, inner)
}

/// Points of a circle of radius `r` around `center`, from direction `from` to `to` the short
/// way round.
fn corner_arc(out: &mut Vec<Pos2>, center: Pos2, r: f32, from: Vec2, to: Vec2) {
    let start = from.angle();
    let mut sweep = to.angle() - start;
    if sweep > PI {
        sweep -= TAU;
    } else if sweep < -PI {
        sweep += TAU;
    }
    let steps = ((sweep.abs() * r / 2.0).ceil() as usize).max(2);
    for i in 0..=steps {
        out.push(center + Vec2::angled(start + sweep * i as f32 / steps as f32) * r);
    }
}

/// Points strictly between angles `from` and `to` (counter to the clock in screen space, i.e.
/// increasing angle) on a circle of radius `r`.
fn arc_points(out: &mut Vec<Pos2>, center: Pos2, r: f32, from: f32, to: f32) {
    let mut to = to;
    while to < from {
        to += TAU;
    }
    let steps = (((to - from) * r / 5.0).ceil() as usize).max(2);
    for i in 1..steps {
        out.push(center + Vec2::angled(from + (to - from) * i as f32 / steps as f32) * r);
    }
}

/// Fills the area between two chains running the same way, pairing points by arc length.
fn strip(a: &[Pos2], b: &[Pos2], color: impl Fn(Pos2) -> Color32) -> Shape {
    let n = a.len().max(b.len()).max(2);
    let (a, b) = (resample(a, n), resample(b, n));
    let mut mesh = Mesh::default();
    for i in 0..n {
        mesh.colored_vertex(a[i], color(a[i]));
        mesh.colored_vertex(b[i], color(b[i]));
        if i > 0 {
            let k = 2 * i as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    Shape::mesh(mesh)
}

/// `n` points spaced evenly along the polyline.
fn resample(points: &[Pos2], n: usize) -> Vec<Pos2> {
    let mut lengths = vec![0.0];
    for w in points.windows(2) {
        lengths.push(lengths.last().unwrap() + (w[1] - w[0]).length());
    }
    let total = *lengths.last().unwrap();
    let mut out = Vec::with_capacity(n);
    let mut seg = 0;
    for i in 0..n {
        let target = total * i as f32 / (n - 1) as f32;
        while seg + 2 < lengths.len() && lengths[seg + 1] < target {
            seg += 1;
        }
        let span = (lengths[seg + 1] - lengths[seg]).max(f32::EPSILON);
        let t = ((target - lengths[seg]) / span).clamp(0.0, 1.0);
        out.push(points[seg] + (points[seg + 1] - points[seg]) * t);
    }
    out
}

/// Pill under the wheel naming the files, or previewing the aimed-at choice.
/// Returns where it was drawn, for the backdrop blur.
pub fn caption(p: &Painter, center: Pos2, s: f32, text: &str, alpha: f32) -> Rect {
    let galley = p.layout_no_wrap(text.to_owned(), FontId::proportional(12.5), TEXT);
    let size = galley.size() + vec2(28.0, 14.0);
    let top = center.y + (DISC_R + CAPTION_GAP) * s;
    let rect = Rect::from_min_size(Pos2::new(center.x - size.x / 2.0, top), size);
    glass::capsule(p, rect, alpha);
    glass::text(
        p,
        rect.center(),
        text,
        12.5,
        TEXT.gamma_multiply(alpha),
        false,
    );
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_follow_the_rings() {
        let c = Pos2::new(500.0, 500.0);
        let all = &Mode::ALL;
        assert_eq!(hit(c, c, all, 6), Hit::Hub);
        assert_eq!(hit(c, c + vec2(0.0, -(HUB_R + 8.0)), all, 6), Hit::Disc);
        assert_eq!(hit(c, c + vec2(0.0, -120.0), all, 6), Hit::Choice(0));
        assert_eq!(hit(c, c + vec2(120.0, 0.0), all, 4), Hit::Choice(1));
        assert_eq!(hit(c, c + vec2(0.0, DISC_R + 40.0), all, 6), Hit::Outside);
    }

    #[test]
    fn mode_slices_sit_around_the_centre() {
        let c = Pos2::new(500.0, 500.0);
        let at = |degrees: f32| c + Vec2::angled(degrees.to_radians()) * (INFO_R + HUB_R) / 2.0;
        // Three modes: thirds, Convert at the top.
        let all = &Mode::ALL;
        assert_eq!(hit(c, at(-90.0), all, 6), Hit::Mode(Mode::Convert));
        assert_eq!(hit(c, at(30.0), all, 6), Hit::Mode(Mode::Tools));
        assert_eq!(hit(c, at(150.0), all, 6), Hit::Mode(Mode::Actions));
        // With no choices at all, the slices still work.
        assert_eq!(hit(c, at(150.0), all, 0), Hit::Mode(Mode::Actions));
        assert_eq!(hit(c, c + vec2(INFO_R - 2.0, 0.0), all, 6), Hit::Hub);
        // Two: halves, the first on top.
        let two = &[Mode::Convert, Mode::Actions];
        assert_eq!(hit(c, at(-10.0), two, 6), Hit::Mode(Mode::Convert));
        assert_eq!(hit(c, at(-170.0), two, 6), Hit::Mode(Mode::Convert));
        assert_eq!(hit(c, at(10.0), two, 6), Hit::Mode(Mode::Actions));
        assert_eq!(hit(c, at(170.0), two, 6), Hit::Mode(Mode::Actions));
        // One: no ring, the band belongs to the centre.
        assert_eq!(hit(c, at(150.0), &[Mode::Actions], 6), Hit::Hub);
    }

    #[test]
    fn rounded_sector_stays_inside_its_wedge() {
        let (a0, a1) = sector_angles(0, 6);
        let (outer, inner) = rounded_sector(Pos2::ZERO, RING_R0, RING_R1, a0, a1, GAP, CORNER);
        for p in outer.iter().chain(&inner) {
            let r = p.to_vec2().length();
            assert!((RING_R0 - 0.5..=RING_R1 + 0.5).contains(&r), "radius {r}");
            let angle = p.to_vec2().angle();
            assert!(angle > a0 && angle < a1, "angle {angle} outside {a0}..{a1}");
        }
    }
}
