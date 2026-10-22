//! Once a day the app asks its site whether a newer version is out, and the tray and the
//! settings say so. The question also says which version asks, on which system and processor,
//! in which language, and for which machine: the machine's own ID hashed with a salt of the
//! app's, so the site can count machines without being able to name one. Nothing else goes: no
//! file, no file name, nothing done with the wheel (the README's Updates and privacy, and the
//! site's Privacy note, say the same). curl asks; every system the app runs on has it (Windows
//! since 10, version 1803).

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::settings::{APP_NAME, Settings};

/// This build's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where a newer version is downloaded, as the tray and the settings open it.
pub const DOWNLOAD: &str = "https://mcgyver.ferdiunal.tr/?utm_source=mcgyver#download";
/// The app's site, which answers.
const SITE: &str = "https://mcgyver.ferdiunal.tr";
/// The site is asked again once its last answer is this old,
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// which is looked at this often (a day's sleep would stop with the computer's sleep), the first
/// time this long after the start, once login has settled.
const LOOK_EVERY: Duration = Duration::from_secs(60 * 60);
const FIRST_AFTER: Duration = Duration::from_secs(60);
/// Mixed into the machine's ID before it is hashed, so the hash matches no other app's.
const SALT: &str = "McGyver usage info, v1";

/// The newest version the site named, if it is newer than this one.
fn newer() -> &'static Mutex<Option<String>> {
    static NEWER: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    NEWER.get_or_init(|| Mutex::new(read_answer().1.filter(|v| is_newer(v, VERSION))))
}

/// A newer version than this one, as the site last named it: in the settings window too, which
/// reads what the running app was told.
pub fn available() -> Option<String> {
    newer().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Asks on a thread from now on, and calls `found` each time the newer version changes: one
/// turns up, or one taken back goes.
pub fn start(found: impl Fn() + Send + 'static) {
    // Not from a build being worked on, nor from the end-to-end tests.
    let offline = std::env::var_os("MCGYVER_OFFLINE").is_some_and(|v| !v.is_empty() && v != "0");
    if cfg!(debug_assertions) || offline {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("McGyver update".into())
        .spawn(move || {
            std::thread::sleep(FIRST_AFTER);
            loop {
                if due()
                    && let Some(latest) = ask()
                {
                    let latest = Some(latest).filter(|v| is_newer(v, VERSION));
                    let mut newer = newer().lock().unwrap_or_else(|e| e.into_inner());
                    if *newer != latest {
                        *newer = latest;
                        drop(newer);
                        found();
                    }
                }
                std::thread::sleep(LOOK_EVERY);
            }
        });
    if let Err(e) = spawned {
        eprintln!("McGyver: won't look for updates: {e}");
    }
}

/// Whether the site's last answer is a day old, or there is none.
fn due() -> bool {
    let now = now();
    read_answer()
        .0
        .is_none_or(|at| now < at || now - at >= EVERY.as_secs())
}

/// Asks the site, and keeps its answer: the newest version, if it named one.
fn ask() -> Option<String> {
    let settings = Settings::load();
    let mut curl = curl();
    curl.args([
        "--silent",
        "--fail",
        "--max-time",
        "30",
        "--proto",
        "=https",
    ])
    .arg("--user-agent")
    .arg(format!("{APP_NAME}/{VERSION}"));
    // Without the machine's ID there is nothing to count it by: only the version is asked for.
    match machine_id() {
        Some(machine) => {
            let fields = [
                ("machine", machine),
                ("version", VERSION.to_owned()),
                ("os", std::env::consts::OS.to_owned()),
                ("os_version", os_version().unwrap_or_default()),
                ("arch", std::env::consts::ARCH.to_owned()),
                ("lang", settings.language.code().to_owned()),
            ];
            for (key, value) in fields {
                curl.arg("--data-urlencode").arg(format!("{key}={value}"));
            }
            curl.arg(format!("{SITE}/api/ping"));
        }
        None => {
            curl.arg(format!("{SITE}/api/latest"));
        }
    }
    let out = curl
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    // An answer without a version keeps the one named before: the site may have failed to
    // read its releases, and the offer shouldn't vanish for a day.
    let latest = latest_in(&String::from_utf8_lossy(&out.stdout)).or_else(|| read_answer().1);
    save_answer(latest.as_deref());
    latest
}

/// The `latest = 1.2.3` line of the site's answer: a version, and nothing else to show in a menu.
fn latest_in(answer: &str) -> Option<String> {
    answer.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        let value = value.trim();
        let plain = value.len() <= 32
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b));
        (key.trim() == "latest" && plain && version_key(value).is_some()).then(|| value.to_owned())
    })
}

/// Whether version `a` comes after `b`: by its numbers, then a release (1.0.0) after its
/// pre-releases (1.0.0-rc.1).
fn is_newer(a: &str, b: &str) -> bool {
    match (version_key(a), version_key(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// A version's numbers, without the zeros at their end (1.0 is 1.0.0), and whether it is a
/// release.
fn version_key(version: &str) -> Option<(Vec<u64>, bool)> {
    let version = version.split('+').next()?;
    let (numbers, release) = match version.split_once('-') {
        Some((numbers, _)) => (numbers, false),
        None => (version, true),
    };
    let mut numbers = numbers
        .split('.')
        .map(|n| n.parse().ok())
        .collect::<Option<Vec<u64>>>()?;
    while numbers.last() == Some(&0) {
        numbers.pop();
    }
    Some((numbers, release))
}

/// The machine's own ID, salted and hashed: the same on every start, and on reinstalls.
fn machine_id() -> Option<String> {
    let id = system_id()?;
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    let hash = Sha256::digest(format!("{SALT}\n{id}"));
    Some(hash.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(target_os = "linux")]
fn system_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
}

#[cfg(target_os = "macos")]
fn system_id() -> Option<String> {
    let out = Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    // `  "IOPlatformUUID" = "0A1B2C3D-…"`
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|line| line.contains("\"IOPlatformUUID\""))?
        .rsplit('"')
        .nth(1)
        .map(str::to_owned)
}

#[cfg(windows)]
fn system_id() -> Option<String> {
    registry_text(r"SOFTWARE\Microsoft\Cryptography", "MachineGuid")
}

/// The system's name and version, as it calls itself.
#[cfg(target_os = "linux")]
fn os_version() -> Option<String> {
    let release = ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())?;
    release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|name| name.trim_matches(['"', '\'']).to_owned())
}

#[cfg(target_os = "macos")]
fn os_version() -> Option<String> {
    let out = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!version.is_empty()).then(|| format!("macOS {version}"))
}

/// Windows 11 still names itself Windows 10 in the registry; its builds start at 22000.
#[cfg(windows)]
fn os_version() -> Option<String> {
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build = registry_text(KEY, "CurrentBuild")?;
    let name = if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
        "Windows 11"
    } else {
        "Windows 10"
    };
    Some(match registry_text(KEY, "DisplayVersion") {
        Some(release) => format!("{name} {release} ({build})"),
        None => format!("{name} ({build})"),
    })
}

/// A text value under HKEY_LOCAL_MACHINE, from the 64-bit view.
#[cfg(windows)]
fn registry_text(key: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
    };
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, name) = (wide(key), wide(name));
    let mut value = [0u16; 256];
    let mut size = std::mem::size_of_val(&value) as u32;
    // SAFETY: NUL-terminated names; the value is written into `value`, of the size given.
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if code != 0 {
        return None;
    }
    let end = value.iter().position(|&c| c == 0).unwrap_or(value.len());
    Some(String::from_utf16_lossy(&value[..end]))
}

/// The system's own curl: on Windows not one a folder on PATH could stand in for, and without
/// a console window.
fn curl() -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let mut command = Command::new(system.join(r"System32\curl.exe"));
        // CREATE_NO_WINDOW
        command.creation_flags(0x0800_0000);
        command
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("/usr/bin/curl")
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("curl")
    }
}

/// Where the site's last answer is kept, beside the settings.
fn answer_file() -> Option<PathBuf> {
    crate::settings::app_dir().map(|dir| dir.join("update.conf"))
}

/// When the site last answered (seconds since 1970), and the newest version it named.
fn read_answer() -> (Option<u64>, Option<String>) {
    let text = answer_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let checked = text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "checked").then(|| value.trim().parse().ok())?
    });
    (checked, latest_in(&text))
}

/// Writes the answer whole, so the settings window never reads half of it.
fn save_answer(latest: Option<&str>) {
    let Some(path) = answer_file() else {
        return;
    };
    let mut text = format!("# {APP_NAME}'s last update check\nchecked = {}\n", now());
    if let Some(latest) = latest {
        text.push_str(&format!("latest = {latest}\n"));
    }
    let temp = path.with_extension("conf.part");
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temp, text))
        .and_then(|()| std::fs::rename(&temp, &path));
    if let Err(e) = saved {
        eprintln!("McGyver: couldn't keep the update check's answer: {e}");
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_their_numbers_then_releases_after_pre_releases() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("2.0", "1.99.99"));
        assert!(is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(is_newer("1.0.1-rc.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("1.0.0.0", "1.0.0"));
        assert!(is_newer("1.0.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0+build.2", "1.0.0"));
        assert!(!is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("1.0.0-rc.2", "1.0.0"));
        assert!(!is_newer("one", "1.0.0"));
        assert!(!is_newer("", "1.0.0"));
    }

    #[test]
    fn the_answer_names_the_latest_version_on_a_line_of_its_own() {
        let answer = "latest = 1.2.3\nurl = https://mcgyver.ferdiunal.tr/#download\n";
        assert_eq!(latest_in(answer).as_deref(), Some("1.2.3"));
        assert_eq!(latest_in("url = https://example.com\n"), None);
        assert_eq!(latest_in("latest = <html>\n"), None);
        assert_eq!(latest_in("latest = 1.0.1-<b>new</b>\n"), None);
        assert_eq!(latest_in("<!doctype html>\n"), None);
    }

    #[test]
    fn the_machine_is_sent_as_a_hash_that_stays_the_same() {
        let Some(id) = machine_id() else {
            return;
        };
        assert_eq!(id.len(), 64);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_eq!(machine_id(), Some(id.clone()));
        let raw = system_id().unwrap_or_default();
        assert!(!id.contains(raw.trim()));
    }

    #[test]
    fn this_build_names_its_system() {
        let Some(name) = os_version() else {
            return;
        };
        assert!(!name.trim().is_empty() && name.len() < 200, "{name}");
    }
}
