//! macOS backend.
//!
//! A transparent, borderless window that floats above everything, on every Space, is the
//! overlay. It stays on screen and lets clicks through until the trigger key goes down while
//! files are being dragged; it then moves under the pointer and takes the drag, and macOS's own
//! drag and drop hands it the files when the pointer moves over it. (Hidden instead, eframe would
//! look at it a tenth of a second late.) A thread reads the modifier keys, the mouse button and
//! the pointer from CoreGraphics, which needs no permission for their state, and wakes the app
//! when something changes. With Input Monitoring allowed, an event tap hears the keys and the
//! mouse instead, and nothing is read while nothing happens; the app asks for it on first start
//! and reads the state until it has it. The menu bar icon has Pause, Settings… and Quit; the
//! settings window is a process of its own, as on Linux, and the app reads the settings file
//! again when it changes.

use std::ffi::{CString, c_void};
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, Pos2, Rect, Vec2, ViewportCommand, pos2, vec2};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{Encode, msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSView, NSWindow};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::catalog::Mode;
use crate::i18n::tr;
use crate::overlay::{CardSpot, Overlay};
use crate::settings::{APP_ID, APP_NAME, Settings, Trigger};

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

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

// kCGEventSourceStateCombinedSessionState, kCGMouseButtonLeft.
const COMBINED_SESSION: i32 = 0;
const LEFT_BUTTON: u32 = 0;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGEventCreate(source: *const c_void) -> *mut c_void;
    fn CGEventGetLocation(event: *const c_void) -> CGPoint;
    fn CGGetDisplaysWithPoint(
        point: CGPoint,
        max_displays: u32,
        displays: *mut u32,
        count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetFlags(event: *const c_void) -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: *const c_void;
}

type TapCallback = unsafe extern "C" fn(
    proxy: *mut c_void,
    kind: u32,
    event: *mut c_void,
    info: *mut c_void,
) -> *mut c_void;

// kCGSessionEventTap, kCGHeadInsertEventTap, kCGEventTapOptionListenOnly.
const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
const LISTEN_ONLY: u32 = 1;
// CGEventType: the mouse's left button, its moves, and the modifier keys.
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const MOUSE_MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const FLAGS_CHANGED: u32 = 12;
// The system turns a tap off when it answers too slowly, or when the user is typing a password.
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

/// The modifier flag (`kCGEventFlagMask…`) for each trigger key.
fn flag(trigger: Trigger) -> u64 {
    match trigger {
        Trigger::Shift => 0x0002_0000,
        Trigger::Ctrl => 0x0004_0000,
        Trigger::Alt => 0x0008_0000,
        Trigger::Cmd => 0x0010_0000,
    }
}

/// The keys, the button and the pointer, as the window needs them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Input {
    trigger_down: bool,
    button_down: bool,
    /// In points, from the top left of the main display, as window positions are.
    pointer: Pos2,
}

fn read_input(trigger: Trigger) -> Input {
    // SAFETY: these CoreGraphics calls only read the session's state; the event that
    // carries the pointer location is released here.
    unsafe {
        let flags = CGEventSourceFlagsState(COMBINED_SESSION);
        let button_down = CGEventSourceButtonState(COMBINED_SESSION, LEFT_BUTTON);
        let event = CGEventCreate(std::ptr::null());
        let pointer = if event.is_null() {
            Pos2::ZERO
        } else {
            let at = CGEventGetLocation(event);
            CFRelease(event);
            pos2(at.x as f32, at.y as f32)
        };
        Input {
            trigger_down: flags & flag(trigger) != 0,
            button_down,
            pointer,
        }
    }
}

/// The display under `pointer`, in the same points as the pointer.
fn display_at(pointer: Pos2) -> Option<Rect> {
    let point = CGPoint {
        x: pointer.x.into(),
        y: pointer.y.into(),
    };
    let (mut display, mut count) = (0u32, 0u32);
    // SAFETY: room for one display ID is passed, and one at most is asked for.
    let bounds = unsafe {
        if CGGetDisplaysWithPoint(point, 1, &mut display, &mut count) != 0 || count == 0 {
            return None;
        }
        CGDisplayBounds(display)
    };
    Some(Rect::from_min_size(
        pos2(bounds.origin.x as f32, bounds.origin.y as f32),
        vec2(bounds.size.width as f32, bounds.size.height as f32),
    ))
}

/// Whether the app may read input while another app is in front (Input Monitoring).
pub fn input_monitoring() -> bool {
    // SAFETY: no arguments; it only reports the app's permission.
    unsafe { CGPreflightListenEventAccess() }
}

/// Asks for Input Monitoring: macOS shows its prompt the first time only.
pub fn request_input_monitoring() -> bool {
    // SAFETY: no arguments.
    unsafe { CGRequestListenEventAccess() }
}

/// Makes this app the active one, so its window comes forward.
pub fn activate() {
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

/// A Dock icon and an app menu, for the settings window: the bundle starts as a menu bar app.
pub fn regular_app() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Regular);
    }
}

/// The drag pasteboard's change count: it moves on when a drag starts.
fn drag_count(_main_thread: MainThreadMarker) -> isize {
    use objc2_app_kit::{NSPasteboard, NSPasteboardNameDrag};
    // SAFETY: a constant AppKit defines.
    NSPasteboard::pasteboardWithName(unsafe { NSPasteboardNameDrag }).changeCount()
}

/// Whether the drag pasteboard holds files (winit reads them as the filenames AppKit makes of
/// file URLs). Only its types are read, which macOS lets an app do without asking.
fn drag_has_files(_main_thread: MainThreadMarker) -> bool {
    use objc2_app_kit::{NSPasteboard, NSPasteboardNameDrag, NSPasteboardTypeFileURL};
    // SAFETY: constants AppKit defines.
    let (name, file_url) = unsafe { (NSPasteboardNameDrag, NSPasteboardTypeFileURL) };
    NSPasteboard::pasteboardWithName(name)
        .types()
        .is_some_and(|types| types.iter().any(|t| &*t == file_url))
}

/// Adds a method to a class winit defines, which it lacks.
///
/// # Safety
/// `imp` must be a function with the signature `types` describes.
unsafe fn add_method(class: &str, name: Sel, imp: Imp, types: &str) {
    let (Ok(class), Ok(types)) = (CString::new(class), CString::new(types)) else {
        return;
    };
    let Some(class) = AnyClass::get(&class) else {
        eprintln!("mcgyver: no {class:?} to teach");
        return;
    };
    // SAFETY: the caller vouches for `imp` and `types`; the class is registered already.
    unsafe {
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            name,
            imp,
            types.as_ptr(),
        );
    }
}

/// `-[WinitWindowDelegate draggingUpdated:]`. winit's `draggingEntered:` always answers Copy,
/// which macOS refuses while Control (link) or Command (move) is held, the very keys that may
/// be the trigger. This answers with an operation the drag allows, Copy first.
unsafe extern "C-unwind" fn dragging_updated(
    _this: &AnyObject,
    _cmd: Sel,
    sender: &AnyObject,
) -> usize {
    // SAFETY: `sender` is the NSDraggingInfo AppKit passes.
    let allowed: usize = unsafe { msg_send![sender, draggingSourceOperationMask] };
    // NSDragOperationCopy, Generic, Link, Move.
    [1, 4, 2, 16]
        .into_iter()
        .find(|op| allowed & op != 0)
        .unwrap_or(0)
}

/// `-[WinitApplicationDelegate applicationShouldHandleReopen:hasVisibleWindows:]`: the app,
/// opened again from Finder while it runs, shows its settings, as a second start does.
unsafe extern "C-unwind" fn reopened(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
    _visible: Bool,
) -> Bool {
    show_settings();
    Bool::NO
}

/// What AppKit asks of the app that winit doesn't answer.
fn teach_appkit() {
    let object = <*mut AnyObject>::ENCODING;
    // SAFETY: the signatures match the type strings: `NSDragOperation (id, SEL, id)` and
    // `BOOL (id, SEL, id, BOOL)`.
    unsafe {
        add_method(
            "WinitWindowDelegate",
            sel!(draggingUpdated:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> usize,
                Imp,
            >(dragging_updated),
            &format!("{}{object}{}{object}", usize::ENCODING, Sel::ENCODING),
        );
        add_method(
            "WinitApplicationDelegate",
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, Bool) -> Bool,
                Imp,
            >(reopened),
            &format!(
                "{}{object}{}{object}{}",
                Bool::ENCODING,
                Sel::ENCODING,
                Bool::ENCODING
            ),
        );
    }
}

/// Copy on the Actions wheel puts the files on the pasteboard as Finder's Copy does. Finder has
/// no Cut for files (Option-Command-V moves what was copied), so neither has the wheel.
fn serve_files() {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};
    crate::actions::serve_files_with(crate::actions::FileClipboard {
        put: Box::new(|files, _cut| {
            let urls = files
                .iter()
                .map(|file| {
                    let path = file.canonicalize()?;
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                    Ok(ProtocolObject::<dyn NSPasteboardWriting>::from_retained(
                        url,
                    ))
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            let board = NSPasteboard::generalPasteboard();
            board.clearContents();
            if board.writeObjects(&NSArray::from_retained_slice(&urls)) {
                Ok(())
            } else {
                Err(std::io::Error::other("couldn't reach the clipboard"))
            }
        }),
        can_cut: false,
    });
}

/// The overlay's NSWindow, from the view eframe draws in.
fn overlay_window(cc: &eframe::CreationContext) -> Option<Retained<NSWindow>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let RawWindowHandle::AppKit(handle) = cc.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: an AppKit window handle's view is an NSView, alive as long as the window.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

/// The overlay goes with the pointer to every Space, full-screen apps' too, and floats above
/// the menu bar and the Dock.
fn float_everywhere(window: &NSWindow) {
    use objc2_app_kit::{NSStatusWindowLevel, NSWindowCollectionBehavior as B};
    window.setCollectionBehavior(
        B::CanJoinAllSpaces | B::FullScreenAuxiliary | B::Stationary | B::IgnoresCycle,
    );
    window.setLevel(NSStatusWindowLevel);
}

/// Sets the window's and the app's delegates again, so AppKit sees the methods
/// [`teach_appkit`] added even if it noted what they answer when they were set.
fn reintroduce_delegates(mtm: MainThreadMarker, window: &NSWindow) {
    window.setDelegate(window.delegate().as_deref());
    let app = NSApplication::sharedApplication(mtm);
    app.setDelegate(app.delegate().as_deref());
}

/// What the event tap's callback works with; it lives as long as the app.
struct Tap {
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    /// The tap itself, to turn it back on when the system turns it off.
    port: AtomicPtr<c_void>,
}

/// Hears the keys and the mouse through an event tap that only listens, on a thread of its own,
/// so nothing is read while nothing happens. Needs Input Monitoring; false without it, or if
/// the tap can't be made.
fn listen(shared: Arc<Mutex<Shared>>, ctx: egui::Context) -> bool {
    if !input_monitoring() {
        return false;
    }
    let (made, told) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("mcgyver keys".into())
        .spawn(move || {
            let tap: &'static Tap = Box::leak(Box::new(Tap {
                shared,
                ctx,
                port: AtomicPtr::new(std::ptr::null_mut()),
            }));
            let events = [LEFT_DOWN, LEFT_UP, MOUSE_MOVED, LEFT_DRAGGED, FLAGS_CHANGED]
                .iter()
                .fold(0u64, |mask, kind| mask | 1 << kind);
            // SAFETY: `heard` matches the callback type, and `tap` outlives the tap.
            let port = unsafe {
                CGEventTapCreate(
                    SESSION_TAP,
                    HEAD_INSERT,
                    LISTEN_ONLY,
                    events,
                    heard,
                    tap as *const Tap as *mut c_void,
                )
            };
            if port.is_null() {
                let _ = made.send(false);
                return;
            }
            tap.port.store(port, Ordering::Release);
            // SAFETY: the port is a live event tap; its source runs on this thread's run loop,
            // which runs from here on.
            unsafe {
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(port, true);
            }
            let _ = made.send(true);
            // SAFETY: runs this thread's run loop, for good.
            unsafe { CFRunLoopRun() };
        });
    spawned.is_ok() && told.recv().unwrap_or(false)
}

/// The tap's callback: a key, the button or the pointer changed. Wakes the window when that
/// matters to it.
unsafe extern "C" fn heard(
    _proxy: *mut c_void,
    kind: u32,
    event: *mut c_void,
    info: *mut c_void,
) -> *mut c_void {
    // SAFETY: `info` is the `Tap` given to `CGEventTapCreate`, which is never freed.
    let tap = unsafe { &*(info as *const Tap) };
    if kind == TAP_DISABLED_BY_TIMEOUT || kind == TAP_DISABLED_BY_USER_INPUT {
        let port = tap.port.load(Ordering::Acquire);
        if !port.is_null() {
            // SAFETY: the tap made in `listen`.
            unsafe { CGEventTapEnable(port, true) };
        }
        return event;
    }
    // SAFETY: AppKit hands a live event for the kinds asked for.
    let (flags, at) = unsafe { (CGEventGetFlags(event), CGEventGetLocation(event)) };
    let wake = {
        let mut s = tap.shared.lock().unwrap_or_else(|e| e.into_inner());
        let before = s.input;
        let mut input = before;
        input.trigger_down = flags & flag(s.trigger.unwrap_or(Trigger::Shift)) != 0;
        match kind {
            LEFT_DOWN | LEFT_DRAGGED => input.button_down = true,
            LEFT_UP => input.button_down = false,
            _ => {}
        }
        if kind != FLAGS_CHANGED {
            input.pointer = pos2(at.x as f32, at.y as f32);
        }
        s.input = input;
        input.trigger_down != before.trigger_down
            || input.button_down != before.button_down
            || ((s.tracking || s.cards) && input.pointer != before.pointer)
    };
    if wake {
        tap.ctx.request_repaint();
    }
    event
}

fn lock_file() -> Option<PathBuf> {
    crate::settings::config_home().map(|dir| dir.join(APP_ID).join("app.lock"))
}

/// Where the settings window asks the running app to restart.
fn restart_file() -> Option<PathBuf> {
    crate::settings::config_home().map(|dir| dir.join(APP_ID).join("restart"))
}

/// Holds the app's lock: one app at a time. The file also says how the running app hears the
/// keys (see `keys_heard`), so opening it doesn't empty it.
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

/// Writes in the lock file whether this app hears the keys through the event tap.
fn say_keys_heard(tapped: bool) {
    if let Some(path) = lock_file() {
        let _ = std::fs::write(path, if tapped { "tap\n" } else { "poll\n" });
    }
}

/// How the running app hears the keys: `Some(true)` through the event tap, `Some(false)` by
/// reading them (Input Monitoring wasn't allowed when it started), `None` if it isn't running.
pub fn keys_heard() -> Option<bool> {
    if !running() {
        return None;
    }
    let said = std::fs::read_to_string(lock_file()?).ok()?;
    match said.trim() {
        "tap" => Some(true),
        "poll" => Some(false),
        _ => None,
    }
}

/// Asks the running app to start over, once nothing is converting; it looks every second.
pub fn ask_restart() -> bool {
    restart_file().is_some_and(|path| std::fs::write(path, "").is_ok())
}

/// Starts this program over in the same process, as the Linux app does after `--install`.
fn restart() -> ! {
    use std::os::unix::process::CommandExt;
    let _ = restart_file().map(std::fs::remove_file);
    match crate::settings::own_exe() {
        Ok(exe) => {
            let e = std::process::Command::new(exe).exec();
            eprintln!("mcgyver: can't restart: {e}");
        }
        Err(e) => eprintln!("mcgyver: can't restart: {e}"),
    }
    std::process::exit(1)
}

/// The McGyver.app this binary is in, if it is in one.
fn bundle() -> Option<PathBuf> {
    let exe = crate::settings::own_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_owned())
}

/// Opens the settings window, a process of its own that outlives this one. From the bundle,
/// through Launch Services, which lets a new app come to the front.
fn show_settings() {
    let mut command = match bundle() {
        Some(app) => {
            let mut open = std::process::Command::new("open");
            open.arg("-n").arg(app).args(["--args", "--settings"]);
            open
        }
        None => {
            let Ok(exe) = crate::settings::own_exe() else {
                return;
            };
            let mut sh = std::process::Command::new("sh");
            sh.args(["-c", "\"$0\" --settings </dev/null >/dev/null 2>&1 &"])
                .arg(exe);
            sh
        }
    };
    let _ = command.status();
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

/// Reads the input and the settings file on a thread, and wakes the window when something it
/// cares about changed. With the event tap (`heard`), the input is read only while a drag is
/// over the wheel, which a drag between apps may not tell the tap about, and the thread
/// otherwise sleeps until the window wakes it or the settings file is due a look.
fn watch(shared: Arc<Mutex<Shared>>, ctx: egui::Context, tapped: bool) -> std::thread::Thread {
    std::thread::Builder::new()
        .name("mcgyver input".into())
        .spawn(move || {
            let mut settings_at = settings_changed_at();
            let mut looked_at = Instant::now();
            loop {
                let (trigger, tracking, cards) = {
                    let s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    (s.trigger.unwrap_or(Trigger::Shift), s.tracking, s.cards)
                };
                let reading = !tapped || tracking;
                let input = reading.then(|| read_input(trigger));
                let mut wake = false;
                {
                    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(input) = input {
                        let before = s.input;
                        wake |= input.trigger_down != before.trigger_down
                            || input.button_down != before.button_down
                            || ((tracking || cards) && input.pointer != before.pointer);
                        s.input = input;
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
                if tracking {
                    std::thread::sleep(POLL_FAST);
                } else if tapped {
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

/// The menu bar icon and its menu.
struct MenuBar {
    _icon: TrayIcon,
    /// Greyed out: the app's name and whether it is paused.
    status: MenuItem,
    pause: CheckMenuItem,
    settings: MenuItem,
    quit: MenuItem,
}

/// The menu is at least this wide, in points, whichever line is longest.
const MENU_WIDTH: f64 = 180.0;

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
        {
            use tray_icon::menu::ContextMenu;
            // SAFETY: the menu's own NSMenu, alive as long as the menu, on the main thread.
            let ns_menu = unsafe { &*menu.ns_menu().cast::<objc2_app_kit::NSMenu>() };
            ns_menu.setMinimumWidth(MENU_WIDTH);
        }
        // The menu is all that's used; clicks and hovers on the icon would only pile up.
        tray_icon::TrayIconEvent::set_event_handler(Some(|_| {}));
        // 22 points high in the menu bar, drawn for Retina.
        let size = 44;
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
    let _lock = match claim() {
        Ok(lock) => lock,
        // Started again while running: show its settings, as on Linux.
        Err(()) => {
            show_settings();
            return Ok(());
        }
    };
    let settings = Settings::load();
    crate::i18n::set(settings.language);
    // The first time, macOS asks whether McGyver may hear the keys; until it may, it reads them.
    if !input_monitoring() {
        request_input_monitoring();
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id(APP_ID)
            .with_inner_size(WINDOW)
            .with_decorations(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_resizable(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_mouse_passthrough(true),
        renderer: eframe::Renderer::Glow,
        multisampling: 4,
        // A menu bar app (also as a bare binary) that doesn't take the focus when it starts.
        event_loop_builder: Some(Box::new(|builder| {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder
                .with_activation_policy(ActivationPolicy::Accessory)
                .with_activate_ignoring_other_apps(false)
                .with_default_menu(false);
        })),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            crate::glass::install_fonts(&cc.egui_ctx);
            teach_appkit();
            serve_files();
            if let (Some(mtm), Some(window)) = (MainThreadMarker::new(), overlay_window(cc)) {
                float_everywhere(&window);
                reintroduce_delegates(mtm, &window);
            }
            let shared = Arc::new(Mutex::new(Shared {
                input: read_input(settings.trigger),
                trigger: Some(settings.trigger),
                ..Shared::default()
            }));
            let tapped = listen(shared.clone(), cc.egui_ctx.clone());
            say_keys_heard(tapped);
            let watcher = watch(shared.clone(), cc.egui_ctx.clone(), tapped);
            // Picks from the menu bar menu come on the main thread; the window takes them.
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
                eprintln!(
                    "keys and mouse: {}",
                    if tapped {
                        "heard through an event tap"
                    } else {
                        "read every 30 ms (Input Monitoring isn't allowed)"
                    }
                );
            }
            Ok(Box::new(App {
                shared,
                watcher,
                overlay: Overlay::default(),
                clicker: crate::sound::Clicker::new(),
                menu: MenuBar::new(),
                paused: false,
                quitting: false,
                active: false,
                held: false,
                released_at: None,
                let_go_at: None,
                drag_seen: MainThreadMarker::new().map_or(0, drag_count),
                origin: None,
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
    overlay: Overlay,
    clicker: crate::sound::Clicker,
    menu: Option<MenuBar>,
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
    /// The drag pasteboard's change count when the button last came up: a new drag moves it on.
    drag_seen: isize,
    /// The window's top left on screen, once it has been put somewhere.
    origin: Option<Pos2>,
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
        let mut origin = pointer - WHEEL_AT;
        let display = display_at(pointer);
        if let Some(display) = display {
            let max = (display.max - WINDOW).max(display.min);
            origin = origin.clamp(display.min, max);
        }
        self.log(format!(
            "taking the drag: pointer {pointer:?}, window at {origin:?}, display {display:?}"
        ));
        if self.origin != Some(origin) {
            if let Some(before) = self.origin {
                // Cards still fading stay where they are on screen.
                self.overlay.shift(before - origin);
            }
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(origin));
            self.origin = Some(origin);
        }
        ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(false));
        self.active = true;
        self.on_card = false;
    }

    /// Lets clicks and drags through to whatever is below.
    fn let_through(&mut self, ctx: &egui::Context) {
        if self.active {
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(true));
            self.active = false;
        }
    }

    /// The pointer in the window's points.
    fn local(&self, pointer: Pos2) -> Pos2 {
        pointer - self.origin.unwrap_or_default().to_vec2()
    }

    /// Whether files are being dragged: the drag pasteboard changed since the button last came
    /// up, and holds files. Shift held while dragging a shape or selecting text isn't one.
    fn dragging_files(&self) -> bool {
        MainThreadMarker::new()
            .is_some_and(|mtm| drag_count(mtm) != self.drag_seen && drag_has_files(mtm))
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
            show_settings();
        } else if id == menu.quit.id() {
            self.quitting = true;
            self.overlay.cancel(Instant::now());
        }
    }

    fn reload(&mut self) {
        let settings = Settings::load();
        crate::i18n::set(settings.language);
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .trigger = Some(settings.trigger);
        if let Some(menu) = &self.menu {
            menu.retitle(self.paused);
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
            restart();
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
            if self.last.button_down
                && let Some(mtm) = MainThreadMarker::new()
            {
                self.drag_seen = drag_count(mtm);
            }
        }

        // The key went down during a drag of files: take it under the pointer. Looked at again
        // as the pointer moves for a moment, in case the drag hadn't begun yet.
        if input.trigger_down && !self.last.trigger_down && input.button_down {
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
            } else if self.dragging_files() {
                self.pressed_at = None;
                self.held = true;
                self.let_go_at = None;
                self.take_drag(ctx, input.pointer);
            }
        }
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
            let over = self.origin.is_some()
                && (self.overlay.holding_card()
                    || self.overlay.card_spot(self.local(input.pointer)).is_some());
            if over != self.on_card {
                ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(!over));
                self.on_card = over;
            }
        }
        self.last = input;

        if self.quitting && self.overlay.is_idle() {
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
        // eframe shows the window after the first frame, which makes it the key window if the
        // app was opened from Finder: hand the keyboard back to the app that had it.
        if self.frames < 2 {
            self.frames += 1;
            if self.frames == 2 {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).deactivate();
                }
            } else {
                ctx.request_repaint();
            }
        }
        let local = self.local(self.last.pointer);
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
            // The click made this app the active one; give the keyboard back.
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).deactivate();
            }
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
