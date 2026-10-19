//! What the fullscreen overlay shows, independent of the platform that feeds it events:
//! the wheel while a drag is over it, then a small progress card where it was dropped.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Ui, Vec2, vec2};

use crate::catalog::{self, Choice, Mode};
use crate::glass::{self, ACCENT, GREEN, GlassShape, RED, TEXT, TEXT_MUTED};
use crate::i18n::{tr, trf};
use crate::jobs::{self, Job};
use crate::wheel::{self, Hit};

const OPEN_TIME: f32 = 0.16;
const SWAP_TIME: f32 = 0.12;
/// How long the picked wedge stays lit before the wheel folds into the card.
const FLASH: Duration = Duration::from_millis(140);
/// How far a drag has to move after the wheel opens before it can pick anything.
const ARM_DISTANCE: f32 = 12.0;
/// A drag switches modes by stopping on another mode's slice for this long, too short to notice;
/// moving over a slice on the way somewhere else never switches.
const SETTLE: Duration = Duration::from_millis(50);
const CARD_APPEAR: f32 = 0.18;
const CARD_HOLD: Duration = Duration::from_millis(1600);
/// A failure stays up longer, so the reason can be read.
const CARD_HOLD_FAILED: Duration = Duration::from_millis(4500);
const CARD_FADE: f32 = 0.3;
/// How often a card redraws while real work is running.
const PROGRESS_TICK: Duration = Duration::from_millis(50);
/// Vertical distance between cards stacked at the same spot.
const CARD_STACK: f32 = 60.0;
/// The close button at a card's right end: drawn radius, and how near counts as on it.
const CLOSE_R: f32 = 10.0;
const CLOSE_HIT: f32 = 14.0;
/// A finished card the pointer was on stays this long after it leaves, before it fades.
const KEEP_AFTER_HOVER: Duration = Duration::from_millis(800);
/// Keeps the whole disc on screen when the drag is near an edge.
const EDGE_MARGIN: f32 = wheel::DISC_R + 16.0;

struct Wheel {
    serial: u32,
    center: Pos2,
    files: Vec<PathBuf>,
    summary: String,
    /// Size or count of the files, for the hub.
    info: String,
    /// Modes with something to offer; the ring shows only these.
    modes: Vec<Mode>,
    mode: Mode,
    choices: Vec<Choice>,
    /// Caption per choice, computed once: building it stats the files.
    previews: Vec<String>,
    pointer: Option<Pos2>,
    origin: Pos2,
    /// The drag has moved since the wheel opened. Until then nothing can be picked: if the
    /// wheel had to shift to fit on screen, the pointer may sit on a wedge it never aimed at.
    armed: bool,
    /// Another mode's slice under the drag, and when the drag last moved.
    passing: Option<(Mode, Instant)>,
    epoch: u32,
    swapped_at: Option<Instant>,
    opened_at: Instant,
    chosen: Option<usize>,
    closing_at: Option<Instant>,
}

impl Wheel {
    /// Returns whether the mode changed.
    fn set_mode(&mut self, mode: Mode, now: Instant) -> bool {
        self.passing = None;
        if mode == self.mode || !self.modes.contains(&mode) {
            return false;
        }
        self.mode = mode;
        self.choices = catalog::choices(&self.files, mode);
        self.previews = self
            .choices
            .iter()
            .map(|c| preview(&self.files, *c))
            .collect();
        self.epoch += 1;
        self.swapped_at = Some(now);
        self.origin = self.pointer.unwrap_or(self.origin);
        self.armed = false;
        true
    }

    /// Notes another mode's slice under the drag; the wheel switches once the drag stops there.
    fn update_ring(&mut self, now: Instant) {
        self.passing = match self.aim(now) {
            Some(Hit::Mode(m)) if m != self.mode && self.modes.contains(&m) => Some((m, now)),
            _ => None,
        };
    }

    fn swap_t(&self, now: Instant) -> f32 {
        self.swapped_at.map_or(1.0, |t| {
            (now.saturating_duration_since(t).as_secs_f32() / SWAP_TIME).min(1.0)
        })
    }

    fn aim(&self, now: Instant) -> Option<Hit> {
        let pos = self.pointer.filter(|_| self.armed)?;
        let rotation = wheel::rotation(self.swap_t(now));
        Some(wheel::hit_rotated(
            self.center,
            pos,
            &self.modes,
            self.choices.len(),
            rotation,
        ))
    }

    fn open_t(&self, now: Instant) -> f32 {
        let since = |t: Instant| now.saturating_duration_since(t).as_secs_f32() / OPEN_TIME;
        match self.closing_at {
            Some(at) => 1.0 - since(at).min(1.0),
            None => since(self.opened_at).min(1.0),
        }
    }

    fn closed(&self, now: Instant) -> bool {
        self.closing_at.is_some_and(|_| self.open_t(now) <= 0.0)
    }
}

struct Card {
    at: Pos2,
    batch: u32,
    title: String,
    shown_at: Instant,
    selection_requested: bool,
    /// Where it was last drawn, to be held or closed there.
    rect: Rect,
    /// Last time the pointer was on it: a finished card waits for it to go before fading.
    kept_at: Option<Instant>,
    /// Put away with its close button once done: fades out from then.
    closed_at: Option<Instant>,
}

impl Card {
    fn close_button(&self) -> Pos2 {
        self.rect.right_center() - vec2(self.rect.height() / 2.0, 0.0)
    }

    fn spot(&self, pos: Pos2) -> Option<CardSpot> {
        if (pos - self.close_button()).length() <= CLOSE_HIT {
            Some(CardSpot::Close)
        } else if self.rect.contains(pos) {
            Some(CardSpot::Body)
        } else {
            None
        }
    }
}

/// What is under the pointer on a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardSpot {
    /// The card itself: holding it moves it.
    Body,
    /// Its close button: cancels the work, or puts a finished card away.
    Close,
}

#[derive(Default)]
pub struct Overlay {
    wheel: Option<Wheel>,
    /// One per drop, oldest first; each stays until its work is done and it has faded.
    cards: Vec<Card>,
    jobs: Vec<Job>,
    batch: u32,
    serial: u32,
    /// Glass painted in the last frame, for the platform's backdrop blur.
    glass: Vec<GlassShape>,
    /// Something changed that deserves a click: the wheel opened, switched modes, moved onto
    /// another wedge or picked one.
    clicked: bool,
    /// The pointer over the overlay while no wheel takes it, for the cards.
    hover: Option<Pos2>,
    /// The card being moved, and where on it the pointer holds it.
    holding: Option<(u32, Vec2)>,
    /// The card whose close button is pressed, until the button comes up.
    closing: Option<u32>,
    /// Skip time-based egui animations (for screenshots).
    pub still: bool,
}

impl Overlay {
    /// A drag carrying `files` arrived at `pointer`; open the wheel around it.
    pub fn open(
        &mut self,
        files: Vec<PathBuf>,
        pointer: Pos2,
        screen: Rect,
        mode: Mode,
        now: Instant,
    ) {
        let mut files = files;
        let mut seen = std::collections::HashSet::new();
        files.retain(|f| seen.insert(f.clone()));

        let fit = screen.shrink(EDGE_MARGIN);
        let center = if fit.is_positive() {
            pointer.clamp(fit.min, fit.max)
        } else {
            screen.center()
        };
        // A mode with nothing for these files isn't offered: open on the first that has some.
        let modes = catalog::modes(&files);
        let mode = match modes.first() {
            Some(first) if !modes.contains(&mode) => *first,
            _ => mode,
        };
        let choices = catalog::choices(&files, mode);
        let previews = choices.iter().map(|c| preview(&files, *c)).collect();
        self.serial += 1;
        // The wheel takes the pointer now; a card held or pressed is let go.
        self.hover = None;
        self.holding = None;
        self.closing = None;
        self.wheel = Some(Wheel {
            serial: self.serial,
            center,
            summary: catalog::describe(&files),
            info: files_info(&files),
            modes,
            previews,
            files,
            mode,
            choices,
            pointer: Some(pointer),
            origin: pointer,
            armed: false,
            passing: None,
            epoch: 0,
            swapped_at: None,
            opened_at: now,
            chosen: None,
            closing_at: None,
        });
        self.clicked = true;
    }

    /// The drag moved to `pos`. Told the same place again, nothing moved: the pointer resting on
    /// a mode's slice is what switches to it, so that must not count as moving.
    pub fn drag_moved(&mut self, pos: Pos2, now: Instant) {
        let Some(w) = self.active_wheel() else {
            return;
        };
        if w.pointer == Some(pos) {
            return;
        }
        let before = w.aim(now);
        w.pointer = Some(pos);
        w.armed |= (pos - w.origin).length() > ARM_DISTANCE;
        w.update_ring(now);
        let after = w.aim(now);
        self.clicked |= matches!(after, Some(Hit::Choice(_))) && after != before;
    }

    /// Whether something clicked since the last call.
    pub fn take_click(&mut self) -> bool {
        std::mem::take(&mut self.clicked)
    }

    /// Whether a drop right now would pick something.
    pub fn would_accept(&self, now: Instant) -> bool {
        self.wheel
            .as_ref()
            .filter(|w| w.chosen.is_none() && w.closing_at.is_none())
            .is_some_and(|w| matches!(w.aim(now), Some(Hit::Choice(_))))
    }

    /// The drag was released over the overlay. Returns whether it picked something.
    pub fn drop(&mut self, now: Instant) -> bool {
        let pick = match self.active_wheel() {
            Some(w) => match w.aim(now) {
                Some(Hit::Choice(i)) => Some(i),
                _ => None,
            },
            _ => None,
        };
        match pick {
            Some(i) => {
                self.choose(i, now);
                true
            }
            None => {
                self.cancel(now);
                false
            }
        }
    }

    /// A click on a wheel opened without a drag: picks like a drop would, and a click on the
    /// mode ring switches to that mode at once.
    pub fn click(&mut self, now: Instant) -> bool {
        if let Some(w) = self.active_wheel()
            && let Some(Hit::Mode(mode)) = w.aim(now)
        {
            self.clicked |= w.set_mode(mode, now);
            return false;
        }
        self.drop(now)
    }

    /// The drag left, was cancelled, or dropped on nothing.
    pub fn cancel(&mut self, now: Instant) {
        if let Some(w) = self.active_wheel() {
            w.pointer = None;
            w.armed = false;
            w.passing = None;
            w.closing_at = Some(now);
        }
    }

    /// The overlay must take pointer input (to receive the drag) only while the wheel is live;
    /// the card that follows is click-through.
    pub fn wants_input(&self) -> bool {
        self.wheel
            .as_ref()
            .is_some_and(|w| w.chosen.is_none() && w.closing_at.is_none())
    }

    /// The window moved by `-by` on screen: what is drawn stays where it was.
    pub fn shift(&mut self, by: Vec2) {
        if let Some(w) = &mut self.wheel {
            w.center += by;
            w.origin += by;
            if let Some(p) = &mut w.pointer {
                *p += by;
            }
        }
        for card in &mut self.cards {
            card.at += by;
        }
    }

    /// Where the cards were last drawn. They take the pointer; the rest of the screen lets it
    /// through while no wheel is open.
    pub fn card_rects(&self) -> Vec<Rect> {
        self.cards
            .iter()
            .map(|c| c.rect)
            .filter(|r| r.is_positive())
            .collect()
    }

    /// What is under `pos` on the topmost card there.
    pub fn card_spot(&self, pos: Pos2) -> Option<CardSpot> {
        self.cards.iter().rev().find_map(|c| c.spot(pos))
    }

    /// The close button of the card under `pos`.
    pub fn close_button_of(&self, pos: Pos2) -> Option<Pos2> {
        self.cards
            .iter()
            .rev()
            .find(|c| c.spot(pos).is_some())
            .map(Card::close_button)
    }

    /// Whether a card is being moved.
    pub fn holding_card(&self) -> bool {
        self.holding.is_some()
    }

    /// The pointer moved to `pos` (`None`: it left). A card being held follows it.
    pub fn card_hover(&mut self, pos: Option<Pos2>) {
        self.hover = pos;
        if let (Some(pos), Some((batch, offset))) = (pos, self.holding)
            && let Some(card) = self.cards.iter_mut().find(|c| c.batch == batch)
        {
            card.at = pos - offset;
        }
    }

    /// The button went down at `pos`: takes hold of the card there, or presses its close
    /// button. Returns whether it was on a card.
    pub fn card_press(&mut self, pos: Pos2) -> bool {
        self.hover = Some(pos);
        let Some(card) = self.cards.iter().rev().find(|c| c.spot(pos).is_some()) else {
            return false;
        };
        match card.spot(pos) {
            Some(CardSpot::Close) => self.closing = Some(card.batch),
            _ => self.holding = Some((card.batch, pos - card.at)),
        }
        true
    }

    /// The button came up at `pos`: a card being moved stays where it is, and a close button
    /// still under the pointer closes its card.
    pub fn card_release(&mut self, pos: Pos2, now: Instant) {
        self.holding = None;
        let Some(batch) = self.closing.take() else {
            return;
        };
        let on_it = self
            .cards
            .iter()
            .any(|c| c.batch == batch && c.spot(pos) == Some(CardSpot::Close));
        if on_it {
            self.close_card(batch, now);
        }
    }

    /// Cancels the card's work, or, once it is done, puts the card away.
    fn close_card(&mut self, batch: u32, now: Instant) {
        let mut working = false;
        for job in self.jobs.iter_mut().filter(|j| j.batch == batch) {
            if job.finished_at(now).is_none() {
                job.cancel(now);
                working = true;
            }
        }
        if !working && let Some(card) = self.cards.iter_mut().find(|c| c.batch == batch) {
            card.closed_at = Some(now);
        }
    }

    /// Where the last frame drew glass; the platform blurs the backdrop there.
    pub fn glass(&self) -> &[GlassShape] {
        &self.glass
    }

    pub fn is_idle(&self) -> bool {
        self.wheel.is_none() && self.cards.is_empty()
    }

    fn active_wheel(&mut self) -> Option<&mut Wheel> {
        self.wheel
            .as_mut()
            .filter(|w| w.chosen.is_none() && w.closing_at.is_none())
    }

    fn choose(&mut self, index: usize, now: Instant) {
        let Some(w) = self.wheel.as_mut() else {
            return;
        };
        let Some(&choice) = w.choices.get(index) else {
            return;
        };
        w.chosen = Some(index);
        w.closing_at = Some(now + FLASH);
        self.clicked = true;
        self.batch += 1;
        self.jobs
            .extend(jobs::start(&w.files, choice, self.batch, now + FLASH));
        // Cards still showing at this spot move up out of the way, newest nearest.
        let mut slot = w.center;
        for card in self.cards.iter_mut().rev() {
            if (card.at.x - slot.x).abs() < 220.0 && (card.at.y - slot.y).abs() < CARD_STACK {
                card.at.y = slot.y - CARD_STACK;
                slot = card.at;
            }
        }
        self.cards.push(Card {
            at: w.center,
            batch: self.batch,
            title: w.previews[index].clone(),
            shown_at: now + FLASH,
            selection_requested: false,
            rect: Rect::NOTHING,
            kept_at: None,
            closed_at: None,
        });
    }

    /// Paints everything; returns how soon it wants to be painted again (`None`: never).
    pub fn paint(&mut self, ui: &mut Ui, now: Instant) -> Option<Duration> {
        let mut next: Option<Duration> = None;
        let mut soon = |d: Duration| next = Some(next.map_or(d, |n| n.min(d)));
        self.glass.clear();

        if self.wheel.as_ref().is_some_and(|w| w.closed(now)) {
            self.wheel = None;
        }
        if let Some(w) = self.active_wheel()
            && let Some((mode, since)) = w.passing
        {
            if now >= since + SETTLE {
                self.clicked |= w.set_mode(mode, now);
            } else {
                soon(since + SETTLE - now);
            }
        }
        if let Some(w) = &self.wheel {
            let open_t = w.open_t(now);
            let animating = open_t < 1.0
                || w.closing_at.is_some()
                || w.swapped_at
                    .is_some_and(|t| now < t + Duration::from_secs_f32(SWAP_TIME));
            if animating {
                soon(Duration::ZERO);
            }
            paint_wheel(ui, w, open_t, now, !self.still, &mut self.glass);
        }

        for card in &mut self.cards {
            if card.selection_requested || now < card.shown_at {
                continue;
            }
            let batch: Vec<&Job> = self.jobs.iter().filter(|j| j.batch == card.batch).collect();
            if batch.is_empty() || !batch.iter().all(|job| job.finished_at(now).is_some()) {
                continue;
            }
            card.selection_requested = true;
            let outputs: Vec<PathBuf> = batch
                .iter()
                .filter_map(|job| job.successful_output())
                .collect();
            if !outputs.is_empty() {
                crate::file_manager::select_items(outputs);
            }
        }

        let (jobs, glass) = (&self.jobs, &mut self.glass);
        let (hover, holding) = (self.hover, self.holding.map(|(batch, _)| batch));
        self.cards.retain_mut(|card| {
            let touched =
                holding == Some(card.batch) || hover.is_some_and(|h| card.rect.contains(h));
            if touched {
                card.kept_at = Some(now);
            }
            match paint_card(ui, card, jobs, now, hover, glass) {
                Some(wait) => {
                    soon(wait);
                    true
                }
                None => false,
            }
        });
        if self
            .holding
            .is_some_and(|(batch, _)| !self.cards.iter().any(|c| c.batch == batch))
        {
            self.holding = None;
        }
        let cards = &self.cards;
        self.jobs
            .retain(|j| cards.iter().any(|c| c.batch == j.batch));
        next
    }
}

fn paint_wheel(
    ui: &Ui,
    w: &Wheel,
    open_t: f32,
    now: Instant,
    animate: bool,
    shapes: &mut Vec<GlassShape>,
) {
    let hovered = if w.chosen.is_some() { None } else { w.aim(now) };
    let swap_t = w.swap_t(now);
    let view = wheel::View {
        choices: &w.choices,
        modes: &w.modes,
        mode: w.mode,
        hovered,
        chosen: w.chosen,
        info: &w.info,
        open_t,
        swap_t,
        epoch: w.epoch,
        serial: w.serial,
        animate,
    };
    wheel::paint(ui, w.center, &view);
    let s = wheel::scale(open_t);
    if open_t > 0.02 {
        shapes.push(GlassShape::Circle {
            center: w.center,
            radius: wheel::DISC_R * s,
        });
    }

    let caption = match hovered {
        Some(Hit::Choice(i)) if i < w.previews.len() => &w.previews[i],
        _ => &w.summary,
    };
    let rect = wheel::caption(ui.painter(), w.center, s, caption, open_t);
    if open_t > 0.02 {
        shapes.push(GlassShape::Capsule(rect));
    }
}

/// Progress card, with a close button at its right end. Returns how soon it needs repainting,
/// or `None` once it has faded out. Records where it was drawn in `card.rect`.
fn paint_card(
    ui: &Ui,
    card: &mut Card,
    jobs: &[Job],
    now: Instant,
    hover: Option<Pos2>,
    shapes: &mut Vec<GlassShape>,
) -> Option<Duration> {
    if now < card.shown_at {
        return Some(card.shown_at - now);
    }
    let batch: Vec<&Job> = jobs.iter().filter(|j| j.batch == card.batch).collect();
    let finished: Vec<Instant> = batch.iter().filter_map(|j| j.finished_at(now)).collect();
    let done_at = (finished.len() == batch.len())
        .then(|| finished.iter().copied().max().unwrap_or(card.shown_at));
    let cancelled = batch.iter().any(|j| j.is_cancelled());
    let errors: Vec<String> = batch.iter().filter_map(|j| j.error()).collect();
    let hold = if errors.is_empty() || cancelled {
        CARD_HOLD
    } else {
        CARD_HOLD_FAILED
    };
    // Fades once it has been up long enough and the pointer has left it, or at once if closed.
    let fade_from = match (card.closed_at, done_at) {
        (Some(closed), _) => Some(closed),
        (None, Some(at)) => Some(
            card.kept_at
                .map_or(at + hold, |kept| (at + hold).max(kept + KEEP_AFTER_HOVER)),
        ),
        (None, None) => None,
    };
    let fade = match fade_from {
        Some(from) if now >= from => 1.0 - (now - from).as_secs_f32() / CARD_FADE,
        _ => 1.0,
    };
    if fade <= 0.0 {
        return None;
    }
    let appear =
        wheel::ease_out((now.duration_since(card.shown_at).as_secs_f32() / CARD_APPEAR).min(1.0));
    let alpha = appear * fade;

    let subtitle = match done_at {
        Some(_) if cancelled => tr("Cancelled").to_owned(),
        Some(_) if !errors.is_empty() => {
            let reason = capitalised(&translated_error(&errors[0]));
            if batch.len() > 1 {
                trf(
                    "{} of {} failed · {}",
                    &[&errors.len(), &batch.len(), &reason],
                )
            } else {
                reason
            }
        }
        Some(_) if batch.iter().all(|j| j.is_simulated()) => tr("Done · simulated").to_owned(),
        Some(_) => match &batch[..] {
            [one] => one.message().unwrap_or_default(),
            _ => trf("Done · {} files", &[&batch.len()]),
        },
        None if batch.len() > 1 => trf(
            "Working on {} of {}…",
            &[&(finished.len() + 1).min(batch.len()), &batch.len()],
        ),
        None => tr("Working…").to_owned(),
    };

    let p = ui.painter();
    let title = p.layout_no_wrap(
        card.title.clone(),
        FontId::proportional(13.0),
        TEXT.gamma_multiply(alpha),
    );
    let sub = p.layout_no_wrap(
        shorten(&subtitle, 64),
        FontId::proportional(11.0),
        TEXT_MUTED.gamma_multiply(alpha),
    );
    let text_w = title.size().x.max(sub.size().x);
    let size = vec2(56.0 + text_w + 26.0 + 2.0 * CLOSE_R + 8.0, 52.0) * (0.85 + 0.15 * appear);
    let rect = Rect::from_center_size(card.at, size);
    card.rect = rect;
    let radius = size.y / 2.0;
    glass::capsule(p, rect, alpha);
    shapes.push(GlassShape::Capsule(rect));

    let icon = rect.left_center() + vec2(radius + 2.0, 0.0);
    match done_at {
        Some(at) => {
            let pop = back_out((now.duration_since(at).as_secs_f32() / 0.25).min(1.0));
            if cancelled {
                cross_mark(p, icon, 14.0 * pop, TEXT_MUTED, alpha);
            } else if errors.is_empty() {
                check_mark(p, icon, 14.0 * pop, alpha);
            } else {
                cross_mark(p, icon, 14.0 * pop, RED, alpha);
            }
        }
        None => {
            let progress =
                batch.iter().map(|j| j.progress(now)).sum::<f32>() / batch.len().max(1) as f32;
            progress_ring(p, icon, 12.0, progress, alpha);
        }
    }

    let text_x = icon.x + radius;
    p.galley(
        Pos2::new(text_x, rect.center().y - title.size().y + 1.0),
        title,
        TEXT,
    );
    p.galley(Pos2::new(text_x, rect.center().y + 2.0), sub, TEXT_MUTED);
    close_button(
        p,
        card.close_button(),
        hover.is_some_and(|h| card.spot(h) == Some(CardSpot::Close)),
        alpha,
    );

    Some(match (done_at, fade_from) {
        // Animate the pop, then sit still until the fade.
        (Some(at), _) if now < at + Duration::from_millis(250) => Duration::ZERO,
        (_, Some(from)) if now < from => from - now,
        (Some(_), _) => Duration::ZERO,
        (None, _) if batch.iter().any(|j| !j.is_simulated()) => PROGRESS_TICK,
        (None, _) => Duration::ZERO,
    })
}

/// The × that cancels or puts a card away; filled in while the pointer is on it.
fn close_button(p: &Painter, c: Pos2, hovered: bool, alpha: f32) {
    if hovered {
        p.circle_filled(c, CLOSE_R, Color32::BLACK.gamma_multiply(0.08 * alpha));
    }
    let s = CLOSE_R * 0.38;
    let stroke = Stroke::new(1.6, TEXT_MUTED.gamma_multiply(alpha));
    p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke);
    p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke);
}

/// An error in the current language: whole, or after the "file name: " a merge puts first.
fn translated_error(e: &str) -> String {
    let whole = tr(e);
    if whole != e {
        return whole.to_owned();
    }
    match e.split_once(": ") {
        Some((name, rest)) => format!("{name}: {}", tr(rest)),
        None => e.to_owned(),
    }
}

fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn shorten(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", s[..end].trim_end()),
        None => s.to_owned(),
    }
}

/// Clockwise ring from twelve o'clock over a faint track.
fn progress_ring(p: &Painter, c: Pos2, r: f32, progress: f32, alpha: f32) {
    p.circle_stroke(
        c,
        r,
        Stroke::new(3.0, Color32::BLACK.gamma_multiply(0.10 * alpha)),
    );
    let sweep = progress.clamp(0.0, 1.0) * TAU;
    let steps = ((sweep * r / 3.0).ceil() as usize).max(1);
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| c + Vec2::angled(-FRAC_PI_2 + sweep * i as f32 / steps as f32) * r)
        .collect();
    if points.len() > 1 {
        p.add(Shape::line(
            points,
            Stroke::new(3.0, ACCENT.gamma_multiply(alpha)),
        ));
    }
}

fn cross_mark(p: &Painter, c: Pos2, r: f32, color: Color32, alpha: f32) {
    p.circle_filled(c, r, color.gamma_multiply(alpha));
    let s = r * 0.38;
    let stroke = Stroke::new(2.4, Color32::WHITE.gamma_multiply(alpha));
    p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke);
    p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke);
}

fn check_mark(p: &Painter, c: Pos2, r: f32, alpha: f32) {
    p.circle_filled(c, r, GREEN.gamma_multiply(alpha));
    let s = r * 0.5;
    p.add(Shape::line(
        vec![
            c + vec2(-0.75, 0.05) * s,
            c + vec2(-0.2, 0.6) * s,
            c + vec2(0.8, -0.5) * s,
        ],
        Stroke::new(2.4, Color32::WHITE.gamma_multiply(alpha)),
    ));
}

/// "2.4 MB" for one file, "3 files" for several.
fn files_info(files: &[PathBuf]) -> String {
    match files {
        [one] => match std::fs::metadata(one) {
            Ok(meta) if meta.is_file() => human_size(meta.len()),
            Ok(meta) if meta.is_dir() => tr("Folder").to_owned(),
            _ => catalog::ext_of(one).to_uppercase(),
        },
        _ => trf("{} files", &[&files.len()]),
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

fn preview(files: &[PathBuf], choice: Choice) -> String {
    let Some(first) = files.iter().find(|f| jobs::applies(f, choice)) else {
        return trf("{} · nothing to change", &[&tr(choice.label)]);
    };
    let others = files.iter().filter(|f| jobs::applies(f, choice)).count() - 1;
    let more = if others > 0 {
        format!("  +{others}")
    } else {
        String::new()
    };
    match choice.mode {
        Mode::Actions => match crate::actions::Action::from_label(choice.label) {
            Some(action) => crate::actions::preview(files, action),
            None => format!("{} {}{more}", tr(choice.label), tr(choice.hint)),
        },
        _ => format!(
            "{}  ›  {}{more}",
            catalog::display_name(first),
            jobs::outcome(first, choice)
        ),
    }
}

/// Ease-out with a little overshoot, for the check pop.
fn back_out(t: f32) -> f32 {
    let (c1, c3) = (1.70158, 2.70158);
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point on the mode ring, `degrees` clockwise from three o'clock.
    fn on_ring(center: Pos2, degrees: f32) -> Pos2 {
        center + Vec2::angled(degrees.to_radians()) * (wheel::INFO_R + wheel::HUB_R) / 2.0
    }

    fn ring_point(center: Pos2, mode: Mode) -> Pos2 {
        let degrees = match mode {
            Mode::Convert => -90.0,
            Mode::Tools => 30.0,
            Mode::Actions => 150.0,
        };
        on_ring(center, degrees)
    }

    #[test]
    fn stopping_on_a_ring_slice_switches_modes_and_passing_over_does_not() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("holiday.jpg")],
            center,
            screen,
            Mode::Convert,
            t0,
        );
        let ctx = egui::Context::default();
        let paint = |overlay: &mut Overlay, now: Instant| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                overlay.paint(ui, now);
            });
            output.textures_delta.clear();
        };
        let mode = |overlay: &Overlay| overlay.wheel.as_ref().unwrap().mode;
        let toward_actions = |r: f32| center + Vec2::angled(150f32.to_radians()) * r;

        // Passing over the Actions slice on the way out to a wedge doesn't switch,
        overlay.drag_moved(toward_actions(20.0), t0);
        overlay.drag_moved(toward_actions(48.0), t0 + ms(10));
        paint(&mut overlay, t0 + ms(20));
        overlay.drag_moved(toward_actions(110.0), t0 + ms(30));
        paint(&mut overlay, t0 + ms(500));
        assert_eq!(mode(&overlay), Mode::Convert);

        // nor on the way back in, across the wheel.
        overlay.drag_moved(toward_actions(48.0), t0 + ms(510));
        overlay.drag_moved(center, t0 + ms(520));
        paint(&mut overlay, t0 + ms(1000));
        assert_eq!(mode(&overlay), Mode::Convert);

        // Stopping on it switches, and the drag has to move again before it can pick.
        overlay.drag_moved(toward_actions(48.0), t0 + ms(1010));
        paint(&mut overlay, t0 + ms(1010) + SETTLE);
        assert_eq!(mode(&overlay), Mode::Actions);
        let w = overlay.wheel.as_ref().unwrap();
        assert!(w.choices.iter().all(|c| c.mode == Mode::Actions));
        assert!(!overlay.would_accept(t0 + ms(1200)));

        // Moving along the ring through the Tools slice doesn't switch either, until it stops.
        let mut t = t0 + ms(1300);
        for degrees in [140.0, 120.0, 100.0, 80.0, 60.0, 40.0, 20.0, 0.0, -20.0] {
            overlay.drag_moved(on_ring(center, degrees), t);
            t += ms(20);
            paint(&mut overlay, t);
        }
        assert_eq!(mode(&overlay), Mode::Actions);
        overlay.drag_moved(on_ring(center, 40.0), t);
        paint(&mut overlay, t + SETTLE);
        assert_eq!(mode(&overlay), Mode::Tools);

        // Its own slice does nothing.
        overlay.drag_moved(on_ring(center, 70.0), t + ms(400));
        let w = overlay.wheel.as_ref().unwrap();
        assert!(w.armed);
        assert_eq!(w.mode, Mode::Tools);
        assert!(w.passing.is_none());
    }

    #[test]
    fn clicks_switch_on_the_ring_and_pick_on_wedges() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let now = Instant::now();
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("missing.jpg")],
            center,
            screen,
            Mode::Convert,
            now,
        );

        // On the ring a click switches at once, without stopping first.
        overlay.drag_moved(ring_point(center, Mode::Actions), now);
        assert!(!overlay.click(now));
        assert_eq!(overlay.wheel.as_ref().unwrap().mode, Mode::Actions);
        assert!(overlay.wants_input());

        // On a wedge it picks, like a drop.
        overlay.drag_moved(center + vec2(0.0, -120.0), now);
        assert!(overlay.click(now));
        assert!(!overlay.wants_input());
    }

    #[test]
    fn transitions_click_once_each() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let t0 = Instant::now();
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("missing.jpg")],
            center,
            screen,
            Mode::Convert,
            t0,
        );
        assert!(overlay.take_click(), "opening");
        assert!(!overlay.take_click());

        overlay.drag_moved(center + vec2(0.0, -120.0), t0);
        assert!(overlay.take_click(), "onto a wedge");
        overlay.drag_moved(center + vec2(4.0, -122.0), t0);
        assert!(!overlay.take_click(), "within the same wedge");
        overlay.drag_moved(center, t0);
        assert!(!overlay.take_click(), "off the wedges");

        overlay.drag_moved(ring_point(center, Mode::Actions), t0);
        assert!(overlay.click(t0) || overlay.wheel.as_ref().unwrap().mode == Mode::Actions);
        assert!(overlay.take_click(), "switching modes");

        overlay.drag_moved(center + vec2(0.0, -120.0), t0);
        overlay.take_click();
        assert!(overlay.click(t0));
        assert!(overlay.take_click(), "picking");
    }

    #[test]
    fn modes_without_options_are_left_out() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let t0 = Instant::now();
        let mut overlay = Overlay::default();
        // Nothing converts a JSON file, and no tool takes one.
        overlay.open(
            vec![PathBuf::from("trace.json")],
            center,
            screen,
            Mode::Convert,
            t0,
        );
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.modes, [Mode::Actions]);
        assert_eq!(w.mode, Mode::Actions);
        assert!(!w.choices.is_empty());

        // With one mode there is no ring: the band belongs to the centre.
        overlay.drag_moved(ring_point(center, Mode::Convert), t0);
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.aim(t0), Some(Hit::Hub));
        assert!(w.passing.is_none());
    }

    #[test]
    fn dropping_on_a_mode_slice_picks_nothing() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let now = Instant::now();
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("holiday.jpg")],
            center,
            screen,
            Mode::Convert,
            now,
        );
        overlay.drag_moved(ring_point(center, Mode::Tools), now);
        assert!(!overlay.would_accept(now));
        assert!(!overlay.drop(now));
    }

    #[test]
    fn shifting_keeps_the_wheel_and_cards_where_they_were_on_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let now = Instant::now();
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("holiday.jpg")],
            center,
            screen,
            Mode::Convert,
            now,
        );
        overlay.drag_moved(
            center + vec2(0.0, -(wheel::RING_R0 + wheel::RING_R1) / 2.0),
            now,
        );
        assert!(overlay.drop(now));
        overlay.open(
            vec![PathBuf::from("notes.txt")],
            center,
            screen,
            Mode::Convert,
            now,
        );

        let by = vec2(-120.0, 45.0);
        overlay.shift(by);
        let wheel = overlay.wheel.as_ref().unwrap();
        assert_eq!(wheel.center, center + by);
        assert_eq!(wheel.origin, center + by);
        assert_eq!(wheel.pointer, Some(center + by));
        assert_eq!(overlay.cards.len(), 1);
        assert_eq!(overlay.cards[0].at, center + by);
    }

    #[test]
    fn being_told_the_same_place_again_still_lets_a_slice_switch() {
        // macOS reads the pointer on every frame rather than on each move.
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("holiday.jpg")],
            center,
            screen,
            Mode::Convert,
            t0,
        );
        let ctx = egui::Context::default();
        let paint = |overlay: &mut Overlay, now: Instant| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                overlay.paint(ui, now);
            });
            output.textures_delta.clear();
        };
        let actions = ring_point(center, Mode::Actions);
        overlay.drag_moved(actions, t0);
        for k in 1..=10 {
            let now = t0 + ms(k * 30);
            overlay.drag_moved(actions, now);
            paint(&mut overlay, now);
        }
        assert_eq!(overlay.wheel.as_ref().unwrap().mode, Mode::Actions);
    }

    #[test]
    fn a_card_can_be_moved_and_closed() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("holiday.jpg")],
            center,
            screen,
            Mode::Convert,
            t0,
        );
        overlay.drag_moved(
            center + vec2(0.0, -(wheel::RING_R0 + wheel::RING_R1) / 2.0),
            t0,
        );
        assert!(overlay.drop(t0));
        let ctx = egui::Context::default();
        let paint = |overlay: &mut Overlay, now: Instant| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                overlay.paint(ui, now);
            });
            output.textures_delta.clear();
        };
        let t1 = t0 + FLASH + ms(10);
        paint(&mut overlay, t1);
        let rect = overlay.card_rects()[0];

        // Held by its middle, it follows the pointer and stays where it is let go.
        let grip = rect.left_center() + vec2(40.0, 0.0);
        assert_eq!(overlay.card_spot(grip), Some(CardSpot::Body));
        assert!(overlay.card_press(grip));
        assert!(overlay.holding_card());
        let by = vec2(-200.0, 150.0);
        overlay.card_hover(Some(grip + by));
        overlay.card_release(grip + by, t1);
        assert!(!overlay.holding_card());
        paint(&mut overlay, t1);
        assert!((overlay.card_rects()[0].center() - (rect.center() + by)).length() < 0.5);
        assert!(!overlay.card_press(grip), "nothing is left where it was");

        // Its close button: let go elsewhere, nothing; let go on it, it closes.
        let close = overlay.cards[0].close_button();
        assert_eq!(overlay.card_spot(close), Some(CardSpot::Close));
        overlay.card_press(close);
        overlay.card_release(close + vec2(0.0, 80.0), t1);
        assert!(overlay.cards[0].closed_at.is_none());
        assert!(!overlay.jobs.iter().any(|j| j.is_cancelled()));
        overlay.card_press(close);
        overlay.card_release(close, t1);
        assert!(
            overlay.cards[0].closed_at.is_some() || overlay.jobs.iter().all(|j| j.is_cancelled())
        );

        // Then it goes, once the pointer has left it.
        overlay.card_hover(None);
        paint(&mut overlay, t1 + ms(10_000));
        assert!(overlay.card_rects().is_empty());
    }

    #[test]
    fn sizes_read_like_finder() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2_400_000), "2.4 MB");
        assert_eq!(human_size(48_000_000), "48 MB");
    }
}
