/// Prints `log` records (egui_glow reports GL problems through it) when `MCGYVER_DEBUG` is set.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{} {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

#[cfg(target_os = "linux")]
const USAGE: &str = "\
mcgyver [--settings | --quit | --install | --uninstall]

Starts in the background with an icon in the system tray. Drag files, then hold the trigger key
(Shift unless the settings say otherwise); the ring around the wheel's centre switches between
Convert, Tools and Actions. Without a drag: select files in Dolphin and tap the trigger key on
its own, then click an option. Starting it again while it runs opens its settings.
  --settings   open the settings window: language, trigger key, start at login, and the
               permissions KWin and KDE granted, each with the button that fixes it
  --quit       quit the running app
  --install    let KWin offer this binary fake input, so the wheel opens at once, and have
               KWin tell it when the trigger key is tapped on its own (also in the settings)
  --uninstall  undo --install";

#[cfg(target_os = "macos")]
const MAC_USAGE: &str = "\
mcgyver [--settings]

Starts in the background with an icon in the menu bar. Drag files, then hold the trigger key
(Shift unless the settings say otherwise) and drop them on an option; the ring around the
wheel's centre switches between Convert, Tools and Actions. Starting it again while it runs
opens its settings.
  --settings   open the settings window: language, trigger key, start at login, and Input
               Monitoring";

/// An app opened from Finder gets only the system's PATH; ffmpeg usually comes from Homebrew.
#[cfg(target_os = "macos")]
fn homebrew_on_path() {
    let mut paths: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let dir = std::path::PathBuf::from(dir);
        if dir.is_dir() && !paths.contains(&dir) {
            paths.push(dir);
        }
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        // SAFETY: nothing else runs yet; no other thread reads the environment.
        unsafe { std::env::set_var("PATH", joined) };
    }
}

fn main() {
    if std::env::var_os("MCGYVER_DEBUG").is_some() {
        let _ = log::set_logger(&StderrLog);
        log::set_max_level(log::LevelFilter::Warn);
    }
    #[cfg(target_os = "linux")]
    let result = match std::env::args().nth(1).as_deref() {
        None => mcgyver::kde::run(),
        Some("--settings") => mcgyver::settings_window::run(),
        Some("--quit") => {
            if !mcgyver::service::call("Quit") {
                eprintln!("mcgyver: it isn't running");
            }
            Ok(())
        }
        Some("--install") => mcgyver::kde::install(),
        Some("--uninstall") => mcgyver::kde::uninstall(),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(_) => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    #[cfg(target_os = "macos")]
    let result = {
        homebrew_on_path();
        // A first launch that Gatekeeper let through may still carry a process serial number.
        match std::env::args()
            .skip(1)
            .find(|a| !a.starts_with("-psn_"))
            .as_deref()
        {
            None => mcgyver::mac::run(),
            Some("--settings") => mcgyver::settings_window::run(),
            Some("-h" | "--help") => {
                println!("{MAC_USAGE}");
                Ok(())
            }
            Some(_) => {
                eprintln!("{MAC_USAGE}");
                std::process::exit(2);
            }
        }
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let result: Result<(), String> =
        Err("only KDE Plasma on Wayland and macOS are supported so far".into());

    if let Err(err) = result {
        eprintln!("mcgyver: {err}");
        std::process::exit(1);
    }
}
