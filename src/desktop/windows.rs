//! The overlay on Windows: an always-visible, topmost window that lets clicks through until it
//! takes a drag (see the parent module).
//!
//! Low-level keyboard and mouse hooks tell the app as the keys, the button and the pointer
//! change, which Windows asks no permission for; without them, the state is read with
//! GetAsyncKeyState and GetCursorPos. Windows can't tell what a drag carries before it reaches
//! a window, so the window takes any drag the key goes down in, and lets it go when no files
//! come. Positions are in pixels: eframe makes the app aware of each screen's scale, and the
//! window is moved with SetWindowPos. The window keeps its own extended styles (layered, never
//! activated, out of Alt+Tab, and letting clicks through when it should), since winit sets them
//! anew whenever it changes the window.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use eframe::egui::{self, Pos2, Vec2, pos2, vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::menu::Menu;
use windows_sys::Win32::Foundation::{GlobalFree, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ScreenToClient,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY, DROPEFFECT_MOVE};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LBUTTON, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_MENU, VK_RBUTTON, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::DROPFILES;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GWL_EXSTYLE, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowRect, HC_ACTION, HWND_TOPMOST, KBDLLHOOKSTRUCT, LWA_ALPHA, MSG, MSLLHOOKSTRUCT,
    SM_SWAPBUTTON, SWP_NOACTIVATE, SWP_NOSIZE, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_SYSKEYDOWN,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

use super::{Input, Shared, WHEEL_AT, WINDOW};
use crate::settings::Trigger;

/// Windows shows what a drag carries only to the window under it.
pub(super) const SEES_DRAGGED_FILES: bool = false;
/// The hooks hear drags between apps too, so the input needn't be read while one is on.
pub(super) const HEARS_DRAGS: bool = true;
/// The tray icon's size in pixels: 16 points, drawn for 200 %.
pub(super) const TRAY_ICON: u32 = 32;
/// How the keys are known, for `MCGYVER_DEBUG`.
pub(super) const HEARD: &str = "heard through low-level hooks";
pub(super) const READ: &str = "read every 30 ms (the hooks couldn't be set)";

/// The overlay's window, for the folder picker to sit above: set once it exists.
static OVERLAY: OnceLock<isize> = OnceLock::new();

/// The left and the right key for each trigger, and the one that stands for either (which
/// input made up by programs may send).
fn keys(trigger: Trigger) -> [VIRTUAL_KEY; 3] {
    match trigger {
        Trigger::Shift => [VK_LSHIFT, VK_RSHIFT, VK_SHIFT],
        Trigger::Ctrl => [VK_LCONTROL, VK_RCONTROL, VK_CONTROL],
        Trigger::Alt => [VK_LMENU, VK_RMENU, VK_MENU],
        Trigger::Cmd => [VK_LWIN, VK_RWIN, VK_LWIN],
    }
}

fn down(key: VIRTUAL_KEY) -> bool {
    // SAFETY: only reads the key's state.
    let state = unsafe { GetAsyncKeyState(key.into()) };
    // The top bit: down now.
    state < 0
}

/// Whether the primary button is the right one. The keys' state sees the buttons as they are,
/// not as Mouse settings swapped them (the mouse hook sees them swapped already).
fn swapped() -> bool {
    // SAFETY: no arguments.
    unsafe { GetSystemMetrics(SM_SWAPBUTTON) != 0 }
}

pub(super) fn read_input(trigger: Trigger) -> Input {
    let button = if swapped() { VK_RBUTTON } else { VK_LBUTTON };
    let mut at = POINT { x: 0, y: 0 };
    // SAFETY: `at` is there to be written.
    let pointer = if unsafe { GetCursorPos(&mut at) } != 0 {
        pos2(at.x as f32, at.y as f32)
    } else {
        Pos2::ZERO
    };
    Input {
        trigger_down: keys(trigger).into_iter().any(down),
        button_down: down(button),
        pointer,
    }
}

/// What the hooks hand what they hear to; set once, when they are.
struct Hooks {
    shared: Arc<Mutex<Shared>>,
    /// Wakes the window from a thread of its own: waking it takes egui's lock, which a frame
    /// being drawn may hold, and a hook that keeps the system's input waiting too long is
    /// removed without a word.
    wake: mpsc::Sender<()>,
}

impl Hooks {
    /// Takes in what the input is now, and has the window look if it should.
    fn heard(&self, change: impl FnOnce(&Shared, &mut Input)) {
        let wake = {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let mut input = s.input;
            change(&s, &mut input);
            s.heard(input)
        };
        if wake {
            let _ = self.wake.send(());
        }
    }
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// Hears the keys and the mouse through low-level hooks, on a thread of their own that waits
/// for them, so nothing is read while nothing happens. False if they can't be set.
pub(super) fn listen(shared: Arc<Mutex<Shared>>, ctx: egui::Context) -> bool {
    let (wake, woken) = mpsc::channel();
    if HOOKS.set(Hooks { shared, wake }).is_err() {
        return false;
    }
    let waking = std::thread::Builder::new()
        .name("mcgyver wake".into())
        .spawn(move || {
            for () in woken {
                ctx.request_repaint();
            }
        });
    if waking.is_err() {
        return false;
    }
    let (made, told) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("mcgyver keys".into())
        .spawn(move || {
            // SAFETY: the hook procedures have the signature hooks are called with; this thread
            // waits for messages from here on, which is when Windows calls them.
            let hooked = unsafe {
                let module = GetModuleHandleW(std::ptr::null());
                let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), module, 0);
                let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
                if keys.is_null() || mouse.is_null() {
                    for hook in [keys, mouse].into_iter().filter(|h| !h.is_null()) {
                        UnhookWindowsHookEx(hook);
                    }
                    false
                } else {
                    true
                }
            };
            let _ = made.send(hooked);
            if hooked {
                // SAFETY: a zeroed MSG is valid to write into.
                let mut message: MSG = unsafe { std::mem::zeroed() };
                // SAFETY: waits for this thread's messages, for good.
                while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {}
            }
        });
    spawned.is_ok() && told.recv().unwrap_or(false)
}

unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, `lparam` points at the key's KBDLLHOOKSTRUCT.
        let key = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let pressed = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        heard_key(key.vkCode, pressed);
    }
    // SAFETY: passes the event on as it came.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, `lparam` points at the event's MSLLHOOKSTRUCT.
        let mouse = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        heard_mouse(wparam as u32, mouse.pt);
    }
    // SAFETY: passes the event on as it came.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// A key went down or up; wakes the window if it was the trigger.
fn heard_key(key: u32, pressed: bool) {
    let Some(hooks) = HOOKS.get() else {
        return;
    };
    hooks.heard(|s, input| {
        let [left, right, either] = keys(s.trigger.unwrap_or(Trigger::Shift)).map(u32::from);
        // Windows notes a key's new state only after the hooks: when one side comes up, the
        // other is looked up.
        let other = if key == left {
            Some(right)
        } else if key == right {
            Some(left)
        } else if key == either {
            None
        } else {
            return;
        };
        input.trigger_down = pressed || other.is_some_and(|k| down(k as VIRTUAL_KEY));
    });
}

/// The mouse moved, or a button went down or up; wakes the window when that matters to it.
/// The hook is told the messages the buttons will send, so the left one's are the primary
/// button's even when Mouse settings swapped them.
fn heard_mouse(message: u32, at: POINT) {
    let Some(hooks) = HOOKS.get() else {
        return;
    };
    hooks.heard(|_, input| {
        input.pointer = pos2(at.x as f32, at.y as f32);
        match message {
            WM_LBUTTONDOWN => input.button_down = true,
            WM_LBUTTONUP => input.button_down = false,
            _ => {}
        }
    });
}

/// Opens the settings window, a process of its own.
pub(super) fn show_settings() {
    if let Ok(exe) = crate::settings::own_exe() {
        let _ = std::process::Command::new(exe).arg("--settings").spawn();
    }
}

/// Starts this program over: a new process, once this one has let go of the lock.
pub(super) fn restart() -> ! {
    match crate::settings::own_exe().and_then(|exe| std::process::Command::new(exe).spawn()) {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("mcgyver: can't restart: {e}");
            std::process::exit(1)
        }
    }
}

/// Windows asks nothing of the app before it starts.
pub(super) fn first_start() {}

/// The window never takes the focus, not even when it is first shown.
pub(super) fn viewport(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    viewport.with_active(false)
}

pub(super) fn event_loop() -> Option<eframe::EventLoopBuilderHook> {
    None
}

/// The tray menu's status line gives it its width.
pub(super) fn widen(_menu: &Menu) {}

/// The overlay's window as Windows has it.
#[derive(Debug)]
pub(super) struct NativeWindow {
    hwnd: HWND,
    /// The window's top left on screen, in pixels, once it has been put somewhere.
    origin: Option<(i32, i32)>,
    /// The screen it was last put on, in pixels.
    screen: Option<(i32, i32, i32, i32)>,
    /// Clicks and drags go through to what is below.
    through: bool,
}

impl NativeWindow {
    pub(super) fn new(cc: &eframe::CreationContext) -> NativeWindow {
        let hwnd = match cc.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get() as HWND,
            _ => std::ptr::null_mut(),
        };
        if !hwnd.is_null() {
            let _ = OVERLAY.set(hwnd as isize);
        }
        serve_files();
        let mut window = NativeWindow {
            hwnd,
            origin: None,
            screen: None,
            through: true,
        };
        window.keep();
        window
    }

    /// Pixels per point where the window is now.
    fn scale(&self) -> f32 {
        // SAFETY: the overlay's own window.
        match unsafe { GetDpiForWindow(self.hwnd) } {
            0 => 1.0,
            dpi => dpi as f32 / 96.0,
        }
    }

    /// Moves the window so the wheel is under the pointer, inside the screen. How far it moved,
    /// in its points, if it was somewhere already.
    pub(super) fn place(&mut self, _ctx: &egui::Context, pointer: Pos2) -> Option<Vec2> {
        let at = POINT {
            x: pointer.x as i32,
            y: pointer.y as i32,
        };
        // SAFETY: MONITORINFO says its own size; the DPI goes into the two numbers given.
        let (screen, scale) = unsafe {
            let monitor = MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            let screen = (GetMonitorInfoW(monitor, &mut info) != 0).then_some(info.rcMonitor);
            let (mut dpi, mut dpi_y) = (0, 0);
            let found = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) == 0;
            (screen, if found && dpi > 0 { dpi as f32 / 96.0 } else { 1.0 })
        };
        let mut origin = pointer - WHEEL_AT * scale;
        if let Some(r) = screen {
            let min = pos2(r.left as f32, r.top as f32);
            let max = (pos2(r.right as f32, r.bottom as f32) - WINDOW * scale).max(min);
            origin = origin.clamp(min, max);
        }
        self.screen = screen.map(|r| (r.left, r.top, r.right, r.bottom));
        let origin = (origin.x.round() as i32, origin.y.round() as i32);
        if self.origin == Some(origin) {
            return None;
        }
        // SAFETY: the overlay's own window; only its place changes (moved onto a screen of
        // another scale, winit sizes it anew).
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                origin.0,
                origin.1,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        let before = self.origin.replace(origin)?;
        Some(vec2((before.0 - origin.0) as f32, (before.1 - origin.1) as f32) / self.scale())
    }

    /// Whether the window has been put anywhere yet.
    pub(super) fn placed(&self) -> bool {
        self.origin.is_some()
    }

    /// The pointer in the window's points.
    pub(super) fn local(&self, pointer: Pos2) -> Pos2 {
        let mut at = POINT {
            x: pointer.x as i32,
            y: pointer.y as i32,
        };
        // SAFETY: the overlay's own window, and a point to write into.
        if self.hwnd.is_null() || unsafe { ScreenToClient(self.hwnd, &mut at) } == 0 {
            return pointer;
        }
        let scale = self.scale();
        pos2(at.x as f32 / scale, at.y as f32 / scale)
    }

    /// Lets clicks and drags through to what is below, or takes them.
    pub(super) fn let_through(&mut self, _ctx: &egui::Context, through: bool) {
        self.through = through;
        self.keep();
    }

    /// Any drag the key goes down in might be one of files.
    pub(super) fn dragging_files(&self) -> bool {
        true
    }

    pub(super) fn button_up(&mut self) {}

    /// The window is never activated, so it has nothing to give back.
    pub(super) fn give_back_focus(&self) {}

    /// Puts back the window's styles, and its place when moving it onto a screen of another
    /// scale shifted it.
    pub(super) fn keep(&mut self) {
        if self.hwnd.is_null() {
            return;
        }
        let through = if self.through { WS_EX_TRANSPARENT } else { 0 };
        // SAFETY: the overlay's own window, on its own thread.
        unsafe {
            let now = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            let wanted = (now & !WS_EX_TRANSPARENT)
                | WS_EX_LAYERED
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW
                | through;
            if wanted != now {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, wanted as isize);
                // A layered window shows nothing until it is told how opaque it is.
                SetLayeredWindowAttributes(self.hwnd, 0, 255, LWA_ALPHA);
            }
            let mut rect: RECT = std::mem::zeroed();
            if let Some((x, y)) = self.origin
                && GetWindowRect(self.hwnd, &mut rect) != 0
                && (rect.left, rect.top) != (x, y)
            {
                SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
}

/// `text` as Windows takes it: UTF-16, ending in a NUL.
fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}

/// Copy and Cut on the Actions wheel put the files on the clipboard as Explorer does: a list
/// of files, and whether pasting them should move them.
fn serve_files() {
    crate::actions::serve_files_with(crate::actions::FileClipboard {
        put: Box::new(put_files),
        can_cut: true,
    });
}

/// The files as Explorer's CF_HDROP: a DROPFILES, then each full path ending in a NUL, then one
/// more NUL, in UTF-16.
fn file_drop(files: &[PathBuf]) -> io::Result<Vec<u8>> {
    let header = size_of::<DROPFILES>();
    let mut drop = vec![0u8; header];
    // SAFETY: `drop` has room for a DROPFILES, written unaligned.
    unsafe {
        std::ptr::write_unaligned(
            drop.as_mut_ptr().cast::<DROPFILES>(),
            DROPFILES {
                pFiles: header as u32,
                pt: POINT { x: 0, y: 0 },
                fNC: 0,
                fWide: 1,
            },
        );
    }
    for file in files {
        let path = wide(std::path::absolute(file)?.as_os_str());
        drop.extend(path.iter().flat_map(|unit| unit.to_le_bytes()));
    }
    drop.extend([0, 0]);
    Ok(drop)
}

fn put_files(files: &[PathBuf], cut: bool) -> io::Result<()> {
    let drop = file_drop(files)?;
    let effect = if cut { DROPEFFECT_MOVE } else { DROPEFFECT_COPY };
    let name = wide("Preferred DropEffect".as_ref());
    // SAFETY: a NUL-terminated name.
    let preferred = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
    on_clipboard(|| {
        give(CF_HDROP.into(), &drop)?;
        if preferred != 0 {
            give(preferred, &effect.to_le_bytes())?;
        }
        Ok(())
    })
}

/// Copy Path: the text on the clipboard.
pub(crate) fn put_text(text: &str) -> io::Result<()> {
    let units: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    on_clipboard(|| give(CF_UNICODETEXT.into(), &units))
}

/// Opens the clipboard, empties it and has `put` fill it. Another app may hold it for a moment.
fn on_clipboard(put: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    // The overlay owns what is put there (without an owner, Windows may refuse it).
    let owner = OVERLAY.get().map_or(std::ptr::null_mut(), |&hwnd| hwnd as HWND);
    let mut tries = 0;
    // SAFETY: the overlay's window, or none; the clipboard is closed again below.
    while unsafe { OpenClipboard(owner) } == 0 {
        tries += 1;
        if tries == 10 {
            return Err(io::Error::other("couldn't reach the clipboard"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: the clipboard is open.
    let result = if unsafe { EmptyClipboard() } == 0 {
        Err(io::Error::last_os_error())
    } else {
        put()
    };
    // SAFETY: opened above.
    unsafe { CloseClipboard() };
    result
}

/// Hands the clipboard `bytes` in `format`; it owns the memory from then on.
fn give(format: u32, bytes: &[u8]) -> io::Result<()> {
    // SAFETY: the memory is `bytes.len()` long, locked while written, and freed only if the
    // clipboard didn't take it.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if memory.is_null() {
            return Err(io::Error::last_os_error());
        }
        let to = GlobalLock(memory).cast::<u8>();
        if to.is_null() {
            GlobalFree(memory);
            return Err(io::Error::last_os_error());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), to, bytes.len());
        GlobalUnlock(memory);
        if SetClipboardData(format, memory).is_null() {
            let error = io::Error::last_os_error();
            GlobalFree(memory);
            return Err(error);
        }
    }
    Ok(())
}

/// The overlay's window as the folder picker's owner, so the picker comes up above everything
/// even though another app is in front.
struct Owner(isize);

impl raw_window_handle::HasWindowHandle for Owner {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let hwnd = std::num::NonZeroIsize::new(self.0)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = raw_window_handle::Win32WindowHandle::new(hwnd);
        // SAFETY: the overlay's window lives as long as the app.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
    }
}

impl raw_window_handle::HasDisplayHandle for Owner {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::windows())
    }
}

/// Asks for a folder; `Err` if none is picked.
pub(crate) fn pick_folder(title: &str, start: &Path) -> io::Result<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .set_directory(start);
    if let Some(&hwnd) = OVERLAY.get() {
        dialog = dialog.set_parent(&Owner(hwnd));
    }
    dialog
        .pick_folder()
        .ok_or_else(|| io::Error::other("cancelled"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_go_on_the_clipboard_as_explorer_puts_them() {
        let files = [PathBuf::from(r"C:\Users\me\a.txt"), PathBuf::from(r"D:\ş b")];
        let drop = file_drop(&files).unwrap();
        let word = |at: usize| u32::from_le_bytes(drop[at..at + 4].try_into().unwrap());
        // pFiles, the point (x, y), fNC, fWide.
        assert_eq!(word(0), 20);
        assert_eq!(word(16), 1);
        let list: Vec<u16> = drop[20..]
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let expected: Vec<u16> = "C:\\Users\\me\\a.txt\0D:\\ş b\0\0".encode_utf16().collect();
        assert_eq!(list, expected);
    }
}
