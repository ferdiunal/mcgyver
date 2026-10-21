//! The desk on Linux: players over MPRIS, KDE's keyboard layouts and screen brightness over
//! D-Bus, the volume through PipeWire's `wpctl` (or PulseAudio's `pactl`), and apps from their
//! desktop files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

use super::{App, Command, Layout, MediaKey, Messenger, Player, Repeat, Volume};
use crate::actions::has;

const MPRIS: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const KEYBOARD: &str = "org.kde.keyboard";
const BRIGHTNESS: &str = "org.kde.ScreenBrightness";
const BRIGHTNESS_PATH: &str = "/org/kde/ScreenBrightness";
const SOLID: &str = "org.kde.Solid.PowerManagement";
const SOLID_BRIGHTNESS: &str = "/org/kde/Solid/PowerManagement/Actions/BrightnessControl";
/// How long the list of installed apps is trusted before the folders are read again.
const APPS_FRESH: Duration = Duration::from_secs(30);

/// How long a D-Bus call may take: a player that hangs mustn't hold the wheel up.
const CALL_TIMEOUT: Duration = Duration::from_millis(500);

fn bus() -> Option<Connection> {
    zbus::blocking::connection::Builder::session()
        .ok()?
        .method_timeout(CALL_TIMEOUT)
        .build()
        .ok()
}

/// A D-Bus property.
fn get<T: TryFrom<OwnedValue>>(
    bus: &Connection,
    dest: &str,
    path: &str,
    interface: &str,
    name: &str,
) -> Option<T> {
    let reply = bus
        .call_method(
            Some(dest),
            path,
            Some(PROPERTIES),
            "Get",
            &(interface, name),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    // A variant may come wrapped once more.
    let value = match &*value {
        Value::Value(inner) => inner.try_to_owned().ok()?,
        _ => value,
    };
    T::try_from(value).ok()
}

fn set(
    bus: &Connection,
    dest: &str,
    path: &str,
    interface: &str,
    name: &str,
    value: Value<'_>,
) -> Result<(), String> {
    bus.call_method(
        Some(dest),
        path,
        Some(PROPERTIES),
        "Set",
        &(interface, name, value),
    )
    .map(drop)
    .map_err(|e| e.to_string())
}

fn call(
    bus: &Connection,
    dest: &str,
    path: &str,
    interface: &str,
    method: &str,
) -> Result<(), String> {
    bus.call_method(Some(dest), path, Some(interface), method, &())
        .map(drop)
        .map_err(|e| e.to_string())
}

/// The strings in a D-Bus array (or a lone string).
fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Str(s) => vec![s.to_string()],
        Value::Array(items) => items
            .iter()
            .filter_map(|v| match v {
                Value::Str(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        Value::Value(inner) => strings(inner),
        _ => Vec::new(),
    }
}

/// The player playing, or else one paused; a stopped one has nothing to show.
pub fn player() -> Option<Player> {
    let bus = bus()?;
    let names = zbus::blocking::fdo::DBusProxy::new(&bus)
        .ok()?
        .list_names()
        .ok()?;
    let mut players: Vec<Player> = names
        .iter()
        .map(|name| name.as_str())
        .filter(|name| name.starts_with(MPRIS))
        .filter_map(|name| read_player(&bus, name))
        .collect();
    players.sort_by_key(|p| !p.playing);
    players.into_iter().next()
}

fn read_player(bus: &Connection, name: &str) -> Option<Player> {
    let status: String = get(bus, name, MPRIS_PATH, MPRIS_PLAYER, "PlaybackStatus")?;
    if status == "Stopped" {
        return None;
    }
    let metadata: HashMap<String, OwnedValue> =
        get(bus, name, MPRIS_PATH, MPRIS_PLAYER, "Metadata").unwrap_or_default();
    let text = |key: &str| {
        metadata
            .get(key)
            .map(|v| strings(v).join(", "))
            .unwrap_or_default()
    };
    let flag = |property: &str| get::<bool>(bus, name, MPRIS_PATH, MPRIS_PLAYER, property);
    let identity: Option<String> = get(bus, name, MPRIS_PATH, "org.mpris.MediaPlayer2", "Identity");
    let repeat = get::<String>(bus, name, MPRIS_PATH, MPRIS_PLAYER, "LoopStatus").map(|s| match s
        .as_str()
    {
        "Track" => Repeat::Track,
        "Playlist" => Repeat::All,
        _ => Repeat::Off,
    });
    Some(Player {
        id: name.to_owned(),
        name: identity.unwrap_or_else(|| {
            let short = name.trim_start_matches(MPRIS);
            short.split('.').next().unwrap_or(short).to_owned()
        }),
        title: text("xesam:title"),
        artist: text("xesam:artist"),
        playing: status == "Playing",
        can_next: flag("CanGoNext").unwrap_or(true),
        can_previous: flag("CanGoPrevious").unwrap_or(true),
        shuffle: flag("Shuffle"),
        repeat,
    })
}

/// KDE's keyboard layouts, and the one in use.
pub fn layouts() -> (Vec<Layout>, Option<usize>) {
    let Some(bus) = bus() else {
        return (Vec::new(), None);
    };
    let call = |method: &str| {
        bus.call_method(
            Some(KEYBOARD),
            "/Layouts",
            Some("org.kde.KeyboardLayouts"),
            method,
            &(),
        )
    };
    let list: Vec<(String, String, String)> = call("getLayoutsList")
        .ok()
        .and_then(|reply| reply.body().deserialize().ok())
        .unwrap_or_default();
    let current: Option<u32> = call("getLayout")
        .ok()
        .and_then(|reply| reply.body().deserialize().ok());
    let layouts = list
        .into_iter()
        .enumerate()
        .map(|(i, (short, _variant, name))| Layout {
            id: i.to_string(),
            short: short.to_uppercase(),
            name,
        })
        .collect();
    (layouts, current.map(|i| i as usize))
}

/// The default output's volume, from PipeWire or PulseAudio.
pub fn volume() -> Option<Volume> {
    let output = |program: &str, args: &[&str]| -> Option<String> {
        let out = Process::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    if has("wpctl") {
        // "Volume: 0.40" or "Volume: 0.40 [MUTED]".
        let said = output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
        let level = said.split_whitespace().nth(1)?.parse::<f32>().ok()?;
        return Some(Volume {
            level: level.clamp(0.0, 1.5),
            muted: said.contains("MUTED"),
        });
    }
    if has("pactl") {
        // "Volume: front-left: 26214 /  40% / -23.88 dB, …" and "Mute: no".
        let said = output("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?;
        let percent = said.split('/').nth(1)?.trim().trim_end_matches('%');
        let level = percent.parse::<f32>().ok()? / 100.0;
        let muted = output("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])
            .is_some_and(|m| m.contains("yes"));
        return Some(Volume { level, muted });
    }
    None
}

/// The built-in screen's brightness (or the first screen's), as KDE sets it.
pub fn brightness() -> Option<f32> {
    let bus = bus()?;
    if let Some(display) = display(&bus) {
        let path = format!("{BRIGHTNESS_PATH}/{display}");
        let interface = "org.kde.ScreenBrightness.Display";
        let now: i32 = get(&bus, BRIGHTNESS, &path, interface, "Brightness")?;
        let max: i32 = get(&bus, BRIGHTNESS, &path, interface, "MaxBrightness")?;
        return (max > 0).then(|| now as f32 / max as f32);
    }
    let call = |method: &str| -> Option<i32> {
        let reply = bus
            .call_method(
                Some(SOLID),
                SOLID_BRIGHTNESS,
                Some("org.kde.Solid.PowerManagement.Actions.BrightnessControl"),
                method,
                &(),
            )
            .ok()?;
        reply.body().deserialize().ok()
    };
    let (now, max) = (call("brightness")?, call("brightnessMax")?);
    (max > 0).then(|| now as f32 / max as f32)
}

/// The screen KDE's brightness control knows, the built-in one first.
fn display(bus: &Connection) -> Option<String> {
    let names: Vec<String> = get(
        bus,
        BRIGHTNESS,
        BRIGHTNESS_PATH,
        BRIGHTNESS,
        "DisplaysDBusNames",
    )?;
    names
        .iter()
        .find(|name| {
            let path = format!("{BRIGHTNESS_PATH}/{name}");
            get::<bool>(
                bus,
                BRIGHTNESS,
                &path,
                "org.kde.ScreenBrightness.Display",
                "IsInternal",
            ) == Some(true)
        })
        .or(names.first())
        .cloned()
}

pub fn run(command: &Command) -> Result<(), String> {
    let bus = || bus().ok_or_else(|| "no session bus".to_owned());
    match command {
        Command::Media { player, key } => {
            let (interface, method) = match key {
                MediaKey::PlayPause => (MPRIS_PLAYER, "PlayPause"),
                MediaKey::Next => (MPRIS_PLAYER, "Next"),
                MediaKey::Previous => (MPRIS_PLAYER, "Previous"),
                MediaKey::Raise => ("org.mpris.MediaPlayer2", "Raise"),
            };
            call(&bus()?, player, MPRIS_PATH, interface, method)
        }
        Command::Shuffle { player, on } => set(
            &bus()?,
            player,
            MPRIS_PATH,
            MPRIS_PLAYER,
            "Shuffle",
            Value::from(*on),
        ),
        Command::Repeat { player, to } => {
            let status = match to {
                Repeat::Off => "None",
                Repeat::Track => "Track",
                Repeat::All => "Playlist",
            };
            set(
                &bus()?,
                player,
                MPRIS_PATH,
                MPRIS_PLAYER,
                "LoopStatus",
                Value::from(status),
            )
        }
        Command::Layout(id) => {
            let index: u32 = id.parse().map_err(|_| format!("no layout {id}"))?;
            bus()?
                .call_method(
                    Some(KEYBOARD),
                    "/Layouts",
                    Some("org.kde.KeyboardLayouts"),
                    "setLayout",
                    &(index,),
                )
                .map(drop)
                .map_err(|e| e.to_string())
        }
        Command::Volume(level) => {
            if has("wpctl") {
                quietly("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"])?;
                quietly(
                    "wpctl",
                    &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{level:.2}")],
                )
            } else {
                quietly("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"])?;
                quietly(
                    "pactl",
                    &[
                        "set-sink-volume",
                        "@DEFAULT_SINK@",
                        &format!("{}%", (level * 100.0).round()),
                    ],
                )
            }
        }
        Command::Mute(on) => {
            let on = if *on { "1" } else { "0" };
            if has("wpctl") {
                quietly("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", on])
            } else {
                quietly("pactl", &["set-sink-mute", "@DEFAULT_SINK@", on])
            }
        }
        Command::Brightness(level) => {
            let bus = bus()?;
            if let Some(display) = display(&bus) {
                let path = format!("{BRIGHTNESS_PATH}/{display}");
                let interface = "org.kde.ScreenBrightness.Display";
                let max: i32 = get(&bus, BRIGHTNESS, &path, interface, "MaxBrightness")
                    .ok_or("the screen's brightness can't be read")?;
                let value = (level * max as f32).round() as i32;
                return bus
                    .call_method(
                        Some(BRIGHTNESS),
                        path.as_str(),
                        Some(interface),
                        "SetBrightness",
                        &(value, 0u32),
                    )
                    .map(drop)
                    .map_err(|e| e.to_string());
            }
            let max: i32 = bus
                .call_method(
                    Some(SOLID),
                    SOLID_BRIGHTNESS,
                    Some("org.kde.Solid.PowerManagement.Actions.BrightnessControl"),
                    "brightnessMax",
                    &(),
                )
                .and_then(|reply| reply.body().deserialize())
                .map_err(|e| e.to_string())?;
            let value = (level * max as f32).round() as i32;
            bus.call_method(
                Some(SOLID),
                SOLID_BRIGHTNESS,
                Some("org.kde.Solid.PowerManagement.Actions.BrightnessControl"),
                "setBrightness",
                &(value,),
            )
            .map(drop)
            .map_err(|e| e.to_string())
        }
        Command::Launch(app) => {
            let entry = entry(&app.id).ok_or_else(|| format!("{} isn't installed", app.name))?;
            launch(&entry, &[])
        }
        Command::Settings => unreachable!("the settings are opened by `desk::run`"),
    }
}

/// Runs `program` and waits for it; an error if it fails.
fn quietly(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Process::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed"))
    }
}

/// A desktop file's launcher, as far as it matters here.
#[derive(Clone, Debug)]
struct Entry {
    id: String,
    path: PathBuf,
    name: String,
    exec: String,
    /// The icon's name in the theme, or a path.
    icon: Option<String>,
}

/// Where desktop files live, the user's own first (they hide the system's of the same id).
fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut dirs: Vec<PathBuf> = data_home.into_iter().collect();
    dirs.extend(std::env::split_paths(&data_dirs));
    // Flatpak's and snap's exports, when the session didn't list them.
    if let Some(home) = &home {
        dirs.push(home.join(".local/share/flatpak/exports/share"));
    }
    dirs.push("/var/lib/flatpak/exports/share".into());
    dirs.push("/var/lib/snapd/desktop".into());
    let mut seen = Vec::new();
    for dir in dirs {
        let dir = dir.join("applications");
        if !seen.contains(&dir) {
            seen.push(dir);
        }
    }
    seen
}

/// Every app a launcher would show, by id, read again every `APPS_FRESH`.
fn entries() -> Vec<Entry> {
    static CACHE: Mutex<Option<(Instant, Vec<Entry>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, entries)) = &*cache
        && at.elapsed() < APPS_FRESH
    {
        return entries.clone();
    }
    let mut found: Vec<Entry> = Vec::new();
    // An id seen once hides the same id further down, hidden or not: that is how a user's own
    // desktop file takes an app off the menus.
    let mut seen = std::collections::HashSet::new();
    for dir in application_dirs() {
        let mut files = Vec::new();
        desktop_files(&dir, &dir, &mut files);
        for (id, path) in files {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(entry) = parse_entry(&id, &path) {
                found.push(entry);
            }
        }
    }
    found.sort_by_key(|e| e.name.to_lowercase());
    *cache = Some((Instant::now(), found.clone()));
    found
}

/// The desktop files under `dir`, with their ids (the path below `root`, `/` turned to `-`).
fn desktop_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            desktop_files(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "desktop")
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push((rel.to_string_lossy().replace('/', "-"), path));
        }
    }
}

/// An application's entry, unless it is hidden or not an application.
fn parse_entry(id: &str, path: &Path) -> Option<Entry> {
    let text = std::fs::read_to_string(path).ok()?;
    let lang = crate::i18n::current().code();
    let mut in_entry = false;
    let (mut name, mut local_name, mut exec, mut icon) = (None, None, None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_entry) else {
            continue;
        };
        match key.trim() {
            "Type" if value.trim() != "Application" => return None,
            "NoDisplay" | "Hidden" if value.trim() == "true" => return None,
            "Name" => name = Some(value.trim().to_owned()),
            k if k == format!("Name[{lang}]") => local_name = Some(value.trim().to_owned()),
            "Exec" => exec = Some(value.trim().to_owned()),
            "Icon" => icon = Some(value.trim().to_owned()).filter(|i| !i.is_empty()),
            _ => {}
        }
    }
    Some(Entry {
        id: id.to_owned(),
        path: path.to_owned(),
        name: local_name.or(name)?,
        exec: exec?,
        icon,
    })
}

fn entry(id: &str) -> Option<Entry> {
    entries().into_iter().find(|e| e.id == id)
}

/// The icon an app's desktop file names (a theme icon's name, or a path). `id` may leave off
/// `.desktop`, as KWin's names for windows' apps do.
pub(crate) fn icon_of(id: &str) -> Option<String> {
    let with = format!("{}.desktop", id.trim_end_matches(".desktop"));
    entries()
        .into_iter()
        .find(|e| e.id == with || e.id.eq_ignore_ascii_case(&with))
        .and_then(|e| e.icon)
}

/// Every app a launcher would show, by name.
pub fn installed_apps() -> Vec<App> {
    entries()
        .into_iter()
        .map(|e| App {
            id: e.id,
            name: e.name,
        })
        .collect()
}

pub fn app(id: &str) -> Option<App> {
    entry(id).map(|e| App {
        id: e.id,
        name: e.name,
    })
}

/// Splits an `Exec` line into words, honouring quotes and dropping the field codes (`%f`…).
fn exec_words(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = exec.chars().peekable();
    let mut in_word = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
        .into_iter()
        .filter(|w| !(w.len() == 2 && w.starts_with('%')))
        .map(|w| w.replace("%%", "%"))
        .collect()
}

/// Starts the app, with `extra` after its own arguments.
fn launch(entry: &Entry, extra: &[String]) -> Result<(), String> {
    let spawn = |program: &str, args: &[String]| -> Result<(), String> {
        let mut child = Process::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{program}: {e}"))?;
        // Reaped when it ends, so nothing is left behind.
        std::thread::spawn(move || child.wait());
        Ok(())
    };
    if extra.is_empty() {
        let path = entry.path.to_string_lossy().into_owned();
        if has("gio") {
            return spawn("gio", &["launch".into(), path]);
        }
        if has("kioclient") {
            return spawn("kioclient", &["exec".into(), path]);
        }
        if has("gtk-launch") {
            return spawn("gtk-launch", std::slice::from_ref(&entry.id));
        }
    }
    let mut words = exec_words(&entry.exec);
    if words.is_empty() {
        return Err(format!("{} has no command", entry.name));
    }
    let program = words.remove(0);
    words.extend(extra.iter().cloned());
    spawn(&program, &words)
}

pub fn messenger(which: Messenger) -> Option<App> {
    let apps = installed_apps();
    let words: &[&str] = match which {
        Messenger::Telegram => &["telegram"],
        Messenger::WhatsApp => &["whatsapp", "zapzap", "wasistlos"],
        Messenger::Teams => &["teams"],
        Messenger::Slack => &["slack"],
    };
    words
        .iter()
        .find_map(|word| super::find_app(&apps, word))
        .cloned()
}

/// Opens Telegram's "share with" window for `files`, when the app can be told to (`-sendpath`):
/// installed natively or as a snap, or as a flatpak with the files forwarded in.
pub fn telegram_send(app: &App, files: &[PathBuf]) -> Result<bool, String> {
    let Some(entry) = entry(&app.id) else {
        return Ok(false);
    };
    let words = exec_words(&entry.exec);
    let paths = files.iter().map(|f| f.to_string_lossy().into_owned());
    // The program itself, not an option (`--command=Telegram`) or a variable (`BAMF_…=…`).
    let named = |w: &String| {
        !w.starts_with('-')
            && !w.contains('=')
            && Path::new(w)
                .file_name()
                .is_some_and(|n| n.to_string_lossy().to_lowercase().contains("telegram"))
    };
    let flatpak = words.first().is_some_and(|w| w.ends_with("flatpak"));
    let (program, args): (String, Vec<String>) = if flatpak
        && let Some(app_id) = words
            .iter()
            .skip_while(|w| *w != "run")
            .skip(1)
            .find(|w| !w.starts_with('-'))
    {
        let mut args = vec![
            "run".to_owned(),
            "--file-forwarding".to_owned(),
            app_id.clone(),
            "-sendpath".to_owned(),
            "@@".to_owned(),
        ];
        args.extend(paths);
        args.push("@@".to_owned());
        ("flatpak".to_owned(), args)
    } else if !flatpak && let Some(program) = words.iter().find(|w| named(w)) {
        let mut args = vec!["-sendpath".to_owned()];
        args.extend(paths);
        (program.clone(), args)
    } else {
        return Ok(false);
    };
    // Not there after all: the files go the clipboard's way instead.
    let Ok(mut child) = Process::new(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Ok(false);
    };
    std::thread::spawn(move || child.wait());
    Ok(true)
}

pub fn open_messenger(_which: Messenger, app: &App) -> Result<(), String> {
    let entry = entry(&app.id).ok_or_else(|| format!("{} isn't installed", app.name))?;
    launch(&entry, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_lines_split_like_a_launcher_would() {
        assert_eq!(
            exec_words(r#"/opt/Telegram/Telegram -workdir "/home/me/My Files" -- %u"#),
            [
                "/opt/Telegram/Telegram",
                "-workdir",
                "/home/me/My Files",
                "--"
            ]
        );
        assert_eq!(exec_words("firefox %U"), ["firefox"]);
        assert_eq!(exec_words("printf 100%%"), ["printf", "100%"]);
    }

    #[test]
    fn a_hidden_entry_isnt_an_app() {
        let dir = tempfile::tempdir().unwrap();
        let shown = dir.path().join("shown.desktop");
        std::fs::write(
            &shown,
            "[Desktop Entry]\nType=Application\nName=Shown\nName[tr]=Görünen\nExec=shown %f\n\
             [Desktop Action new]\nName=New window\n",
        )
        .unwrap();
        let hidden = dir.path().join("hidden.desktop");
        std::fs::write(
            &hidden,
            "[Desktop Entry]\nType=Application\nName=Hidden\nNoDisplay=true\nExec=hidden\n",
        )
        .unwrap();
        let entry = parse_entry("shown.desktop", &shown).unwrap();
        assert_eq!(
            (entry.name.as_str(), entry.exec.as_str()),
            ("Shown", "shown %f")
        );
        assert!(parse_entry("hidden.desktop", &hidden).is_none());
    }
}
