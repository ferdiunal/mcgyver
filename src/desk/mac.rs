//! The desk on macOS: Spotify and Music through AppleScript, the volume through `osascript`,
//! keyboard layouts through Text Input Sources (on the main thread, which they want), and the
//! apps in the Applications folders. macOS has no public way to set a screen's brightness, so
//! there is no Brightness wedge.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process, Stdio};
use std::sync::Mutex;

use super::{App, Command, Language, Layout, MediaKey, Messenger, Player, Shot, Volume};

/// Players AppleScript can drive, by process name.
const PLAYERS: [&str; 2] = ["Spotify", "Music"];

/// Runs AppleScript lines and returns what they say.
/// How long AppleScript gets: an app that doesn't answer mustn't hold the wheel up.
const SCRIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

fn osascript(lines: &[&str]) -> Option<String> {
    let mut command = Process::new("osascript");
    for line in lines {
        command.args(["-e", line]);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < SCRIPT_TIMEOUT => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut said = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut said).ok()?;
    status.success().then(|| said.trim_end().to_owned())
}

fn running(process: &str) -> bool {
    Process::new("pgrep")
        .args(["-xq", process])
        .status()
        .is_ok_and(|s| s.success())
}

/// Spotify's or Music's track, the one playing first.
pub fn player() -> Option<Player> {
    let mut players: Vec<Player> = PLAYERS
        .iter()
        .filter(|name| running(name))
        .filter_map(|name| {
            let said = osascript(&[&format!(
                "tell application \"{name}\" to return (player state as string) & linefeed & \
                 (name of current track) & linefeed & (artist of current track)"
            )])?;
            let mut lines = said.lines();
            let state = lines.next()?.to_owned();
            if state == "stopped" {
                return None;
            }
            Some(Player {
                id: (*name).to_owned(),
                name: (*name).to_owned(),
                title: lines.next().unwrap_or_default().to_owned(),
                artist: lines.next().unwrap_or_default().to_owned(),
                playing: state == "playing",
                can_next: true,
                can_previous: true,
                shuffle: None,
                repeat: None,
            })
        })
        .collect();
    players.sort_by_key(|p| !p.playing);
    players.into_iter().next()
}

/// Finder is in front with files selected. (`lsappinfo` names the app in front without asking
/// for any permission; Finder is asked only when it is that app.)
pub fn selecting() -> bool {
    let front = Process::new("lsappinfo")
        .args(["info", "-only", "name", "front"])
        .stderr(Stdio::null())
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    if !front.contains("\"Finder\"") {
        return false;
    }
    osascript(&["tell application \"Finder\" to return (count of (get selection)) as string"])
        .and_then(|count| count.trim().parse::<u32>().ok())
        .is_some_and(|count| count > 0)
}

pub fn volume() -> Option<Volume> {
    let said = osascript(&[
        "set s to get volume settings",
        "return (output volume of s as string) & \",\" & (output muted of s as string)",
    ])?;
    let (level, muted) = said.split_once(',')?;
    Some(Volume {
        level: level.trim().parse::<f32>().ok()? / 100.0,
        muted: muted.trim() == "true",
    })
}

pub fn brightness() -> Option<f32> {
    None
}

// Text Input Sources (HIToolbox, in Carbon) and the Core Foundation it hands back.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCreateInputSourceList(properties: *const c_void, all_installed: u8) -> *const c_void;
    fn TISCopyCurrentKeyboardInputSource() -> *const c_void;
    fn TISGetInputSourceProperty(source: *const c_void, key: *const c_void) -> *const c_void;
    fn TISSelectInputSource(source: *const c_void) -> i32;
    static kTISPropertyInputSourceCategory: *const c_void;
    static kTISCategoryKeyboardInputSource: *const c_void;
    static kTISPropertyInputSourceIsSelectCapable: *const c_void;
    static kTISPropertyInputSourceID: *const c_void;
    static kTISPropertyLocalizedName: *const c_void;
    static kTISPropertyInputSourceLanguages: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFEqual(a: *const c_void, b: *const c_void) -> u8;
    fn CFRelease(cf: *const c_void);
    fn CFStringGetCString(s: *const c_void, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFBooleanGetValue(b: *const c_void) -> u8;
}

// Grand Central Dispatch, to run on the main thread.
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_sync_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    fn pthread_main_np() -> i32;
}

/// Runs `f` on the main thread, where Text Input Sources must be used, and waits for it.
fn on_main<T: Send, F: FnOnce() -> T + Send>(f: F) -> T {
    // SAFETY: only asks which thread this is.
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }
    struct Job<F, T> {
        f: Option<F>,
        out: Option<T>,
    }
    extern "C" fn work<F: FnOnce() -> T, T>(context: *mut c_void) {
        // SAFETY: `context` is the `Job` below, alive until `dispatch_sync_f` returns.
        let job = unsafe { &mut *(context as *mut Job<F, T>) };
        job.out = job.f.take().map(|f| f());
    }
    let mut job = Job {
        f: Some(f),
        out: None,
    };
    // SAFETY: the main queue runs `work` once with the job, before this returns.
    unsafe {
        dispatch_sync_f(
            &raw const _dispatch_main_q,
            &mut job as *mut Job<F, T> as *mut c_void,
            work::<F, T>,
        );
    }
    job.out.expect("the main queue ran the job")
}

fn cf_string(s: *const c_void) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let mut buffer = [0 as c_char; 256];
    // SAFETY: a CFString, written into a buffer of the size given, as UTF-8.
    let ok =
        unsafe { CFStringGetCString(s, buffer.as_mut_ptr(), buffer.len() as isize, 0x0800_0100) };
    // SAFETY: NUL-terminated by CFStringGetCString.
    (ok != 0).then(|| {
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    })
}

/// The enabled keyboard layouts that can be switched to, with each source (owned by `list`).
fn keyboard_sources(list: *const c_void) -> Vec<(*const c_void, Layout)> {
    let mut found = Vec::new();
    // SAFETY: `list` is a CFArray of input sources; the properties read are borrowed from them.
    unsafe {
        for i in 0..CFArrayGetCount(list) {
            let source = CFArrayGetValueAtIndex(list, i);
            let category = TISGetInputSourceProperty(source, kTISPropertyInputSourceCategory);
            if category.is_null() || CFEqual(category, kTISCategoryKeyboardInputSource) == 0 {
                continue;
            }
            let selectable =
                TISGetInputSourceProperty(source, kTISPropertyInputSourceIsSelectCapable);
            if selectable.is_null() || CFBooleanGetValue(selectable) == 0 {
                continue;
            }
            let Some(id) = cf_string(TISGetInputSourceProperty(source, kTISPropertyInputSourceID))
            else {
                continue;
            };
            let name = cf_string(TISGetInputSourceProperty(source, kTISPropertyLocalizedName))
                .unwrap_or_else(|| id.clone());
            let languages = TISGetInputSourceProperty(source, kTISPropertyInputSourceLanguages);
            let short = if !languages.is_null() && CFArrayGetCount(languages) > 0 {
                cf_string(CFArrayGetValueAtIndex(languages, 0))
            } else {
                None
            }
            .map(|lang| lang.split('-').next().unwrap_or(&lang).to_uppercase())
            .unwrap_or_else(|| id.rsplit('.').next().unwrap_or(&id).to_uppercase());
            found.push((source, Layout { id, short, name }));
        }
    }
    found
}

pub fn layouts() -> (Vec<Layout>, Option<usize>) {
    on_main(|| {
        // SAFETY: the list and the current source are released after use.
        unsafe {
            let list = TISCreateInputSourceList(std::ptr::null(), 0);
            if list.is_null() {
                return (Vec::new(), None);
            }
            let layouts: Vec<Layout> = keyboard_sources(list).into_iter().map(|(_, l)| l).collect();
            let current = TISCopyCurrentKeyboardInputSource();
            let current_id = if current.is_null() {
                None
            } else {
                let id = cf_string(TISGetInputSourceProperty(
                    current,
                    kTISPropertyInputSourceID,
                ));
                CFRelease(current);
                id
            };
            CFRelease(list);
            let index = layouts
                .iter()
                .position(|l| Some(&l.id) == current_id.as_ref());
            (layouts, index)
        }
    })
}

fn select_layout(id: &str) -> Result<(), String> {
    let id = id.to_owned();
    on_main(move || {
        // SAFETY: the list owns the sources until it is released, after selecting one.
        unsafe {
            let list = TISCreateInputSourceList(std::ptr::null(), 0);
            if list.is_null() {
                return Err("no keyboard layouts".to_owned());
            }
            let status = keyboard_sources(list)
                .into_iter()
                .find(|(_, l)| l.id == id)
                .map(|(source, _)| TISSelectInputSource(source));
            CFRelease(list);
            match status {
                Some(0) => Ok(()),
                Some(code) => Err(format!("couldn't switch the layout ({code})")),
                None => Err(format!("no layout {id}")),
            }
        }
    })
}

pub fn run(command: &Command) -> Result<(), String> {
    let script = |lines: &[&str]| {
        osascript(lines)
            .map(drop)
            .ok_or_else(|| "AppleScript failed".to_owned())
    };
    match command {
        Command::Media { player, key } => {
            let verb = match key {
                MediaKey::PlayPause => "playpause",
                MediaKey::Next => "next track",
                MediaKey::Previous => "previous track",
                MediaKey::Raise => "activate",
            };
            script(&[&format!("tell application \"{player}\" to {verb}")])
        }
        Command::Shuffle { .. } | Command::Repeat { .. } => Ok(()),
        Command::Layout(id) => select_layout(id),
        Command::Volume(level) => script(&[
            "set volume output muted false",
            &format!("set volume output volume {}", (level * 100.0).round()),
        ]),
        Command::Mute(on) => script(&[&format!("set volume output muted {on}")]),
        Command::Brightness(_) => Err("macOS can't set the brightness here".into()),
        Command::Launch(app) => open(&[&app.id]),
        Command::Activate(id) => crate::switcher::activate(id),
        Command::Screenshot(shot) => screenshot(*shot),
        // The Screenshot app's bar, where recording a part or all of the screen is picked.
        Command::Record(_) => {
            open(&["-b", "com.apple.screenshot.launcher"]).or_else(|_| open(&["-a", "Screenshot"]))
        }
        Command::Language(id) => set_language(id),
        Command::MuteMic(on) => mute_mic(*on),
        Command::DarkMode(on) => script(&[&format!(
            "tell application \"System Events\" to tell appearance preferences to set dark mode to {on}"
        )]),
        Command::StayAwake(on) => stay_awake(*on),
        Command::Lock => lock(),
        Command::Sleep => quietly("pmset", &["sleepnow"]),
        Command::EmptyTrash => script(&["tell application \"Finder\" to empty trash"]),
        Command::ShowDesktop => Err("macOS shows the desktop with its own gesture".into()),
        Command::Monitor => open(&["-a", "Activity Monitor"]),
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

/// Locks the screen as the menu bar's Lock Screen does, with the call it makes (in login's
/// private framework). Without it, the screen goes to sleep, which locks when macOS asks for the
/// password at once (as it does unless told otherwise).
fn lock() -> Result<(), String> {
    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    const RTLD_LAZY: c_int = 1;
    let path = c"/System/Library/PrivateFrameworks/login.framework/Versions/Current/login";
    // SAFETY: a NUL-terminated path and symbol; the library stays loaded (never closed).
    let call = unsafe {
        let library = dlopen(path.as_ptr(), RTLD_LAZY);
        if library.is_null() {
            std::ptr::null_mut()
        } else {
            dlsym(library, c"SACLockScreenImmediate".as_ptr())
        }
    };
    if call.is_null() {
        return quietly("pmset", &["displaysleepnow"]);
    }
    // SAFETY: `SACLockScreenImmediate` takes nothing and returns an int.
    let call: extern "C" fn() -> c_int = unsafe { std::mem::transmute(call) };
    call();
    Ok(())
}

/// What a program prints, if it succeeds.
fn said(program: &str, args: &[&str]) -> Option<String> {
    let out = Process::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// macOS adds input sources in its own settings.
pub const ADDS_LAYOUTS: bool = false;

/// `screencapture` takes all three; recording goes through the Screenshot app's bar.
pub fn captures() -> (Vec<Shot>, Vec<Shot>) {
    (
        vec![Shot::Region, Shot::Screen, Shot::Window],
        vec![Shot::Region],
    )
}

/// Takes a screenshot where macOS keeps them (the Desktop unless changed), named as it names
/// them. (macOS asks once for McGyver to be let record the screen.)
fn screenshot(shot: Shot) -> Result<(), String> {
    let folder = said("defaults", &["read", "com.apple.screencapture", "location"])
        .filter(|f| !f.is_empty())
        .map(|f| match f.strip_prefix("~/") {
            Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
            None => PathBuf::from(f),
        })
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Desktop")))
        .ok_or("no folder for screenshots")?;
    let stamp = said("date", &["+%Y-%m-%d %H.%M.%S"]).unwrap_or_default();
    let file = folder.join(format!("{} {stamp}.png", crate::i18n::tr("Screenshot")));
    let mode: &[&str] = match shot {
        Shot::Region => &["-i", "-s"],
        Shot::Screen => &[],
        Shot::Window => &["-i", "-w"],
    };
    let mut child = Process::new("screencapture")
        .args(mode)
        .arg(&file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("screencapture: {e}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The languages macOS may speak, the one spoken first; English is always there to go back to.
pub fn languages() -> (Vec<Language>, Option<usize>) {
    let listed = apple_languages();
    let mut found: Vec<Language> = Vec::new();
    for id in listed.iter().map(String::as_str).chain(["en-US"]) {
        let language = super::languages::language(id);
        if !found.iter().any(|l| l.short == language.short) {
            found.push(language);
        }
    }
    let current = (!found.is_empty()).then_some(0);
    (found, current)
}

/// The languages macOS may speak, as it lists them.
fn apple_languages() -> Vec<String> {
    said("defaults", &["read", "-g", "AppleLanguages"])
        .map(|text| super::languages::defaults_list(&text))
        .unwrap_or_default()
}

/// Puts `id` first in macOS's languages, the others kept as they were; apps speak it when they
/// start again.
fn set_language(id: &str) -> Result<(), String> {
    let mut list = apple_languages();
    list.retain(|l| l != id);
    list.insert(0, id.to_owned());
    let mut args = vec!["write", "-g", "AppleLanguages", "-array"];
    args.extend(list.iter().map(String::as_str));
    quietly("defaults", &args)
}

/// The input volume the microphone had before it was muted here.
static MIC_LEVEL: Mutex<Option<u32>> = Mutex::new(None);

/// macOS has no mute for the microphone; it is muted with its volume at 0.
pub fn mic_muted() -> Option<bool> {
    let level: u32 = osascript(&["return input volume of (get volume settings)"])?
        .trim()
        .parse()
        .ok()?;
    Some(level == 0)
}

fn mute_mic(on: bool) -> Result<(), String> {
    let mut kept = MIC_LEVEL.lock().unwrap_or_else(|e| e.into_inner());
    let level = if on {
        let now = osascript(&["return input volume of (get volume settings)"])
            .and_then(|l| l.trim().parse::<u32>().ok())
            .filter(|&l| l > 0);
        if now.is_some() {
            *kept = now;
        }
        0
    } else {
        kept.take().unwrap_or(75)
    };
    osascript(&[&format!("set volume input volume {level}")])
        .map(drop)
        .ok_or_else(|| "AppleScript failed".to_owned())
}

/// Whether macOS is dark.
pub fn dark() -> Option<bool> {
    Some(crate::settings::system_dark_now())
}

/// `caffeinate` keeping the screen and the computer awake, for as long as it runs (and no
/// longer than McGyver: it watches McGyver's process).
static AWAKE: Mutex<Option<Child>> = Mutex::new(None);

pub fn awake() -> Option<bool> {
    let mut held = AWAKE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(child) = held.as_mut()
        && !matches!(child.try_wait(), Ok(None))
    {
        *held = None;
    }
    Some(held.is_some())
}

fn stay_awake(on: bool) -> Result<(), String> {
    let mut held = AWAKE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut child) = held.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if on {
        let child = Process::new("caffeinate")
            .args(["-d", "-i", "-w", &std::process::id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("caffeinate: {e}"))?;
        *held = Some(child);
    }
    Ok(())
}

pub(super) fn open_link(url: &str) -> Result<(), String> {
    open(&[url])
}

fn open(args: &[&str]) -> Result<(), String> {
    let status = Process::new("open")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("open failed".into())
    }
}

/// The folders apps are kept in.
fn app_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        "/Applications",
        "/Applications/Utilities",
        "/System/Applications",
        "/System/Applications/Utilities",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Applications"));
    }
    dirs
}

fn as_app(path: &Path) -> Option<App> {
    if path.extension()? != "app" {
        return None;
    }
    Some(App {
        id: path.to_string_lossy().into_owned(),
        name: path.file_stem()?.to_string_lossy().into_owned(),
    })
}

pub fn installed_apps() -> Vec<App> {
    let mut apps: Vec<App> = app_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|read| read.flatten())
        .filter_map(|entry| as_app(&entry.path()))
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

pub fn app(id: &str) -> Option<App> {
    let path = Path::new(id);
    path.exists().then(|| as_app(path)).flatten()
}

pub fn messenger(which: Messenger) -> Option<App> {
    let apps = installed_apps();
    let word = match which {
        Messenger::Telegram => "telegram",
        Messenger::WhatsApp => "whatsapp",
        Messenger::Teams => "teams",
        Messenger::Slack => "slack",
    };
    super::find_app(&apps, word).cloned()
}

/// Telegram's own binary takes `-sendpath`, also while it runs (it hands the files over).
/// Only Telegram Desktop (`org.telegram.desktop`) has it; the App Store's Telegram doesn't, and
/// gets the files on the clipboard instead.
pub fn telegram_send(app: &App, files: &[PathBuf]) -> Result<bool, String> {
    let info = Path::new(&app.id).join("Contents/Info");
    let read = |key: &str| {
        let out = Process::new("defaults")
            .arg("read")
            .arg(&info)
            .arg(key)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    if read("CFBundleIdentifier").as_deref() != Some("org.telegram.desktop") {
        return Ok(false);
    }
    let Some(binary) = read("CFBundleExecutable") else {
        return Ok(false);
    };
    let Ok(mut child) = Process::new(Path::new(&app.id).join("Contents/MacOS").join(binary))
        .arg("-sendpath")
        .args(files)
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
    open(&["-a", &app.id])
}
