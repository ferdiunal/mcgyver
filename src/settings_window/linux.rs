//! The Permissions card on KDE Plasma: what KWin and KDE actually granted the running app,
//! checked every second over D-Bus, each with the button that fixes it.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crate::glass::{GREEN, RED};
use crate::i18n::{tr, trf};
use crate::service;
use crate::settings::{APP_ID, APP_NAME, Settings, Trigger};

use super::{AMBER, Row, row};

pub(super) const TRIGGER_HELP: &str = "Hold it while dragging files to open the wheel. Tapped on \
                                       its own, it opens the wheel for the files selected in \
                                       Dolphin.";
const FAST_OPEN_HELP: &str = "Without it, the wheel opens once the mouse moves.";
const TAP_HELP: &str = "For the files selected in Dolphin.";
/// How long a restart this window asked for may take (a conversion delays it) before its
/// buttons come back.
const RESTART_GRACE: Duration = Duration::from_secs(60);
/// How often the permissions are looked at again while the window is open.
pub(super) const CHECK_EVERY: Duration = Duration::from_secs(1);

/// One settings window at a time, however it was opened: the guard holds its bus name.
pub(super) fn claim() -> Option<impl Sized> {
    service::claim_settings()
}

/// Has the running app read the saved settings again; false when it isn't running.
pub(super) fn tell_app() -> bool {
    service::call("Reload")
}

/// The window's app id. KWin shows the icon of the desktop file of that name: the menu entry a
/// package installs, or else the entry `--install` wrote.
pub(super) fn app_id() -> &'static str {
    let installed = crate::kde::DESKTOP_FILE;
    if !crate::desk::has_desktop_file(&format!("{APP_ID}.desktop"))
        && crate::desk::has_desktop_file(installed)
    {
        installed.strip_suffix(".desktop").unwrap_or(installed)
    } else {
        APP_ID
    }
}

/// KDE brings a new window forward by itself.
pub(super) fn bring_forward(_: &egui::Context) {}

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

pub(super) struct Permissions {
    checks: Receiver<Check>,
    /// The latest check; `None` until the first arrives.
    check: Option<Check>,
    /// A permission button's work, on a thread: setting up runs KDE's cache rebuild.
    working: Option<(Job, Receiver<Result<(), String>>)>,
    /// A restart this window asked for that isn't over.
    restart: Option<Restart>,
    /// The running app is one this window restarted.
    fresh: bool,
    /// What a finished job came to, for the window to show.
    note: Option<String>,
}

impl Permissions {
    pub(super) fn new() -> Self {
        Permissions {
            checks: checks(),
            check: None,
            working: None,
            restart: None,
            fresh: false,
            note: None,
        }
    }

    /// Takes in what the checks and the jobs came to; call once a frame.
    pub(super) fn update(&mut self) {
        let checks: Vec<Check> = self.checks.try_iter().collect();
        for check in checks {
            self.checked(check);
        }
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
    }

    pub(super) fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    pub(super) fn all_granted(&self) -> bool {
        self.check
            .is_some_and(|c| c.fast_open() == State::Granted && c.tap() == State::Granted)
    }

    pub(super) fn card(&mut self, ui: &mut egui::Ui, trigger: Trigger) {
        let Some(check) = self.check else {
            ui.label(egui::RichText::new(tr("Checking…")).color(super::muted()));
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
        if self.all_granted() {
            ui.label(egui::RichText::new(tr("Everything is ready")).color(GREEN));
        }
        for (title, help, state) in rows {
            ui.add_space(4.0);
            if let Some(fix) = self.permission(ui, title, help, state, trigger) {
                self.fix(fix);
            }
        }
        if check.set_up && self.working.is_none() {
            ui.add_space(6.0);
            if ui.small_button(tr("Remove")).clicked() {
                self.start(Job::Remove, trigger);
            }
        }
    }

    /// One permission: where it stands, and its buttons. Returns the one clicked.
    fn permission(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        help: &str,
        state: State,
        trigger: Trigger,
    ) -> Option<Fix> {
        // Until a restart this window asked for is over, the app's answers are about to change.
        let restarting = self.restarting() && state != State::Granted;
        let (color, status) = match state {
            _ if restarting => (super::muted(), tr("Restarting…").to_owned()),
            State::Granted => (GREEN, tr("On").to_owned()),
            State::NotSetUp => (RED, tr("Not set up").to_owned()),
            State::NeedsRestart => (AMBER, tr("Needs a restart").to_owned()),
            State::Refused => (RED, tr("Still not allowed").to_owned()),
            State::Unsupported => (
                super::muted(),
                tr("This desktop can't share Dolphin's selection").to_owned(),
            ),
            State::NotRunning => (super::muted(), trf("{} isn't running", &[&APP_NAME])),
            State::KeyTaken => (
                RED,
                trf("{} alone is taken by another shortcut", &[&trigger.label()]),
            ),
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
        let labels: Vec<&str> = fixes.iter().map(|fix| tr(fix.label())).collect();
        let shown = Row {
            title,
            help,
            color,
            status: &status,
            buttons: &labels,
            busy: self.working.is_some() || restarting,
        };
        row(ui, &shown).map(|i| fixes[i])
    }

    fn start(&mut self, job: Job, trigger: Trigger) {
        self.fresh = false;
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
        self.note = Some(match done {
            Ok(()) => {
                // KWin hands new rights to a new connection.
                if job != Job::Rebind {
                    self.restart();
                }
                tr("Saved").to_owned()
            }
            Err(e) => trf("Couldn't save: {}", &[&e]),
        });
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
        let trigger = Settings::load().trigger;
        match fix {
            Fix::SetUp => self.start(Job::SetUp, trigger),
            Fix::TryAgain => self.start(Job::Rebind, trigger),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, translate};

    #[test]
    fn everything_the_card_says_has_turkish() {
        let fixes = [
            Fix::SetUp,
            Fix::Restart,
            Fix::Start,
            Fix::OpenShortcuts,
            Fix::TryAgain,
        ];
        for text in [
            TRIGGER_HELP,
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
        let mut permissions = Permissions::new();
        let answer = |started| Check {
            set_up: true,
            granted: Some(Granted {
                started,
                ..Granted::default()
            }),
            ..Check::default()
        };
        permissions.restart = Some(Restart {
            asked_at: Instant::now(),
            from: Some(7),
        });
        // The old process still answering, or nobody answering, doesn't end it.
        permissions.checked(answer(7));
        permissions.checked(Check::default());
        assert!(permissions.restarting());
        // The new one does (a restart keeps the PID, so it is told by when it started).
        permissions.checked(answer(8));
        assert!(!permissions.restarting());
        assert_eq!(permissions.check.unwrap().fast_open(), State::Refused);
    }
}
