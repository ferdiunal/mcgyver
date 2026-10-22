//! Where OpenGL is older than 2.0 (a virtual machine without 3D acceleration, Remote Desktop, a PC
//! without its graphics driver: Windows' own fallback is OpenGL 1.1), the app draws with Mesa's
//! software OpenGL (llvmpipe), shipped in the `opengl` folder beside the exe. Windows takes
//! `opengl32.dll` from the exe's own folder before its own, and only as the process starts; so
//! when OpenGL fails, the app starts again from a hard link to itself in that folder. With a
//! graphics driver it never comes to that.

use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::OnceLock;

use windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW;

/// Set for the process started on Mesa's OpenGL, so it doesn't start again when that fails too.
const SOFTWARE: &str = "MCGYVER_SOFTWARE_GL";
/// The exe the app was started as, for that process: the one Start at login should start, and
/// the folder `heif.dll` is put in.
const STARTED_AS: &str = "MCGYVER_EXE";
/// Mesa's two files, as the `opengl` folder holds them.
const DLLS: [&str; 2] = ["opengl32.dll", "libgallium_wgl.dll"];

/// The exe the app was started as, when this process draws with Mesa's OpenGL.
static ON_MESA: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Takes what `start_again` told this process out of the environment, so the programs it starts
/// (a terminal, and a McGyver started from that) don't inherit it; `mark` hands it on to the app's
/// own processes. On Mesa's OpenGL, the app's own folder is searched for DLLs again (`heif.dll`):
/// this process runs from the `opengl` folder. Called first thing, before any other thread.
pub fn take_env() {
    let started_as = std::env::var_os(SOFTWARE)
        .and(std::env::var_os(STARTED_AS))
        .map(PathBuf::from);
    // SAFETY: no other thread is there yet to read the environment.
    unsafe {
        std::env::remove_var(SOFTWARE);
        std::env::remove_var(STARTED_AS);
    }
    if let Some(dir) = started_as.as_deref().and_then(Path::parent) {
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated path; LoadLibrary searches it after the exe's folder.
        unsafe { SetDllDirectoryW(wide.as_ptr()) };
    }
    let _ = ON_MESA.set(started_as);
}

/// Whether this process draws with Mesa's software OpenGL.
pub fn active() -> bool {
    started_as().is_some()
}

/// The exe the app was started as, when this process runs from the `opengl` folder.
pub fn started_as() -> Option<&'static Path> {
    ON_MESA.get().and_then(Option::as_deref)
}

/// Keeps a McGyver this process starts (its settings, a restart) on Mesa's OpenGL with it.
pub fn mark(command: &mut Command) -> &mut Command {
    if let Some(exe) = started_as() {
        command.env(SOFTWARE, "1").env(STARTED_AS, exe);
    }
    command
}

/// Starts the app again on Mesa's OpenGL when `error` was OpenGL's and Mesa is shipped: the new
/// process goes on with the same arguments, and this one should leave.
pub fn start_again(error: &str) -> Option<Child> {
    let lower = error.to_lowercase();
    if active() || !(lower.contains("opengl") || lower.contains("glutin")) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let mesa = exe.parent()?.join("opengl");
    if !DLLS.iter().all(|dll| mesa.join(dll).is_file()) {
        return None;
    }
    let Some(beside) = beside_mesa(&exe, &mesa) else {
        let _ = writeln!(
            std::io::stderr(),
            "McGyver: {error}; Mesa's OpenGL is shipped, but the app can't be put beside it"
        );
        return None;
    };
    let _ = writeln!(
        std::io::stderr(),
        "McGyver: {error}; starting again with Mesa's software OpenGL"
    );
    Command::new(&beside)
        .args(std::env::args_os().skip(1))
        .env(SOFTWARE, "1")
        .env(STARTED_AS, &exe)
        // llvmpipe itself: Mesa would try Direct3D 12 first, which needs a DLL not shipped.
        .env("GALLIUM_DRIVER", "llvmpipe")
        .spawn()
        .ok()
}

/// The exe in `mesa`'s folder: a hard link to `exe` (no copy, and the same file again after an
/// update), else, on a drive without hard links (FAT) or a folder it can't write (Program Files),
/// a copy with Mesa's files in `%LOCALAPPDATA%\McGyver\opengl`.
fn beside_mesa(exe: &Path, mesa: &Path) -> Option<PathBuf> {
    let name = exe.file_name()?;
    let linked = mesa.join(name);
    if same_file(exe, &linked) {
        return Some(linked);
    }
    let _ = std::fs::remove_file(&linked);
    // Linked meanwhile by a McGyver starting at the same time is as good.
    if std::fs::hard_link(exe, &linked).is_ok() || same_file(exe, &linked) {
        return Some(linked);
    }
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())?
        .join(crate::settings::APP_NAME)
        .join("opengl");
    std::fs::create_dir_all(&dir).ok()?;
    // Copied again only when it changed (the exe is big): an update may keep its size, not its
    // time, which a copy keeps.
    let stamp = |path: &Path| {
        let meta = std::fs::metadata(path).ok()?;
        Some((meta.len(), meta.modified().ok()?))
    };
    let files = DLLS
        .iter()
        .map(|dll| (mesa.join(dll), dir.join(dll)))
        .chain([(exe.to_path_buf(), dir.join(name))]);
    for (from, to) in files {
        if stamp(&to) != stamp(&from) {
            std::fs::copy(&from, &to).ok()?;
        }
    }
    Some(dir.join(name))
}

/// Whether two paths are one file: a hard link and its original are.
fn same_file(a: &Path, b: &Path) -> bool {
    crate::logbook::path_id(a).is_some_and(|a| Some(a) == crate::logbook::path_id(b))
}
