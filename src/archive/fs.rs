//! The file system side: progress counting, walking what gets packed, and writing what gets
//! unpacked without ever leaving the destination folder.

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::SystemTime;

/// How far a job has got, shared with the thread doing it, and whether it was cancelled.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    step: AtomicU32,
    steps: AtomicU32,
    cancelled: AtomicBool,
}

impl Progress {
    /// Asks the work to stop: it does at the next [`check`](Self::check), and puts nothing in
    /// place after that.
    pub fn cancel(&self) {
        self.cancelled.store(true, Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Relaxed)
    }

    /// An error once the job was cancelled, for the work to stop on.
    pub fn check(&self) -> io::Result<()> {
        if self.is_cancelled() {
            Err(io::Error::other("cancelled"))
        } else {
            Ok(())
        }
    }

    /// The work happens in `steps` passes of similar weight (unpack, then pack).
    pub fn set_steps(&self, steps: u32) {
        self.steps.store(steps, Relaxed);
    }

    /// Starts a pass over `total` bytes.
    pub fn begin(&self, total: u64) {
        self.done.store(0, Relaxed);
        self.total.store(total, Relaxed);
    }

    pub fn next_step(&self) {
        self.step.fetch_add(1, Relaxed);
        self.begin(0);
    }

    pub fn add(&self, bytes: u64) {
        self.done.fetch_add(bytes, Relaxed);
    }

    pub fn fraction(&self) -> f32 {
        let steps = self.steps.load(Relaxed).max(1);
        let step = self.step.load(Relaxed).min(steps - 1);
        let (done, total) = (self.done.load(Relaxed), self.total.load(Relaxed));
        let part = if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64).min(1.0)
        };
        ((step as f64 + part) / steps as f64) as f32
    }
}

/// Counts the bytes read through it.
pub struct Counted<'a, R> {
    inner: R,
    progress: &'a Progress,
}

impl<'a, R> Counted<'a, R> {
    pub fn new(inner: R, progress: &'a Progress) -> Self {
        Self { inner, progress }
    }
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.progress.check()?;
        let n = self.inner.read(buf)?;
        self.progress.add(n as u64);
        Ok(n)
    }
}

impl<R: Seek> Seek for Counted<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

pub enum Kind {
    File,
    Dir,
    Symlink(PathBuf),
}

/// Something to pack, named as it will be inside the archive.
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub meta: Metadata,
}

impl Item {
    pub fn size(&self) -> u64 {
        if matches!(self.kind, Kind::File) {
            self.meta.len()
        } else {
            0
        }
    }

    pub fn mtime(&self) -> Option<SystemTime> {
        self.meta.modified().ok()
    }

    #[cfg(unix)]
    pub fn mode(&self) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        self.meta.permissions().mode() & 0o7777
    }

    #[cfg(not(unix))]
    pub fn mode(&self) -> u32 {
        match self.kind {
            Kind::Dir => 0o755,
            _ if self.meta.permissions().readonly() => 0o444,
            _ => 0o644,
        }
    }
}

/// Every file, folder and link under `roots`, parents first, siblings sorted. A root that is a
/// link is followed (it is what the user picked); links inside are kept as links.
pub fn walk(roots: &[(PathBuf, String)]) -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    for (path, name) in roots {
        visit(path, name.clone(), true, &mut items)?;
    }
    Ok(items)
}

fn visit(path: &Path, name: String, root: bool, items: &mut Vec<Item>) -> io::Result<()> {
    let meta = if root {
        fs::metadata(path)?
    } else {
        fs::symlink_metadata(path)?
    };
    let kind = if meta.is_dir() {
        Kind::Dir
    } else if meta.file_type().is_symlink() {
        Kind::Symlink(fs::read_link(path)?)
    } else if meta.is_file() {
        Kind::File
    } else {
        // Sockets, pipes, devices: nothing an archive should carry.
        return Ok(());
    };
    let is_dir = matches!(kind, Kind::Dir);
    items.push(Item {
        path: path.to_owned(),
        name: name.clone(),
        kind,
        meta,
    });
    if is_dir {
        let mut children: Vec<_> = fs::read_dir(path)?.collect::<io::Result<_>>()?;
        children.sort_by_key(|c| c.file_name());
        for child in children {
            let file_name = child.file_name().to_string_lossy().into_owned();
            // Another job's unfinished output.
            if file_name.starts_with(".mcgyver-") && file_name.ends_with(".part") {
                continue;
            }
            visit(&child.path(), format!("{name}/{file_name}"), false, items)?;
        }
    }
    Ok(())
}

/// Reads exactly `len` bytes: a file that shrank while being packed is an error, and what it
/// grew by is left out, so the size already written in a header stays true.
pub struct Exact<R> {
    inner: R,
    left: u64,
}

impl<R> Exact<R> {
    pub fn new(inner: R, len: u64) -> Self {
        Self { inner, left: len }
    }
}

impl<R: Read> Read for Exact<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            return Ok(0);
        }
        let max = buf
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..max])?;
        if n == 0 {
            return Err(io::Error::other("a file changed while it was being packed"));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

/// A link entry's target, which archives store as the entry's data; anything longer than a
/// path can be is refused rather than read into memory.
pub fn link_target(data: &mut dyn Read) -> io::Result<String> {
    const MAX: u64 = 4096;
    let mut target = String::new();
    data.take(MAX + 1).read_to_string(&mut target)?;
    if target.len() as u64 > MAX {
        return Err(io::Error::other("this archive has a broken link entry"));
    }
    Ok(target)
}

/// Writes archive entries under `root`. Names that would leave it (`..`, absolute paths) are
/// made relative or skipped, and nothing is ever written through a link.
pub struct Unpacker {
    root: PathBuf,
    /// Folder modes and times, applied last so filling the folders doesn't undo them.
    dirs: Vec<(PathBuf, Option<u32>, Option<SystemTime>)>,
}

impl Unpacker {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            dirs: Vec::new(),
        }
    }

    pub fn dir(
        &mut self,
        name: &str,
        mode: Option<u32>,
        mtime: Option<SystemTime>,
    ) -> io::Result<()> {
        let Some(path) = self.place(name)? else {
            return Ok(());
        };
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(in_the_way(&path)),
            Err(_) => fs::create_dir(&path)?,
        }
        self.dirs.push((path, mode, mtime));
        Ok(())
    }

    pub fn file(
        &mut self,
        name: &str,
        data: &mut dyn Read,
        mode: Option<u32>,
        mtime: Option<SystemTime>,
    ) -> io::Result<()> {
        let Some(path) = self.prepare(name)? else {
            io::copy(data, &mut io::sink())?;
            return Ok(());
        };
        io::copy(data, &mut File::create_new(&path)?)?;
        set_mode(&path, mode.map(|m| m & 0o777))?;
        set_mtime(&path, mtime)
    }

    pub fn symlink(
        &mut self,
        name: &str,
        target: &str,
        mtime: Option<SystemTime>,
    ) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(path) = self.prepare(name)? {
            std::os::unix::fs::symlink(target, &path)?;
            if let Some(t) = mtime {
                let t = filetime::FileTime::from_system_time(t);
                filetime::set_symlink_file_times(&path, t, t)?;
            }
        }
        #[cfg(not(unix))]
        let _ = (name, target, mtime);
        Ok(())
    }

    /// Repeats the already unpacked file `original` as `name` (an archive's hard link or file
    /// copy). The original has to be a regular file inside the root, reached without links.
    #[cfg_attr(not(feature = "rar"), allow(dead_code))]
    pub fn copy(&mut self, original: &str, name: &str) -> io::Result<()> {
        let outside = || io::Error::other("this archive refers to a file it doesn't hold");
        let relative = relative(original).ok_or_else(outside)?;
        let mut from = self.root.clone();
        for part in relative.components() {
            from.push(part);
            let meta = fs::symlink_metadata(&from).map_err(|_| outside())?;
            if meta.file_type().is_symlink() {
                return Err(outside());
            }
        }
        if !from.is_file() {
            return Err(outside());
        }
        if let Some(to) = self.prepare(name)? {
            fs::copy(&from, &to)?;
        }
        Ok(())
    }

    /// Makes room for a file entry and returns where it goes, or `None` to skip it.
    pub fn prepare(&mut self, name: &str) -> io::Result<Option<PathBuf>> {
        let Some(path) = self.place(name)? else {
            return Ok(None);
        };
        // A later entry with the same name replaces the earlier one.
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => return Err(in_the_way(&path)),
            Ok(_) => fs::remove_file(&path)?,
            Err(_) => {}
        }
        Ok(Some(path))
    }

    pub fn finish(mut self) -> io::Result<()> {
        // Deepest first, so a read-only parent is set after its children.
        self.dirs.sort_by(|a, b| b.0.cmp(&a.0));
        for (path, mode, mtime) in self.dirs {
            set_mode(&path, mode.map(|m| m & 0o777 | 0o700))?;
            set_mtime(&path, mtime)?;
        }
        Ok(())
    }

    /// Where `name` lands, with its parent folders created; `None` if it has no safe place.
    fn place(&self, name: &str) -> io::Result<Option<PathBuf>> {
        let Some(relative) = relative(name) else {
            return Ok(None);
        };
        let mut path = self.root.clone();
        let parts: Vec<Component> = relative.components().collect();
        for part in &parts[..parts.len() - 1] {
            path.push(part);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(in_the_way(&path)),
                Err(_) => fs::create_dir(&path)?,
            }
        }
        path.push(parts[parts.len() - 1]);
        Ok(Some(path))
    }
}

/// An archive name as a relative path: separators either way, leading `/` dropped, `.`
/// ignored. `None` for names that climb out with `..` or are empty.
fn relative(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            _ if part.contains('\0') || (cfg!(windows) && part.contains(':')) => return None,
            _ => out.push(part),
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn in_the_way(path: &Path) -> io::Error {
    io::Error::other(format!(
        "{} is both a file and a folder in this archive",
        path.display()
    ))
}

fn set_mode(path: &Path, mode: Option<u32>) -> io::Result<()> {
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

fn set_mtime(path: &Path, mtime: Option<SystemTime>) -> io::Result<()> {
    match mtime {
        Some(t) => filetime::set_file_mtime(path, filetime::FileTime::from_system_time(t)),
        None => Ok(()),
    }
}

/// Lets the owner read every file and use every folder under `path`, nobody else write to them,
/// and drops setuid, setgid and sticky bits: what unpacking with the usual umask gives (whoever
/// did the unpacking, unRAR included), and what packing again, moving or deleting need. Links
/// are left alone.
pub fn tidy_modes(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        let need = if meta.is_dir() {
            0o700
        } else if meta.is_file() {
            0o400
        } else {
            return;
        };
        let mode = meta.permissions().mode() & 0o7777;
        let tidy = (mode | need) & 0o755;
        if tidy != mode {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(tidy));
        }
        if meta.is_dir() {
            // Collected first, so a deep tree doesn't hold a handle open per level.
            let children: Vec<PathBuf> = fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|c| c.path())
                .collect();
            for child in children {
                tidy_modes(&child);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_inside() {
        assert_eq!(relative("a/b.txt"), Some(PathBuf::from("a/b.txt")));
        assert_eq!(relative("/etc/passwd"), Some(PathBuf::from("etc/passwd")));
        assert_eq!(relative("./a\\b"), Some(PathBuf::from("a/b")));
        assert_eq!(relative("a/../../b"), None);
        assert_eq!(relative(".."), None);
        assert_eq!(relative("/"), None);
    }

    #[cfg(unix)]
    #[test]
    fn copies_come_only_from_inside() {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "s").unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut out = Unpacker::new(root.path());
        out.file("a/real.txt", &mut &b"data"[..], None, None)
            .unwrap();
        out.copy("a/real.txt", "b/copy.txt").unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("b/copy.txt")).unwrap(),
            "data"
        );
        out.symlink("away", outside.path().to_str().unwrap(), None)
            .unwrap();
        for original in ["away/secret", "../secret", "/etc/passwd", "missing", "a"] {
            assert!(out.copy(original, "stolen").is_err(), "{original}");
        }
        assert!(!root.path().join("stolen").exists());
    }

    #[cfg(unix)]
    #[test]
    fn nothing_is_written_through_a_link() {
        let outside = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut out = Unpacker::new(root.path());
        out.symlink("escape", outside.path().to_str().unwrap(), None)
            .unwrap();
        assert!(
            out.file("escape/owned.txt", &mut &b"x"[..], None, None)
                .is_err()
        );
        // A file entry replaces the link instead of writing to where it points.
        out.symlink(
            "link",
            &outside.path().join("target.txt").to_string_lossy(),
            None,
        )
        .unwrap();
        out.file("link", &mut &b"x"[..], None, None).unwrap();
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
        assert!(root.path().join("link").is_file());
    }

    #[test]
    fn exact_reads_refuse_short_files() {
        let mut out = Vec::new();
        assert!(io::copy(&mut Exact::new(&b"abc"[..], 5), &mut out).is_err());
        out.clear();
        io::copy(&mut Exact::new(&b"abcdef"[..], 4), &mut out).unwrap();
        assert_eq!(out, b"abcd");
    }

    #[test]
    fn progress_spans_steps() {
        let p = Progress::default();
        p.set_steps(2);
        p.begin(100);
        p.add(50);
        assert_eq!(p.fraction(), 0.25);
        p.next_step();
        p.begin(10);
        p.add(10);
        assert_eq!(p.fraction(), 1.0);
    }
}
