//! The overlay on macOS and Windows.
//!
//! A transparent, borderless window that floats above everything is the overlay. It stays on
//! screen and lets clicks through until the trigger key goes down while files are being dragged;
//! it then moves under the pointer and takes the drag, and the system's own drag and drop hands
//! it the files when the pointer moves over it. (Hidden instead, eframe would look at it a tenth
//! of a second late.) A thread reads the trigger key, the mouse button and the pointer, which
//! needs no permission, and wakes the app when something changes; where the system tells the app
//! as they change (an event tap on macOS, with Input Monitoring, and low-level hooks on Windows),
//! nothing is read while nothing happens. The menu bar or tray icon has Pause, Settings… and
//! Quit; the settings window is a process of its own, as on Linux, and the app reads the
//! settings file again when it changes.
//!
//! What differs is in `mac` and `windows`: reading and hearing the input, putting the window
//! under the pointer and letting clicks through it, and opening the settings.

use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, Pos2, Rect, Vec2};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::catalog::Mode;
use crate::i18n::tr;
use crate::overlay::{CardSpot, Overlay};
use crate::settings::{APP_ID, APP_NAME, Settings, Trigger};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as platform;
#[cfg(target_os = "macos")]
pub use mac::{activate, input_monitoring, keys_heard, regular_app, request_input_monitoring};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;
#[cfg(windows)]
pub(crate) use windows::{pick_folder, put_text};

use platform::NativeWindow;

/// The overlay window: the wheel, with room above it for the cards that stack up.
const WINDOW: Vec2 = Vec2::new(560.0, 700.0);
/// Where in the window the wheel sits, under the pointer (unless near a screen edge).
const WHEEL_AT: Vec2 = Vec2::new(280.0, 440.0);
/// How often the keys and the pointer are read while the wheel is up, and otherwise.
const POLL_FAST: Duration = Duration::from_millis(10);
const POLL_IDLE: Duration = Duration::from_millis(30);
/// How often the settings file is looked at.
const SETTINGS_EVERY: Duration = Duration::from_secs(1);
/// The button can be seen coming up before the drop that goes with it arrives: wait this long
/// for it before folding the wheel.
const DROP_WAIT: Duration = Duration::from_millis(300);
/// Letting go of the key and the button together is a drop: the key coming up first folds the
/// wheel only if the button is still down this much later.
const LET_GO: Duration = Duration::from_millis(80);
/// The key can go down before the drag has properly begun (the mouse hasn't moved far enough
/// yet): how long after it files are still looked for.
const LOOK_FOR_DRAG: Duration = Duration::from_secs(1);
/// Where the system can't tell beforehand what is dragged, the drag is taken on a guess: once
/// the pointer has moved this far over the window, files would have been offered…
const NOTHING_OFFERED_AFTER: f32 = 16.0;
/// …within this long, or it isn't files, and the drag goes through to what is below.
const NOTHING_OFFERED_WAIT: Duration = Duration::from_millis(300);

/// The keys, the button and the pointer, as the window needs them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Input {
    trigger_down: bool,
    button_down: bool,
    /// On the screen, as the platform places windows: in points from the top left of the main
    /// display on macOS, in pixels on Windows.
    pointer: Pos2,
}

fn lock_file() -> Option<PathBuf> {
    crate::settings::config_home().map(|dir| dir.join(APP_ID).join("app.lock"))
}

/// Where the settings window asks the running app to restart.
fn restart_file() -> Option<PathBuf> {
    crate::settings::config_home().map(|dir| dir.join(APP_ID).join("restart"))
}

/// Holds the app's lock: one app at a time. On macOS the file also says how the running app
/// hears the keys (see `keys_heard`), so opening it doesn't empty it.
fn claim() -> Result<Option<File>, ()> {
    let Some(path) = lock_file() else {
        return Ok(None);
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    else {
        return Ok(None);
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err(()),
        Err(_) => Ok(None),
    }
}

/// Whether the app is running: someone holds its lock.
pub fn running() -> bool {
    let Some(file) = lock_file().and_then(|p| File::options().write(true).open(p).ok()) else {
        return false;
    };
    matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// Asks the running app to start over, once nothing is converting; it looks every second.
pub fn ask_restart() -> bool {
    restart_file().is_some_and(|path| std::fs::write(path, "").is_ok())
}

fn settings_changed_at() -> Option<SystemTime> {
    let path = crate::settings::config_home()?
        .join(APP_ID)
        .join("settings.conf");
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// What the input thread hands the window.
#[derive(Default)]
struct Shared {
    input: Input,
    trigger: Option<Trigger>,
    /// The window is up, or a drag of files may be starting under the key, or a card is under
    /// the pointer: read the pointer often.
    tracking: bool,
    /// Cards are up: the pointer coming onto one has to be noticed.
    cards: bool,
    menu: Vec<MenuId>,
    settings_changed: bool,
    /// The settings window asked for a restart.
    restart: bool,
}

impl Shared {
    /// Takes in what the input is now; whether the window should look.
    fn heard(&mut self, input: Input) -> bool {
        let before = self.input;
        self.input = input;
        input.trigger_down != before.trigger_down
            || input.button_down != before.button_down
            || ((self.tracking || self.cards) && input.pointer != before.pointer)
    }
}

/// Reads the input and the settings file on a thread, and wakes the window when something it
/// cares about changed. When the system tells the app as the input changes (`heard`), the input
/// is read only while a drag is over the wheel, which a drag between apps may not tell (on
/// macOS; Windows' hooks hear it), and the thread otherwise sleeps until the window wakes it or
/// the settings file is due a look.
fn watch(shared: Arc<Mutex<Shared>>, ctx: egui::Context, heard: bool) -> std::thread::Thread {
    std::thread::Builder::new()
        .name("mcgyver input".into())
        .spawn(move || {
            let mut settings_at = settings_changed_at();
            let mut looked_at = Instant::now();
            loop {
                let (trigger, tracking) = {
                    let s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    (s.trigger.unwrap_or(Trigger::Shift), s.tracking)
                };
                let reading = !heard || (tracking && !platform::HEARS_DRAGS);
                let input = reading.then(|| platform::read_input(trigger));
                let mut wake = false;
                {
                    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(input) = input {
                        wake |= s.heard(input);
                    }
                    if looked_at.elapsed() >= SETTINGS_EVERY {
                        looked_at = Instant::now();
                        if restart_file().is_some_and(|path| path.exists()) {
                            s.restart = true;
                            wake = true;
                        }
                        let now = settings_changed_at();
                        if now != settings_at {
                            settings_at = now;
                            s.settings_changed = true;
                            wake = true;
                        }
                    }
                }
                if wake {
                    ctx.request_repaint();
                }
                if reading && tracking {
                    std::thread::sleep(POLL_FAST);
                } else if heard {
                    std::thread::park_timeout(SETTINGS_EVERY);
                } else {
                    std::thread::sleep(POLL_IDLE);
                }
            }
        })
        .expect("the input thread starts")
        .thread()
        .clone()
}

/// The menu bar or tray icon and its menu.
struct MenuBar {
    _icon: TrayIcon,
    /// Greyed out: the app's name and whether it is paused.
    status: MenuItem,
    pause: CheckMenuItem,
    settings: MenuItem,
    quit: MenuItem,
}

impl MenuBar {
    fn new() -> Option<MenuBar> {
        let status = MenuItem::new(crate::settings::status_line(false), false, None);
        let pause = CheckMenuItem::new(tr("Pause"), true, false, None);
        let settings = MenuItem::new(tr("Settings…"), true, None);
        let quit = MenuItem::new(tr("Quit"), true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
            &pause,
            &settings,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .ok()?;
        platform::widen(&menu);
        // The menu is all that's used; clicks and hovers on the icon would only pile up.
        tray_icon::TrayIconEvent::set_event_handler(Some(|_| {}));
        let size = platform::TRAY_ICON;
        let icon = tray_icon::Icon::from_rgba(crate::icon::rgba(size)?, size, size).ok()?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip(APP_NAME)
            .build()
            .map_err(|e| eprintln!("mcgyver: no menu bar icon: {e}"))
            .ok()?;
        Some(MenuBar {
            _icon: icon,
            status,
            pause,
            settings,
            quit,
        })
    }

    /// The menu in the current language, and whether the app is paused.
    fn retitle(&self, paused: bool) {
        self.status.set_text(crate::settings::status_line(paused));
        self.pause.set_text(tr("Pause"));
        self.settings.set_text(tr("Settings…"));
        self.quit.set_text(tr("Quit"));
    }
}

pub fn run() -> Result<(), String> {
    crate::settings::migrate();
    let lock = match claim() {
        Ok(lock) => lock,
        // Started again while running: show its settings, as on Linux.
        Err(()) => {
            platform::show_settings();
            return Ok(());
        }
    };
    let settings = Settings::load();
    crate::i18n::set(settings.language);
    platform::first_start();
    let viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_app_id(APP_ID)
        .with_inner_size(WINDOW)
        .with_decorations(false)
        .with_transparent(true)
        .with_has_shadow(false)
        .with_resizable(false)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_taskbar(false);
    let options = eframe::NativeOptions {
        viewport: platform::viewport(viewport),
        renderer: eframe::Renderer::Glow,
        multisampling: 4,
        event_loop_builder: platform::event_loop(),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            crate::glass::install_fonts(&cc.egui_ctx);
            let window = NativeWindow::new(cc);
            let shared = Arc::new(Mutex::new(Shared {
                input: platform::read_input(settings.trigger),
                trigger: Some(settings.trigger),
                ..Shared::default()
            }));
            let heard = platform::listen(shared.clone(), cc.egui_ctx.clone());
            let watcher = watch(shared.clone(), cc.egui_ctx.clone(), heard);
            // Picks from the menu come on the main thread; the window takes them.
            {
                let (shared, ctx) = (shared.clone(), cc.egui_ctx.clone());
                MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                    shared
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .menu
                        .push(event.id);
                    ctx.request_repaint();
                }));
            }
            if std::env::var_os("MCGYVER_DEBUG").is_some() {
                let how = if heard {
                    platform::HEARD
                } else {
                    platform::READ
                };
                eprintln!("keys and mouse: {how}");
            }
            Ok(Box::new(App {
                shared,
                watcher,
                window,
                overlay: Overlay::default(),
                clicker: crate::sound::Clicker::new(),
                menu: MenuBar::new(),
                lock,
                paused: false,
                quitting: false,
                active: false,
                held: false,
                released_at: None,
                let_go_at: None,
                guessed: None,
                last: Input::default(),
                pressed_at: None,
                on_card: false,
                frames: 0,
                debug: std::env::var_os("MCGYVER_DEBUG").map(|_| Debug {
                    start: Instant::now(),
                    logic_passes: 0,
                    ui_passes: 0,
                    hovered: 0,
                    reported_at: Instant::now(),
                }),
            }))
        }),
    )
    .map_err(|e| e.to_string())
}

struct App {
    shared: Arc<Mutex<Shared>>,
    /// The thread reading the input, woken when it has to read it often again.
    watcher: std::thread::Thread,
    window: NativeWindow,
    overlay: Overlay,
    clicker: crate::sound::Clicker,
    menu: Option<MenuBar>,
    /// One app at a time; let go of to restart.
    lock: Option<File>,
    paused: bool,
    /// Quit was picked; waits for running conversions.
    quitting: bool,
    /// The window is under the pointer and takes the drag; otherwise it lets everything through.
    active: bool,
    /// The trigger key went down during a drag and hasn't come up.
    held: bool,
    /// The mouse button came up over a live wheel; the drop may still be on its way.
    released_at: Option<Instant>,
    /// The key came up with the button still down; it may yet come up too (a drop).
    let_go_at: Option<Instant>,
    /// The drag was taken without knowing it carries files: where the pointer was then, when,
    /// and when it had moved on with nothing offered.
    guessed: Option<Guess>,
    last: Input,
    /// When the key went down during a drag, while files are still looked for.
    pressed_at: Option<Instant>,
    /// The window takes clicks for a card under the pointer (no drag going on).
    on_card: bool,
    /// Frames painted, up to the one after eframe showed the window.
    frames: u8,
    /// `MCGYVER_DEBUG` is set: say what happens on stderr.
    debug: Option<Debug>,
}

/// A drag taken on a guess, until files come over the window or it is let go.
#[derive(Clone, Copy)]
struct Guess {
    from: Pos2,
    at: Instant,
    moved_at: Option<Instant>,
}

/// What `MCGYVER_DEBUG` reports while a wheel is up: how often the app runs and where it sees
/// the pointer, so a wheel that doesn't follow the pointer can be told from one not redrawn.
struct Debug {
    start: Instant,
    logic_passes: u32,
    ui_passes: u32,
    hovered: usize,
    reported_at: Instant,
}

impl App {
    fn log(&self, said: impl std::fmt::Display) {
        if let Some(debug) = &self.debug {
            eprintln!("[{:>7} ms] {said}", debug.start.elapsed().as_millis());
        }
    }

    /// Moves the window under the pointer, inside the display, and has it take the drag.
    fn take_drag(&mut self, ctx: &egui::Context, pointer: Pos2) {
        if let Some(moved) = self.window.place(ctx, pointer) {
            // Cards still fading stay where they are on screen.
            self.overlay.shift(moved);
        }
        self.log(format!(
            "taking the drag: pointer {pointer:?}, {:?}",
            self.window
        ));
        self.window.let_through(ctx, false);
        self.active = true;
        self.on_card = false;
    }

    /// Lets clicks and drags through to whatever is below.
    fn let_through(&mut self, ctx: &egui::Context) {
        if self.active {
            self.window.let_through(ctx, true);
            self.active = false;
        }
    }

    fn menu_picked(&mut self, id: &MenuId) {
        let Some(menu) = &self.menu else {
            return;
        };
        if id == menu.pause.id() {
            // Kept here and shown on the item, whether or not the menu ticked it itself.
            self.paused = !self.paused;
            menu.pause.set_checked(self.paused);
            menu.retitle(self.paused);
            if self.paused {
                self.held = false;
                self.overlay.cancel(Instant::now());
            }
        } else if id == menu.settings.id() {
            platform::show_settings();
        } else if id == menu.quit.id() {
            self.quitting = true;
            self.overlay.cancel(Instant::now());
        }
    }

    fn reload(&mut self) {
        let settings = Settings::load();
        crate::i18n::set(settings.language);
        {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            s.trigger = Some(settings.trigger);
            // Hooks only hear the trigger's own keys go down and up: a new trigger is read once.
            s.input.trigger_down = platform::read_input(settings.trigger).trigger_down;
        }
        if let Some(menu) = &self.menu {
            menu.retitle(self.paused);
        }
    }

    /// Where the system can't tell beforehand that files are dragged: once the pointer has moved
    /// on over the window with nothing offered, or hasn't moved for a while, the key lets go of
    /// the drag.
    fn nothing_offered(&mut self, ctx: &egui::Context, pointer: Pos2, now: Instant) {
        let Some(guess) = self.guessed else {
            return;
        };
        let give_up_at = match guess.moved_at {
            Some(moved_at) => moved_at + NOTHING_OFFERED_WAIT,
            None => guess.at + LOOK_FOR_DRAG,
        };
        if !self.active || self.overlay.wants_input() {
            self.guessed = None;
        } else if now >= give_up_at {
            self.guessed = None;
            self.held = false;
            self.log("no files came over the window: the drag goes through");
        } else if guess.moved_at.is_none() && (pointer - guess.from).length() > NOTHING_OFFERED_AFTER
        {
            self.guessed = Some(Guess {
                moved_at: Some(now),
                ..guess
            });
            ctx.request_repaint_after(NOTHING_OFFERED_WAIT);
        } else {
            ctx.request_repaint_after(give_up_at - now);
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    /// The keys, the button, the menu and the settings.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        self.window.keep();
        let (input, picked, settings_changed, restart_asked) = {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            (
                s.input,
                std::mem::take(&mut s.menu),
                std::mem::take(&mut s.settings_changed),
                s.restart,
            )
        };
        // A restart cuts nothing off halfway: it waits for conversions and cards.
        if restart_asked && self.overlay.is_idle() {
            self.log("restarting, as the settings asked");
            let _ = restart_file().map(std::fs::remove_file);
            // Windows takes the tray icon away only when it is dropped.
            drop(self.menu.take());
            drop(self.lock.take());
            platform::restart();
        }
        for id in &picked {
            self.menu_picked(id);
        }
        if settings_changed {
            self.reload();
        }
        if let Some(debug) = &mut self.debug {
            debug.logic_passes += 1;
        }
        if input.trigger_down != self.last.trigger_down
            || input.button_down != self.last.button_down
        {
            self.log(format!(
                "key {}, button {}, pointer {:?}",
                if input.trigger_down { "down" } else { "up" },
                if input.button_down { "down" } else { "up" },
                input.pointer
            ));
        }
        if !input.button_down {
            // The drag is over; the next one needs the key pressed again.
            self.held = false;
            if self.last.button_down {
                self.window.button_up();
            }
        }

        // The key went down during a drag of files: take it under the pointer. Looked at again
        // as the pointer moves for a moment, in case the drag hadn't begun yet.
        // (Not a card being moved: Windows would take that for a drag.)
        if input.trigger_down
            && !self.last.trigger_down
            && input.button_down
            && !self.overlay.holding_card()
        {
            self.pressed_at = Some(now);
        }
        if !input.trigger_down || !input.button_down || self.held {
            self.pressed_at = None;
        }
        if let Some(at) = self.pressed_at
            && !self.paused
        {
            if now >= at + LOOK_FOR_DRAG {
                self.pressed_at = None;
            } else if self.window.dragging_files() {
                self.pressed_at = None;
                self.held = true;
                self.let_go_at = None;
                self.take_drag(ctx, input.pointer);
                if !platform::SEES_DRAGGED_FILES {
                    self.guessed = Some(Guess {
                        from: input.pointer,
                        at: now,
                        moved_at: None,
                    });
                }
            }
        }
        self.nothing_offered(ctx, input.pointer, now);
        // The button before the key: the two coming up together is a drop, not a cancel.
        if !input.button_down && self.overlay.wants_input() && self.released_at.is_none() {
            self.released_at = Some(now);
            self.let_go_at = None;
        }
        if !input.trigger_down && self.last.trigger_down && self.held {
            self.held = false;
            if input.button_down && self.overlay.wants_input() {
                self.let_go_at = Some(now);
            }
        }
        // Let go of the key alone: the wheel folds and the drag carries on below it.
        if let Some(at) = self.let_go_at {
            if !self.overlay.wants_input() {
                self.let_go_at = None;
            } else if now >= at + LET_GO {
                self.log("key let go during the drag: wheel folded");
                self.let_go_at = None;
                self.overlay.cancel(now);
                self.let_through(ctx);
            } else {
                ctx.request_repaint_after(at + LET_GO - now);
            }
        }
        // The drag ended without a drop on the wheel.
        if let Some(at) = self.released_at {
            if !self.overlay.wants_input() {
                self.released_at = None;
            } else if now >= at + DROP_WAIT {
                self.log("button up, but no drop came: wheel folded");
                self.released_at = None;
                self.overlay.cancel(now);
            } else {
                ctx.request_repaint_after(at + DROP_WAIT - now);
            }
        }
        // Nothing to take: no wheel, and the key or the button is up.
        if self.active && !self.overlay.wants_input() && (!self.held || !input.button_down) {
            self.let_through(ctx);
        }
        // No drag: the window takes clicks only where a card is, to move or close it.
        if !self.active {
            let over = self.window.placed()
                && (self.overlay.holding_card()
                    || self
                        .overlay
                        .card_spot(self.window.local(input.pointer))
                        .is_some());
            if over != self.on_card {
                self.window.let_through(ctx, !over);
                self.on_card = over;
            }
        }
        self.last = input;

        if self.quitting && self.overlay.is_idle() {
            drop(self.menu.take());
            std::process::exit(0);
        }
        let tracking = self.active || self.pressed_at.is_some() || self.on_card;
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tracking = tracking;
        if tracking {
            self.watcher.unpark();
        }
        let cards = !self.overlay.card_rects().is_empty();
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).cards = cards;
    }

    /// The drag, the drop, and the wheel and cards.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        // eframe shows the window after the first frame, which can make it the active one:
        // hand the keyboard back to the app that had it.
        if self.frames < 2 {
            self.frames += 1;
            if self.frames == 2 {
                self.window.give_back_focus();
            } else {
                ctx.request_repaint();
            }
        }
        let local = self.window.local(self.last.pointer);
        self.report(ui, local, now);
        let (hovered, dropped) = ui.input(|i| {
            let hovered: Vec<PathBuf> = i
                .raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect();
            (hovered, !i.raw.dropped_files.is_empty())
        });
        if !hovered.is_empty() {
            self.guessed = None;
        }
        if self.active
            && self.held
            && self.last.button_down
            && !hovered.is_empty()
            && !self.overlay.wants_input()
        {
            let window = Rect::from_min_size(Pos2::ZERO, WINDOW);
            self.log(format!("wheel opened at {local:?} in the window"));
            self.overlay
                .open(hovered, local, window, Mode::Convert, now);
        }
        if self.overlay.wants_input() {
            self.overlay.drag_moved(local, now);
        }
        if dropped && self.overlay.wants_input() {
            let picked = self.overlay.drop(now);
            self.log(format!("dropped at {local:?}, picked: {picked}"));
            self.released_at = None;
            self.let_go_at = None;
            // The card that follows lets clicks through.
            self.let_through(&ctx);
        }

        if !self.active && !self.overlay.wants_input() {
            self.cards(ui, local, now);
        }

        if self.overlay.take_click() {
            self.clicker.play();
        }
        if let Some(wait) = self.overlay.paint(ui, now) {
            ctx.request_repaint_after(wait);
        }
    }
}

impl App {
    /// With `MCGYVER_DEBUG`: the dragged files coming and going, and while a wheel is up, every
    /// quarter second, how many passes ran and where the pointer is in the window.
    fn report(&mut self, ui: &egui::Ui, local: Pos2, now: Instant) {
        let live = self.overlay.wants_input();
        let Some(debug) = &mut self.debug else {
            return;
        };
        debug.ui_passes += 1;
        let (hovered, viewport) = ui.input(|i| {
            let v = i.viewport();
            (
                i.raw.hovered_files.len(),
                format!(
                    "focused {:?}, occluded {:?}, minimized {:?}",
                    v.focused, v.occluded, v.minimized
                ),
            )
        });
        let mut said = Vec::new();
        if hovered != debug.hovered {
            said.push(format!(
                "{hovered} file(s) dragged over the window ({viewport})"
            ));
            debug.hovered = hovered;
        }
        if live && now >= debug.reported_at + Duration::from_millis(250) {
            said.push(format!(
                "wheel up: {} logic and {} ui passes since last, pointer at {local:?} in the window",
                debug.logic_passes, debug.ui_passes
            ));
            debug.logic_passes = 0;
            debug.ui_passes = 0;
            debug.reported_at = now;
        }
        for said in said {
            self.log(said);
        }
    }

    /// A card under the pointer: holding it moves it, its close button cancels or puts it away.
    fn cards(&mut self, ui: &egui::Ui, local: Pos2, now: Instant) {
        self.overlay.card_hover(self.on_card.then_some(local));
        if !self.on_card {
            return;
        }
        let (pressed, released) =
            ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released()));
        if pressed {
            self.overlay.card_press(local);
        }
        if released {
            self.overlay.card_release(local, now);
            // The click may have made this app the active one; give the keyboard back.
            self.window.give_back_focus();
        }
        ui.ctx().set_cursor_icon(if self.overlay.holding_card() {
            egui::CursorIcon::Grabbing
        } else {
            match self.overlay.card_spot(local) {
                Some(CardSpot::Close) => egui::CursorIcon::PointingHand,
                Some(CardSpot::Body) => egui::CursorIcon::Grab,
                None => egui::CursorIcon::Default,
            }
        });
    }
}
