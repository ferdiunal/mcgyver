// No console window behind the app; `attach_console` borrows the one it was started from, when
// there is something to say.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::io::Write;

use mcgyver::settings::APP_NAME;

/// Prints `log` records: warnings and errors, and egui_glow's account of the OpenGL it got, the
/// first thing to know when the wheel won't show. On Windows always (stderr is the log there),
/// elsewhere when `MCGYVER_DEBUG` is set.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Warn
            || (meta.target() == "egui_glow::painter" && meta.level() <= log::Level::Debug)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            // One write, so two processes on one log don't interleave; a full disk isn't a panic.
            let line = format!(
                "{} {}: {}\n",
                record.level(),
                record.target(),
                record.args()
            );
            let _ = std::io::stderr().write_all(line.as_bytes());
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
McGyver [--settings]

Starts in the background with an icon in the menu bar. Drag files, then hold the trigger key
(Shift unless the settings say otherwise) and drop them on an option; the ring around the
wheel's centre switches between Convert, Tools and Actions. Starting it again while it runs
opens its settings.
  --settings   open the settings window: language, trigger key, start at login, and Input
               Monitoring";

#[cfg(windows)]
const WINDOWS_USAGE: &str = "\
McGyver [--settings]

Starts in the background with an icon in the notification area. Drag files, then hold the
trigger key (Shift unless the settings say otherwise) and drop them on an option; the ring
around the wheel's centre switches between Convert, Tools and Actions. Starting it again while
it runs opens its settings.
  --settings   open the settings window: language, trigger key and start at login";

/// A windowed program has no console of its own: started from one, it writes to that one, so
/// `--help`, errors and `MCGYVER_DEBUG` are seen (after the prompt, which doesn't wait for it).
/// Only then: attached, it would also be closed by Ctrl+C there, or with the terminal.
#[cfg(windows)]
fn attach_console() -> bool {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: fails harmlessly when there is no console to attach to.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

/// Runs the app or its settings window: the folder renamed before the log is opened in it, and a
/// panic while the window comes up (eframe panics when no pixel format fits) taken as the failure
/// to start it is.
#[cfg(windows)]
fn windowed(run: fn() -> Result<(), String>, console: bool) -> Result<(), String> {
    mcgyver::settings::migrate();
    mcgyver::logbook::start(console);
    std::panic::catch_unwind(run).unwrap_or_else(|panic| {
        Err(panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "it panicked".into()))
    })
}

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
    if cfg!(windows) || std::env::var_os("MCGYVER_DEBUG").is_some() {
        let _ = log::set_logger(&StderrLog);
        log::set_max_level(log::LevelFilter::Debug);
    }
    #[cfg(target_os = "linux")]
    let result = match std::env::args().nth(1).as_deref() {
        None => mcgyver::kde::run(),
        Some("--settings") => mcgyver::settings_window::run(),
        Some("--quit") => {
            if !mcgyver::service::call("Quit") {
                eprintln!("{APP_NAME}: it isn't running");
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
            None => mcgyver::desktop::run(),
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
    #[cfg(windows)]
    let console = {
        mcgyver::software_gl::take_env();
        // std's panic hook, which ignores a failed write, with the backtrace; nothing else runs yet.
        if std::env::var_os("RUST_BACKTRACE").is_none() {
            // SAFETY: no other thread is there to read the environment.
            unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        }
        std::env::var_os("MCGYVER_DEBUG").is_some() && attach_console()
    };
    #[cfg(windows)]
    let result = {
        match std::env::args().nth(1).as_deref() {
            None => windowed(mcgyver::desktop::run, console),
            Some("--settings") => windowed(mcgyver::settings_window::run, console),
            Some("-h" | "--help") => {
                attach_console();
                println!("{WINDOWS_USAGE}");
                Ok(())
            }
            Some(_) => {
                attach_console();
                eprintln!("{WINDOWS_USAGE}");
                std::process::exit(2);
            }
        }
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let result: Result<(), String> =
        Err("only KDE Plasma on Wayland, macOS and Windows are supported so far".into());

    if let Err(err) = result {
        // OpenGL older than 2.0: once more, on Mesa's software OpenGL, in a process of its own.
        #[cfg(windows)]
        if let Some(mut again) = mcgyver::software_gl::start_again(&err) {
            // The terminal is this process's: the new one writes to it while this one waits.
            if console {
                let _ = again.wait();
            }
            std::process::exit(0);
        }
        // Started from a terminal, it is told there; unless the log has stderr already.
        #[cfg(windows)]
        if !mcgyver::logbook::is_open() {
            attach_console();
        }
        let _ = writeln!(std::io::stderr(), "{APP_NAME}: {err}");
        #[cfg(windows)]
        mcgyver::logbook::alert(&err);
        std::process::exit(1);
    }
}
