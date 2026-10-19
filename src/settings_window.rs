//! The settings window (`--settings`), a process of its own so the overlay never waits on it:
//! language, trigger key, starting at login, and the permissions KWin and KDE grant, each with
//! the button that fixes it. Each change is saved at once and the running app is told to reload.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};

use crate::glass::{ACCENT, GREEN, RED, TEXT, TEXT_MUTED};
use crate::i18n::{self, Lang, tr, trf};
use crate::service;
use crate::settings::{self, APP_ID, APP_NAME, Settings, Trigger};

const TRIGGER_HELP: &str = "Hold it while dragging files to open the wheel. Tapped on its own, it \
                            opens the wheel for the files selected in Dolphin.";
const FAST_OPEN_HELP: &str = "Without it, the wheel opens once the mouse moves.";
const TAP_HELP: &str = "For the files selected in Dolphin.";
/// How long a restart this window asked for may take (a conversion delays it) before its
/// buttons come back.
const RESTART_GRACE: Duration = Duration::from_secs(60);
/// How often the permissions are looked at again while the window is open.
const CHECK_EVERY: Duration = Duration::from_secs(1);
const AMBER: Color32 = Color32::from_rgb(226, 146, 18);
/// How long "Saved" stays up.
const NOTE_FOR: Duration = Duration::from_secs(3);
const BACKGROUND: Color32 = Color32::from_rgb(242, 243, 245);
const CARD: Color32 = Color32::WHITE;

pub fn run() -> Result<(), String> {
    // One window at a time, however it was opened.
    let Some(_name) = service::claim_settings() else {
        println!("The settings are open already.");
        return Ok(());
    };
    settings::migrate();
    let settings = Settings::load();
    i18n::set(settings.language);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title())
        .with_app_id(APP_ID)
        .with_inner_size([440.0, 680.0])
        .with_min_inner_size([380.0, 360.0]);
    if let Some(rgba) = crate::icon::rgba(64) {
        viewport = viewport.with_icon(egui::IconData {
            rgba,
            width: 64,
            height: 64,
        });
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            crate::glass::install_fonts(&cc.egui_ctx);
            style(&cc.egui_ctx);
            Ok(Box::new(Window::new(settings)))
        }),
    )
    .map_err(|e| e.to_string())
}

fn title() -> String {
    format!("{} — {APP_NAME}", tr("Settings"))
}

fn style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 6.0);
        let v = &mut style.visuals;
        v.panel_fill = BACKGROUND;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.inactive.corner_radius = CornerRadius::same(8);
        v.widgets.hovered.corner_radius = CornerRadius::same(8);
        v.widgets.active.corner_radius = CornerRadius::same(8);
    });
}

/// What the running app was granted (see `service::Granted`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Granted {
    fast_open: bool,
    tapping: bool,
    /// It can read Dolphin's selection, which a tap needs.
    selection: bool,
    /// Which process answered: a restart changes it.
    started: u64,
}

/// What the permission check found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Check {
    /// `--install` wrote its KWin entry.
    set_up: bool,
    /// `None` when the app isn't running (or is still starting).
    granted: Option<Granted>,
    /// Whether the KDE shortcut is on the trigger key; `None` when KDE can't be asked.
    bound: Option<bool>,
    /// The running app is one this window restarted after setting up: a restart won't help.
    fresh: bool,
}

/// Where one permission stands, which says what its button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Granted,
    NotSetUp,
    /// Set up, but the running app started before: KWin grants rights when it connects.
    NeedsRestart,
    /// Set up, and still not granted after a restart.
    Refused,
    NotRunning,
    /// Another global shortcut has the trigger key on its own.
    KeyTaken,
    /// The compositor can't do it at all.
    Unsupported,
}

impl Check {
    /// Opening at once under a drag: KWin's fake input.
    fn fast_open(&self) -> State {
        match (self.set_up, self.granted) {
            (_, Some(g)) if g.fast_open => State::Granted,
            (false, _) => State::NotSetUp,
            (true, None) => State::NotRunning,
            (true, Some(_)) if self.fresh => State::Refused,
            (true, Some(_)) => State::NeedsRestart,
        }
    }

    /// Opening when the trigger key is tapped alone: the KDE shortcut.
    fn tap(&self) -> State {
        if !self.set_up {
            return State::NotSetUp;
        }
        let Some(granted) = self.granted else {
            return State::NotRunning;
        };
        if !granted.selection {
            return State::Unsupported;
        }
        if self.bound == Some(false) {
            return State::KeyTaken;
        }
        match granted.tapping {
            true => State::Granted,
            false if self.fresh => State::Refused,
            false => State::NeedsRestart,
        }
    }
}

/// Looks at the permissions every `CHECK_EVERY` on a thread (a few D-Bus calls on one
/// connection), until the window is gone.
fn checks() -> Receiver<Check> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bus = None;
        loop {
            if bus.is_none() {
                bus = zbus::blocking::Connection::session().ok();
            }
            let check = Check {
                set_up: crate::kde::installed(),
                granted: bus.as_ref().and_then(service::granted).map(
                    |(fast_open, tapping, selection, started)| Granted {
                        fast_open,
                        tapping,
                        selection,
                        started,
                    },
                ),
                bound: bus
                    .as_ref()
                    .and_then(|bus| crate::tap::bound(bus, Settings::load().trigger)),
                fresh: false,
            };
            if tx.send(check).is_err() {
                break;
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
    rx
}

/// A restart this window asked for, until the app is back.
#[derive(Clone, Copy, Debug)]
struct Restart {
    asked_at: Instant,
    /// The process asked to restart, when it had answered; any answer ends it otherwise.
    from: Option<u64>,
}

/// What a permission button starts on a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    SetUp,
    Remove,
    /// Bind the shortcut to the trigger key again, after the key was freed.
    Rebind,
}

struct Window {
    settings: Settings,
    start_at_login: bool,
    checks: Receiver<Check>,
    /// The latest check; `None` until the first arrives.
    check: Option<Check>,
    /// A permission button's work, on a thread: setting up runs KDE's cache rebuild.
    working: Option<(Job, Receiver<Result<(), String>>)>,
    /// A restart this window asked for that isn't over.
    restart: Option<Restart>,
    /// The running app is one this window restarted.
    fresh: bool,
    /// What the last change came to, and when.
    note: Option<(String, Instant)>,
}

impl Window {
    fn new(settings: Settings) -> Self {
        Window {
            settings,
            start_at_login: settings::starts_at_login(),
            checks: checks(),
            check: None,
            working: None,
            restart: None,
            fresh: false,
            note: None,
        }
    }

    fn start(&mut self, job: Job) {
        self.fresh = false;
        let trigger = self.settings.trigger;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let done = match job {
                // Both return once KWin has caught up, so the restart in `finished` connects
                // with the new rights.
                Job::SetUp => crate::kde::install(),
                Job::Remove => crate::kde::uninstall(),
                Job::Rebind => crate::tap::rebind(trigger),
            };
            let _ = tx.send(done);
        });
        self.working = Some((job, rx));
    }

    fn finished(&mut self, job: Job, done: Result<(), String>) {
        match done {
            Ok(()) => {
                // KWin hands new rights to a new connection.
                if job != Job::Rebind {
                    self.restart();
                }
                self.note(tr("Saved").to_owned());
            }
            Err(e) => self.note(trf("Couldn't save: {}", &[&e])),
        }
    }

    /// Saves, and has the running app pick the change up.
    fn changed(&mut self) {
        i18n::set(self.settings.language);
        let said = match self.settings.save() {
            Err(e) => trf("Couldn't save: {}", &[&e]),
            Ok(()) if service::call("Reload") => tr("Saved").to_owned(),
            Ok(()) => trf(
                "{} isn't running; it will use these when it starts.",
                &[&APP_NAME],
            ),
        };
        self.note = Some((said, Instant::now()));
    }

    fn note(&mut self, said: String) {
        self.note = Some((said, Instant::now()));
    }
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let before = self.settings;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BACKGROUND).inner_margin(Margin::same(18)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.cards(ui));
            });
        if self.settings != before {
            self.changed();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title()));
        }
    }
}

impl Window {
    fn cards(&mut self, ui: &mut egui::Ui) {
        let checks: Vec<Check> = self.checks.try_iter().collect();
        for check in checks {
            self.checked(check);
        }
        ui.ctx().request_repaint_after(CHECK_EVERY);
        if let Some((job, rx)) = &self.working {
            let done = match rx.try_recv() {
                Ok(done) => Some(done),
                Err(mpsc::TryRecvError::Empty) => None,
                // The thread is gone without an answer: it panicked.
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(tr("it stopped unexpectedly").to_owned()))
                }
            };
            if let Some(done) = done {
                let job = *job;
                self.working = None;
                self.finished(job, done);
            }
        }
        // Something missing comes first; once all is granted it moves to the bottom.
        let all_granted = self
            .check
            .is_some_and(|c| c.fast_open() == State::Granted && c.tap() == State::Granted);

        ui.label(RichText::new(tr("Settings")).size(22.0).strong());
        ui.add_space(6.0);
        if !all_granted {
            card(ui, tr("Permissions"), None, |ui| self.permissions(ui));
        }

        card(ui, tr("Language"), None, |ui| {
            ui.horizontal(|ui| {
                for lang in Lang::ALL {
                    segment(ui, &mut self.settings.language, lang, lang.name());
                }
            });
        });

        card(ui, tr("Trigger key"), Some(tr(TRIGGER_HELP)), |ui| {
            ui.horizontal(|ui| {
                for trigger in Trigger::ALL {
                    segment(ui, &mut self.settings.trigger, trigger, trigger.label());
                }
            });
        });

        card(ui, tr("Start at login"), None, |ui| {
            if toggle(ui, &mut self.start_at_login).changed() {
                match settings::set_start_at_login(self.start_at_login) {
                    Ok(()) => self.note(tr("Saved").to_owned()),
                    Err(e) => {
                        self.start_at_login = settings::starts_at_login();
                        self.note(trf("Couldn't save: {}", &[&e]));
                    }
                }
            }
        });

        if all_granted {
            card(ui, tr("Permissions"), None, |ui| self.permissions(ui));
        }

        if let Some((said, at)) = &self.note {
            let left = NOTE_FOR.saturating_sub(at.elapsed());
            if left.is_zero() {
                self.note = None;
            } else {
                ui.add_space(4.0);
                ui.label(RichText::new(said.as_str()).color(TEXT_MUTED));
                ui.ctx().request_repaint_after(left);
            }
        }
    }
}

impl Window {
    fn permissions(&mut self, ui: &mut egui::Ui) {
        let Some(check) = self.check else {
            ui.label(RichText::new(tr("Checking…")).color(TEXT_MUTED));
            return;
        };
        let rows = [
            (
                tr("Opens at once under a drag"),
                tr(FAST_OPEN_HELP),
                check.fast_open(),
            ),
            (
                tr("Opens when the trigger key is tapped alone"),
                tr(TAP_HELP),
                check.tap(),
            ),
        ];
        if rows.iter().all(|(_, _, state)| *state == State::Granted) {
            ui.label(RichText::new(tr("Everything is ready")).color(GREEN));
        }
        for (title, help, state) in rows {
            ui.add_space(4.0);
            if let Some(fix) = self.permission(ui, title, help, state) {
                self.fix(fix);
            }
        }
        if check.set_up && self.working.is_none() {
            ui.add_space(6.0);
            if ui.small_button(tr("Remove")).clicked() {
                self.start(Job::Remove);
            }
        }
    }

    /// One permission: where it stands, and its buttons. Returns the one clicked.
    fn permission(&self, ui: &mut egui::Ui, title: &str, help: &str, state: State) -> Option<Fix> {
        let key = self.settings.trigger.label();
        // Until a restart this window asked for is over, the app's answers are about to change.
        let restarting = self.restarting() && state != State::Granted;
        let (color, status) = match state {
            _ if restarting => (TEXT_MUTED, tr("Restarting…").to_owned()),
            State::Granted => (GREEN, tr("On").to_owned()),
            State::NotSetUp => (RED, tr("Not set up").to_owned()),
            State::NeedsRestart => (AMBER, tr("Needs a restart").to_owned()),
            State::Refused => (RED, tr("Still not allowed").to_owned()),
            State::Unsupported => (
                TEXT_MUTED,
                tr("This desktop can't share Dolphin's selection").to_owned(),
            ),
            State::NotRunning => (TEXT_MUTED, trf("{} isn't running", &[&APP_NAME])),
            State::KeyTaken => (RED, trf("{} alone is taken by another shortcut", &[&key])),
        };
        let fixes: &[Fix] = match state {
            State::Granted => &[],
            State::NotSetUp => &[Fix::SetUp],
            State::NeedsRestart => &[Fix::Restart],
            // Writing the entry again may help if it went stale.
            State::Refused => &[Fix::SetUp],
            State::Unsupported => &[],
            State::NotRunning => &[Fix::Start],
            State::KeyTaken => &[Fix::OpenShortcuts, Fix::TryAgain],
        };
        let mut clicked = None;
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 5.0, color);
            ui.label(RichText::new(title).strong());
        });
        ui.indent(title, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(status).size(12.5).color(color));
                ui.label(RichText::new(help).size(12.5).color(TEXT_MUTED));
            });
            if fixes.is_empty() {
                return;
            }
            ui.horizontal(|ui| {
                if self.working.is_some() || restarting {
                    ui.spinner();
                    return;
                }
                for fix in fixes {
                    if ui.button(tr(fix.label())).clicked() {
                        clicked = Some(*fix);
                    }
                }
            });
        });
        clicked
    }

    fn restart(&mut self) {
        let from = self.check.and_then(|c| c.granted).map(|g| g.started);
        if service::call("Restart") {
            self.restart = Some(Restart {
                asked_at: Instant::now(),
                from,
            });
        }
    }

    /// The app is restarting at this window's request, and isn't back yet.
    fn restarting(&self) -> bool {
        self.restart.is_some()
    }

    /// Takes in a new check, following a restart: over once another process answers.
    fn checked(&mut self, check: Check) {
        if let (Some(restart), Some(granted)) = (self.restart, check.granted)
            && restart.from != Some(granted.started)
        {
            self.restart = None;
            self.fresh = true;
        }
        if self
            .restart
            .is_some_and(|r| r.asked_at.elapsed() > RESTART_GRACE)
        {
            self.restart = None;
        }
        self.check = Some(Check {
            fresh: self.fresh,
            ..check
        });
    }

    fn fix(&mut self, fix: Fix) {
        match fix {
            Fix::SetUp => self.start(Job::SetUp),
            Fix::TryAgain => self.start(Job::Rebind),
            Fix::Restart => self.restart(),
            Fix::Start => detach(
                crate::settings::own_exe().map(|exe| exe.into_os_string()),
                &[],
            ),
            Fix::OpenShortcuts => {
                // Plasma's shortcuts page; kcmshell6 where System Settings isn't installed.
                let _ = std::process::Command::new("sh")
                    .args([
                        "-c",
                        "(systemsettings kcm_keys || kcmshell6 kcm_keys) </dev/null >/dev/null 2>&1 &",
                    ])
                    .status();
            }
        }
    }
}

/// What a permission's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fix {
    SetUp,
    Restart,
    /// Start the app, which isn't running.
    Start,
    OpenShortcuts,
    TryAgain,
}

impl Fix {
    fn label(self) -> &'static str {
        match self {
            Fix::SetUp => "Set up",
            Fix::Restart => "Restart",
            Fix::Start => "Start",
            Fix::OpenShortcuts => "Open shortcuts",
            Fix::TryAgain => "Try again",
        }
    }
}

/// Runs `program` with `args` through `sh`, in the background and not as our child, so it
/// outlives this window.
fn detach(program: std::io::Result<std::ffi::OsString>, args: &[&str]) {
    let Ok(program) = program else {
        return;
    };
    let _ = std::process::Command::new("sh")
        .args(["-c", "\"$0\" \"$@\" </dev/null >/dev/null 2>&1 &"])
        .arg(program)
        .args(args)
        .status();
}

/// A white panel with a heading, an optional line under it, and the controls.
fn card(ui: &mut egui::Ui, heading: &str, help: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(12))
        .stroke(Stroke::new(1.0, Color32::from_black_alpha(18)))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(heading).size(15.0).strong());
            if let Some(help) = help {
                ui.label(RichText::new(help).size(12.5).color(TEXT_MUTED));
            }
            ui.add_space(2.0);
            add(ui);
        });
    ui.add_space(4.0);
}

/// One button of a row where exactly one is on.
fn segment<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, this: T, text: &str) {
    let on = *value == this;
    let label = RichText::new(text).color(if on { Color32::WHITE } else { TEXT });
    let button = egui::Button::new(label)
        .fill(if on { ACCENT } else { BACKGROUND })
        .min_size(Vec2::new(84.0, 32.0));
    if ui.add(button).clicked() {
        *value = this;
    }
}

/// An on/off switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(42.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let track = BACKGROUND.lerp_to_gamma(ACCENT, t);
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(12),
        track,
        Stroke::new(1.0, Color32::from_black_alpha(25)),
        egui::StrokeKind::Inside,
    );
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    painter.circle(
        egui::pos2(x, rect.center().y),
        9.0,
        Color32::WHITE,
        Stroke::new(1.0, Color32::from_black_alpha(30)),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::translate;

    #[test]
    fn everything_the_window_says_has_turkish() {
        let fixes = [
            Fix::SetUp,
            Fix::Restart,
            Fix::Start,
            Fix::OpenShortcuts,
            Fix::TryAgain,
        ];
        for text in [
            "Settings",
            "Language",
            "Trigger key",
            TRIGGER_HELP,
            "Start at login",
            "Permissions",
            "Checking…",
            "Everything is ready",
            "Opens at once under a drag",
            FAST_OPEN_HELP,
            "Opens when the trigger key is tapped alone",
            TAP_HELP,
            "On",
            "Not set up",
            "Needs a restart",
            "Restarting…",
            "Still not allowed",
            "This desktop can't share Dolphin's selection",
            "it stopped unexpectedly",
            "{} isn't running",
            "{} alone is taken by another shortcut",
            "Remove",
            "Saved",
            "Couldn't save: {}",
            "{} isn't running; it will use these when it starts.",
        ]
        .into_iter()
        .chain(fixes.map(Fix::label))
        {
            assert_ne!(translate(Lang::Turkish, text), text, "{text}");
        }
    }

    #[test]
    fn each_permission_says_what_would_fix_it() {
        let granted = |fast_open, tapping| {
            Some(Granted {
                fast_open,
                tapping,
                selection: true,
                started: 1,
            })
        };
        let check = |set_up, granted, bound| Check {
            set_up,
            granted,
            bound,
            fresh: false,
        };
        // Nothing done yet, app running or not.
        for g in [None, granted(false, false)] {
            let c = check(false, g, Some(false));
            assert_eq!((c.fast_open(), c.tap()), (State::NotSetUp, State::NotSetUp));
        }
        // Set up after the app started: KWin's rights come with a new connection.
        let c = check(true, granted(false, false), Some(true));
        assert_eq!(
            (c.fast_open(), c.tap()),
            (State::NeedsRestart, State::NeedsRestart)
        );
        // ...and after the restart this window asked for, still nothing: restarting won't help.
        let c = Check { fresh: true, ..c };
        assert_eq!((c.fast_open(), c.tap()), (State::Refused, State::Refused));
        // All there.
        let c = check(true, granted(true, true), Some(true));
        assert_eq!((c.fast_open(), c.tap()), (State::Granted, State::Granted));
        // Another shortcut holds the key; fast open doesn't care.
        let c = check(true, granted(true, true), Some(false));
        assert_eq!((c.fast_open(), c.tap()), (State::Granted, State::KeyTaken));
        // The app isn't running: that comes before what KDE says about the key.
        let c = check(true, None, Some(false));
        assert_eq!(
            (c.fast_open(), c.tap()),
            (State::NotRunning, State::NotRunning)
        );
        // KDE couldn't be asked about the key: not reported as taken.
        let c = check(true, granted(true, true), None);
        assert_eq!(c.tap(), State::Granted);
        // A compositor that can't share the selection: nothing to press.
        let c = Check {
            granted: Some(Granted {
                fast_open: true,
                tapping: true,
                selection: false,
                started: 1,
            }),
            ..check(true, None, Some(true))
        };
        assert_eq!(c.tap(), State::Unsupported);
    }

    #[test]
    fn a_restart_is_over_once_another_process_answers() {
        let mut window = Window::new(Settings::default());
        let answer = |started| Check {
            set_up: true,
            granted: Some(Granted {
                started,
                ..Granted::default()
            }),
            ..Check::default()
        };
        window.restart = Some(Restart {
            asked_at: Instant::now(),
            from: Some(7),
        });
        // The old process still answering, or nobody answering, doesn't end it.
        window.checked(answer(7));
        window.checked(Check::default());
        assert!(window.restarting());
        // The new one does (a restart keeps the PID, so it is told by when it started).
        window.checked(answer(8));
        assert!(!window.restarting());
        assert_eq!(window.check.unwrap().fast_open(), State::Refused);
    }
}
