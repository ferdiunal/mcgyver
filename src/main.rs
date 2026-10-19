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
    #[cfg(not(target_os = "linux"))]
    let result: Result<(), String> = Err("only KDE Plasma on Wayland is supported so far".into());

    if let Err(err) = result {
        eprintln!("mcgyver: {err}");
        std::process::exit(1);
    }
}
