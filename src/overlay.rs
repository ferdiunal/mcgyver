//! What the fullscreen overlay shows, independent of the platform that feeds it events:
//! the wheel while a drag is over it, then a progress card at the bottom right of the screen,
//! newest at the bottom, as notifications are. Opened without files (the trigger key held on
//! its own), the wheel is the desk (see `desk`): its wedges do things at once, a click while
//! the key is held or letting the key go. The first time the app starts, the wheel opens on
//! its own in the middle of the screen, slowly, to show where it is (see `welcome`).

use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Ui, Vec2, vec2};

use crate::actions::Action;
use crate::catalog::{self, Choice, Mode};
use crate::desk::{self, Command, Menu};
use crate::glass::{self, ACCENT, GREEN, GlassShape, RED, palette};
use crate::i18n::{tr, trf};
use crate::jobs::{self, Job};
use crate::motion::{self, Spring};
use crate::textures::Textures;
use crate::wheel::{self, Hit};

const OPEN_TIME: f32 = 0.16;
/// The wedges unfold one after another, clockwise from the top, each this much after the
/// last, all of them within `UNFOLD_SPREAD`.
const UNFOLD_STAGGER: f32 = 0.03;
const UNFOLD_SPREAD: f32 = 0.18;
/// How long the hub's text takes to slide over to the new one.
const HUB_CHANGE: f32 = 0.2;
/// How long the picked wedge stays lit before the wheel folds into the card.
const FLASH: Duration = Duration::from_millis(140);
/// How far a drag has to move after the wheel opens before it can pick anything.
const ARM_DISTANCE: f32 = 12.0;
/// A drag switches modes by stopping on another mode's slice for this long, too short to notice;
/// moving over a slice on the way somewhere else never switches.
const SETTLE: Duration = Duration::from_millis(50);
/// How long the droplet takes to come round the hub before the disc grows out of it, and the
/// wedges unfold this long after that.
const DROPLET: f32 = 0.14;
const UNFOLD_AFTER: f32 = 0.05;
/// The welcome: the droplet slower, and how long the wheel stays before it goes.
const WELCOME_DROPLET: f32 = 0.7;
const WELCOME_TIME: Duration = Duration::from_millis(5200);
/// How far a click flicks the hub toward it, at most (see `motion::NUDGE`).
const NUDGE_REACH: f32 = 14.0;
const CARD_APPEAR: f32 = 0.18;
const CARD_HOLD: Duration = Duration::from_millis(1600);
/// A failure stays up longer, so the reason can be read.
const CARD_HOLD_FAILED: Duration = Duration::from_millis(4500);
const CARD_FADE: f32 = 0.3;
/// How often a card redraws while real work is running.
const PROGRESS_TICK: Duration = Duration::from_millis(50);
/// A card's height, and the room between two stacked.
const CARD_H: f32 = 52.0;
const CARD_GAP: f32 = 10.0;
/// Where the newest card sits, from the bottom right of the screen (clear of a panel).
const TOAST_RIGHT: f32 = 20.0;
const TOAST_BOTTOM: f32 = 64.0;
/// How far a card slides in from the right as it appears.
const TOAST_SLIDE: f32 = 48.0;
/// The close button at a card's right end: drawn radius, and how near counts as on it.
const CLOSE_R: f32 = 10.0;
const CLOSE_HIT: f32 = 14.0;
/// A finished card the pointer was on stays this long after it leaves, before it fades.
const KEEP_AFTER_HOVER: Duration = Duration::from_millis(800);
/// Keeps the whole disc on screen when the drag is near an edge, a swelling lobe too.
const EDGE_MARGIN: f32 = wheel::DISC_R + wheel::FRAME_SWELL * 1.25 + 4.0;
/// The desk's wedges pop bubbles out past their lobes: those stay on screen too.
const DESK_MARGIN: f32 = wheel::BUBBLE_AT + wheel::BUBBLE_R * 1.1 + 4.0;
/// A wedge's bubbles pop out one after another, this long apart, once it starts to swell.
const BUBBLE_DELAY: f32 = 0.04;
const BUBBLE_STAGGER: f32 = 0.035;

struct Wheel {
    center: Pos2,
    files: Vec<PathBuf>,
    summary: String,
    /// Size or count of the files, for the hub.
    info: String,
    /// Modes with something to offer; the ring shows only these.
    modes: Vec<Mode>,
    mode: Mode,
    /// The mode's choices for the files (none on the desk).
    choices: Vec<Choice>,
    /// Opened without files: what the system has, and what the mode's wedges do.
    desk: Option<Desk>,
    /// Per wedge, its label as shown,
    labels: Vec<String>,
    /// its caption, computed once: building it stats the files,
    previews: Vec<String>,
    /// an app's icon (as `icons::app` takes it), a switch's state,
    icons: Vec<Option<String>>,
    switches: Vec<Option<bool>>,
    /// its options as bubbles,
    bubbles: Vec<Vec<Pop>>,
    /// and on a wheel of files, what those do (the desk's do what its items say).
    file_subs: Vec<Vec<Choice>>,
    pointer: Option<Pos2>,
    origin: Pos2,
    /// The drag has moved since the wheel opened. Until then nothing can be picked: if the
    /// wheel had to shift to fit on screen, the pointer may sit on a wedge it never aimed at.
    armed: bool,
    /// Another mode's slice under the drag, and when the drag last moved.
    passing: Option<(Mode, Instant)>,
    /// When the wedges began to unfold: as the wheel opened, or the mode switched.
    unfolded_at: Instant,
    opened_at: Instant,
    /// How far each wedge swells toward the pointer,
    swell: Vec<Spring>,
    /// and when it last began to, for its bubbles to pop out after.
    lit_at: Vec<Instant>,
    hub: HubText,
    chosen: Option<usize>,
    /// What a click on the desk ran, lit for a moment.
    pressed: Option<(Hit, Instant)>,
    /// A click flicked the hub: which way, and when.
    nudged: Option<(Vec2, Instant)>,
    /// How long the droplet comes round the hub before the disc grows out of it.
    intro: f32,
    /// The welcome (see `Overlay::welcome`): what the caption says; it takes no input.
    welcome: Option<String>,
    closing_at: Option<Instant>,
}

/// One option popping out of a wedge, as shown.
struct Pop {
    label: String,
    caption: String,
    icon: Option<String>,
    gauge: Option<f32>,
    /// A speaker instead of the label (see `wheel::Bubble`).
    speaker: Option<bool>,
    on: bool,
}

struct Desk {
    menu: Menu,
    items: Vec<desk::Item>,
    /// A click ran something during this hold: letting go of the key then only closes.
    used: bool,
}

/// A spring per wedge, none of them swelling.
fn calm(wedges: usize, now: Instant) -> Vec<Spring> {
    vec![Spring::new(motion::SWELL, 0.0, now); wedges]
}

/// What the hub says (see `wheel::Hub`), the text it replaced and how visible that was when it
/// began to go, and since when.
struct HubText {
    text: String,
    bold: bool,
    before: Option<(String, bool, f32)>,
    since: Instant,
}

impl HubText {
    fn showing(text: String, now: Instant) -> HubText {
        HubText {
            text,
            bold: false,
            before: None,
            since: now,
        }
    }

    /// How far the change is (0 → 1).
    fn t(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.since).as_secs_f32() / HUB_CHANGE).min(1.0)
    }

    /// Changes to `text`. Changed again before the last change is over, whichever of the two
    /// texts shows more is the one that goes, from as visible as it is, so nothing flashes up.
    fn set(&mut self, text: String, bold: bool, now: Instant) {
        if text == self.text && bold == self.bold {
            return;
        }
        let Some((before, before_bold, was)) = self.before.take() else {
            self.before = Some((std::mem::replace(&mut self.text, text), self.bold, 1.0));
            self.bold = bold;
            self.since = now;
            return;
        };
        let (going, coming) = wheel::hub_fade(self.t(now));
        let (going, coming) = (was * going, coming);
        if text == before && bold == before_bold {
            // Back to what it said: it never quite left.
            self.text = text;
            self.bold = bold;
            self.before = None;
        } else if coming <= 0.0 {
            // The new text hadn't shown yet: the old one keeps going, the newest comes instead.
            self.before = Some((before, before_bold, was));
            self.text = text;
            self.bold = bold;
        } else if going >= coming {
            self.before = Some((before, before_bold, going));
            self.text = text;
            self.bold = bold;
            self.since = now;
        } else {
            let shown = std::mem::replace(&mut self.text, text);
            self.before = Some((shown, self.bold, coming));
            self.bold = bold;
            self.since = now;
        }
    }
}

impl Wheel {
    /// Fills in the wedges of the mode, and on the desk what the hub and caption say.
    fn fill(&mut self) {
        match &mut self.desk {
            None => {
                self.choices = catalog::choices(&self.files, self.mode);
                self.labels = self
                    .choices
                    .iter()
                    .map(|c| tr(c.label).to_owned())
                    .collect();
                self.previews = self
                    .choices
                    .iter()
                    .map(|c| preview(&self.files, *c))
                    .collect();
                self.icons = vec![None; self.choices.len()];
                self.switches = vec![None; self.choices.len()];
                self.file_subs = self.choices.iter().map(|c| catalog::subs(*c)).collect();
                let files = &self.files;
                self.bubbles = self
                    .file_subs
                    .iter()
                    .map(|subs| subs.iter().map(|sub| share_pop(files, *sub)).collect())
                    .collect();
            }
            Some(desk) => {
                desk.items = desk.menu.items(self.mode);
                self.labels = desk.items.iter().map(|i| i.label.clone()).collect();
                self.previews = desk.items.iter().map(|i| i.caption.clone()).collect();
                self.icons = desk.items.iter().map(|i| i.icon.clone()).collect();
                self.switches = desk.items.iter().map(|i| i.on).collect();
                self.file_subs = Vec::new();
                self.bubbles = desk
                    .items
                    .iter()
                    .map(|item| item.subs.iter().map(desk_pop).collect())
                    .collect();
                self.info = desk.menu.info(self.mode);
                self.summary = desk.menu.summary(self.mode);
            }
        }
    }

    /// Wedge `i`'s options, popping out as bubbles while it is aimed at.
    fn subs(&self, i: usize) -> &[Pop] {
        self.bubbles.get(i).map_or(&[], Vec::as_slice)
    }

    /// Whether picking wedge `i` of a wheel of files does something: Share without a phone to
    /// send to only holds its bubbles.
    fn runs(&self, i: usize) -> bool {
        self.choices.get(i).is_some_and(|c| {
            Action::from_label(c.label) != Some(Action::Share) || Action::Share.available()
        })
    }

    /// The hub flicks toward the pointer, and springs back.
    fn nudge(&mut self, now: Instant) {
        if let Some(pointer) = self.pointer {
            let way = (pointer - self.center).normalized();
            if way.is_finite() {
                self.nudged = Some((way, now));
            }
        }
    }

    fn hub_offset(&self, now: Instant) -> Vec2 {
        self.nudged.map_or(Vec2::ZERO, |(way, at)| {
            let t = now.saturating_duration_since(at).as_secs_f32();
            way * NUDGE_REACH * motion::NUDGE.kick(t)
        })
    }

    /// The wedge swollen out, if any.
    fn swollen(&self) -> Option<usize> {
        self.swell.iter().position(|s| s.target() == 1.0)
    }

    /// Returns whether the mode changed.
    fn set_mode(&mut self, mode: Mode, now: Instant) -> bool {
        self.passing = None;
        if mode == self.mode || !self.modes.contains(&mode) {
            return false;
        }
        self.mode = mode;
        self.fill();
        self.swell = calm(self.labels.len(), now);
        self.lit_at = vec![now; self.labels.len()];
        self.unfolded_at = now;
        self.origin = self.pointer.unwrap_or(self.origin);
        self.armed = false;
        self.follow(now);
        true
    }

    /// Notes another mode's slice under the drag; the wheel switches once the drag stops there.
    fn update_ring(&mut self, now: Instant) {
        self.passing = match self.aim() {
            Some(Hit::Mode(m)) if m != self.mode && self.modes.contains(&m) => Some((m, now)),
            _ => None,
        };
    }

    /// What the drag points at; the wedge swollen out keeps it out to the end of its lobe, and
    /// on to its bubbles.
    fn aim(&self) -> Option<Hit> {
        let pos = self.pointer.filter(|_| self.armed)?;
        let swollen = self.swollen();
        let bubbles = swollen.map_or(0, |i| self.subs(i).len());
        Some(wheel::hit_swollen(
            self.center,
            pos,
            &self.modes,
            self.labels.len(),
            swollen,
            bubbles,
        ))
    }

    /// The wedge picked, or else the one aimed at (its bubbles too).
    fn named(&self) -> Option<usize> {
        let aimed = match self.chosen.is_none().then(|| self.aim()).flatten() {
            Some(Hit::Choice(i)) => Some(i),
            Some(Hit::Sub(_)) => self.swollen(),
            _ => None,
        };
        self.chosen.or(aimed).filter(|&i| i < self.labels.len())
    }

    /// What a drop would run on a wheel of files, the wedge's index and the card's title: the
    /// wedge aimed at (if it runs anything) or one of its bubbles.
    fn pick(&self) -> Option<(usize, Choice, String)> {
        match self.aim()? {
            Hit::Choice(i) if self.runs(i) => {
                Some((i, *self.choices.get(i)?, self.previews.get(i)?.clone()))
            }
            Hit::Sub(k) => {
                let i = self.swollen()?;
                let sub = *self.file_subs.get(i)?.get(k)?;
                Some((i, sub, preview(&self.files, sub)))
            }
            _ => None,
        }
    }

    /// The bubble aimed at, if any.
    fn aimed_sub(&self) -> Option<usize> {
        match self.chosen.is_none().then(|| self.aim()).flatten() {
            Some(Hit::Sub(k)) => Some(k),
            _ => None,
        }
    }

    /// Swells the wedge aimed at (or picked) and lets the others settle back, and has the hub
    /// name it.
    fn follow(&mut self, now: Instant) {
        let named = self.named();
        for (i, swell) in self.swell.iter_mut().enumerate() {
            let to = if named == Some(i) { 1.0 } else { 0.0 };
            if to == 1.0 && swell.target() != 1.0 {
                self.lit_at[i] = now;
            }
            swell.set(to, now);
        }
        let (text, bold) = match named {
            Some(i) => (self.labels[i].clone(), true),
            None => (self.info.clone(), false),
        };
        self.hub.set(text, bold, now);
    }

    /// How far each of wedge `i`'s bubbles has popped out: one after another once it began to
    /// swell, all going back as it settles.
    fn pops(&self, i: usize, now: Instant) -> Vec<f32> {
        let t = now.saturating_duration_since(self.lit_at[i]).as_secs_f32();
        let swell = self.swell[i].value(now).clamp(0.0, 1.0);
        let shown = if self.swell[i].target() == 1.0 {
            1.0
        } else {
            swell
        };
        (0..self.subs(i).len())
            .map(|k| motion::POP.step(t - BUBBLE_DELAY - k as f32 * BUBBLE_STAGGER) * shown)
            .collect()
    }

    /// How far each wedge has unfolded.
    fn unfold(&self, now: Instant) -> Vec<f32> {
        let t = now
            .saturating_duration_since(self.unfolded_at)
            .as_secs_f32();
        let n = self.labels.len();
        let stagger = UNFOLD_STAGGER.min(UNFOLD_SPREAD / n.max(1) as f32);
        (0..n)
            .map(|i| motion::POP.step(t - i as f32 * stagger))
            .collect()
    }

    /// The disc's size: as small as the hub while the droplet comes round, then growing out of
    /// it a little past full; shrinking as it closes (from wherever the pop was).
    fn scale(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.opened_at).as_secs_f32() - self.intro;
        let from = wheel::HUB_R / wheel::DISC_R;
        let pop = from + (1.0 - from) * motion::POP.step(t);
        match self.closing_at {
            Some(_) => pop * wheel::scale(self.open_t(now)),
            None => pop,
        }
    }

    /// The hub's size: popping open as the wheel does, before the disc.
    fn hub_scale(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.opened_at).as_secs_f32();
        let pop = 0.55 + 0.45 * motion::POP.step(t);
        match self.closing_at {
            Some(_) => pop * wheel::scale(self.open_t(now)),
            None => pop,
        }
    }

    /// How far the droplet has come round (0 → 1), while it does.
    fn droplet(&self, now: Instant) -> Option<f32> {
        let t = now.saturating_duration_since(self.opened_at).as_secs_f32() / self.intro;
        (self.closing_at.is_none() && t < 1.0).then_some(t)
    }

    /// Whether anything on it still moves.
    fn moving(&self, now: Instant) -> bool {
        let since = |t: Instant| now.saturating_duration_since(t).as_secs_f32();
        self.open_t(now) < 1.0
            || self.closing_at.is_some()
            || since(self.opened_at) < self.intro + motion::POP.settle()
            || self
                .nudged
                .is_some_and(|(_, at)| since(at) < motion::NUDGE.settle())
            || since(self.unfolded_at) < UNFOLD_SPREAD + motion::POP.settle()
            || (self.hub.before.is_some() && self.hub.t(now) < 1.0)
            || self.swell.iter().any(|s| s.moving(now))
            || self.lit_at.iter().enumerate().any(|(i, &at)| {
                let subs = self.subs(i).len() as f32;
                since(at) < BUBBLE_DELAY + subs * BUBBLE_STAGGER + motion::POP.settle()
            })
            || self.pressed.is_some_and(|(_, at)| now < at + FLASH)
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
    /// Its place in the stack: 0 at the bottom (the newest), 1 above it…
    row: Spring,
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
    /// The card itself.
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
    /// Glass painted in the last frame, for the platform's backdrop blur.
    glass: Vec<GlassShape>,
    /// Something changed that deserves a click: the wheel opened, switched modes, moved onto
    /// another wedge or picked one.
    clicked: bool,
    /// The pointer over the overlay while no wheel takes it, for the cards.
    hover: Option<Pos2>,
    /// The card whose close button is pressed, until the button comes up.
    closing: Option<u32>,
    /// What the desk was asked to do, for the platform to run (see `take_commands`).
    commands: Vec<Command>,
    /// Where the newest card's bottom right corner goes, if not the screen's (see `set_corner`).
    corner: Option<Pos2>,
    /// Apps' icons for the wedges and bubbles.
    textures: Textures,
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
        let info = files_info(&files);
        let summary = catalog::describe(&files);
        self.show(
            center, files, None, summary, info, modes, mode, pointer, now,
        );
    }

    /// The trigger key was held on its own: opens the desk around `pointer`, on the apps.
    pub fn open_desk(&mut self, menu: Menu, pointer: Pos2, screen: Rect, now: Instant) {
        let fit = screen.shrink(DESK_MARGIN);
        let center = if fit.is_positive() {
            pointer.clamp(fit.min, fit.max)
        } else {
            screen.center()
        };
        let modes = menu.modes();
        let desk = Desk {
            menu,
            items: Vec::new(),
            used: false,
        };
        let mode = modes[0];
        self.show(
            center,
            Vec::new(),
            Some(desk),
            String::new(),
            String::new(),
            modes,
            mode,
            pointer,
            now,
        );
    }

    /// The first time the app starts: the desk opens in the middle of `screen` on its own,
    /// slowly, says `hint` (how to open it) under it, takes no input, and goes after a while.
    pub fn welcome(&mut self, menu: Menu, screen: Rect, hint: String, now: Instant) {
        self.open_desk(menu, screen.center(), screen, now);
        if let Some(w) = &mut self.wheel {
            w.intro = WELCOME_DROPLET;
            w.unfolded_at = now + Duration::from_secs_f32(WELCOME_DROPLET + UNFOLD_AFTER);
            w.pointer = None;
            w.welcome = Some(hint);
        }
        self.clicked = false;
    }

    /// Where the newest card's bottom right corner goes, when the screen's isn't in the
    /// window (or a panel covers it).
    pub fn set_corner(&mut self, corner: Pos2) {
        self.corner = Some(corner);
    }

    /// What the desk open now shows, to read again from (see `Menu::read_live`).
    pub fn desk_menu(&self) -> Option<Menu> {
        self.wheel
            .as_ref()
            .filter(|_| self.desk_open())
            .and_then(|w| w.desk.as_ref())
            .map(|d| d.menu.clone())
    }

    /// Whether the desktop is dark, as the desk last read it or a click on its Dark Mode just set
    /// it (while it is up, open or closing).
    pub fn desk_dark(&self) -> Option<bool> {
        self.wheel.as_ref()?.desk.as_ref()?.menu.dark
    }

    /// The mode the desk is open on, if it is.
    pub fn desk_mode(&self) -> Option<Mode> {
        self.wheel
            .as_ref()
            .filter(|_| self.desk_open())
            .map(|w| w.mode)
    }

    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        center: Pos2,
        files: Vec<PathBuf>,
        desk: Option<Desk>,
        summary: String,
        info: String,
        modes: Vec<Mode>,
        mode: Mode,
        pointer: Pos2,
        now: Instant,
    ) {
        // The wheel takes the pointer now; a card's close button pressed is let go.
        self.hover = None;
        self.closing = None;
        let mut wheel = Wheel {
            center,
            summary,
            swell: Vec::new(),
            lit_at: Vec::new(),
            hub: HubText::showing(String::new(), now),
            info,
            modes,
            labels: Vec::new(),
            previews: Vec::new(),
            icons: Vec::new(),
            switches: Vec::new(),
            bubbles: Vec::new(),
            file_subs: Vec::new(),
            files,
            desk,
            mode,
            choices: Vec::new(),
            pointer: Some(pointer),
            origin: pointer,
            armed: false,
            passing: None,
            unfolded_at: now + Duration::from_secs_f32(DROPLET + UNFOLD_AFTER),
            opened_at: now,
            chosen: None,
            pressed: None,
            nudged: None,
            intro: DROPLET,
            welcome: None,
            closing_at: None,
        };
        wheel.fill();
        wheel.swell = calm(wheel.labels.len(), now);
        wheel.lit_at = vec![now; wheel.labels.len()];
        wheel.hub = HubText::showing(wheel.info.clone(), now);
        self.wheel = Some(wheel);
        self.clicked = true;
    }

    /// Whether the desk is open and taking clicks.
    pub fn desk_open(&self) -> bool {
        self.wheel.as_ref().is_some_and(|w| {
            w.desk.is_some() && w.welcome.is_none() && w.chosen.is_none() && w.closing_at.is_none()
        })
    }

    /// What the system has now, read again after a command: the wedges and bubbles follow,
    /// and the player's mode goes when nothing plays any more.
    pub fn refresh_desk(&mut self, menu: Menu, now: Instant) {
        let Some(w) = self.active_wheel() else {
            return;
        };
        let Some(desk) = &mut w.desk else {
            return;
        };
        if desk.menu == menu {
            return;
        }
        desk.menu = menu;
        w.modes = desk.menu.modes();
        let wedges = w.labels.len();
        if !w.modes.contains(&w.mode) {
            w.mode = w.modes[0];
            w.unfolded_at = now;
        }
        w.fill();
        if w.labels.len() != wedges {
            w.swell = calm(w.labels.len(), now);
            w.lit_at = vec![now; w.labels.len()];
        }
        w.follow(now);
    }

    /// What the desk was asked to do since the last call, for the platform to run (then to
    /// read the system again and `refresh_desk`).
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    /// The trigger key came up while the desk was open: does what it aims at, unless a click
    /// already did something while the key was held, and closes. Returns whether it did
    /// something.
    pub fn release(&mut self, now: Instant) -> bool {
        if !self.desk_open() {
            return false;
        }
        let used = self
            .wheel
            .as_ref()
            .and_then(|w| w.desk.as_ref())
            .is_some_and(|d| d.used);
        if !used && self.press(now, true) {
            return true;
        }
        self.cancel(now);
        false
    }

    /// Does what the desk aims at: a wedge or one of its bubbles. The wheel stays for more,
    /// unless the key came up (`last`) or it opens something.
    fn press(&mut self, now: Instant, last: bool) -> bool {
        let Some(w) = self.active_wheel() else {
            return false;
        };
        let Some(desk) = &w.desk else {
            return false;
        };
        let aim = w.aim();
        let command = match aim {
            Some(Hit::Choice(i)) => desk.items.get(i).and_then(|item| item.command.clone()),
            Some(Hit::Sub(k)) => w
                .swollen()
                .and_then(|i| desk.items.get(i))
                .and_then(|item| item.subs.get(k))
                .map(|sub| sub.command.clone()),
            _ => None,
        };
        let (Some(command), Some(aim)) = (command, aim) else {
            return false;
        };
        if last && command.needs_click() {
            return false;
        }
        let leaves = last || command.leaves();
        if let Some(desk) = &mut w.desk {
            desk.used = true;
            // Shown at once; the system is read again after.
            if !leaves {
                desk.menu.apply(&command);
            }
        }
        w.pressed = Some((aim, now));
        w.nudge(now);
        if leaves {
            w.chosen = w.named();
            w.closing_at = Some(now + FLASH);
        } else {
            let wedges = w.labels.len();
            w.fill();
            if w.labels.len() != wedges {
                w.swell = calm(w.labels.len(), now);
                w.lit_at = vec![now; w.labels.len()];
            }
        }
        w.follow(now);
        self.commands.push(command);
        self.clicked = true;
        true
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
        let before = w.aim();
        w.pointer = Some(pos);
        w.armed |= (pos - w.origin).length() > ARM_DISTANCE;
        w.update_ring(now);
        w.follow(now);
        let after = w.aim();
        self.clicked |= matches!(after, Some(Hit::Choice(_) | Hit::Sub(_))) && after != before;
    }

    /// Whether something clicked since the last call.
    pub fn take_click(&mut self) -> bool {
        std::mem::take(&mut self.clicked)
    }

    /// Whether a drop right now would pick something.
    pub fn would_accept(&self) -> bool {
        self.wheel
            .as_ref()
            .filter(|w| w.desk.is_none() && w.chosen.is_none() && w.closing_at.is_none())
            .is_some_and(|w| w.pick().is_some())
    }

    /// The drag was released over the overlay. Returns whether it picked something.
    pub fn drop(&mut self, now: Instant) -> bool {
        let pick = self
            .active_wheel()
            .filter(|w| w.desk.is_none())
            .and_then(|w| w.pick());
        match pick {
            Some((i, choice, title)) => {
                self.choose(i, choice, title, now);
                true
            }
            None => {
                self.cancel(now);
                false
            }
        }
    }

    /// A click on a wheel opened without a drag: picks like a drop would, and a click on the
    /// mode ring switches to that mode at once. On the desk it does what the wedge or bubble
    /// does, and the wheel stays.
    pub fn click(&mut self, now: Instant) -> bool {
        if let Some(w) = self.active_wheel()
            && let Some(Hit::Mode(mode)) = w.aim()
        {
            self.clicked |= w.set_mode(mode, now);
            return false;
        }
        if self.desk_open() {
            return self.press(now, false);
        }
        self.drop(now)
    }

    /// The drag left, was cancelled, or dropped on nothing; or the welcome is cut short.
    pub fn cancel(&mut self, now: Instant) {
        if let Some(w) = self.active_wheel() {
            w.pointer = None;
            w.armed = false;
            w.passing = None;
            w.closing_at = Some(now);
            w.follow(now);
        }
    }

    /// The overlay must take pointer input (to receive the drag) only while the wheel is live;
    /// the card that follows is click-through.
    pub fn wants_input(&self) -> bool {
        self.wheel
            .as_ref()
            .is_some_and(|w| w.chosen.is_none() && w.closing_at.is_none() && w.welcome.is_none())
    }

    /// The window moved by `-by` on screen: the wheel stays where it was. (The cards go where
    /// `set_corner` says.)
    pub fn shift(&mut self, by: Vec2) {
        if let Some(w) = &mut self.wheel {
            w.center += by;
            w.origin += by;
            if let Some(p) = &mut w.pointer {
                *p += by;
            }
        }
    }

    /// Whether there are cards and no wheel: the platform may then move the window to where
    /// the cards go.
    pub fn only_cards(&self) -> bool {
        self.wheel.is_none() && !self.cards.is_empty()
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

    /// The pointer moved to `pos` (`None`: it left).
    pub fn card_hover(&mut self, pos: Option<Pos2>) {
        self.hover = pos;
    }

    /// The button went down at `pos`: presses the close button there. Returns whether it was
    /// on a card.
    pub fn card_press(&mut self, pos: Pos2) -> bool {
        self.hover = Some(pos);
        let Some(card) = self.cards.iter().rev().find(|c| c.spot(pos).is_some()) else {
            return false;
        };
        if card.spot(pos) == Some(CardSpot::Close) {
            self.closing = Some(card.batch);
        }
        true
    }

    /// The button came up at `pos`: a close button still under the pointer closes its card.
    pub fn card_release(&mut self, pos: Pos2, now: Instant) {
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

    /// Runs `choice` (wedge `index`'s, or one of its bubbles') on the files, and puts up a card
    /// for it once the wheel has folded.
    fn choose(&mut self, index: usize, choice: Choice, title: String, now: Instant) {
        let Some(w) = self.wheel.as_mut() else {
            return;
        };
        w.chosen = Some(index);
        w.closing_at = Some(now + FLASH);
        w.nudge(now);
        w.follow(now);
        self.clicked = true;
        self.batch += 1;
        self.jobs
            .extend(jobs::start(&w.files, choice, self.batch, now + FLASH));
        let shown_at = now + FLASH + Duration::from_secs_f32(OPEN_TIME);
        self.cards.push(Card {
            row: Spring::new(motion::POP, 0.0, shown_at),
            batch: self.batch,
            title,
            shown_at,
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
        if let Some(w) = &mut self.wheel
            && w.welcome.is_some()
            && w.closing_at.is_none()
        {
            let until = w.opened_at + WELCOME_TIME;
            if now >= until {
                w.closing_at = Some(now);
            } else {
                soon(until - now);
            }
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
            if w.moving(now) {
                soon(Duration::ZERO);
            }
            paint_wheel(ui, w, &mut self.textures, now, &mut self.glass);
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

        // Newest at the bottom; the others move up for it, and down as one goes.
        let shown = self.cards.iter().filter(|c| now >= c.shown_at).count();
        let mut row = shown;
        for card in &mut self.cards {
            if now >= card.shown_at {
                row -= 1;
                card.row.set(row as f32, now);
            }
        }
        let corner = self
            .corner
            .unwrap_or(ui.max_rect().max - vec2(TOAST_RIGHT, TOAST_BOTTOM));
        let (jobs, glass, hover) = (&self.jobs, &mut self.glass, self.hover);
        self.cards.retain_mut(|card| {
            if hover.is_some_and(|h| card.rect.contains(h)) {
                card.kept_at = Some(now);
            }
            match paint_card(ui, card, jobs, now, hover, corner, glass) {
                Some(wait) => {
                    soon(wait);
                    true
                }
                None => false,
            }
        });
        let cards = &self.cards;
        self.jobs
            .retain(|j| cards.iter().any(|c| c.batch == j.batch));
        next
    }
}

fn paint_wheel(
    ui: &Ui,
    w: &Wheel,
    textures: &mut Textures,
    now: Instant,
    shapes: &mut Vec<GlassShape>,
) {
    let hovered = if w.chosen.is_some() { None } else { w.aim() };
    let open_t = w.open_t(now);
    let s = w.scale(now);
    let hub_s = w.hub_scale(now);
    let mut icon = |id: &Option<String>| id.as_ref().and_then(|id| textures.get(ui.ctx(), id));
    let icons: Vec<_> = w.icons.iter().map(&mut icon).collect();
    let unfold = w.unfold(now);
    let swell: Vec<f32> = w.swell.iter().map(|spring| spring.value(now)).collect();
    let pressed = w
        .pressed
        .filter(|(_, at)| now < *at + FLASH)
        .map(|(hit, _)| hit);
    let lit = match pressed {
        Some(Hit::Choice(i)) => Some(i),
        _ => w.chosen,
    };
    let view = wheel::View {
        labels: &w.labels,
        modes: &w.modes,
        mode: w.mode,
        hovered,
        chosen: lit,
        open_t,
        scale: s,
        unfold: &unfold,
        swell: &swell,
        icons: &icons,
        on: &w.switches,
        hub_scale: hub_s,
        hub_offset: w.hub_offset(now),
        hub: wheel::Hub {
            text: &w.hub.text,
            bold: w.hub.bold,
            before: w
                .hub
                .before
                .as_ref()
                .map(|(t, b, was)| (t.as_str(), *b, *was)),
            t: w.hub.t(now),
        },
    };
    wheel::paint(ui.painter(), w.center, &view);
    let droplet = w
        .droplet(now)
        .and_then(|t| wheel::paint_droplet(ui.painter(), w.center, hub_s, t, open_t));
    if open_t > 0.02 {
        shapes.push(GlassShape::Circle {
            center: w.center,
            radius: wheel::DISC_R * s,
        });
        if let Some((center, radius)) = droplet {
            shapes.push(GlassShape::Circle { center, radius });
        }
        for (center, radius) in wheel::bulges(w.center, s, &swell) {
            shapes.push(GlassShape::Circle { center, radius });
        }
    }

    // The bubbles of a wedge swelling or settling back, over everything.
    let aimed_sub = w.aimed_sub();
    let mut lowest = None::<f32>;
    for (i, &shown) in swell.iter().enumerate() {
        let subs = w.subs(i);
        if subs.is_empty() || shown <= 0.01 && w.swell[i].target() != 1.0 {
            continue;
        }
        let pops = w.pops(i, now);
        let swollen = w.swell[i].target() == 1.0;
        let bubbles: Vec<wheel::Bubble> = subs
            .iter()
            .zip(&pops)
            .enumerate()
            .map(|(k, (sub, &pop))| wheel::Bubble {
                label: &sub.label,
                speaker: sub.speaker,
                icon: icon(&sub.icon),
                gauge: sub.gauge,
                on: sub.on,
                hovered: swollen && (aimed_sub == Some(k) || pressed == Some(Hit::Sub(k))),
                pop,
            })
            .collect();
        let drawn = wheel::paint_bubbles(
            ui.painter(),
            w.center,
            s,
            i,
            w.labels.len(),
            &bubbles,
            open_t,
        );
        for (center, radius) in drawn {
            if open_t > 0.02 {
                shapes.push(GlassShape::Circle { center, radius });
            }
            // The caption keeps clear of bubbles below the wheel.
            if (center.x - w.center.x).abs() < 130.0 * s + radius {
                let bottom = center.y + radius;
                lowest = Some(lowest.map_or(bottom, |l: f32| l.max(bottom)));
            }
        }
    }

    let sub_caption = aimed_sub
        .zip(w.named())
        .and_then(|(k, i)| w.subs(i).get(k))
        .map(|sub| &sub.caption);
    let caption = match (sub_caption, w.named(), &w.welcome) {
        (_, _, Some(hint)) => hint,
        (Some(caption), _, _) => caption,
        (None, Some(i), _) if i < w.previews.len() => &w.previews[i],
        _ => &w.summary,
    };
    let mut drop = wheel::bulge_at(FRAC_PI_2, &swell);
    if let Some(bottom) = lowest {
        let clear = (bottom + 6.0 - w.center.y) / s - wheel::DISC_R - wheel::CAPTION_GAP;
        drop = drop.max(clear);
    }
    let rise = wheel::bulge_at(-FRAC_PI_2, &swell);
    let bottom = ui.max_rect().bottom();
    // It comes with the disc, after the droplet.
    let from = wheel::HUB_R / wheel::DISC_R;
    let caption_a = open_t * ((s - from) / (1.0 - from)).clamp(0.0, 1.0);
    let rect = wheel::caption(
        ui.painter(),
        w.center,
        s,
        caption,
        caption_a,
        drop,
        rise,
        bottom,
    );
    if caption_a > 0.02 {
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
    corner: Pos2,
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
        palette().text.gamma_multiply(alpha),
    );
    let sub = p.layout_no_wrap(
        shorten(&subtitle, 64),
        FontId::proportional(11.0),
        palette().text_muted.gamma_multiply(alpha),
    );
    let text_w = title.size().x.max(sub.size().x);
    let size = vec2(56.0 + text_w + 26.0 + 2.0 * CLOSE_R + 8.0, CARD_H) * (0.85 + 0.15 * appear);
    // In from the right, and a little back that way as it fades.
    let slide = TOAST_SLIDE * ((1.0 - appear) + 0.5 * (1.0 - fade));
    let row = card.row.value(now);
    let bottom_right = corner + vec2(slide, -row * (CARD_H + CARD_GAP));
    let rect = Rect::from_min_max(bottom_right - size, bottom_right);
    card.rect = rect;
    let radius = size.y / 2.0;
    glass::capsule(p, rect, alpha);
    shapes.push(GlassShape::Capsule(rect));

    let icon = rect.left_center() + vec2(radius + 2.0, 0.0);
    match done_at {
        Some(at) => {
            let pop = back_out((now.duration_since(at).as_secs_f32() / 0.25).min(1.0));
            if cancelled {
                cross_mark(p, icon, 14.0 * pop, palette().text_muted, alpha);
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
        palette().text,
    );
    p.galley(
        Pos2::new(text_x, rect.center().y + 2.0),
        sub,
        palette().text_muted,
    );
    close_button(
        p,
        card.close_button(),
        hover.is_some_and(|h| card.spot(h) == Some(CardSpot::Close)),
        alpha,
    );

    Some(match (done_at, fade_from) {
        _ if card.row.moving(now) => Duration::ZERO,
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
        p.circle_filled(c, CLOSE_R, palette().edge.gamma_multiply(0.08 * alpha));
    }
    let s = CLOSE_R * 0.38;
    let stroke = Stroke::new(1.6, palette().text_muted.gamma_multiply(alpha));
    p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke);
    p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke);
}

/// A bubble out of Share: the phone, or a messaging app with its icon.
fn share_pop(files: &[PathBuf], sub: Choice) -> Pop {
    let action = Action::from_label(sub.label);
    let app = match action {
        Some(Action::Send(messenger)) => messenger.app(),
        _ => None,
    };
    Pop {
        label: match action {
            Some(Action::Share) => tr("Phone").to_owned(),
            _ => sub.label.to_owned(),
        },
        caption: preview(files, sub),
        icon: app.map(|a| a.id),
        gauge: None,
        speaker: None,
        on: false,
    }
}

/// A bubble of the desk's, as its item says.
fn desk_pop(sub: &desk::Sub) -> Pop {
    Pop {
        label: sub.label.clone(),
        caption: sub.caption.clone(),
        icon: sub.icon.clone(),
        gauge: sub.gauge,
        speaker: match sub.command {
            Command::Mute(muted) => Some(muted),
            _ => None,
        },
        on: sub.on,
    }
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
        Stroke::new(3.0, palette().edge.gamma_multiply(0.10 * alpha)),
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
    use egui::pos2;

    /// A point on the mode ring, `degrees` clockwise from three o'clock.
    fn on_ring(center: Pos2, degrees: f32) -> Pos2 {
        center + Vec2::angled(degrees.to_radians()) * (wheel::INFO_R + wheel::HUB_R) / 2.0
    }

    fn ring_point(center: Pos2, mode: Mode) -> Pos2 {
        let degrees = match mode {
            Mode::Convert | Mode::Media => -90.0,
            Mode::Tools | Mode::Everyday => 30.0,
            Mode::Actions | Mode::Apps | Mode::System => 150.0,
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
        assert!(!overlay.would_accept());

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
        assert_eq!(w.aim(), Some(Hit::Hub));
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
        assert!(!overlay.would_accept());
        assert!(!overlay.drop(now));
    }

    #[test]
    fn shifting_keeps_the_wheel_where_it_was_on_screen() {
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
    fn cards_stack_at_the_corner_newest_lowest_and_close() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut overlay = Overlay::default();
        let corner = pos2(1580.0, 936.0);
        overlay.set_corner(corner);
        for k in 0..2 {
            let now = t0 + ms(20 * k);
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
        }
        let ctx = egui::Context::default();
        let paint = |overlay: &mut Overlay, now: Instant| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                overlay.paint(ui, now);
            });
            output.textures_delta.clear();
        };
        // None while the wheel folds; then in from the right, the older one moving up.
        paint(&mut overlay, t0 + FLASH);
        assert!(overlay.card_rects().is_empty());
        let t1 = t0 + ms(20) + FLASH + Duration::from_secs_f32(OPEN_TIME);
        paint(&mut overlay, t1);
        let t1 = t1 + ms(800);
        paint(&mut overlay, t1);
        let rects = overlay.card_rects();
        assert_eq!(rects.len(), 2);
        assert!((rects[1].max - corner).length() < 0.5, "{:?}", rects[1]);
        assert!((rects[0].max.x - corner.x).abs() < 0.5);
        assert!(rects[0].max.y < rects[1].min.y);

        // Holding one doesn't move it.
        let newest = rects[1];
        let grip = newest.left_center() + vec2(40.0, 0.0);
        assert_eq!(overlay.card_spot(grip), Some(CardSpot::Body));
        assert!(overlay.card_press(grip));
        overlay.card_hover(Some(grip + vec2(-200.0, -150.0)));
        overlay.card_release(grip + vec2(-200.0, -150.0), t1);
        paint(&mut overlay, t1);
        assert_eq!(overlay.card_rects()[1], newest);

        // Its close button: let go elsewhere, nothing; let go on it, it closes.
        let close = overlay.cards[1].close_button();
        assert_eq!(overlay.card_spot(close), Some(CardSpot::Close));
        overlay.card_press(close);
        overlay.card_release(close + vec2(0.0, -80.0), t1);
        assert!(overlay.cards[1].closed_at.is_none());
        overlay.card_press(close);
        overlay.card_release(close, t1);
        assert!(
            overlay.cards[1].closed_at.is_some()
                || overlay
                    .jobs
                    .iter()
                    .filter(|j| j.batch == overlay.cards[1].batch)
                    .all(|j| j.is_cancelled())
        );

        // Then they go, once the pointer has left.
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

    /// A wheel open on a picture at the middle of a big screen, armed and aiming at nothing.
    fn wheel_on_a_picture(t0: Instant) -> (Overlay, Pos2) {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let mut overlay = Overlay::default();
        overlay.open(
            vec![PathBuf::from("holiday.jpg")],
            center,
            screen,
            Mode::Convert,
            t0,
        );
        (overlay, center)
    }

    fn wedge(center: Pos2, i: usize, n: usize) -> Pos2 {
        center + Vec2::angled(-FRAC_PI_2 + i as f32 * TAU / n as f32) * 118.0
    }

    #[test]
    fn the_hub_keeps_the_name_of_the_wedge_picked() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (mut overlay, center) = wheel_on_a_picture(t0);
        let n = overlay.wheel.as_ref().unwrap().choices.len();
        overlay.drag_moved(wedge(center, 0, n), t0 + ms(300));
        let label = tr(overlay.wheel.as_ref().unwrap().choices[0].label).to_owned();
        assert_eq!(overlay.wheel.as_ref().unwrap().hub.text, label);
        assert!(overlay.drop(t0 + ms(600)));
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.hub.text, label);
        assert_eq!(w.swell[0].target(), 1.0);
    }

    #[test]
    fn a_wedge_crossed_in_passing_doesnt_flash_its_name() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (mut overlay, center) = wheel_on_a_picture(t0);
        let n = overlay.wheel.as_ref().unwrap().choices.len();
        overlay.drag_moved(wedge(center, 0, n), t0 + ms(300));
        // Settled on wedge 0, then across wedge 1 to wedge 2 within a frame or two.
        overlay.drag_moved(wedge(center, 1, n), t0 + ms(1000));
        overlay.drag_moved(wedge(center, 2, n), t0 + ms(1016));
        let w = overlay.wheel.as_ref().unwrap();
        let first = tr(w.choices[0].label);
        // Wedge 0's name is the one going, from fully shown; wedge 1's never showed.
        assert_eq!(w.hub.text, tr(w.choices[2].label));
        let (going, _, was) = w.hub.before.as_ref().unwrap();
        assert_eq!((going.as_str(), *was), (first, 1.0));
    }

    fn playing() -> desk::Player {
        desk::Player {
            id: "org.mpris.MediaPlayer2.test".into(),
            name: "Test".into(),
            title: "Song".into(),
            artist: "Band".into(),
            playing: true,
            can_next: true,
            can_previous: true,
            shuffle: None,
            repeat: None,
        }
    }

    fn levels() -> Menu {
        Menu {
            volume: Some(desk::Volume {
                level: 0.3,
                muted: false,
            }),
            brightness: Some(0.6),
            ..Menu::default()
        }
    }

    /// The desk open at the middle of a big screen.
    fn desk_with(menu: Menu, t0: Instant) -> (Overlay, Pos2) {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let center = screen.center();
        let mut overlay = Overlay::default();
        overlay.open_desk(menu, center, screen, t0);
        (overlay, center)
    }

    /// Switches the desk to `mode` with a click on its slice of the ring.
    fn switch_to(overlay: &mut Overlay, center: Pos2, mode: Mode, now: Instant) {
        let modes = overlay.wheel.as_ref().unwrap().modes.clone();
        let i = modes.iter().position(|&m| m == mode).unwrap();
        overlay.drag_moved(
            on_ring(center, -90.0 + 360.0 * i as f32 / modes.len() as f32),
            now,
        );
        assert!(!overlay.click(now));
        assert_eq!(overlay.wheel.as_ref().unwrap().mode, mode);
    }

    #[test]
    fn a_click_on_the_desk_does_it_and_the_wheel_stays() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (mut overlay, center) = desk_with(
            Menu {
                player: Some(playing()),
                ..levels()
            },
            t0,
        );
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.mode, Mode::Apps);
        assert_eq!(
            w.modes,
            [Mode::Apps, Mode::Media, Mode::Everyday, Mode::System]
        );
        switch_to(&mut overlay, center, Mode::Media, t0);
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.hub.text, "Playing");
        assert_eq!(w.summary, "Song — Band");

        let n = w.labels.len();
        overlay.drag_moved(wedge(center, 0, n), t0);
        assert!(overlay.click(t0));
        let play_pause = desk::Command::Media {
            player: "org.mpris.MediaPlayer2.test".into(),
            key: desk::MediaKey::PlayPause,
        };
        assert_eq!(overlay.take_commands(), [play_pause]);
        assert!(overlay.desk_open());

        // The hub flicks toward the click, and springs back.
        let w = overlay.wheel.as_ref().unwrap();
        let flicked = w.hub_offset(t0 + ms(40));
        assert!(
            flicked.dot(wedge(center, 0, n) - center) > 0.0,
            "{flicked:?}"
        );
        assert!(flicked.length() > 2.0);
        assert!(w.hub_offset(t0 + ms(1500)).length() < 0.5);

        // Letting go of the key after a click only closes.
        assert!(!overlay.release(t0));
        assert!(overlay.take_commands().is_empty());
        assert!(!overlay.desk_open());
    }

    #[test]
    fn letting_go_of_the_key_does_what_is_aimed_at() {
        let t0 = Instant::now();
        let (mut overlay, center) = desk_with(
            Menu {
                player: Some(playing()),
                ..Menu::default()
            },
            t0,
        );
        switch_to(&mut overlay, center, Mode::Media, t0);
        let n = overlay.wheel.as_ref().unwrap().labels.len();
        overlay.drag_moved(wedge(center, 1, n), t0);
        assert_eq!(overlay.wheel.as_ref().unwrap().hub.text, "Next");
        assert!(overlay.release(t0));
        let next = desk::Command::Media {
            player: "org.mpris.MediaPlayer2.test".into(),
            key: desk::MediaKey::Next,
        };
        assert_eq!(overlay.take_commands(), [next]);
        assert!(!overlay.desk_open());

        // Let go on nothing, it just closes.
        let (mut overlay, _) = desk_with(levels(), t0);
        assert!(!overlay.release(t0));
        assert!(overlay.take_commands().is_empty());
        assert!(!overlay.wants_input());
    }

    #[test]
    fn a_wedges_options_pop_out_and_take_clicks() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (mut overlay, center) = desk_with(levels(), t0);
        switch_to(&mut overlay, center, Mode::Everyday, t0);
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.labels, ["Volume", "Brightness"]);

        overlay.drag_moved(wedge(center, 0, 2), t0);
        let spots = wheel::bubble_spots(center, 0, 2, 5);
        let w = overlay.wheel.as_ref().unwrap();
        // They pop out one after another, and are all out soon after.
        let pops = w.pops(0, t0 + ms(60));
        assert!(pops[0] > pops[4], "{pops:?}");
        assert!(
            w.pops(0, t0 + ms(800))
                .iter()
                .all(|p| (p - 1.0).abs() < 0.02)
        );

        overlay.drag_moved(spots[2], t0 + ms(400));
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.aimed_sub(), Some(2));
        assert_eq!(w.hub.text, "Volume", "the wedge stays named");
        assert_eq!(w.swollen(), Some(0));
        assert!(overlay.click(t0 + ms(400)));
        assert_eq!(overlay.take_commands(), [desk::Command::Volume(0.5)]);

        // Across to the next bubble, the wedge stays out, and the click there sets that level.
        let between = center + ((spots[2] - center) + (spots[3] - center)) / 2.0;
        overlay.drag_moved(between, t0 + ms(500));
        assert_eq!(overlay.wheel.as_ref().unwrap().swollen(), Some(0));
        overlay.drag_moved(spots[3], t0 + ms(520));
        assert!(overlay.click(t0 + ms(520)));
        assert_eq!(overlay.take_commands(), [desk::Command::Volume(0.75)]);

        // Brightness has no command of its own: a click on it does nothing.
        overlay.drag_moved(wedge(center, 1, 2), t0 + ms(700));
        assert!(!overlay.click(t0 + ms(700)));
        assert!(overlay.take_commands().is_empty());
    }

    #[test]
    fn opening_an_app_closes_the_desk() {
        let t0 = Instant::now();
        let (mut overlay, center) = desk_with(
            Menu {
                apps: vec![desk::App {
                    id: "firefox.desktop".into(),
                    name: "Firefox".into(),
                }],
                ..Menu::default()
            },
            t0,
        );
        assert_eq!(overlay.wheel.as_ref().unwrap().mode, Mode::Apps);
        overlay.drag_moved(wedge(center, 0, 1), t0);
        assert!(overlay.click(t0));
        assert!(matches!(
            overlay.take_commands()[..],
            [desk::Command::Launch(_)]
        ));
        assert!(!overlay.desk_open());
    }

    #[test]
    fn when_the_player_goes_its_mode_goes_too() {
        let t0 = Instant::now();
        let (mut overlay, center) = desk_with(
            Menu {
                player: Some(playing()),
                ..levels()
            },
            t0,
        );
        switch_to(&mut overlay, center, Mode::Media, t0);
        overlay.drag_moved(center + vec2(0.0, 20.0), t0);
        overlay.refresh_desk(levels(), t0);
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.modes, [Mode::Apps, Mode::Everyday, Mode::System]);
        assert_eq!(w.mode, Mode::Apps);
        assert_eq!(w.labels, ["Choose apps…"]);
        assert_eq!(w.swell.len(), 1);
        assert_eq!(w.hub.text, "Apps");
    }

    #[test]
    fn emptying_the_trash_takes_a_click() {
        let t0 = Instant::now();
        let (mut overlay, center) = desk_with(Menu::default(), t0);
        switch_to(&mut overlay, center, Mode::System, t0);
        let w = overlay.wheel.as_ref().unwrap();
        let n = w.labels.len();
        let trash = w.labels.iter().position(|l| l == "Empty Trash").unwrap();
        overlay.drag_moved(wedge(center, trash, n), t0);
        // Letting go of the key on it only closes;
        assert!(!overlay.release(t0));
        assert!(overlay.take_commands().is_empty());
        // a click does it.
        let (mut overlay, center) = desk_with(Menu::default(), t0);
        switch_to(&mut overlay, center, Mode::System, t0);
        overlay.drag_moved(wedge(center, trash, n), t0);
        assert!(overlay.click(t0));
        assert_eq!(overlay.take_commands(), [desk::Command::EmptyTrash]);
    }

    #[test]
    fn the_welcome_takes_no_input_and_goes_by_itself() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let mut overlay = Overlay::default();
        overlay.welcome(levels(), screen, "Hold Shift".into(), t0);
        assert!(!overlay.wants_input() && !overlay.desk_open());
        let ctx = egui::Context::default();
        let paint = |overlay: &mut Overlay, now: Instant| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                overlay.paint(ui, now);
            });
            output.textures_delta.clear();
        };
        let w = overlay.wheel.as_ref().unwrap();
        assert_eq!(w.center, screen.center());
        // The droplet comes round first, with the disc no bigger than the hub.
        assert!(w.droplet(t0 + ms(300)).is_some());
        assert!(w.scale(t0 + ms(300)) <= wheel::HUB_R / wheel::DISC_R + 0.001);
        assert!(w.droplet(t0 + ms(800)).is_none());
        paint(&mut overlay, t0 + ms(2000));
        assert!(overlay.wheel.is_some());
        paint(&mut overlay, t0 + WELCOME_TIME);
        paint(&mut overlay, t0 + WELCOME_TIME + ms(500));
        assert!(overlay.is_idle());
    }

    #[test]
    fn the_desk_keeps_its_bubbles_on_screen() {
        let t0 = Instant::now();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let mut overlay = Overlay::default();
        overlay.open_desk(levels(), pos2(5.0, 990.0), screen, t0);
        let center = overlay.wheel.as_ref().unwrap().center;
        for i in 0..2 {
            for spot in wheel::bubble_spots(center, i, 2, 5) {
                assert!(screen.shrink(wheel::BUBBLE_R).contains(spot), "{spot:?}");
            }
        }
    }

    #[test]
    fn once_everything_settles_it_stops_asking_to_be_drawn() {
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (mut overlay, center) = wheel_on_a_picture(t0);
        let n = overlay.wheel.as_ref().unwrap().choices.len();
        overlay.drag_moved(wedge(center, 3, n), t0 + ms(500));
        overlay.drag_moved(center + vec2(0.0, 10.0), t0 + ms(800));
        let w = overlay.wheel.as_ref().unwrap();
        assert!(w.moving(t0 + ms(810)));
        assert!(!w.moving(t0 + ms(2000)));
    }
}
