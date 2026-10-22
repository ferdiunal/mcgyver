//! What the settings window sets, kept as `key = value` lines in
//! `$XDG_CONFIG_HOME/mcgyver/settings.conf` (on macOS `~/Library/Application Support`, on
//! Windows `%APPDATA%`), and starting at login.

use std::io;
use std::path::PathBuf;

use crate::i18n::Lang;

/// Names the config folder, the desktop files and the tray item.
pub const APP_ID: &str = "mcgyver";
/// What people see.
pub const APP_NAME: &str = "McGyver";
/// The macOS bundle's identifier, which also names its login item.
pub const BUNDLE_ID: &str = "io.github.ferdiunal.mcgyver";
/// The ID before the app was called McGyver.
const OLD_APP_ID: &str = "zero-click-converter";

/// The tray menu's first line, greyed out: the app's name and whether it is paused. It also
/// gives the menu a comfortable width.
pub fn status_line(paused: bool) -> String {
    let state = if paused {
        "{} — paused"
    } else {
        "{} — ready"
    };
    crate::i18n::trf(state, &[&APP_NAME])
}

/// The modifier held during a drag to open the wheel, and tapped on its own to open it for
/// Dolphin's selection. On Linux, Meta (`Cmd`) isn't offered: Plasma opens its launcher when it
/// is tapped. On macOS, Command tapped alone does nothing, so it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Shift,
    Ctrl,
    /// Option on a Mac.
    Alt,
    /// Command on a Mac, Meta elsewhere.
    Cmd,
}

impl Trigger {
    #[cfg(not(target_os = "macos"))]
    pub const ALL: &[Trigger] = &[Trigger::Shift, Trigger::Ctrl, Trigger::Alt];
    #[cfg(target_os = "macos")]
    pub const ALL: &[Trigger] = &[Trigger::Shift, Trigger::Ctrl, Trigger::Alt, Trigger::Cmd];

    /// How the settings file names it.
    pub fn code(self) -> &'static str {
        match self {
            Trigger::Shift => "shift",
            Trigger::Ctrl => "ctrl",
            Trigger::Alt => "alt",
            Trigger::Cmd => "cmd",
        }
    }

    /// Only keys this platform offers.
    pub fn from_code(code: &str) -> Option<Trigger> {
        Trigger::ALL.iter().copied().find(|t| t.code() == code)
    }

    /// The name on the key, the same in every language.
    pub fn label(self) -> &'static str {
        match self {
            Trigger::Shift => "Shift",
            Trigger::Ctrl if cfg!(target_os = "macos") => "Control",
            Trigger::Ctrl => "Ctrl",
            Trigger::Alt if cfg!(target_os = "macos") => "Option",
            Trigger::Alt => "Alt",
            Trigger::Cmd if cfg!(target_os = "macos") => "Command",
            Trigger::Cmd => "Meta",
        }
    }
}

/// Light or dark glass, or whichever the desktop uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    /// How the settings file names it.
    pub fn code(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    pub fn from_code(code: &str) -> Option<Appearance> {
        Appearance::ALL.into_iter().find(|a| a.code() == code)
    }

    /// Its name in English, for `tr`.
    pub fn label(self) -> &'static str {
        match self {
            Appearance::System => "Like the system",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }

    /// Whether it comes out dark now.
    pub fn dark(self) -> bool {
        match self {
            Appearance::System => system_dark(),
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }
}

/// How long the trigger key is held alone before the desk opens, by default, and the range the
/// settings offer.
pub const HOLD_MS: u32 = 450;
pub const HOLD_RANGE: std::ops::RangeInclusive<u32> = 200..=1500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub language: Lang,
    pub trigger: Trigger,
    /// Holding the trigger key without files opens the desk (see `desk`),
    pub hold: bool,
    /// once it has been held this long (milliseconds).
    pub hold_ms: u32,
    /// The desk's quick apps, in order, by the id `desk` knows them by.
    pub quick_apps: Vec<String>,
    pub appearance: Appearance,
    /// The welcome has played (see `Overlay::welcome`).
    pub welcomed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: Lang::from_env(),
            trigger: Trigger::Shift,
            hold: true,
            hold_ms: HOLD_MS,
            quick_apps: Vec::new(),
            appearance: Appearance::System,
            welcomed: false,
        }
    }
}

impl Settings {
    /// How long the trigger key is held alone before the desk opens.
    pub fn hold_time(&self) -> std::time::Duration {
        std::time::Duration::from_millis(
            self.hold_ms
                .clamp(*HOLD_RANGE.start(), *HOLD_RANGE.end())
                .into(),
        )
    }
}

impl Settings {
    /// The saved settings; defaults for whatever is missing or can't be read.
    pub fn load() -> Settings {
        let text = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        Settings::parse(&text)
    }

    pub fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match (key.trim(), value.trim()) {
                ("language", code) => {
                    if let Some(lang) = Lang::from_code(code) {
                        settings.language = lang;
                    }
                }
                ("trigger", code) => {
                    if let Some(trigger) = Trigger::from_code(code) {
                        settings.trigger = trigger;
                    }
                }
                ("hold", "on") => settings.hold = true,
                ("hold", "off") => settings.hold = false,
                ("hold_ms", ms) => {
                    if let Ok(ms) = ms.parse::<u32>() {
                        settings.hold_ms = ms.clamp(*HOLD_RANGE.start(), *HOLD_RANGE.end());
                    }
                }
                ("appearance", code) => {
                    if let Some(appearance) = Appearance::from_code(code) {
                        settings.appearance = appearance;
                    }
                }
                ("welcomed", "yes") => settings.welcomed = true,
                ("quick_app", id)
                    if !id.is_empty() && !settings.quick_apps.iter().any(|a| a == id) =>
                {
                    settings.quick_apps.push(id.to_owned());
                }
                _ => {}
            }
        }
        settings
    }

    pub fn to_text(&self) -> String {
        let mut text = format!(
            "# {APP_NAME} settings\nlanguage = {}\ntrigger = {}\nhold = {}\nhold_ms = {}\n\
             appearance = {}\nwelcomed = {}\n",
            self.language.code(),
            self.trigger.code(),
            if self.hold { "on" } else { "off" },
            self.hold_ms,
            self.appearance.code(),
            if self.welcomed { "yes" } else { "no" },
        );
        for id in &self.quick_apps {
            text.push_str(&format!("quick_app = {id}\n"));
        }
        text
    }

    /// Writes the settings whole: a reader never sees half a file.
    pub fn save(&self) -> io::Result<()> {
        let path =
            path().ok_or_else(|| io::Error::other("neither XDG_CONFIG_HOME nor HOME is set"))?;
        let dir = path.parent().expect("the settings file is in a folder");
        std::fs::create_dir_all(dir)?;
        let temp = dir.join(".settings.conf.part");
        std::fs::write(&temp, self.to_text())?;
        std::fs::rename(&temp, &path)
    }
}

/// Carries over what the app left behind when it was called Zero-Click: its settings, when
/// there are none yet under the new name, and its start-at-login entry, which would start the
/// old binary. The old settings file stays where it is.
pub fn migrate() {
    let Some(config) = config_home() else {
        return;
    };
    let old = config.join(OLD_APP_ID).join("settings.conf");
    if path().is_some_and(|new| !new.exists())
        && let Ok(text) = std::fs::read_to_string(old)
    {
        let _ = Settings::parse(&text).save();
    }
    let old_autostart = config
        .join("autostart")
        .join(format!("{OLD_APP_ID}.desktop"));
    if old_autostart.exists() && set_start_at_login(true).is_ok() {
        let _ = std::fs::remove_file(old_autostart);
    }
}

/// When the desktop was last looked at, and whether it was dark; and a look under way.
static LOOKED_DARK: std::sync::Mutex<Option<(std::time::Instant, bool)>> =
    std::sync::Mutex::new(None);
static LOOKING_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the desktop is dark, as last looked at; looked at again behind the scenes once that
/// is a second old (asking can take a moment, and wheels open on the thread asking).
pub fn system_dark() -> bool {
    use std::sync::atomic::Ordering;
    let looked = *LOOKED_DARK.lock().unwrap_or_else(|e| e.into_inner());
    let Some((at, dark)) = looked else {
        return system_dark_now();
    };
    if at.elapsed() >= std::time::Duration::from_secs(1)
        && !LOOKING_DARK.swap(true, Ordering::AcqRel)
    {
        std::thread::spawn(|| {
            system_dark_now();
            LOOKING_DARK.store(false, Ordering::Release);
        });
    }
    dark
}

/// Takes the desktop to be `dark` from now on: the desk just switched it.
pub fn system_dark_is(dark: bool) {
    *LOOKED_DARK.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((std::time::Instant::now(), dark));
}

/// Whether the desktop is dark, looked at again now (after the desk switched it).
pub fn system_dark_now() -> bool {
    let dark = desktop_dark();
    *LOOKED_DARK.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((std::time::Instant::now(), dark));
    dark
}

/// KDE says so through the desktop portal (`color-scheme` 1 is dark); without it, its colours
/// say so: a dark window background.
#[cfg(target_os = "linux")]
fn desktop_dark() -> bool {
    let portal = || -> Option<u32> {
        let bus = zbus::blocking::connection::Builder::session()
            .ok()?
            .method_timeout(std::time::Duration::from_millis(300))
            .build()
            .ok()?;
        let reply = bus
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Settings"),
                "ReadOne",
                &("org.freedesktop.appearance", "color-scheme"),
            )
            .ok()?;
        let value: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
        u32::try_from(value).ok()
    };
    if let Some(scheme) = portal() {
        return scheme == 1;
    }
    let colours = config_home()
        .and_then(|dir| std::fs::read_to_string(dir.join("kdeglobals")).ok())
        .unwrap_or_default();
    let mut in_window = false;
    for line in colours.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_window = line == "[Colors:Window]";
        } else if in_window && let Some(rgb) = line.strip_prefix("BackgroundNormal=") {
            let parts: Vec<f32> = rgb
                .split(',')
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            if let [r, g, b, ..] = parts[..] {
                return 0.299 * r + 0.587 * g + 0.114 * b < 128.0;
            }
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn desktop_dark() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Dark")
}

/// Apps are dark when `AppsUseLightTheme` is 0.
#[cfg(windows)]
fn desktop_dark() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reads a DWORD into `value`, of the size given.
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    } == 0;
    found && value == 0
}

/// `$XDG_CONFIG_HOME`, or `~/.config` (`~/Library/Application Support` on macOS, `%APPDATA%`
/// on Windows). Per the XDG spec a relative one is ignored.
pub fn config_home() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    if cfg!(windows) {
        return xdg.or_else(|| {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        });
    }
    let default = if cfg!(target_os = "macos") {
        "Library/Application Support"
    } else {
        ".config"
    };
    xdg.or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(default)))
}

fn path() -> Option<PathBuf> {
    config_home().map(|dir| dir.join(APP_ID).join("settings.conf"))
}

/// What starts the app at login: an XDG autostart entry, or on macOS a launch agent.
#[cfg(not(windows))]
fn autostart_file() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library/LaunchAgents")
                .join(format!("{BUNDLE_ID}.plist"))
        });
    }
    config_home().map(|dir| dir.join("autostart").join(format!("{APP_ID}.desktop")))
}

/// Whether the desktop starts the app at login.
#[cfg(not(windows))]
pub fn starts_at_login() -> bool {
    autostart_file().is_some_and(|p| p.exists())
}

#[cfg(not(windows))]
pub fn set_start_at_login(on: bool) -> io::Result<()> {
    let path = autostart_file()
        .ok_or_else(|| io::Error::other("neither XDG_CONFIG_HOME nor HOME is set"))?;
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    let exec = launcher()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let entry = if cfg!(target_os = "macos") {
        // `launcher` refuses the characters XML would need escaped, but `&`, `<` and `>`.
        let exec = exec
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n\
             <key>Label</key><string>{BUNDLE_ID}</string>\n\
             <key>ProgramArguments</key><array><string>{exec}</string></array>\n\
             <key>RunAtLoad</key><true/>\n\
             <key>ProcessType</key><string>Interactive</string>\n\
             </dict></plist>\n"
        )
    } else {
        format!(
            "[Desktop Entry]\nType=Application\nName={APP_NAME}\nExec=\"{exec}\"\nIcon={APP_ID}\n\
             NoDisplay=true\nX-GNOME-Autostart-enabled=true\n"
        )
    };
    std::fs::write(&path, entry)
}

/// Where Windows keeps what starts at login for this user; the app's line there is named
/// `APP_NAME`.
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Whether Windows starts the app at login.
#[cfg(windows)]
pub fn starts_at_login() -> bool {
    has_run_entry(APP_NAME)
}

#[cfg(windows)]
pub fn set_start_at_login(on: bool) -> io::Result<()> {
    let command = if on {
        Some(format!("\"{}\"", launcher()?))
    } else {
        None
    };
    set_run_entry(APP_NAME, command.as_deref())
}

/// Whether the Run key has a line called `name`.
#[cfg(windows)]
fn has_run_entry(name: &str) -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let (key, name) = (wide(RUN_KEY), wide(name));
    let mut size = 0u32;
    // SAFETY: NUL-terminated names; only the value's size is asked for.
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    found == 0
}

/// Writes the Run key's line called `name`, or takes it away.
#[cfg(windows)]
fn set_run_entry(name: &str, command: Option<&str>) -> io::Result<()> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
    };
    let (key, name) = (wide(RUN_KEY), wide(name));
    let code = match command {
        Some(command) => {
            let command = wide(command);
            // SAFETY: NUL-terminated names, and the command with its size in bytes, NUL
            // included.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * 2) as u32,
                )
            }
        }
        // SAFETY: NUL-terminated names.
        None => match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) }
        {
            // ERROR_FILE_NOT_FOUND: it wasn't there.
            2 => 0,
            code => code,
        },
    };
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

/// Whether macOS runs this copy only for now: from a disk image (mounted read-only), or the
/// copy it makes of an app opened straight from Downloads (App Translocation). Either is gone by
/// the next login.
#[cfg(target_os = "macos")]
fn fleeting(exe: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
    }
    if exe.to_string_lossy().contains("/AppTranslocation/") {
        return true;
    }
    if !exe.starts_with("/Volumes") {
        return false;
    }
    let Some(Ok(dir)) = exe
        .parent()
        .map(|dir| std::ffi::CString::new(dir.as_os_str().as_bytes()))
    else {
        return false;
    };
    // SAFETY: a NUL-terminated path; W_OK (2) only asks.
    let writable = unsafe { access(dir.as_ptr(), 2) } == 0;
    // EROFS: a read-only file system, which a mounted disk image is.
    !writable && io::Error::last_os_error().raw_os_error() == Some(30)
}

/// This program's path, also after a package upgrade replaced it: the kernel then reports
/// "… (deleted)", and the new binary is at the same path. `/proc/self/exe` if it isn't.
pub fn own_exe() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(path) if std::path::Path::new(path).exists() => Ok(PathBuf::from(path)),
        Some(_) => Ok(PathBuf::from("/proc/self/exe")),
        None => Ok(exe),
    }
}

/// What starts this app: the snap's or AppImage's launcher when packaged so (the binary inside
/// moves with every update), else this binary.
fn launcher() -> io::Result<String> {
    let exe = if let Some(name) = std::env::var_os("SNAP_INSTANCE_NAME") {
        PathBuf::from("/snap/bin").join(name)
    } else if let Some(image) = std::env::var_os("APPIMAGE") {
        PathBuf::from(image)
    } else if cfg!(windows) {
        // Already absolute; made canonical it would start with \\?\, which not every program
        // that reads the Run key takes.
        own_exe()?
    } else {
        own_exe()?.canonicalize()?
    };
    #[cfg(target_os = "macos")]
    if fleeting(&exe) {
        return Err(io::Error::other(crate::i18n::tr(
            "Move McGyver to Applications first.",
        )));
    }
    // Opened from inside a zip, Explorer runs a copy in the temporary folder, gone in time.
    #[cfg(windows)]
    if exe.starts_with(std::env::temp_dir()) {
        return Err(io::Error::other(crate::i18n::tr(
            "Unzip McGyver to a folder first.",
        )));
    }
    // Quoted in Exec (or the Run key); anything that would need escaping there is refused.
    let refused: &[char] = if cfg!(windows) {
        &['"']
    } else {
        &['"', '\\', '`', '$', '%']
    };
    exe.to_str()
        .filter(|s| !s.contains(refused))
        .map(str::to_owned)
        .ok_or_else(|| {
            io::Error::other(format!(
                "move the program to a plainer path than {}",
                exe.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let settings = Settings {
            language: Lang::Turkish,
            trigger: Trigger::Ctrl,
            hold: false,
            hold_ms: 700,
            quick_apps: vec!["firefox.desktop".into(), r"C:\Apps\Kate.lnk".into()],
            appearance: Appearance::Dark,
            welcomed: true,
        };
        assert_eq!(Settings::parse(&settings.to_text()), settings);
        // Older files say nothing of these.
        let old = Settings::parse("language = tr\ntrigger = ctrl\n");
        assert!(old.hold && old.quick_apps.is_empty() && !old.welcomed);
        assert_eq!((old.hold_ms, old.appearance), (HOLD_MS, Appearance::System));
        assert_eq!(
            Settings::parse("hold_ms = 5\n").hold_ms,
            *HOLD_RANGE.start()
        );
    }

    /// Under a name of its own, so McGyver's own line is left alone.
    #[cfg(windows)]
    #[test]
    fn a_run_entry_comes_and_goes() {
        let name = "McGyver test";
        set_run_entry(name, Some(r#""C:\Program Files\McGyver\mcgyver.exe""#)).unwrap();
        assert!(has_run_entry(name));
        set_run_entry(name, None).unwrap();
        assert!(!has_run_entry(name));
        // Already gone is fine.
        set_run_entry(name, None).unwrap();
    }

    #[test]
    fn unknown_lines_and_values_keep_the_defaults() {
        let settings = Settings::parse("trigger = meta\nlanguage=tr\ncolour = red\nnonsense\n");
        assert_eq!(settings.trigger, Trigger::Shift);
        assert_eq!(settings.language, Lang::Turkish);
    }
}
