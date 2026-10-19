//! The running app on the session bus. A second launch asks it to show its settings instead of
//! starting again, and the settings window asks it to reload what was saved.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use zbus::blocking::Connection;

/// The app's bus name, and the interface its object serves.
pub const NAME: &str = "io.github.McGyver";
const PATH: &str = "/io/github/McGyver";

/// What the app is asked to do, over the bus or from the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    ShowSettings,
    /// Read the settings file again.
    Reload,
    /// Start over as a new process, to pick up what `--install` changed.
    Restart,
    TogglePause,
    Quit,
}

/// What KWin and KDE actually granted the running app, for the settings window to show.
#[derive(Default)]
pub struct Granted {
    /// KWin offered the fake-input protocol, so the wheel opens at once under a drag.
    pub fast_open: AtomicBool,
    /// It listens for the trigger key tapped on its own.
    pub tapping: AtomicBool,
    /// The compositor lets it read Dolphin's selection (ext-data-control), which a tap needs.
    pub selection: AtomicBool,
    /// Both are known: the app has connected to Wayland. Until then it answers nothing.
    pub ready: AtomicBool,
    /// Tells this process from the one before a restart, which keeps the PID.
    pub started: AtomicU64,
}

impl Granted {
    pub fn new() -> Granted {
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        Granted {
            started: AtomicU64::new(started),
            ..Granted::default()
        }
    }
}

struct Service {
    commands: Sender<Command>,
    granted: Arc<Granted>,
}

#[zbus::interface(name = "io.github.McGyver")]
impl Service {
    fn show_settings(&self) {
        let _ = self.commands.send(Command::ShowSettings);
    }

    fn reload(&self) {
        let _ = self.commands.send(Command::Reload);
    }

    fn restart(&self) {
        let _ = self.commands.send(Command::Restart);
    }

    fn quit(&self) {
        let _ = self.commands.send(Command::Quit);
    }

    /// `(fast_open, tapping, selection, started)`, see [`Granted`].
    fn granted(&self) -> zbus::fdo::Result<(bool, bool, bool, u64)> {
        if !self.granted.ready.load(Ordering::Acquire) {
            return Err(zbus::fdo::Error::Failed("still starting".into()));
        }
        let g = &self.granted;
        Ok((
            g.fast_open.load(Ordering::Relaxed),
            g.tapping.load(Ordering::Relaxed),
            g.selection.load(Ordering::Relaxed),
            g.started.load(Ordering::Relaxed),
        ))
    }
}

pub enum Error {
    /// Another instance holds the name.
    Running,
    Bus(zbus::Error),
}

/// Takes the app's name and forwards what callers ask to `commands`. Keep the connection: the
/// name goes with it.
pub fn serve(commands: Sender<Command>, granted: Arc<Granted>) -> Result<Connection, Error> {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    let conn = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.serve_at(PATH, Service { commands, granted }))
        .and_then(|b| b.build())
        .map_err(Error::Bus)?;
    // Asked here rather than through the builder, which would queue behind a running instance
    // and not say so.
    match conn.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(conn),
        Ok(_) | Err(zbus::Error::NameTaken) => Err(Error::Running),
        Err(e) => Err(Error::Bus(e)),
    }
}

/// The settings window's name: one window at a time, whoever starts it.
const SETTINGS_NAME: &str = "io.github.McGyver.Settings";

/// Takes the settings window's name. `None` if a window has it; without a bus, the window
/// opens anyway (`Some(None)`). Keep the connection while the window is up.
pub fn claim_settings() -> Option<Option<Connection>> {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    let Ok(conn) = Connection::session() else {
        return Some(None);
    };
    // Straight to the bus: nothing is served here, which zbus's own request warns about.
    let reply = zbus::blocking::fdo::DBusProxy::new(&conn).and_then(|bus| {
        Ok(bus.request_name(
            SETTINGS_NAME.try_into()?,
            RequestNameFlags::DoNotQueue.into(),
        )?)
    });
    match reply {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Some(Some(conn)),
        Ok(_) => None,
        Err(_) => Some(None),
    }
}

/// Whether a settings window is open.
pub fn settings_open(conn: &Connection) -> bool {
    zbus::blocking::fdo::DBusProxy::new(conn)
        .and_then(|bus| Ok(bus.name_has_owner(SETTINGS_NAME.try_into()?)?))
        .unwrap_or(false)
}

/// What the running app was granted, `(fast_open, tapping, selection, started)` (see
/// [`Granted`]); `None` when it isn't running or is still starting.
pub fn granted(conn: &Connection) -> Option<(bool, bool, bool, u64)> {
    let reply = conn
        .call_method(Some(NAME), PATH, Some(NAME), "Granted", &())
        .ok()?;
    reply.body().deserialize().ok()
}

/// Asks the running app to do `method` ("ShowSettings", "Reload", "Restart" or "Quit"). False
/// when it isn't running.
pub fn call(method: &str) -> bool {
    Connection::session()
        .and_then(|conn| conn.call_method(Some(NAME), PATH, Some(NAME), method, &()))
        .is_ok()
}
