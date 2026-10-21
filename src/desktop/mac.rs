//! The overlay on macOS: an always-visible window that floats above everything, on every Space,
//! and lets clicks through until it takes a drag (see the parent module).
//!
//! CoreGraphics gives the modifier keys, the mouse button and the pointer without asking. With
//! Input Monitoring allowed, an event tap hears them instead, and nothing is read while nothing
//! happens; the app asks for it on first start and reads the state until it has it. Before the
//! key opens the wheel, the drag pasteboard tells whether files are dragged.

use std::ffi::{CString, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use eframe::egui::{self, Pos2, Rect, Vec2, ViewportCommand, pos2, vec2};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{Encode, msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSView, NSWindow};
use tray_icon::menu::Menu;

use super::{Input, Shared, WHEEL_AT, WINDOW};
use crate::settings::Trigger;

/// macOS shows what a drag carries before it reaches the window (the drag pasteboard).
pub(super) const SEES_DRAGGED_FILES: bool = true;
/// The event tap may not hear a drag that comes from another app: while one is over the wheel,
/// the input is read too.
pub(super) const HEARS_DRAGS: bool = false;
/// The menu bar icon's size in pixels: 22 points high, drawn for Retina.
pub(super) const TRAY_ICON: u32 = 44;
/// How the keys are known, for `MCGYVER_DEBUG`.
pub(super) const HEARD: &str = "heard through an event tap";
pub(super) const READ: &str = "read every 30 ms (Input Monitoring isn't allowed)";
/// The menu is at least this wide, in points, whichever line is longest.
const MENU_WIDTH: f64 = 180.0;

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
    fn CGEventSourceCounterForEventType(state: i32, kind: u32) -> u32;
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
const KEY_DOWN: u32 = 10;
const RIGHT_DOWN: u32 = 3;
const OTHER_DOWN: u32 = 25;
const SCROLL: u32 = 22;
/// Every modifier's flag, for telling whether another one is held with the trigger.
const MODIFIERS: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;
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

pub(super) fn read_input(trigger: Trigger) -> Input {
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
            // Read while the key is held (see `tracking`), as the tap doesn't listen to keys.
            keys: keys_now(&Input::default()),
        }
    }
}

/// How many keys, other buttons and scrolls there have been in the session, read now: what was
/// read with the input may be from before the trigger went down.
pub(super) fn keys_now(_input: &Input) -> u32 {
    // SAFETY: only reads the session's counts.
    unsafe {
        [KEY_DOWN, RIGHT_DOWN, OTHER_DOWN, SCROLL]
            .into_iter()
            .map(|kind| CGEventSourceCounterForEventType(COMBINED_SESSION, kind))
            .fold(0u32, u32::wrapping_add)
    }
}

/// Whether a modifier other than the trigger is held.
pub(super) fn other_modifiers(trigger: Trigger) -> bool {
    // SAFETY: only reads the session's state.
    let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION) };
    flags & MODIFIERS & !flag(trigger) != 0
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

/// Hears the keys and the mouse through an event tap, and says so in the lock file for the
/// settings window. False without Input Monitoring, or if the tap can't be made.
pub(super) fn listen(shared: Arc<Mutex<Shared>>, ctx: egui::Context) -> bool {
    let tapped = tap_input(shared, ctx);
    say_keys_heard(tapped);
    tapped
}

/// Hears the keys and the mouse through an event tap that only listens, on a thread of its own,
/// so nothing is read while nothing happens. Needs Input Monitoring.
fn tap_input(shared: Arc<Mutex<Shared>>, ctx: egui::Context) -> bool {
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
        let mut input = s.input;
        input.trigger_down = flags & flag(s.trigger.unwrap_or(Trigger::Shift)) != 0;
        match kind {
            LEFT_DOWN | LEFT_DRAGGED => input.button_down = true,
            LEFT_UP => input.button_down = false,
            _ => {}
        }
        if kind != FLAGS_CHANGED {
            input.pointer = pos2(at.x as f32, at.y as f32);
        }
        s.heard(input)
    };
    if wake {
        tap.ctx.request_repaint();
    }
    event
}

/// Writes in the lock file whether this app hears the keys through the event tap.
fn say_keys_heard(tapped: bool) {
    if let Some(path) = super::lock_file() {
        let _ = std::fs::write(path, if tapped { "tap\n" } else { "poll\n" });
    }
}

/// How the running app hears the keys: `Some(true)` through the event tap, `Some(false)` by
/// reading them (Input Monitoring wasn't allowed when it started), `None` if it isn't running.
pub fn keys_heard() -> Option<bool> {
    if !super::running() {
        return None;
    }
    let said = std::fs::read_to_string(super::lock_file()?).ok()?;
    match said.trim() {
        "tap" => Some(true),
        "poll" => Some(false),
        _ => None,
    }
}

/// Starts this program over in the same process, as the Linux app does after `--install`.
pub(super) fn restart() -> ! {
    use std::os::unix::process::CommandExt;
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
pub(super) fn show_settings() {
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

/// The first time, macOS asks whether McGyver may hear the keys; until it may, it reads them.
pub(super) fn first_start() {
    if !input_monitoring() {
        request_input_monitoring();
    }
}

/// The window lets clicks through from the start.
pub(super) fn viewport(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    viewport.with_mouse_passthrough(true)
}

/// A menu bar app (also as a bare binary) that doesn't take the focus when it starts.
pub(super) fn event_loop() -> Option<eframe::EventLoopBuilderHook> {
    Some(Box::new(|builder| {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_activate_ignoring_other_apps(false)
            .with_default_menu(false);
    }))
}

/// The menu bar menu, no narrower than `MENU_WIDTH`.
pub(super) fn widen(menu: &Menu) {
    use tray_icon::menu::ContextMenu;
    // SAFETY: the menu's own NSMenu, alive as long as the menu, on the main thread.
    let ns_menu = unsafe { &*menu.ns_menu().cast::<objc2_app_kit::NSMenu>() };
    ns_menu.setMinimumWidth(MENU_WIDTH);
}

/// The overlay's window as AppKit has it: where it was put, and what the drag pasteboard said.
#[derive(Debug)]
pub(super) struct NativeWindow {
    /// The window's top left on screen, once it has been put somewhere.
    origin: Option<Pos2>,
    /// The display it was last put on.
    display: Option<Rect>,
    /// The drag pasteboard's change count when the button last came up: a new drag moves it on.
    drag_seen: isize,
}

impl NativeWindow {
    /// Floats the window on every Space, and teaches AppKit what winit doesn't answer.
    pub(super) fn new(cc: &eframe::CreationContext) -> NativeWindow {
        teach_appkit();
        serve_files();
        if let (Some(mtm), Some(window)) = (MainThreadMarker::new(), overlay_window(cc)) {
            float_everywhere(&window);
            reintroduce_delegates(mtm, &window);
        }
        NativeWindow {
            origin: None,
            display: None,
            drag_seen: MainThreadMarker::new().map_or(0, drag_count),
        }
    }

    /// Moves the window so the wheel is under the pointer, inside the display. How far it moved,
    /// in its points, if it was somewhere already.
    pub(super) fn place(&mut self, ctx: &egui::Context, pointer: Pos2) -> Option<Vec2> {
        let mut origin = pointer - WHEEL_AT;
        self.display = display_at(pointer);
        if let Some(display) = self.display {
            let max = (display.max - WINDOW).max(display.min);
            origin = origin.clamp(display.min, max);
        }
        if self.origin == Some(origin) {
            return None;
        }
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(origin));
        self.origin.replace(origin).map(|before| before - origin)
    }

    /// Whether the window has been put anywhere yet.
    pub(super) fn placed(&self) -> bool {
        self.origin.is_some()
    }

    /// The pointer in the window's points.
    pub(super) fn local(&self, pointer: Pos2) -> Pos2 {
        pointer - self.origin.unwrap_or_default().to_vec2()
    }

    /// Lets clicks and drags through to what is below, or takes them.
    pub(super) fn let_through(&mut self, ctx: &egui::Context, through: bool) {
        ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(through));
    }

    /// Whether files are being dragged: the drag pasteboard changed since the button last came
    /// up, and holds files. Shift held while dragging a shape or selecting text isn't one.
    pub(super) fn dragging_files(&self) -> bool {
        MainThreadMarker::new()
            .is_some_and(|mtm| drag_count(mtm) != self.drag_seen && drag_has_files(mtm))
    }

    /// The button came up: what was dragged is done with.
    pub(super) fn button_up(&mut self) {
        if let Some(mtm) = MainThreadMarker::new() {
            self.drag_seen = drag_count(mtm);
        }
    }

    /// Hands the keyboard back to the app that had it.
    pub(super) fn give_back_focus(&self) {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).deactivate();
        }
    }

    /// Nothing to look after between frames.
    pub(super) fn keep(&mut self) {}
}
