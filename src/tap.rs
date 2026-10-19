//! Tapping the trigger key on its own, without a drag, through KDE's global shortcuts
//! (KGlobalAccel, which KWin hosts). A modifier-only shortcut fires when the key goes down and
//! up with nothing else in between, so typing capitals never counts. `--install` binds it to the
//! trigger key; it shows up in System Settings → Shortcuts as "McGyver". The trigger key in
//! the app's settings wins: the app binds it at start and whenever it changes.

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use zbus::blocking::{Connection, MessageIterator};
use zbus::{MatchRule, message};

use crate::i18n::trf;
use crate::settings::Trigger;

const ACCEL: &str = "org.kde.kglobalaccel";
const ACCEL_PATH: &str = "/kglobalaccel";
const ACCEL_INTERFACE: &str = "org.kde.KGlobalAccel";
const COMPONENT: &str = "mcgyver";
/// KGlobalAccel's object for the component: its name with any '-' turned into '_'.
const COMPONENT_PATH: &str = "/component/mcgyver";
const ACTION: &str = "open-wheel";
/// Component, action, and their names in System Settings.
const ACTION_ID: [&str; 4] = [
    COMPONENT,
    ACTION,
    "McGyver",
    "Open the wheel for the files selected in Dolphin",
];
/// What `--install` registered before the app was called McGyver. Left alone it would keep
/// the key, so the first bind takes it over.
const OLD_ACTION_ID: [&str; 4] = [
    "zero-click-converter",
    ACTION,
    "Zero-Click",
    "Open the wheel for the files selected in Dolphin",
];
/// The key as Qt numbers it (`Qt::Key_Shift` and so on).
fn qt_key(trigger: Trigger) -> i32 {
    match trigger {
        Trigger::Shift => 0x0100_0020,
        Trigger::Ctrl => 0x0100_0021,
        Trigger::Alt => 0x0100_0023,
    }
}
// KGlobalAccel's SetShortcutFlag.
const SET_PRESENT: u32 = 2;
const NO_AUTOLOADING: u32 = 4;
const IS_DEFAULT: u32 = 8;

/// Sends on `tx` whenever the shortcut fires, from a thread of its own. Returns false if it was
/// never set up (`--install`, which `set_up` says wrote its KWin entry), so nothing is
/// registered behind the user's back. Listens even when the key is taken: freeing it and
/// binding again (`rebind`) then works without a restart.
pub fn listen(tx: Sender<()>, trigger: Trigger, set_up: bool) -> zbus::Result<bool> {
    let conn = Connection::session()?;
    if !set_up && !installed(&conn)? {
        return Ok(false);
    }
    // Present again, on the trigger key: it may have changed while the app wasn't running.
    if !bind(trigger)? {
        eprintln!(
            "mcgyver: {} alone is taken by another global shortcut",
            trigger.label()
        );
    }
    let rule = MatchRule::builder()
        .msg_type(message::Type::Signal)
        .sender(ACCEL)?
        .path(COMPONENT_PATH)?
        .interface("org.kde.kglobalaccel.Component")?
        .member("globalShortcutPressed")?
        .build();
    let signals = MessageIterator::for_match_rule(rule, &conn, None)?;
    std::thread::Builder::new()
        .name("mcgyver shortcut".into())
        .spawn(move || {
            for message in signals.flatten() {
                let pressed = message.body().deserialize::<(String, String, i64)>();
                if pressed.is_ok_and(|(_, action, _)| action == ACTION) && tx.send(()).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
    Ok(true)
}

/// Binds the shortcut to `trigger`, as default and current key.
pub fn install(trigger: Trigger) -> Result<(), String> {
    let key = trigger.label();
    match bind(trigger) {
        Ok(true) => {
            crate::kde::say(&format!(
                "Tapping {key} on its own now opens the wheel for the files selected in Dolphin \
                 (the trigger key in the settings changes it)."
            ));
            Ok(())
        }
        Ok(false) => {
            crate::kde::say(&format!(
                "{key} alone is taken by another global shortcut; pick another trigger key in \
                 the settings."
            ));
            Ok(())
        }
        Err(e) => Err(trf("couldn't set up the {} shortcut: {}", &[&key, &e])),
    }
}

/// Moves the shortcut to `trigger` if `install` ran; nothing otherwise. Calls made on several
/// threads in a row leave the shortcut on the last one's key.
pub fn rebind(trigger: Trigger) -> Result<(), String> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    static LATEST: AtomicU64 = AtomicU64::new(0);
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let ticket = LATEST.fetch_add(1, Ordering::AcqRel) + 1;
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    if LATEST.load(Ordering::Acquire) != ticket {
        return Ok(());
    }
    // The KWin entry counts too: with the key taken, KGlobalAccel has no key on record.
    let installed = Connection::session()
        .and_then(|conn| installed(&conn))
        .map(|has_key| has_key || crate::kde::installed());
    match installed {
        Ok(true) => match bind(trigger) {
            Ok(true) => Ok(()),
            Ok(false) => Err(trf(
                "{} alone is taken by another global shortcut",
                &[&trigger.label()],
            )),
            Err(e) => Err(trf("couldn't move the shortcut: {}", &[&e])),
        },
        Ok(false) => Ok(()),
        Err(e) => Err(trf("couldn't move the shortcut: {}", &[&e])),
    }
}

/// Whether the shortcut is on `trigger` right now; `None` if KGlobalAccel can't be asked.
pub fn bound(conn: &Connection, trigger: Trigger) -> Option<bool> {
    let keys = keys(conn, &ACTION_ID).ok()?;
    Some(keys.contains(&qt_key(trigger)))
}

/// Whether KGlobalAccel took `trigger` for the shortcut.
fn bind(trigger: Trigger) -> zbus::Result<bool> {
    let key = qt_key(trigger);
    let conn = Connection::session()?;
    if !keys(&conn, &OLD_ACTION_ID)?.is_empty() {
        unregister(&conn, &OLD_ACTION_ID)?;
    }
    conn.call_method(
        Some(ACCEL),
        ACCEL_PATH,
        Some(ACCEL_INTERFACE),
        "doRegister",
        &(&ACTION_ID[..],),
    )?;
    set_shortcut(&conn, &[key], IS_DEFAULT)?;
    let keys = set_shortcut(&conn, &[key], SET_PRESENT | NO_AUTOLOADING)?;
    Ok(keys.contains(&key))
}

pub fn uninstall() -> Result<(), String> {
    let conn = Connection::session().map_err(|e| e.to_string())?;
    // Even without a key: one cleared in System Settings still lists the action there.
    for id in [&ACTION_ID, &OLD_ACTION_ID] {
        let had_key = !keys(&conn, id).unwrap_or_default().is_empty();
        match unregister(&conn, id) {
            Ok(()) if had_key => crate::kde::say(&format!("Removed the {} shortcut.", id[2])),
            Ok(()) => {}
            Err(e) if had_key => {
                return Err(trf("couldn't remove the shortcut: {}", &[&e]));
            }
            Err(_) => {}
        }
    }
    Ok(())
}

/// Whether `--install` set the shortcut up (under this name or the old one), i.e. it has a key.
/// KGlobalAccel may keep a component it no longer has actions for, so its list of components
/// doesn't tell.
fn installed(conn: &Connection) -> zbus::Result<bool> {
    Ok(!keys(conn, &ACTION_ID)?.is_empty() || !keys(conn, &OLD_ACTION_ID)?.is_empty())
}

fn keys(conn: &Connection, id: &[&str; 4]) -> zbus::Result<Vec<i32>> {
    let reply = conn.call_method(
        Some(ACCEL),
        ACCEL_PATH,
        Some(ACCEL_INTERFACE),
        "shortcut",
        &(&id[..],),
    )?;
    reply.body().deserialize()
}

fn unregister(conn: &Connection, id: &[&str; 4]) -> zbus::Result<()> {
    conn.call_method(
        Some(ACCEL),
        ACCEL_PATH,
        Some(ACCEL_INTERFACE),
        "unRegister",
        &(&id[..],),
    )?;
    Ok(())
}

fn set_shortcut(conn: &Connection, keys: &[i32], flags: u32) -> zbus::Result<Vec<i32>> {
    let reply = conn.call_method(
        Some(ACCEL),
        ACCEL_PATH,
        Some(ACCEL_INTERFACE),
        "setShortcut",
        &(&ACTION_ID[..], keys, flags),
    )?;
    reply.body().deserialize()
}
