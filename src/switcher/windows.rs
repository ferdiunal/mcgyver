//! The switcher on Windows: the top-level windows Alt-Tab would list, in z-order (which is the
//! most recently used first), and bringing one forward.

use std::ffi::c_void;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow,
    GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

use super::Window;

/// The window class of a Store app's real window, which ApplicationFrameHost.exe only frames.
const CORE_WINDOW: &str = "Windows.UI.Core.CoreWindow";
const FRAME_HOST: &str = "ApplicationFrameHost.exe";

/// The top-level windows, topmost first.
fn top_level() -> Vec<HWND> {
    unsafe extern "system" fn collect(window: HWND, list: LPARAM) -> i32 {
        // SAFETY: `list` is the vector `top_level` passes, alive during the enumeration.
        unsafe { &mut *(list as *mut Vec<HWND>) }.push(window);
        1
    }
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: the callback only uses the vector passed, which outlives the call.
    unsafe { EnumWindows(Some(collect), &raw mut windows as LPARAM) };
    windows
}

fn title(window: HWND) -> String {
    // SAFETY: the buffer is as long as it is said to be.
    unsafe {
        let length = GetWindowTextLengthW(window);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }
}

fn class(window: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: the buffer is as long as it is said to be.
    let copied = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

fn process_of(window: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: writes the pid to the integer given.
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    pid
}

/// Whether Alt-Tab would list the window: shown, not a tool window (unless it asks to be on the
/// taskbar), not owned by a shown window (a dialog comes with its owner), and not cloaked (on
/// another virtual desktop, or a suspended Store app).
fn in_alt_tab(window: HWND) -> bool {
    // SAFETY: plain queries about a window handle, which may be stale but is only asked about.
    unsafe {
        if IsWindowVisible(window) == 0 {
            return false;
        }
        let style = GetWindowLongW(window, GWL_EXSTYLE) as u32;
        if style & WS_EX_TOOLWINDOW != 0 && style & WS_EX_APPWINDOW == 0 {
            return false;
        }
        let owner = GetWindow(window, GW_OWNER);
        if !owner.is_null() && IsWindowVisible(owner) != 0 && style & WS_EX_APPWINDOW == 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let asked = DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED as u32,
            &raw mut cloaked as *mut c_void,
            size_of::<u32>() as u32,
        );
        asked != 0 || cloaked == 0
    }
}

/// The program's path of a process.
fn exe_path(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed after use; the buffer's length is passed and updated.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);
        (ok != 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

/// The program that really owns a window: a Store app's window is framed by
/// ApplicationFrameHost.exe, and the app is the process of the core window inside.
fn program_of(window: HWND) -> Option<String> {
    let path = exe_path(process_of(window))?;
    let hosted = Path::new(&path)
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(FRAME_HOST));
    if !hosted {
        return Some(path);
    }
    struct Search {
        host: u32,
        found: Option<u32>,
    }
    unsafe extern "system" fn find(child: HWND, search: LPARAM) -> i32 {
        // SAFETY: `search` is the `Search` below, alive during the enumeration.
        let search = unsafe { &mut *(search as *mut Search) };
        let pid = process_of(child);
        if pid != search.host && class(child) == CORE_WINDOW {
            search.found = Some(pid);
            return 0;
        }
        1
    }
    let mut search = Search {
        host: process_of(window),
        found: None,
    };
    // SAFETY: the callback only uses the search passed, which outlives the call.
    unsafe { EnumChildWindows(window, Some(find), &raw mut search as LPARAM) };
    Some(search.found.and_then(exe_path).unwrap_or(path))
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

/// The program's file description from its version resource ("Notepad", "Microsoft Edge").
fn file_description(path: &str) -> Option<String> {
    let path = wide(path);
    // SAFETY: the buffer is as large as the size asked for; the pointers VerQueryValueW gives
    // point into it, and are read before it is dropped.
    unsafe {
        let size = GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr() as *mut c_void) == 0 {
            return None;
        }
        let mut translation: *mut c_void = std::ptr::null_mut();
        let mut length = 0u32;
        let key = wide("\\VarFileInfo\\Translation");
        if VerQueryValueW(
            data.as_ptr() as *const c_void,
            key.as_ptr(),
            &mut translation,
            &mut length,
        ) == 0
            || length < 4
        {
            return None;
        }
        let [language, codepage] = *(translation as *const [u16; 2]);
        let key = wide(&format!(
            "\\StringFileInfo\\{language:04x}{codepage:04x}\\FileDescription"
        ));
        let mut text: *mut c_void = std::ptr::null_mut();
        if VerQueryValueW(
            data.as_ptr() as *const c_void,
            key.as_ptr(),
            &mut text,
            &mut length,
        ) == 0
            || text.is_null()
            || length < 2
        {
            return None;
        }
        // `length` counts the characters, including the closing NUL.
        let characters = std::slice::from_raw_parts(text as *const u16, length as usize - 1);
        let description = std::ffi::OsString::from_wide(characters)
            .to_string_lossy()
            .trim()
            .to_owned();
        (!description.is_empty()).then_some(description)
    }
}

/// A program's name: its file description, else the file's name without `.exe`.
fn app_name(path: &str) -> String {
    file_description(path).unwrap_or_else(|| {
        Path::new(path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

pub fn windows() -> Vec<Window> {
    // SAFETY: only asks for this process's id and the foreground window.
    let (own, foreground) = unsafe { (GetCurrentProcessId(), GetForegroundWindow()) };
    top_level()
        .into_iter()
        .filter(|window| in_alt_tab(*window) && process_of(*window) != own)
        .filter_map(|window| {
            let title = title(window);
            if title.is_empty() {
                return None;
            }
            let program = program_of(window).unwrap_or_default();
            Some(Window {
                id: format!("{:x}", window as usize),
                title,
                app: app_name(&program),
                app_id: program,
                active: window == foreground,
            })
        })
        .collect()
}

/// Waits a moment at most for Shift, Ctrl, Alt and the Windows key to come up: keys sent while
/// one is held make another shortcut (Alt with Shift switches the keyboard's language). Returns
/// whether they did.
pub(crate) fn modifiers_up() -> bool {
    let held = || {
        [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            // SAFETY: asks for one key's state.
            .any(|&vk| unsafe { GetAsyncKeyState(vk.into()) } as u16 & 0x8000 != 0)
    };
    let until = std::time::Instant::now() + std::time::Duration::from_millis(1500);
    while held() {
        if std::time::Instant::now() >= until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    true
}

/// Taps Alt. Windows only lets the program that has the user's latest input (or was handed it)
/// take the foreground; the wheel is driven by hooks, not by a window that has been typed in, so
/// `SetForegroundWindow` alone often just flashes the taskbar button. An Alt press and release
/// from `SendInput` counts as that input and lifts the lock, and unlike `AttachThreadInput` to
/// the foreground window's thread it can't hang on a window that doesn't answer.
fn tap_alt() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_MENU,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(0), key(KEYEVENTF_KEYUP)];
    // SAFETY: two well-formed inputs, and their size as the API wants it.
    unsafe { SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32) };
}

pub fn activate(id: &str) -> Result<(), String> {
    let handle = usize::from_str_radix(id, 16).map_err(|_| format!("not a window id: {id}"))?;
    let window = handle as HWND;
    // SAFETY: plain calls on a window handle, checked to still be one.
    unsafe {
        if IsWindow(window) == 0 {
            return Err("the window is gone".to_owned());
        }
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        // Not with the trigger key still down: Alt would make another shortcut with it.
        if modifiers_up() {
            tap_alt();
        }
        SetForegroundWindow(window);
        if GetForegroundWindow() == window {
            Ok(())
        } else {
            Err("Windows wouldn't bring the window forward".to_owned())
        }
    }
}
