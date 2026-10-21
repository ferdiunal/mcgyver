//! Doing what a wedge promises to one file, for real: pictures, archives, media, office documents,
//! PDF tools and Markdown. Remaining catalog entries are still simulated (see `jobs`); the
//! Actions wheel is in `actions`.
//!
//! Results land next to the original under a free name ("photos 2.zip"). Work happens in
//! hidden scratch files and folders beside it, which are moved into place only once complete,
//! and not at all once the job is cancelled.

use std::cell::RefCell;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::archive::{self, Format, Progress};
use crate::catalog::{self, Choice, Kind, Mode};
use crate::images;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    /// Repack an archive, recompress a file, or pack a folder.
    Convert(Format),
    /// Unpack next to the archive.
    Extract,
    /// Save a picture in another format.
    Picture(images::Target),
    /// Apply a picture tool and save a new file.
    PictureTool(images::Tool),
    /// Convert audio or video through FFmpeg.
    Media(crate::media::Target),
    /// Save a document, spreadsheet or presentation in another format (a target label).
    Office(&'static str),
    /// Compress, rotate or split a PDF.
    Pdf(crate::office::PdfTool),
    /// Take the text of a document, spreadsheet, presentation or PDF out as Markdown.
    Markdown,
}

enum Source {
    Folder,
    File(Format),
}

fn source(path: &Path) -> Option<Source> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Some(Source::Folder),
        Ok(meta) if meta.is_file() => Format::detect(path).map(Source::File),
        // A pipe or device would block reading forever.
        Ok(_) => None,
        // Gone since the drag began: still a real job, which will say so.
        Err(_) => Format::from_name(path).map(Source::File),
    }
}

/// What picking `choice` does to `file`, if it is something this build really does.
pub fn task(file: &Path, choice: Choice) -> Option<Task> {
    match (choice.mode, catalog::kind_of(file)) {
        (Mode::Convert, Some(Kind::Image)) => {
            return images::Target::from_label(choice.label)
                .filter(|t| t.available())
                .map(Task::Picture);
        }
        (Mode::Convert, Some(Kind::Audio | Kind::Video)) if crate::media::available() => {
            return crate::media::Target::from_label(choice.label).map(Task::Media);
        }
        (Mode::Convert, Some(Kind::Document | Kind::Spreadsheet | Kind::Slides))
            if crate::office::Family::of(&catalog::ext_of(file))
                .is_some_and(|family| family.targets().contains(&choice.label)) =>
        {
            return Some(Task::Office(choice.label));
        }
        (Mode::Tools, Some(Kind::Pdf)) => {
            return crate::office::PdfTool::from_label(choice.label).map(Task::Pdf);
        }
        (Mode::Convert, Some(Kind::Document | Kind::Spreadsheet | Kind::Slides | Kind::Pdf))
            if choice.label == "MD" && crate::markdown::reads(&catalog::ext_of(file)) =>
        {
            return Some(Task::Markdown);
        }
        (Mode::Tools, Some(Kind::Image)) => {
            let tool = images::Tool::from_label(choice.label)?;
            return images::tool_target(&catalog::ext_of(file), tool)
                .map(|_| Task::PictureTool(tool));
        }
        _ => {}
    }
    let source = source(file)?;
    let archive = matches!(source, Source::File(_));
    match choice.mode {
        Mode::Convert if choice.label == catalog::UNZIP => archive.then_some(Task::Extract),
        Mode::Convert => {
            let to = Format::from_ext(&catalog::extension_for(choice.label))?;
            let fits = match source {
                Source::Folder => to.is_archive(),
                Source::File(from) => from != to && (!from.is_archive() || to.is_archive()),
            };
            (fits && to.can_write()).then_some(Task::Convert(to))
        }
        Mode::Tools => None,
        Mode::Actions | Mode::Media | Mode::Everyday | Mode::Apps => None,
    }
}

/// The PDFs `files`, in order, merged into one next to the first.
pub fn merge_pdfs(files: &[PathBuf], progress: &Progress) -> io::Result<PathBuf> {
    let first = files
        .first()
        .ok_or_else(|| io::Error::other("nothing to merge"))?;
    progress.set_steps(1);
    let parent = parent_of(first);
    let bytes = crate::office::merge(files, progress)?;
    let temp = Scratch::dir(parent)?;
    let output = temp.path().join("output.pdf");
    fs::write(&output, bytes)?;
    let stem = format!("{} (merged)", catalog::stem_of(first));
    move_to_free_name(&output, parent, &stem, ".pdf")
}

/// The folder holding `file`, `.` for a bare name.
pub(crate) fn parent_of(file: &Path) -> &Path {
    file.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Runs `task` on `file` and returns what it made.
pub fn run(file: &Path, task: Task, progress: &Progress) -> io::Result<PathBuf> {
    let parent = parent_of(file);
    if let Task::Picture(target) = task {
        progress.set_steps(2);
        progress.begin(1);
        let picture = images::decode(file)?;
        progress.next_step();
        progress.begin(1);
        let scratch = Scratch::dir(parent)?;
        let output = scratch
            .path()
            .join(format!("output.{}", target.extension()));
        images::encode(&picture, target, &output)?;
        progress.add(1);
        return move_to_free_name(
            &output,
            parent,
            &catalog::stem_of(file),
            &format!(".{}", target.extension()),
        );
    }
    if let Task::PictureTool(tool) = task {
        progress.set_steps(1);
        progress.begin(1);
        let target = images::tool_target(&catalog::ext_of(file), tool).ok_or_else(|| {
            io::Error::other("this image format cannot be written on this system")
        })?;
        let scratch = Scratch::dir(parent)?;
        let output = scratch
            .path()
            .join(format!("output.{}", target.extension()));
        images::run_tool(file, tool, &output)?;
        progress.add(1);
        return move_to_free_name(
            &output,
            parent,
            &format!("{} ({})", catalog::stem_of(file), tool.suffix()),
            &format!(".{}", target.extension()),
        );
    }
    if let Task::Media(target) = task {
        progress.set_steps(1);
        progress.begin(1);
        let temp = Scratch::dir(parent)?;
        let output = temp.path().join(format!("output.{}", target.extension()));
        crate::media::convert(file, target, &output, progress)?;
        progress.add(1);
        return move_to_free_name(
            &output,
            parent,
            &catalog::stem_of(file),
            &format!(".{}", target.extension()),
        );
    }
    if let Task::Office(label) = task {
        progress.set_steps(1);
        progress.begin(1);
        let ext = catalog::extension_for(label);
        let bytes = crate::office::convert(file, label)?;
        let temp = Scratch::dir(parent)?;
        let output = temp.path().join(format!("output.{ext}"));
        fs::write(&output, bytes)?;
        progress.add(1);
        return move_to_free_name(&output, parent, &catalog::stem_of(file), &format!(".{ext}"));
    }
    if let Task::Pdf(tool) = task {
        use crate::office::PdfTool;
        progress.set_steps(1);
        progress.begin(1);
        let stem = format!("{} ({})", catalog::stem_of(file), tool.suffix());
        let temp = Scratch::dir(parent)?;
        let made = match tool {
            PdfTool::Split => {
                crate::office::split(file, |page, pages, bytes| {
                    if page == 0 {
                        progress.begin(pages as u64);
                    }
                    let name = format!("{} {}.pdf", catalog::stem_of(file), page + 1);
                    fs::write(temp.path().join(name), bytes)?;
                    progress.add(1);
                    Ok(())
                })?;
                temp.finish(parent, &stem, "")?
            }
            PdfTool::Compress | PdfTool::Rotate => {
                let bytes = match tool {
                    PdfTool::Compress => crate::office::compress(file)?,
                    _ => crate::office::rotate(file)?,
                };
                let output = temp.path().join("output.pdf");
                fs::write(&output, bytes)?;
                progress.add(1);
                move_to_free_name(&output, parent, &stem, ".pdf")?
            }
        };
        return Ok(made);
    }
    if matches!(task, Task::Markdown) {
        progress.set_steps(1);
        progress.begin(1);
        let temp = Scratch::dir(parent)?;
        let output = temp.path().join("output.md");
        crate::markdown::convert(file, &output)?;
        progress.add(1);
        return move_to_free_name(&output, parent, &catalog::stem_of(file), ".md");
    }
    let source = source(file).ok_or_else(|| io::Error::other("not a compressed file or folder"))?;
    match (task, source) {
        (Task::Convert(to), Source::Folder) => {
            let name = catalog::display_name(file);
            let mut part = Scratch::file(parent)?;
            archive::pack(
                &[(file.to_owned(), name.clone())],
                to,
                part.create()?,
                progress,
            )?;
            part.finish(parent, &name, &format!(".{}", to.ext()))
        }
        (Task::Convert(to), Source::File(from)) => {
            let stem = archive::stem(file);
            let mut part = Scratch::file(parent)?;
            // When only the compression changes, there is no need to unpack.
            let recompress = match (from, to) {
                (Format::Tar(a), Format::Tar(b)) => Some((a, b)),
                (Format::Single(a), Format::Single(b)) => Some((Some(a), Some(b))),
                _ => None,
            };
            match recompress {
                Some((a, b)) => archive::recompress(file, a, part.create()?, b, progress)?,
                None => {
                    progress.set_steps(2);
                    let unpacked = Scratch::dir(parent)?;
                    archive::extract(file, from, unpacked.path(), progress)?;
                    progress.next_step();
                    archive::pack(&children(unpacked.path())?, to, part.create()?, progress)?;
                }
            }
            part.finish(parent, &stem, &format!(".{}", to.ext()))
        }
        (Task::Extract, Source::File(from)) => {
            let unpacked = Scratch::dir(parent)?;
            archive::extract(file, from, unpacked.path(), progress)?;
            match &children(unpacked.path())?[..] {
                // One thing inside (a folder, or the file of a .gz): it goes next to the archive.
                [(path, name)] => {
                    let (base, ext) = split_name(name, path.is_dir());
                    move_to_free_name(path, parent, base, ext)
                }
                // Loose files get a folder named after the archive.
                _ => unpacked.finish(parent, &archive::stem(file), ""),
            }
        }
        (Task::Extract, Source::Folder) => Err(io::Error::other("a folder isn't an archive")),
        (Task::Picture(_), _) => unreachable!("handled above"),
        (
            Task::PictureTool(_) | Task::Media(_) | Task::Office(_) | Task::Pdf(_) | Task::Markdown,
            _,
        ) => {
            unreachable!("handled above")
        }
    }
}

/// Everything directly inside `dir`, with names, sorted.
fn children(dir: &Path) -> io::Result<Vec<(PathBuf, String)>> {
    let mut out: Vec<(PathBuf, String)> = fs::read_dir(dir)?
        .map(|e| e.map(|e| (e.path(), e.file_name().to_string_lossy().into_owned())))
        .collect::<io::Result<_>>()?;
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

/// "notes.txt" → ("notes", ".txt"); folders keep their whole name.
pub(crate) fn split_name(name: &str, is_dir: bool) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if !is_dir && dot > 0 => name.split_at(dot),
        _ => (name, ""),
    }
}

thread_local! {
    /// The progress of the job running on this thread (see [`as_job`]).
    static JOB: RefCell<Option<Arc<Progress>>> = const { RefCell::new(None) };
}

/// Runs `work` as the job `progress` tracks. Once it is cancelled, [`move_to_free_name`], the
/// one way a result gets into place, refuses: nothing appears after a cancel, however far the
/// work had got.
pub(crate) fn as_job<T>(progress: Arc<Progress>, work: impl FnOnce() -> T) -> T {
    JOB.set(Some(progress));
    let result = work();
    JOB.set(None);
    result
}

/// Moves `from` to a free name in `dir`, choosing each name with an atomic no-replace rename.
pub(crate) fn move_to_free_name(
    from: &Path,
    dir: &Path,
    base: &str,
    ext: &str,
) -> io::Result<PathBuf> {
    static NAMING: Mutex<()> = Mutex::new(());
    let _naming = NAMING.lock().unwrap_or_else(|e| e.into_inner());
    JOB.with_borrow(|job| job.as_ref().map_or(Ok(()), |progress| progress.check()))?;
    let base = if base.is_empty() { "Archive" } else { base };
    for n in 1u64.. {
        let to = match n {
            1 => dir.join(format!("{base}{ext}")),
            n => dir.join(format!("{base} {n}{ext}")),
        };
        match rename_noreplace(from, &to) {
            Ok(()) => return Ok(to),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("some output name is free")
}

#[cfg(target_os = "linux")]
fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const i8,
            newdirfd: i32,
            newpath: *const i8,
            flags: u32,
        ) -> i32;
    }

    let from = CString::new(from.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: both paths are valid NUL-terminated strings and AT_FDCWD resolves them as given.
    let result = unsafe { renameat2(-100, from.as_ptr(), -100, to.as_ptr(), 1) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }

    let c_from = CString::new(from.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let c_to = CString::new(to.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: both paths are valid NUL-terminated strings; RENAME_EXCL fails rather than
    // replace what is at `to`.
    if unsafe { renamex_np(c_from.as_ptr(), c_to.as_ptr(), 0x4) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // ENOTSUP, EINVAL: a file system without exclusive renames (exFAT, FAT, SMB). Naming is
    // serialised within the app, so looking first is enough there.
    if !matches!(error.raw_os_error(), Some(45 | 22)) {
        return Err(error);
    }
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    fs::rename(from, to)
}

#[cfg(windows)]
fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    // Past MAX_PATH only the \\?\ form of a full path works (std does the same for its calls).
    let wide = |path: &Path| -> io::Result<Vec<u16>> {
        let mut text = std::ffi::OsString::new();
        if path.as_os_str().len() >= 248 && !path.as_os_str().to_string_lossy().starts_with(r"\\") {
            text.push(r"\\?\");
            text.push(std::path::absolute(path)?);
        } else {
            text.push(path);
        }
        Ok(text.encode_wide().chain([0]).collect())
    };
    let (wide_from, wide_to) = (wide(from)?, wide(to)?);
    // SAFETY: both paths end in a NUL; without MOVEFILE_REPLACE_EXISTING it fails rather than
    // replace what is at `to` (ERROR_ALREADY_EXISTS, which is `AlreadyExists`).
    if unsafe { MoveFileExW(wide_from.as_ptr(), wide_to.as_ptr(), 0) } != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // A folder in the way is "access denied".
    if error.kind() == io::ErrorKind::PermissionDenied && fs::symlink_metadata(to).is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    Err(error)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    // The application targets Linux; other builds retain the platform's native rename behavior.
    fs::rename(from, to)
}

/// `base + ext` in `dir`, or "base 2" + ext, "base 3" + ext… whichever is free.
#[cfg(test)]
pub(crate) fn free_name(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let base = if base.is_empty() { "Archive" } else { base };
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{base}{ext}")),
            n => dir.join(format!("{base} {n}{ext}")),
        })
        .find(|p| fs::symlink_metadata(p).is_err())
        .expect("some name is free")
}

/// A uniquely created hidden file or folder that is deleted unless moved into place.
pub(crate) struct Scratch {
    path: PathBuf,
    kept: bool,
    kind: ScratchKind,
}

enum ScratchKind {
    File(Option<File>),
    Dir,
}

impl Scratch {
    pub(crate) fn file(dir: &Path) -> io::Result<Scratch> {
        let temp = tempfile::Builder::new()
            .prefix(".mcgyver-")
            .suffix(".part")
            .tempfile_in(dir)
            .map_err(|e| writable(dir, e))?;
        let (file, path) = temp.keep().map_err(|e| writable(dir, e.error))?;
        Ok(Scratch {
            path,
            kept: false,
            kind: ScratchKind::File(Some(file)),
        })
    }

    pub(crate) fn dir(dir: &Path) -> io::Result<Scratch> {
        let temp = tempfile::Builder::new()
            .prefix(".mcgyver-")
            .tempdir_in(dir)
            .map_err(|e| writable(dir, e))?;
        Ok(Scratch {
            path: temp.keep(),
            kept: false,
            kind: ScratchKind::Dir,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn create(&mut self) -> io::Result<File> {
        match &mut self.kind {
            ScratchKind::File(file) => file
                .take()
                .ok_or_else(|| io::Error::other("scratch file was already opened")),
            ScratchKind::Dir => Err(io::Error::other("scratch path is a folder")),
        }
    }

    /// Moves it to a free name in `dir` and returns that.
    pub(crate) fn finish(mut self, dir: &Path, base: &str, ext: &str) -> io::Result<PathBuf> {
        if let ScratchKind::File(file) = &mut self.kind {
            file.take();
        }
        let to = move_to_free_name(&self.path, dir, base, ext)?;
        self.kept = true;
        Ok(to)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        match &self.kind {
            ScratchKind::File(_) => {
                let _ = fs::remove_file(&self.path);
            }
            ScratchKind::Dir => {
                archive::tidy_modes(&self.path);
                let _ = fs::remove_dir_all(&self.path);
            }
        }
    }
}

fn writable(dir: &Path, e: io::Error) -> io::Error {
    match e.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            io::Error::other(format!("can't write to {}", dir.display()))
        }
        _ => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_put_in_place_once_the_job_is_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let made = dir.path().join(".mcgyver-made");
        fs::write(&made, "x").unwrap();
        let cancelled = Arc::new(Progress::default());
        cancelled.cancel();
        let refused = as_job(cancelled, || {
            move_to_free_name(&made, dir.path(), "out", ".txt")
        });
        assert!(refused.is_err());
        assert!(made.exists() && !dir.path().join("out.txt").exists());

        let moved = as_job(Arc::new(Progress::default()), || {
            move_to_free_name(&made, dir.path(), "out", ".txt")
        });
        assert_eq!(moved.unwrap(), dir.path().join("out.txt"));
    }

    #[test]
    fn names_count_up_when_taken() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            free_name(dir.path(), "a", ".tar.gz"),
            dir.path().join("a.tar.gz")
        );
        File::create(dir.path().join("a.tar.gz")).unwrap();
        File::create(dir.path().join("a 2.tar.gz")).unwrap();
        assert_eq!(
            free_name(dir.path(), "a", ".tar.gz"),
            dir.path().join("a 3.tar.gz")
        );
        assert_eq!(split_name("notes.txt", false), ("notes", ".txt"));
        assert_eq!(split_name(".bashrc", false), (".bashrc", ""));
        assert_eq!(split_name("v1.2", true), ("v1.2", ""));
    }
}
