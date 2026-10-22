//! The switcher on macOS: one entry per running app that has a Dock icon (the windows of an app
//! come forward together, as with Cmd-Tab), the most recently used first as the windows on screen
//! say, front to back.

use std::ffi::c_void;

use objc2_app_kit::{
    NSApplicationActivationOptions, NSApplicationActivationPolicy, NSRunningApplication,
    NSWorkspace,
};

use super::Window;

// The windows on screen, front to back, from Core Graphics. Owners' pids are public.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
    static kCGWindowOwnerPID: *const c_void;
    static kCGWindowLayer: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
    fn CFNumberGetValue(number: *const c_void, kind: isize, out: *mut c_void) -> u8;
    fn CFRelease(cf: *const c_void);
}

// Grand Central Dispatch, to run on the main thread.
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_sync_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    fn pthread_main_np() -> i32;
}

const ON_SCREEN_ONLY: u32 = 1 << 0;
const EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
/// `kCFNumberSInt32Type`.
const SINT32: isize = 3;

/// Runs `f` on the main thread, where AppKit wants to be used, and waits for it.
fn on_main<T: Send, F: FnOnce() -> T + Send>(f: F) -> T {
    // SAFETY: only asks which thread this is.
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }
    struct Job<F, T> {
        f: Option<F>,
        out: Option<T>,
    }
    extern "C" fn work<F: FnOnce() -> T, T>(context: *mut c_void) {
        // SAFETY: `context` is the `Job` below, alive until `dispatch_sync_f` returns.
        let job = unsafe { &mut *(context as *mut Job<F, T>) };
        job.out = job.f.take().map(|f| f());
    }
    let mut job = Job {
        f: Some(f),
        out: None,
    };
    // SAFETY: the main queue runs `work` once with the job, before this returns.
    unsafe {
        dispatch_sync_f(
            &raw const _dispatch_main_q,
            &mut job as *mut Job<F, T> as *mut c_void,
            work::<F, T>,
        );
    }
    job.out.expect("the main queue ran the job")
}

/// A number in a Core Foundation dictionary.
///
/// # Safety
/// `dict` is a live CFDictionary and `key` a CFString.
unsafe fn int(dict: *const c_void, key: *const c_void) -> Option<i32> {
    // SAFETY: the caller's dictionary and key; the value, if any, is a CFNumber it owns.
    let number = unsafe { CFDictionaryGetValue(dict, key) };
    let mut out = 0i32;
    // SAFETY: `out` is a 32-bit integer, the size the type asks for.
    let ok = !number.is_null()
        && unsafe { CFNumberGetValue(number, SINT32, &mut out as *mut i32 as *mut c_void) } != 0;
    ok.then_some(out)
}

/// The pids of the apps with windows on screen, the frontmost first, each once.
fn pids_front_to_back() -> Vec<i32> {
    let mut pids = Vec::new();
    // SAFETY: the list is ours until released; its entries are dictionaries it owns.
    unsafe {
        let list = CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP_ELEMENTS, 0);
        if list.is_null() {
            return pids;
        }
        for i in 0..CFArrayGetCount(list) {
            let window = CFArrayGetValueAtIndex(list, i);
            // Layer 0 is where apps' windows are; the menu bar and the Dock are above.
            if int(window, kCGWindowLayer) != Some(0) {
                continue;
            }
            if let Some(pid) = int(window, kCGWindowOwnerPID)
                && !pids.contains(&pid)
            {
                pids.push(pid);
            }
        }
        CFRelease(list);
    }
    pids
}

pub fn windows() -> Vec<Window> {
    let own = std::process::id() as i32;
    let recent = pids_front_to_back();
    let mut windows: Vec<(usize, Window)> = NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|app| {
            app.activationPolicy() == NSApplicationActivationPolicy::Regular
                && app.processIdentifier() != own
        })
        .map(|app| {
            let pid = app.processIdentifier();
            let name = app
                .localizedName()
                .map(|n| n.to_string())
                .unwrap_or_default();
            let bundle = app
                .bundleURL()
                .and_then(|url| url.path())
                .map(|p| p.to_string())
                .unwrap_or_default();
            let rank = recent
                .iter()
                .position(|p| *p == pid)
                .unwrap_or(recent.len());
            (
                rank,
                Window {
                    id: pid.to_string(),
                    title: name.clone(),
                    app: name,
                    app_id: bundle,
                    active: app.isActive(),
                },
            )
        })
        .collect();
    // Apps not on screen (hidden, or with their windows minimized) come after, in the system's
    // own stable order.
    windows.sort_by_key(|(rank, _)| *rank);
    windows.into_iter().map(|(_, window)| window).collect()
}

pub fn activate(id: &str) -> Result<(), String> {
    let pid: i32 = id.parse().map_err(|_| format!("not an app id: {id}"))?;
    on_main(move || {
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            .ok_or_else(|| "the app has quit".to_owned())?;
        if app.isHidden() {
            app.unhide();
        }
        if app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows) {
            Ok(())
        } else {
            Err("the app wouldn't come forward".to_owned())
        }
    })
}
