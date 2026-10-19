//! What the settings window sets, kept as `key = value` lines in
//! `$XDG_CONFIG_HOME/mcgyver/settings.conf` (on macOS `~/Library/Application Support`), and
//! starting at login.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub language: Lang,
    pub trigger: Trigger,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: Lang::from_env(),
            trigger: Trigger::Shift,
        }
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
                _ => {}
            }
        }
        settings
    }

    pub fn to_text(&self) -> String {
        format!(
            "# {APP_NAME} settings\nlanguage = {}\ntrigger = {}\n",
            self.language.code(),
            self.trigger.code()
        )
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

/// `$XDG_CONFIG_HOME`, or `~/.config` (`~/Library/Application Support` on macOS). Per the XDG
/// spec a relative one is ignored.
pub fn config_home() -> Option<PathBuf> {
    let default = if cfg!(target_os = "macos") {
        "Library/Application Support"
    } else {
        ".config"
    };
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(default)))
}

fn path() -> Option<PathBuf> {
    config_home().map(|dir| dir.join(APP_ID).join("settings.conf"))
}

/// What starts the app at login: an XDG autostart entry, or on macOS a launch agent.
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
pub fn starts_at_login() -> bool {
    autostart_file().is_some_and(|p| p.exists())
}

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
    } else {
        own_exe()?.canonicalize()?
    };
    #[cfg(target_os = "macos")]
    if fleeting(&exe) {
        return Err(io::Error::other(crate::i18n::tr(
            "Move McGyver to Applications first.",
        )));
    }
    // Quoted in Exec; anything that would need escaping there is refused.
    exe.to_str()
        .filter(|s| !s.contains(['"', '\\', '`', '$', '%']))
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
        };
        assert_eq!(Settings::parse(&settings.to_text()), settings);
    }

    #[test]
    fn unknown_lines_and_values_keep_the_defaults() {
        let settings = Settings::parse("trigger = meta\nlanguage=tr\ncolour = red\nnonsense\n");
        assert_eq!(settings.trigger, Trigger::Shift);
        assert_eq!(settings.language, Lang::Turkish);
    }
}
