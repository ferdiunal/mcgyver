//! The switcher on KDE Plasma 6 (Wayland). A Wayland client can't see other windows, but KWin can
//! run a script for us: we write a short one to a private file, have KWin load and run it, and
//! it calls back over D-Bus on a small object served here with what it saw (or did). The script
//! is unloaded afterwards, whatever happened.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use zbus::blocking::Connection;

use super::Window;

const KWIN: &str = "org.kde.KWin";
const SCRIPTING_PATH: &str = "/Scripting";
const SCRIPTING: &str = "org.kde.kwin.Scripting";
const SCRIPT: &str = "org.kde.kwin.Script";
/// Where the object that hears the script is, and what it is called (the scripts say the same).
const REPORT_PATH: &str = "/io/github/McGyver/Switcher";
/// How long a single D-Bus call may take, and how long the script may take to answer once it
/// runs: with unloading, a call nobody answers still ends within about 400 ms.
const CALL_TIMEOUT: Duration = Duration::from_millis(120);
const ANSWER_TIMEOUT: Duration = Duration::from_millis(150);
/// Fields of a window are separated by this, windows by the next (neither is in a title).
const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';

/// Lists the windows from the top of the stack down, one record each. Separators a title might
/// hold are turned into spaces, so a title can't break the format. `SERVICE` is put in by
/// `run_script`.
const LIST_SCRIPT: &str = r#"
function clean(value) {
    return String(value === undefined || value === null ? "" : value).replace(/[\u001e\u001f]/g, " ");
}
var out = [];
var windows = workspace.stackingOrder;
var active = workspace.activeWindow;
for (var i = windows.length - 1; i >= 0; i--) {
    var w = windows[i];
    if (!w.normalWindow || w.skipTaskbar || w.skipSwitcher) {
        continue;
    }
    var id = w.internalId.toString();
    out.push([id, w.caption, w.resourceClass, w.desktopFileName, w.pid,
        active !== null && id === active.internalId.toString() ? "1" : "0"]
        .map(clean).join("\u001f"));
}
callDBus(SERVICE, "/io/github/McGyver/Switcher", "io.github.McGyver.Switcher", "Report",
    out.join("\u001e"));
"#;

/// Brings one window forward and says "ok", or "missing" if there is no such window. `ID` and
/// `SERVICE` are put in by `run_script`.
const ACTIVATE_SCRIPT: &str = r#"
var found = "missing";
var windows = workspace.windowList();
for (var i = 0; i < windows.length; i++) {
    if (windows[i].internalId.toString() === ID) {
        windows[i].minimized = false;
        workspace.activeWindow = windows[i];
        found = "ok";
        break;
    }
}
callDBus(SERVICE, "/io/github/McGyver/Switcher", "io.github.McGyver.Switcher", "Report", found);
"#;

/// The object KWin's script calls: it passes on what it is told.
struct Report(Sender<String>);

#[zbus::interface(name = "io.github.McGyver.Switcher")]
impl Report {
    fn report(&self, data: String) {
        let _ = self.0.send(data);
    }
}

/// A script's private file under the runtime directory, removed when dropped.
struct ScriptFile(PathBuf);

impl ScriptFile {
    fn create(name: &str, source: &str) -> Result<ScriptFile, String> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("{name}.js"));
        let _ = std::fs::remove_file(&path);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let script = ScriptFile(path);
        file.write_all(source.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(script)
    }
}

impl Drop for ScriptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Unloads a script from KWin when dropped, so a failure or a timeout can't leave it there.
struct Loaded<'a> {
    bus: &'a Connection,
    name: &'a str,
}

impl Drop for Loaded<'_> {
    fn drop(&mut self) {
        let _ = self.bus.call_method(
            Some(KWIN),
            SCRIPTING_PATH,
            Some(SCRIPTING),
            "unloadScript",
            &(self.name,),
        );
    }
}

/// A name for this call's script that no other call shares.
fn plugin_name() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    format!(
        "mcgyver-switcher-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Has KWin run `body` with `SERVICE` (and `ID`, if given) defined, and returns what it reported.
fn run_script(body: &str, id: Option<&str>) -> Result<String, String> {
    let (reports, heard) = mpsc::channel();
    let bus = zbus::blocking::connection::Builder::session()
        .and_then(|b| {
            b.method_timeout(CALL_TIMEOUT)
                .serve_at(REPORT_PATH, Report(reports))
        })
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())?;
    let service = bus.unique_name().ok_or("no bus name")?.to_string();
    let mut source = format!("var SERVICE = {service:?};\n");
    if let Some(id) = id {
        source.push_str(&format!("var ID = {id:?};\n"));
    }
    source.push_str(body);

    let name = plugin_name();
    let file = ScriptFile::create(&name, &source)?;
    let path = file.0.to_str().ok_or("the script's path isn't text")?;
    let reply = bus
        .call_method(
            Some(KWIN),
            SCRIPTING_PATH,
            Some(SCRIPTING),
            "loadScript",
            &(path, name.as_str()),
        )
        .map_err(|e| e.to_string())?;
    let _unload = Loaded {
        bus: &bus,
        name: &name,
    };
    let number: i32 = reply.body().deserialize().map_err(|e| e.to_string())?;
    if number < 0 {
        return Err("KWin wouldn't load the script".to_owned());
    }
    bus.call_method(
        Some(KWIN),
        format!("{SCRIPTING_PATH}/Script{number}").as_str(),
        Some(SCRIPT),
        "run",
        &(),
    )
    .map_err(|e| e.to_string())?;
    heard
        .recv_timeout(ANSWER_TIMEOUT)
        .map_err(|_| "KWin's script didn't answer".to_owned())
}

/// What the script said about one window.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    id: String,
    title: String,
    class: String,
    desktop_file: String,
    pid: u32,
    active: bool,
}

/// The windows in a list script's report, in its order. Records that are cut short are skipped.
fn parse(report: &str) -> Vec<Seen> {
    report
        .split(RECORD)
        .filter_map(|record| {
            let mut fields = record.split(FIELD);
            Some(Seen {
                id: fields.next().filter(|id| !id.is_empty())?.to_owned(),
                title: fields.next()?.to_owned(),
                class: fields.next()?.to_owned(),
                desktop_file: fields.next()?.to_owned(),
                pid: fields.next()?.parse().ok()?,
                active: fields.next()? == "1",
            })
        })
        .collect()
}

/// The folders desktop files are in, the user's first.
fn application_dirs() -> Vec<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")));
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    data_home
        .into_iter()
        .chain(data_dirs.split(':').map(PathBuf::from))
        .map(|dir| dir.join("applications"))
        .collect()
}

/// The `Name=` of a desktop file's own entry (not a translation).
fn desktop_name(contents: &str) -> Option<String> {
    let mut in_entry = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(name) = line.strip_prefix("Name=") {
            let name = name.trim();
            return (!name.is_empty()).then(|| name.to_owned());
        }
    }
    None
}

/// An app's name: its desktop file's, else its window class with a capital.
fn app_name(seen: &Seen, dirs: &[PathBuf], known: &mut HashMap<String, String>) -> String {
    let key = if seen.desktop_file.is_empty() {
        &seen.class
    } else {
        &seen.desktop_file
    };
    if let Some(name) = known.get(key) {
        return name.clone();
    }
    let from_file = (!seen.desktop_file.is_empty())
        .then(|| {
            dirs.iter().find_map(|dir| {
                let file = dir.join(format!("{}.desktop", seen.desktop_file));
                desktop_name(&std::fs::read_to_string(file).ok()?)
            })
        })
        .flatten();
    let name = from_file.unwrap_or_else(|| {
        let mut chars = seen.class.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    });
    known.insert(key.clone(), name.clone());
    name
}

/// This app's own windows aren't for switching to.
fn is_ours(seen: &Seen) -> bool {
    seen.pid == std::process::id()
        || [&seen.class, &seen.desktop_file]
            .iter()
            .any(|name| name.eq_ignore_ascii_case("mcgyver"))
}

pub fn windows() -> Vec<Window> {
    let Ok(report) = run_script(LIST_SCRIPT, None) else {
        return Vec::new();
    };
    let dirs = application_dirs();
    let mut known = HashMap::new();
    let mut windows: Vec<Window> = parse(&report)
        .into_iter()
        .filter(|seen| !is_ours(seen))
        .map(|seen| Window {
            app: app_name(&seen, &dirs, &mut known),
            id: seen.id,
            title: seen.title,
            app_id: seen.desktop_file,
            active: seen.active,
        })
        .collect();
    // The one with the keyboard is the most recent of all; the stack's order is kept otherwise.
    windows.sort_by_key(|w| !w.active);
    windows
}

pub fn activate(id: &str) -> Result<(), String> {
    // The id goes into a script: only what a window's id is made of gets in.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-' || c == '{' || c == '}')
    {
        return Err(format!("not a window id: {id}"));
    }
    match run_script(ACTIVATE_SCRIPT, Some(id))?.as_str() {
        "ok" => Ok(()),
        _ => Err("the window is gone".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_report() {
        let report = format!(
            "{{a-1}}{FIELD}Docs - a\tb\nc{FIELD}firefox{FIELD}firefox{FIELD}42{FIELD}0{RECORD}\
             {{b-2}}{FIELD}~{FIELD}konsole{FIELD}{FIELD}7{FIELD}1"
        );
        assert_eq!(
            parse(&report),
            vec![
                Seen {
                    id: "{a-1}".into(),
                    title: "Docs - a\tb\nc".into(),
                    class: "firefox".into(),
                    desktop_file: "firefox".into(),
                    pid: 42,
                    active: false,
                },
                Seen {
                    id: "{b-2}".into(),
                    title: "~".into(),
                    class: "konsole".into(),
                    desktop_file: String::new(),
                    pid: 7,
                    active: true,
                },
            ]
        );
    }

    #[test]
    fn skips_broken_records_and_empty_reports() {
        assert!(parse("").is_empty());
        assert!(
            parse(&format!(
                "{{x}}{FIELD}title{RECORD}{FIELD}t{FIELD}c{FIELD}d{FIELD}1{FIELD}1"
            ))
            .is_empty()
        );
    }

    #[test]
    fn reads_the_name_of_a_desktop_file() {
        let file =
            "[Desktop Action new]\nName=New\n[Desktop Entry]\nName[tr]=Konsol\nName=Konsole\n";
        assert_eq!(desktop_name(file).as_deref(), Some("Konsole"));
        assert_eq!(desktop_name("[Desktop Entry]\nExec=x\n"), None);
    }

    /// Needs KWin on the session bus; run it in a nested one, never on a real desktop:
    /// `cargo test --lib switcher -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn activates_the_last_window_listed() {
        let before = windows();
        println!("{before:#?}");
        let last = before.last().expect("a window to switch to");
        activate(&last.id).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let after = windows();
        println!("{after:#?}");
        assert_eq!(after.first().map(|w| &w.id), Some(&last.id));
        assert!(after[0].active);
        assert!(activate("{00000000-0000-0000-0000-000000000000}").is_err());
    }
}
