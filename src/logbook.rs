//! The log a windowed program keeps instead of a console: `McGyver.log` in the app's folder
//! (`%APPDATA%\McGyver`). Started from Explorer the app has no stderr, and started from a
//! terminal it isn't attached to it, so what it would have printed there (its errors, the `log`
//! records, panics with their backtrace) goes to the file, where it can be found and sent. With
//! `MCGYVER_DEBUG` (attached to the terminal), or with stderr sent to a file or a pipe, it goes
//! there instead. A McGyver the app starts (its settings, a restart, the one on Mesa's OpenGL)
//! inherits the log as its stderr, and goes on writing to it.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::io::{AsRawHandle, IntoRawHandle};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileInformationByHandle,
    GetFileType,
};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, SetStdHandle};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

use crate::i18n::trf;
use crate::settings::{self, APP_NAME};

/// A log grown past this is kept as `McGyver.old.log` at the next start, and a new one begun.
const ROLL_AT: u64 = 1 << 20;

/// The log, once stderr is it.
static OPEN: OnceLock<PathBuf> = OnceLock::new();
/// Whether stderr came sent to a file or a pipe: something reads it, maybe a script.
static SENT: AtomicBool = AtomicBool::new(false);

/// Where the log is.
pub fn path() -> Option<PathBuf> {
    settings::app_dir().map(|dir| dir.join(format!("{APP_NAME}.log")))
}

/// Sends stderr to the log, unless the app has a `console` to write to or stderr goes somewhere
/// already. (Panics write to stderr with their backtrace: `main` sets `RUST_BACKTRACE`.)
pub fn start(console: bool) {
    if stderr_is_sent_somewhere() {
        match path() {
            Some(path) if is_the_log(&path) => {
                let _ = OPEN.set(path);
                header();
            }
            _ => SENT.store(true, Ordering::Relaxed),
        }
        return;
    }
    if console {
        return;
    }
    let Some((file, path)) = open() else {
        return;
    };
    // SAFETY: the handle is the log's, open for as long as the process runs: it is let go of
    // here, never closed.
    unsafe { SetStdHandle(STD_ERROR_HANDLE, file.into_raw_handle()) };
    let _ = OPEN.set(path);
    header();
}

fn header() {
    let mesa = if crate::software_gl::active() {
        ", on Mesa's software OpenGL"
    } else {
        ""
    };
    let _ = writeln!(
        std::io::stderr(),
        "\n--- {APP_NAME} {} ({}), started {}{mesa}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S %:z")
    );
}

/// Whether stderr is the file at `log`: inherited from the McGyver that started this one.
fn is_the_log(log: &Path) -> bool {
    // SAFETY: reads the process's standard error handle, checked usable by the caller.
    let stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    file_id(stderr).is_some_and(|id| Some(id) == path_id(log))
}

/// A file's identity: its volume and its index there. A hard link has its original's.
fn file_id(handle: HANDLE) -> Option<(u32, u32, u32)> {
    // SAFETY: plain data, filled in by the call.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: an open handle and a struct to fill; a pipe's fails and gives None.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) } != 0;
    ok.then_some((
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    ))
}

/// The identity of the file at `path`, as `file_id` gives it.
pub(crate) fn path_id(path: &Path) -> Option<(u32, u32, u32)> {
    let file = File::open(path).ok()?;
    file_id(file.as_raw_handle())
}

/// Whether stderr is the log.
pub fn is_open() -> bool {
    OPEN.get().is_some()
}

/// Whether stderr was sent to a file or a pipe (`2> log.txt`, `| more`). A console's handle,
/// which a terminal may hand down, writes nowhere: the app isn't attached to it.
fn stderr_is_sent_somewhere() -> bool {
    // SAFETY: reads the process's standard error handle; null when it was given none.
    let stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if stderr.is_null() || stderr == INVALID_HANDLE_VALUE {
        return false;
    }
    // SAFETY: a handle the process was given; an unusable one is FILE_TYPE_UNKNOWN.
    matches!(
        unsafe { GetFileType(stderr) },
        FILE_TYPE_DISK | FILE_TYPE_PIPE
    )
}

fn open() -> Option<(File, PathBuf)> {
    let path = path()?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > ROLL_AT) {
        let _ = std::fs::rename(&path, path.with_file_name(format!("{APP_NAME}.old.log")));
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    Some((file, path))
}

/// Says why the app couldn't start, in a message box: opened from Explorer, nothing else would.
/// Not when stderr was sent somewhere: what reads it has been told, and nobody may be there to
/// click.
pub fn alert(error: &str) {
    if SENT.load(Ordering::Relaxed) {
        return;
    }
    let mut text = format!("{}\n\n{error}", trf("{} couldn't start.", &[&APP_NAME]));
    if let Some(path) = OPEN.get() {
        text.push_str("\n\n");
        text.push_str(&trf("The details are in {}", &[&path.display()]));
    }
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, title) = (wide(&text), wide(APP_NAME));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}
