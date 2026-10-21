//! Wheel geometry, hit testing and painting.
//!
//! A light frame holds a ring of rounded wedges around a hub. The hub's white centre shows what
//! is being dragged or, while aiming, the format it would become. Around it runs a ring with a
//! slice per mode that has something to offer, the first at the top: halves for two modes,
//! thirds for three, and no ring at all for one. Stopping the drag on another slice switches to
//! that mode. Wedge 0 is centred at twelve o'clock and wedges run clockwise.
//!
//! It moves on springs (see `motion`): it pops open a little past its size, its wedges unfold
//! out of the hub one after another (again when the mode switches), the wedge aimed at moves
//! out and reaches into a lobe the frame swells into around it, and the hub's text slides over
//! to the new one.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

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
/// How far the frame swells out around the wedge aimed at: a lobe as wide as the wedge, rounding
/// back to the disc over `SHOULDER` (radians) either side.
pub const FRAME_SWELL: f32 = 19.0;
const SHOULDER: f32 = 0.32;
/// The wedge aimed at moves out by `WEDGE_LIFT` and reaches into the lobe, `LOBE_MARGIN` short
/// of its edge; its label goes along, halfway between.
const WEDGE_LIFT: f32 = 8.0;
const LOBE_MARGIN: f32 = 7.0;
const WEDGE_SWELL: f32 = DISC_R - RING_R1 + FRAME_SWELL - LOBE_MARGIN;
const LABEL_SWELL: f32 = (WEDGE_LIFT + WEDGE_SWELL) / 2.0;
/// A wedge unfolds from this fraction of its size.
const UNFOLD_FROM: f32 = 0.6;
/// How far the hub's text slides as it changes.
const HUB_SLIDE: f32 = 6.0;
/// Distance between the disc and the caption under it.
const CAPTION_GAP: f32 = 14.0;
/// Room kept clear around a wedge's label, and how small a long label may get to fit.
const LABEL_PAD: f32 = 12.0;
const MIN_LABEL: f32 = 11.0;

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

/// What is under `pos`, around a ring of `modes` with `choices` wedges.
pub fn hit(center: Pos2, pos: Pos2, modes: &[Mode], choices: usize) -> Hit {
    let d = pos - center;
    let r = d.length();
    let ring = modes.len() > 1;
    if r <= INFO_R || (r <= HUB_R + 4.0 && !ring) {
        Hit::Hub
    } else if r <= HUB_R + 4.0 {
        Hit::Mode(modes[sector_at(d, modes.len())])
    } else if r > DISC_R + 10.0 {
        Hit::Outside
    } else if r < RING_R0 || choices == 0 {
        Hit::Disc
    } else {
        Hit::Choice(sector_at(d, choices))
    }
}

/// Like `hit`, but the wedge swollen out (`swollen`) keeps the pointer out to the end of its
/// lobe, where it is drawn.
pub fn hit_swollen(
    center: Pos2,
    pos: Pos2,
    modes: &[Mode],
    choices: usize,
    swollen: Option<usize>,
) -> Hit {
    let hit = hit(center, pos, modes, choices);
    let d = pos - center;
    match swollen {
        Some(i)
            if hit == Hit::Outside
                && i < choices
                && d.length() <= DISC_R + FRAME_SWELL + 10.0
                && sector_at(d, choices) == i =>
        {
            Hit::Choice(i)
        }
        _ => hit,
    }
}

fn sector_at(d: Vec2, n: usize) -> usize {
    let span = TAU / n as f32;
    let a = (d.y.atan2(d.x) + FRAC_PI_2 + span / 2.0).rem_euclid(TAU);
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
    /// 0 → 1 as the wheel opens, 1 → 0 as it closes: how opaque it is.
    pub open_t: f32,
    /// Its size: a little past 1 for a moment as it opens, less as it closes.
    pub scale: f32,
    /// Per wedge, how far it has unfolded out of the hub (0 → 1, a little past on the way).
    pub unfold: &'a [f32],
    /// Per wedge, how far it swells toward the pointer (0 → 1, past it and back on the way).
    pub swell: &'a [f32],
    pub hub: Hub<'a>,
}

/// What the hub says: the name of what is aimed at in bold, or the files' size or count; while
/// that changes, the text it replaces too.
pub struct Hub<'a> {
    pub text: &'a str,
    pub bold: bool,
    /// The text going, and how visible it was when it began to go.
    pub before: Option<(&'a str, bool, f32)>,
    /// 0 → 1 as the new text takes over.
    pub t: f32,
}

/// How visible the hub's going and coming texts are, `t` of the way through a change: the one
/// going is gone halfway, the one coming comes in over the rest, so they hardly overlap.
pub fn hub_fade(t: f32) -> (f32, f32) {
    let t = ease_out(t.clamp(0.0, 1.0));
    ((1.0 - 2.0 * t).max(0.0), ((t - 0.25) / 0.75).clamp(0.0, 1.0))
}

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// The wheel's size as it closes (0 → 1 is how open it still is).
pub fn scale(open_t: f32) -> f32 {
    0.6 + 0.4 * ease_out(open_t)
}

pub fn paint(p: &Painter, center: Pos2, v: &View) {
    let s = v.scale;
    if v.swell.iter().all(|h| *h <= 0.001) {
        glass::frame(p, center, DISC_R * s, v.open_t);
    } else {
        let radius = |angle: f32| (DISC_R + bulge_at(angle, v.swell)) * s;
        glass::frame_shaped(p, center, radius, v.open_t);
    }
    paint_wedges(p, center, s, v);
    paint_hub(p, center, s, v);
}

/// How far out past the disc the frame reaches at `angle`, around the wedges swelling as `swell`
/// says (in points, before scaling). A wedge settling back doesn't dent it.
pub fn bulge_at(angle: f32, swell: &[f32]) -> f32 {
    let n = swell.len();
    let half = PI / n.max(1) as f32;
    let out: f32 = swell
        .iter()
        .enumerate()
        .map(|(i, &h)| {
            let (a0, a1) = sector_angles(i, n);
            let d = (angle - (a0 + a1) / 2.0 + PI).rem_euclid(TAU) - PI;
            h * lobe(d.abs(), half)
        })
        .sum();
    FRAME_SWELL * out.max(0.0)
}

/// 1 within `half` of a wedge's middle, falling smoothly to 0 over the shoulder past it.
fn lobe(d: f32, half: f32) -> f32 {
    if d <= half {
        1.0
    } else if d >= half + SHOULDER {
        0.0
    } else {
        (1.0 + (PI * (d - half) / SHOULDER).cos()) / 2.0
    }
}

/// A wedge's inner and outer radius as it swells (before scaling).
fn wedge_radii(swell: f32) -> (f32, f32) {
    (
        RING_R0 + WEDGE_LIFT * swell,
        RING_R1 + WEDGE_SWELL * swell,
    )
}

/// Where the frame swells out, as circles inside it along each lobe, for the backdrop blur.
pub fn bulges(center: Pos2, scale: f32, swell: &[f32]) -> Vec<(Pos2, f32)> {
    const R: f32 = 30.0;
    let n = swell.len();
    let mut circles = Vec::new();
    for (i, _) in swell.iter().enumerate().filter(|(_, h)| **h > 0.05) {
        let (a0, a1) = sector_angles(i, n);
        let mid = (a0 + a1) / 2.0;
        let reach = DISC_R + bulge_at(mid, swell) - R;
        // Along the lobe's top, kept within the wedge's sides, where the lobe is round.
        let half = ((a1 - a0) / 2.0 - R / reach).max(0.0);
        let count = (2.0 * half * reach / R).ceil() as usize;
        for k in 0..=count {
            let a = if count == 0 {
                mid
            } else {
                mid - half + 2.0 * half * k as f32 / count as f32
            };
            let reach = DISC_R + bulge_at(a, swell) - R;
            circles.push((center + Vec2::angled(a) * reach * scale, R * scale));
        }
    }
    circles
}

fn paint_wedges(p: &Painter, center: Pos2, s: f32, v: &View) {
    let a = v.open_t;
    if v.choices.is_empty() {
        let at = center + vec2(0.0, -(RING_R0 + RING_R1) / 2.0 * s);
        let hint = tr("Nothing to do with these");
        glass::text(p, at, hint, 13.0, TEXT_MUTED.gamma_multiply(a), false);
        return;
    }

    let n = v.choices.len();
    for (i, choice) in v.choices.iter().enumerate() {
        let (a0, a1) = sector_angles(i, n);
        let chosen = v.chosen == Some(i);
        // `swell` runs past 1 and below 0 on the way; colours stop at the ends.
        let swell = v.swell.get(i).copied().unwrap_or(0.0);
        let h = swell.clamp(0.0, 1.0);
        let unfold = v.unfold.get(i).copied().unwrap_or(1.0);
        let k = s * (UNFOLD_FROM + (1.0 - UNFOLD_FROM) * unfold);
        let a = a * (unfold * 3.0).clamp(0.0, 1.0);

        let (r0, r1) = wedge_radii(swell);
        let (outer, inner) = rounded_sector(center, r0 * k, r1 * k, a0, a1, GAP * k, CORNER * k);
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
            let t = ((pos.y - center.y) / (DISC_R * k) * 0.5 + 0.5).clamp(0.0, 1.0);
            light.lerp_to_gamma(shade, t).gamma_multiply(0.94 * a)
        };
        p.add(strip(&outer, &inner, shade_at));
        let mut outline = outer.clone();
        outline.extend(inner.iter().rev());
        let edge = Color32::BLACK
            .lerp_to_gamma(ACCENT, h)
            .gamma_multiply((0.1 + 0.1 * h) * a);
        p.add(Shape::closed_line(outline, Stroke::new(1.0, edge)));

        let mid = (a0 + a1) / 2.0;
        let r_mid = (RING_R0 + RING_R1) / 2.0;
        let at = center + Vec2::angled(mid) * (r_mid + LABEL_SWELL * swell) * k;
        let color = TEXT.lerp_to_gamma(Color32::WHITE, h).gamma_multiply(a);
        // Laid out at full size, so it keeps its lines as the wedge unfolds: across the wedge
        // where it points up or down, along it where it points sideways.
        let across = 2.0 * r_mid * s * ((a1 - a0) / 2.0).sin();
        let along = (RING_R1 - RING_R0) * s;
        let room = 1.0 / ((mid.cos() / along).powi(2) + (mid.sin() / across).powi(2)).sqrt();
        wedge_label(
            p,
            at,
            tr(choice.label),
            15.0 + h,
            color,
            room - LABEL_PAD * s,
            k / s,
        );
    }
}

/// A wedge's label within `room` points: on one line, or on two split at the space nearest
/// its middle, and only then smaller, down to `MIN_LABEL`; then drawn `zoom` times that size.
fn wedge_label(
    p: &Painter,
    at: Pos2,
    label: &str,
    size: f32,
    color: Color32,
    room: f32,
    zoom: f32,
) {
    let width = |s: &str, size: f32| glass::text_width(p, s, size, true);
    if width(label, size) <= room {
        glass::text(p, at, label, size * zoom, color, true);
        return;
    }
    let middle = label.len() / 2;
    let split = label
        .match_indices(' ')
        .map(|(i, _)| i)
        .min_by_key(|i| i.abs_diff(middle));
    let lines: Vec<&str> = match split {
        Some(i) => vec![&label[..i], &label[i + 1..]],
        None => vec![label],
    };
    let widest = lines.iter().map(|l| width(l, size)).fold(0.0, f32::max);
    let size = if widest > room {
        (size * room / widest).max(MIN_LABEL)
    } else {
        size
    };
    let step = size * 1.1 * zoom;
    let top = at.y - step * (lines.len() - 1) as f32 / 2.0;
    for (k, line) in lines.iter().enumerate() {
        glass::text(
            p,
            pos2(at.x, top + step * k as f32),
            line,
            size * zoom,
            color,
            true,
        );
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
        // Labels bend with the ring and read left to right.
        let color = if current { TEXT } else { TEXT_MUTED };
        glass::text_on_arc(
            p,
            center,
            (r0 + r1) / 2.0,
            (a0 + a1) / 2.0,
            tr(mode.title()),
            9.5 * s,
            color.gamma_multiply(a),
            true,
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
    // The text that goes slides up and fades as the new one comes up from below.
    let room = (centre_r * 2.0 - 10.0) * s;
    let line = |text: &str, bold: bool, alpha: f32, dy: f32| {
        let size = 13.0 * s;
        let size = size * (room / glass::text_width(p, text, size, bold)).min(1.0);
        let at = center + vec2(0.0, dy * s);
        glass::text(p, at, text, size, TEXT.gamma_multiply(a * alpha), bold);
    };
    let t = ease_out(v.hub.t.clamp(0.0, 1.0));
    let (going, coming) = hub_fade(v.hub.t);
    match v.hub.before.filter(|_| t < 1.0) {
        Some((before, bold, was)) => {
            line(before, bold, was * going, -HUB_SLIDE * t);
            line(v.hub.text, v.hub.bold, coming, HUB_SLIDE * (1.0 - t));
        }
        None => line(v.hub.text, v.hub.bold, 1.0, 0.0),
    }
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
/// The caption under the disc, `drop` further down while the frame swells out at the bottom.
pub fn caption(p: &Painter, center: Pos2, s: f32, text: &str, alpha: f32, drop: f32) -> Rect {
    let galley = p.layout_no_wrap(text.to_owned(), FontId::proportional(12.5), TEXT);
    let size = galley.size() + vec2(28.0, 14.0);
    let top = center.y + (DISC_R + CAPTION_GAP + drop) * s;
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

    #[test]
    fn a_swollen_wedge_and_its_blur_stay_inside_the_frame() {
        let c = Pos2::ZERO;
        let frame = |p: Pos2, swell: &[f32]| DISC_R + bulge_at((p - c).angle(), swell);
        for n in 1..=12 {
            // Swollen, past it as the spring overshoots, and settling back below rest.
            for h in [1.0, 1.25, -0.25] {
                let mut swell = vec![0.0; n];
                swell[n / 2] = h;
                let (a0, a1) = sector_angles(n / 2, n);
                let (r0, r1) = wedge_radii(h);
                let (outer, _) = rounded_sector(c, r0, r1, a0, a1, GAP, CORNER);
                for p in &outer {
                    let room = frame(*p, &swell) - p.to_vec2().length();
                    assert!(room > 4.0, "{n} wedges, swell {h}: {room} from the edge");
                }
                for (at, r) in bulges(c, 1.0, &swell) {
                    for k in 0..64 {
                        let p = at + Vec2::angled(k as f32 / 64.0 * TAU) * r;
                        let past = p.to_vec2().length() - frame(p, &swell);
                        assert!(past < 0.5, "{n} wedges, swell {h}: blur {past} past the edge");
                    }
                }
            }
        }
    }

    #[test]
    fn the_swollen_wedge_keeps_the_pointer_out_to_its_lobe() {
        let c = Pos2::ZERO;
        let modes = [Mode::Convert];
        let tip = c + Vec2::angled(-FRAC_PI_2) * (DISC_R + FRAME_SWELL);
        assert_eq!(hit(c, tip, &modes, 8), Hit::Outside);
        assert_eq!(hit_swollen(c, tip, &modes, 8, Some(0)), Hit::Choice(0));
        // Only its own wedge: beside it, out there is outside.
        assert_eq!(hit_swollen(c, tip, &modes, 8, Some(1)), Hit::Outside);
    }
}
