//! RAR can only be read, through the official unRAR code (the `rar` feature).
//!
//! RAR5 hard links and file copies refer to another entry by name, and unRAR resolves that
//! name against the app's working folder rather than the destination, so a crafted archive
//! could pull in any of the user's files. We list the entries first (the `unrar` crate doesn't
//! show those references) and make links and copies ourselves, only from files this archive
//! already put in the destination.

use std::io;
use std::path::Path;

use unrar::Archive;
use unrar::error::{Code, UnrarError};
use unrar_sys as ffi;

use super::fs::{Progress, Unpacker};

/// unRAR's FSREDIR_HARDLINK and FSREDIR_FILECOPY.
const HARD_LINK: u32 = 4;
const FILE_COPY: u32 = 5;

struct Entry {
    name: String,
    size: u64,
    /// For hard links and file copies, the entry it repeats.
    copy_of: Option<String>,
}

pub fn extract(src: &Path, dst: &Path, progress: &Progress) -> io::Result<()> {
    let entries = list(src)?;
    // unRAR writes the files itself, so progress goes by entry.
    progress.begin(entries.iter().map(|e| e.size).sum());

    let mut out = Unpacker::new(dst);
    let mut archive = Archive::new(src).open_for_processing().map_err(error)?;
    let mut listed = entries.iter();
    while let Some(header) = archive.read_header().map_err(error)? {
        let entry = header.entry();
        if entry.is_encrypted() {
            return Err(io::Error::other("this archive is password-protected"));
        }
        let name = entry.filename.to_string_lossy().into_owned();
        let size = entry.unpacked_size;
        // Both passes read the same headers; if they ever disagree, stop rather than guess.
        let Some(listed) = listed.next().filter(|l| l.name == name) else {
            return Err(io::Error::other(
                "this RAR archive reads differently on a second pass",
            ));
        };
        archive = if let Some(original) = &listed.copy_of {
            out.copy(original, &name)?;
            progress.add(size);
            header.skip()
        } else if entry.is_directory() {
            out.dir(&name, None, None)?;
            header.skip()
        } else if entry.is_file()
            && let Some(path) = out.prepare(&name)?
        {
            let next = header.extract_to(&path);
            not_linked(&path)?;
            progress.add(size);
            next
        } else {
            header.skip()
        }
        .map_err(error)?;
    }
    out.finish()
}

/// A second guard: a file unRAR just wrote has one name. More means it linked to something,
/// which `list` should have caught; the link goes at once.
fn not_linked(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && meta.is_file()
            && meta.nlink() > 1
        {
            std::fs::remove_file(path)?;
            return Err(io::Error::other(
                "this archive links to files outside itself",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Every entry's name, size and, for links and copies, what it repeats.
fn list(src: &Path) -> io::Result<Vec<Entry>> {
    let name = native_path(src)?;
    let mut open = ffi::OpenArchiveDataEx::new(name.as_ptr(), ffi::RAR_OM_LIST);
    // SAFETY: `open` and the path it points to outlive the call, which writes `open_result`.
    let handle = unsafe { ffi::RAROpenArchiveEx((&raw mut open).cast_const()) };
    if handle.is_null() {
        return Err(code_error(open.open_result as i32));
    }
    struct Close(*const ffi::Handle);
    impl Drop for Close {
        fn drop(&mut self) {
            // SAFETY: opened above, closed once.
            unsafe { ffi::RARCloseArchive(self.0) };
        }
    }
    let _close = Close(handle);

    let mut entries = Vec::new();
    loop {
        let mut target = vec![0 as ffi::WCHAR; 2048];
        let mut header = ffi::HeaderDataEx {
            redir_name: target.as_mut_ptr(),
            redir_name_size: target.len() as u32,
            ..Default::default()
        };
        // SAFETY: `header` and the buffer it points to are valid for the call, which fills them.
        match unsafe { ffi::RARReadHeaderEx(handle, (&raw mut header).cast_const()) } {
            ffi::ERAR_SUCCESS => {}
            ffi::ERAR_END_ARCHIVE => break,
            code => return Err(code_error(code)),
        }
        let repeats = matches!(header.redir_type, HARD_LINK | FILE_COPY);
        entries.push(Entry {
            name: from_wide(&header.filename_w),
            size: u64::from(header.unp_size_high) << 32 | u64::from(header.unp_size),
            copy_of: repeats.then(|| from_wide(&target)),
        });
        // SAFETY: a valid handle; no destination is used when skipping.
        let code = unsafe {
            ffi::RARProcessFile(handle, ffi::RAR_SKIP, std::ptr::null(), std::ptr::null())
        };
        if code != ffi::ERAR_SUCCESS {
            return Err(code_error(code));
        }
    }
    Ok(entries)
}

/// The path the way the `unrar` crate hands it over on this platform.
#[cfg(any(target_os = "linux", target_os = "netbsd"))]
fn native_path(path: &Path) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(io::Error::other)
}

#[cfg(not(any(target_os = "linux", target_os = "netbsd")))]
fn native_path(path: &Path) -> io::Result<Vec<ffi::WCHAR>> {
    let name = path
        .to_str()
        .ok_or_else(|| io::Error::other("the archive's name isn't valid text"))?;
    Ok(to_wide(name))
}

#[cfg(all(not(any(target_os = "linux", target_os = "netbsd")), windows))]
fn to_wide(s: &str) -> Vec<ffi::WCHAR> {
    s.encode_utf16().chain([0]).collect()
}

#[cfg(all(not(any(target_os = "linux", target_os = "netbsd")), not(windows)))]
fn to_wide(s: &str) -> Vec<ffi::WCHAR> {
    s.chars().map(|c| c as ffi::WCHAR).chain([0]).collect()
}

/// A NUL-terminated wide string: UTF-16 on Windows, UTF-32 elsewhere.
fn from_wide(w: &[ffi::WCHAR]) -> String {
    let w = &w[..w.iter().position(|c| *c == 0).unwrap_or(w.len())];
    #[cfg(windows)]
    return String::from_utf16_lossy(w);
    #[cfg(not(windows))]
    w.iter()
        .map(|c| char::from_u32(*c as u32).unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn code_error(code: i32) -> io::Error {
    match Code::from(code) {
        Some(code) => message(code),
        None => io::Error::other(format!("unRAR error {code}")),
    }
}

fn error(e: UnrarError) -> io::Error {
    message(e.code)
}

fn message(code: Code) -> io::Error {
    match code {
        Code::MissingPassword | Code::BadPassword => {
            io::Error::other("this archive is password-protected")
        }
        Code::BadArchive | Code::UnknownFormat | Code::BadData => {
            io::Error::other("this RAR archive is damaged")
        }
        Code::EOpen => io::Error::other("can't open the RAR archive (or one of its parts)"),
        code => io::Error::other(format!("unRAR failed ({code:?})")),
    }
}
