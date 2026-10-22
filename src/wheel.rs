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
//! to the new one. A wedge with options (a keyboard layout, a volume level) pops them out past
//! its lobe as bubbles while it is aimed at. Before the disc grows out of the hub, a droplet
//! comes round the hub and merges into it (see `paint_droplet`); a click flicks the hub toward
//! it, and it springs back.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{
    Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, TextureId, Vec2, pos2, vec2,
};

use crate::catalog::Mode;
use crate::glass::{self, ACCENT, ACCENT_LIGHT, RED, palette};
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
/// The bubbles a wedge's options pop out as: their radius, the room between two, and how far
/// out from the centre they sit, just past the lobe.
pub const BUBBLE_R: f32 = 21.0;
const BUBBLE_GAP: f32 = 7.0;
pub const BUBBLE_AT: f32 = DISC_R + FRAME_SWELL + 6.0 + BUBBLE_R;
/// How far the hub's text slides as it changes.
const HUB_SLIDE: f32 = 6.0;
/// Distance between the disc and the caption under it.
pub const CAPTION_GAP: f32 = 14.0;
/// Room kept clear around a wedge's label, and how small a long label may get to fit.
const LABEL_PAD: f32 = 12.0;
const MIN_LABEL: f32 = 11.0;
/// An app's icon on a wedge, and how far above the middle; its name goes below.
const WEDGE_ICON: f32 = 30.0;
const ICON_RISE: f32 = 11.0;
const ICON_LABEL_DROP: f32 = 18.0;
/// The droplet that comes round the hub as the wheel opens: its radius, and how far out past
/// the hub it starts.
const DROPLET_R: f32 = 7.5;
const DROPLET_OUT: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The white centre.
    Hub,
    /// A mode slice around the centre.
    Mode(Mode),
    Choice(usize),
    /// One of the options popped out of the wedge aimed at.
    Sub(usize),
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
/// lobe, where it is drawn, and on past it to its `bubbles` and between them.
pub fn hit_swollen(
    center: Pos2,
    pos: Pos2,
    modes: &[Mode],
    choices: usize,
    swollen: Option<usize>,
    bubbles: usize,
) -> Hit {
    let hit = hit(center, pos, modes, choices);
    let Some(i) = swollen.filter(|&i| i < choices) else {
        return hit;
    };
    let spots = bubble_spots(center, i, choices, bubbles);
    if let Some(k) = spots
        .iter()
        .position(|s| (pos - *s).length() <= BUBBLE_R + 3.0)
    {
        return Hit::Sub(k);
    }
    if hit != Hit::Outside {
        return hit;
    }
    let d = pos - center;
    let r = d.length();
    let in_lobe = r <= DISC_R + FRAME_SWELL + 10.0 && sector_at(d, choices) == i;
    let angles = bubble_angles(i, choices, bubbles);
    let margin = (BUBBLE_R + 6.0) / BUBBLE_AT;
    let by_bubbles = match (angles.first(), angles.last()) {
        (Some(&first), Some(&last)) => {
            r <= BUBBLE_AT + BUBBLE_R + 10.0
                && (d.angle() - (first - margin)).rem_euclid(TAU) <= last - first + 2.0 * margin
        }
        _ => false,
    };
    if in_lobe || by_bubbles {
        Hit::Choice(i)
    } else {
        hit
    }
}

/// The directions of wedge `i`'s `count` bubbles (of `n` wedges), side by side around its
/// middle, clockwise.
pub fn bubble_angles(i: usize, n: usize, count: usize) -> Vec<f32> {
    let (a0, a1) = sector_angles(i, n);
    let mid = (a0 + a1) / 2.0;
    let step = (2.0 * BUBBLE_R + BUBBLE_GAP) / BUBBLE_AT;
    (0..count)
        .map(|k| mid + (k as f32 - (count as f32 - 1.0) / 2.0) * step)
        .collect()
}

/// Where wedge `i`'s `count` bubbles sit once out.
pub fn bubble_spots(center: Pos2, i: usize, n: usize, count: usize) -> Vec<Pos2> {
    bubble_angles(i, n, count)
        .into_iter()
        .map(|a| center + Vec2::angled(a) * BUBBLE_AT)
        .collect()
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
    /// The wedges' labels, as shown.
    pub labels: &'a [String],
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
    /// Per wedge, an app's icon to show over its label.
    pub icons: &'a [Option<TextureId>],
    /// Per wedge, a switch's state: a dot under the label, lit while on.
    pub on: &'a [Option<bool>],
    pub hub: Hub<'a>,
    /// The hub's size, which grows apart from the disc's as the wheel opens.
    pub hub_scale: f32,
    /// How far the hub is flicked toward a click, springing back.
    pub hub_offset: Vec2,
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
    (
        (1.0 - 2.0 * t).max(0.0),
        ((t - 0.25) / 0.75).clamp(0.0, 1.0),
    )
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
    paint_hub(p, center + v.hub_offset, v.hub_scale, v);
}

/// The opening: a droplet comes round the hub (of scale `s`) clockwise and merges into it,
/// `t` 0 → 1 of the way. Returns where it was drawn, for the backdrop blur.
pub fn paint_droplet(p: &Painter, center: Pos2, s: f32, t: f32, alpha: f32) -> Option<(Pos2, f32)> {
    if !(0.0..1.0).contains(&t) {
        return None;
    }
    let along = ease_in_out(t);
    // It closes in on the hub as it comes round, and shrinks into it at the end.
    let angle = -FRAC_PI_2 + 0.45 + 1.5 * along;
    let reach = HUB_R + DROPLET_OUT * (1.0 - along * along);
    let merge = ((t - 0.7) / 0.3).clamp(0.0, 1.0);
    let r = DROPLET_R * (1.0 - ease_in_out(merge)) * s;
    if r < 0.5 {
        return None;
    }
    let at = center + Vec2::angled(angle) * (reach - DROPLET_R * merge) * s;
    let a = alpha * (t / 0.15).min(1.0);
    glass::capsule(p, Rect::from_center_size(at, Vec2::splat(2.0 * r)), a);
    p.circle_stroke(
        at,
        r,
        Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.35 * a)),
    );
    Some((at, r))
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    (RING_R0 + WEDGE_LIFT * swell, RING_R1 + WEDGE_SWELL * swell)
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
    if v.labels.is_empty() {
        let at = center + vec2(0.0, -(RING_R0 + RING_R1) / 2.0 * s);
        let hint = tr("Nothing to do with these");
        glass::text(
            p,
            at,
            hint,
            13.0,
            palette().text_muted.gamma_multiply(a),
            false,
        );
        return;
    }

    let n = v.labels.len();
    for (i, label) in v.labels.iter().enumerate() {
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
                palette().wedge.lerp_to_gamma(ACCENT_LIGHT, h),
                palette().wedge_shade.lerp_to_gamma(ACCENT, h),
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
        let edge = palette()
            .edge
            .lerp_to_gamma(ACCENT, h)
            .gamma_multiply((0.1 + 0.1 * h) * a);
        p.add(Shape::closed_line(outline, Stroke::new(1.0, edge)));

        let mid = (a0 + a1) / 2.0;
        let r_mid = (RING_R0 + RING_R1) / 2.0;
        let at = center + Vec2::angled(mid) * (r_mid + LABEL_SWELL * swell) * k;
        let color = palette()
            .text
            .lerp_to_gamma(Color32::WHITE, h)
            .gamma_multiply(a);
        // Laid out at full size, so it keeps its lines as the wedge unfolds: across the wedge
        // where it points up or down, along it where it points sideways.
        // One wedge goes all the way round: nothing beside it to keep clear of.
        let across = if n == 1 {
            2.0 * r_mid * s
        } else {
            2.0 * r_mid * s * ((a1 - a0) / 2.0).sin()
        };
        let along = (RING_R1 - RING_R0) * s;
        let room = 1.0 / ((mid.cos() / along).powi(2) + (mid.sin() / across).powi(2)).sqrt();
        // How far below `at` the label ends.
        let below = match v.icons.get(i).copied().flatten() {
            Some(icon) => {
                let size = (WEDGE_ICON + 4.0 * h) * k;
                let rect = Rect::from_center_size(at - vec2(0.0, ICON_RISE * k), Vec2::splat(size));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                p.image(icon, rect, uv, Color32::WHITE.gamma_multiply(a));
                let name_at = at + vec2(0.0, ICON_LABEL_DROP * k);
                ICON_LABEL_DROP * k
                    + wedge_label(
                        p,
                        name_at,
                        label,
                        11.5 + h,
                        color,
                        room - LABEL_PAD * s,
                        k / s,
                    )
            }
            None => wedge_label(p, at, label, 15.0 + h, color, room - LABEL_PAD * s, k / s),
        };
        if let Some(on) = v.on.get(i).copied().flatten() {
            let dot = if on {
                ACCENT.lerp_to_gamma(Color32::WHITE, h)
            } else {
                palette().text_muted.gamma_multiply(0.45)
            };
            p.circle_filled(
                at + vec2(0.0, below + 7.0 * k),
                3.0 * k,
                dot.gamma_multiply(a),
            );
        }
    }
}

/// A wedge's label within `room` points: on one line, or on two split at the space nearest
/// its middle, and only then smaller, down to `MIN_LABEL`; then drawn `zoom` times that size.
/// Returns how far below `at` its last line ends.
fn wedge_label(
    p: &Painter,
    at: Pos2,
    label: &str,
    size: f32,
    color: Color32,
    room: f32,
    zoom: f32,
) -> f32 {
    let width = |s: &str, size: f32| glass::text_width(p, s, size, true);
    if width(label, size) <= room {
        glass::text(p, at, label, size * zoom, color, true);
        return size * zoom * 0.5;
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
    step * (lines.len() - 1) as f32 / 2.0 + size * zoom * 0.5
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
        p.circle_filled(center, r, palette().track.gamma_multiply(0.97 * a));
        p.circle_stroke(
            center,
            r,
            Stroke::new(1.0, palette().edge.gamma_multiply(0.07 * a)),
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
                palette().raised
            } else {
                palette().track.lerp_to_gamma(palette().raised, 0.5)
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
        let color = if current {
            palette().text
        } else {
            palette().text_muted
        };
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
    p.circle_filled(center, centre_r * s, palette().hub.gamma_multiply(a));
    p.circle_stroke(
        center,
        centre_r * s,
        Stroke::new(1.0, palette().edge.gamma_multiply(0.07 * a)),
    );
    // The text that goes slides up and fades as the new one comes up from below.
    let room = (centre_r * 2.0 - 10.0) * s;
    let line = |text: &str, bold: bool, alpha: f32, dy: f32| {
        let size = 13.0 * s;
        let size = size * (room / glass::text_width(p, text, size, bold)).min(1.0);
        let at = center + vec2(0.0, dy * s);
        glass::text(
            p,
            at,
            text,
            size,
            palette().text.gamma_multiply(a * alpha),
            bold,
        );
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

/// One option popping out of the wedge aimed at.
pub struct Bubble<'a> {
    /// One line, or two (the second bold).
    pub label: &'a str,
    /// A speaker instead of the label, crossed out if `true`: what the sound will be.
    pub speaker: Option<bool>,
    /// An app's icon instead of the label.
    pub icon: Option<TextureId>,
    /// A ring around it, full at 1: how busy the computer is.
    pub gauge: Option<f32>,
    /// What is in use now: the layout, the level nearest.
    pub on: bool,
    pub hovered: bool,
    /// 0 → 1 as it pops out of the lobe (a little past on the way), and back as it goes.
    pub pop: f32,
}

/// Paints wedge `i`'s bubbles (of `n` wedges), out of the lobe toward where they sit; returns
/// where each was drawn, for the backdrop blur.
pub fn paint_bubbles(
    p: &Painter,
    center: Pos2,
    s: f32,
    i: usize,
    n: usize,
    bubbles: &[Bubble],
    alpha: f32,
) -> Vec<(Pos2, f32)> {
    let from = DISC_R + FRAME_SWELL / 2.0;
    let mut drawn = Vec::new();
    for (b, angle) in bubbles.iter().zip(bubble_angles(i, n, bubbles.len())) {
        if b.pop <= 0.02 {
            continue;
        }
        let at = center + Vec2::angled(angle) * (from + (BUBBLE_AT - from) * b.pop) * s;
        let r = BUBBLE_R * b.pop * s;
        let a = alpha * b.pop.clamp(0.0, 1.0);
        glass::capsule(p, Rect::from_center_size(at, Vec2::splat(2.0 * r)), a);
        if b.hovered {
            p.circle_filled(at, r - 2.5 * s, ACCENT_LIGHT.gamma_multiply(a));
        } else if b.on {
            p.circle_stroke(
                at,
                r - 3.0 * s,
                Stroke::new(1.8 * s, ACCENT.gamma_multiply(a)),
            );
        }
        let color = if b.hovered {
            Color32::WHITE
        } else {
            palette().text
        };
        if let Some(level) = b.gauge {
            gauge(p, at, r - 3.0 * s, level, s, a);
        }
        match (b.speaker, b.icon) {
            (Some(muted), _) => speaker(p, at, r * 0.36, muted, color.gamma_multiply(a)),
            (None, Some(icon)) => {
                let rect = Rect::from_center_size(at, Vec2::splat(r * 1.3));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                p.image(icon, rect, uv, Color32::WHITE.gamma_multiply(a));
            }
            (None, None) => bubble_label(p, at, r, b.label, b.pop, s, color.gamma_multiply(a)),
        }
        drawn.push((at, r));
    }
    drawn
}

/// A bubble's label: one line, or a small first line over a bold second ("CPU" over "12%").
fn bubble_label(p: &Painter, at: Pos2, r: f32, label: &str, pop: f32, s: f32, color: Color32) {
    let room = 2.0 * r - 9.0 * s;
    let fit = |text: &str, size: f32, bold: bool| {
        size * (room / glass::text_width(p, text, size, bold)).min(1.0)
    };
    match label.split_once('\n') {
        None => {
            let size = fit(label, 12.0 * pop.max(0.0) * s, true);
            glass::text(p, at, label, size, color, true);
        }
        Some((top, bottom)) => {
            let small = fit(top, 8.5 * pop.max(0.0) * s, false);
            let big = fit(bottom, 11.0 * pop.max(0.0) * s, true);
            glass::text(
                p,
                at - vec2(0.0, 6.0 * s),
                top,
                small,
                color.gamma_multiply(0.75),
                false,
            );
            glass::text(p, at + vec2(0.0, 5.0 * s), bottom, big, color, true);
        }
    }
}

/// A ring just inside a bubble, filled clockwise from the top to `level`, red when nearly full.
fn gauge(p: &Painter, c: Pos2, r: f32, level: f32, s: f32, alpha: f32) {
    p.circle_stroke(
        c,
        r,
        Stroke::new(2.5 * s, palette().edge.gamma_multiply(0.12 * alpha)),
    );
    let level = level.clamp(0.0, 1.0);
    let sweep = level * TAU;
    let steps = ((sweep * r / 3.0).ceil() as usize).max(2);
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| c + Vec2::angled(-FRAC_PI_2 + sweep * i as f32 / steps as f32) * r)
        .collect();
    let color = if level > 0.85 { RED } else { ACCENT };
    if level > 0.005 {
        p.add(Shape::line(
            points,
            Stroke::new(2.5 * s, color.gamma_multiply(alpha)),
        ));
    }
}

/// A speaker `u` points tall either side of `c`, with sound coming out or crossed out.
fn speaker(p: &Painter, c: Pos2, u: f32, muted: bool, color: Color32) {
    let c = c - vec2(0.25 * u, 0.0);
    let at = |x: f32, y: f32| c + vec2(x, y) * u;
    p.add(Shape::convex_polygon(
        vec![
            at(-1.0, -0.42),
            at(-0.4, -0.42),
            at(0.25, -1.0),
            at(0.25, 1.0),
            at(-0.4, 0.42),
            at(-1.0, 0.42),
        ],
        color,
        Stroke::NONE,
    ));
    let stroke = Stroke::new(0.17 * u, color);
    if muted {
        p.line_segment([at(0.6, -0.4), at(1.3, 0.4)], stroke);
        p.line_segment([at(0.6, 0.4), at(1.3, -0.4)], stroke);
    } else {
        for reach in [0.55, 0.95] {
            let points = (0..=8)
                .map(|k| at(0.3, 0.0) + Vec2::angled(-0.8 + 1.6 * k as f32 / 8.0) * reach * u)
                .collect();
            p.add(Shape::line(points, stroke));
        }
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
/// The caption under the disc, `drop` further down while the frame swells out at the bottom;
/// with no room below `bottom`, over the disc instead, `rise` further up.
#[allow(clippy::too_many_arguments)]
pub fn caption(
    p: &Painter,
    center: Pos2,
    s: f32,
    text: &str,
    alpha: f32,
    drop: f32,
    rise: f32,
    bottom: f32,
) -> Rect {
    let galley = p.layout_no_wrap(text.to_owned(), FontId::proportional(12.5), palette().text);
    let size = galley.size() + vec2(28.0, 14.0);
    let below = center.y + (DISC_R + CAPTION_GAP + drop) * s;
    let top = if below + size.y <= bottom {
        below
    } else {
        center.y - (DISC_R + CAPTION_GAP + rise) * s - size.y
    };
    let rect = Rect::from_min_size(Pos2::new(center.x - size.x / 2.0, top), size);
    glass::capsule(p, rect, alpha);
    glass::text(
        p,
        rect.center(),
        text,
        12.5,
        palette().text.gamma_multiply(alpha),
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
                        assert!(
                            past < 0.5,
                            "{n} wedges, swell {h}: blur {past} past the edge"
                        );
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
        assert_eq!(hit_swollen(c, tip, &modes, 8, Some(0), 0), Hit::Choice(0));
        // Only its own wedge: beside it, out there is outside.
        assert_eq!(hit_swollen(c, tip, &modes, 8, Some(1), 0), Hit::Outside);
    }

    #[test]
    fn bubbles_sit_past_the_lobe_and_keep_the_wedge_open_on_the_way() {
        let c = Pos2::ZERO;
        let modes = [Mode::Everyday];
        // Three wedges; the one at the bottom right (1) has five options out.
        let spots = bubble_spots(c, 1, 3, 5);
        for (k, spot) in spots.iter().enumerate() {
            assert_eq!(hit_swollen(c, *spot, &modes, 3, Some(1), 5), Hit::Sub(k));
            assert!(spot.to_vec2().length() - BUBBLE_R > DISC_R + FRAME_SWELL);
        }
        for pair in spots.windows(2) {
            assert!((pair[1] - pair[0]).length() >= 2.0 * BUBBLE_R + BUBBLE_GAP - 0.5);
            // Between two of them, and from the lobe out to one, the wedge stays aimed at.
            let between = c + ((pair[0] - c) + (pair[1] - c)) / 2.0;
            assert_eq!(
                hit_swollen(c, between, &modes, 3, Some(1), 5),
                Hit::Choice(1)
            );
        }
        let way_out = c + (spots[2] - c) * ((DISC_R + FRAME_SWELL + 1.0) / BUBBLE_AT);
        assert_eq!(
            hit_swollen(c, way_out, &modes, 3, Some(1), 5),
            Hit::Choice(1)
        );
        // Far past them, it is outside; and another wedge has no bubbles there.
        assert_eq!(
            hit_swollen(c, c + (spots[2] - c) * 1.4, &modes, 3, Some(1), 5),
            Hit::Outside
        );
        assert_eq!(
            hit_swollen(c, spots[2], &modes, 3, Some(0), 5),
            Hit::Outside
        );
    }
}
