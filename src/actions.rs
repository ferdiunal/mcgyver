//! The Actions wheel: things done with the dropped files as a whole, rather than to each one.
//! They lean on the desktop's own tools: a folder picker (kdialog or zenity), the clipboard
//! (wl-copy or Klipper), a terminal, and KDE Connect for sending to a phone. On macOS: AppleScript's
//! folder picker, pbcopy and Terminal. Files go on the clipboard the way the platform hands it
//! over ([`serve_files_with`]).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::archive::{self, Format, Progress};
use crate::catalog;
use crate::convert::{Scratch, move_to_free_name, parent_of, split_name};
use crate::i18n::trf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Zip,
    /// To the clipboard, to be pasted as files, as Ctrl+C in a file manager.
    Copy,
    /// To the clipboard, to be moved where they are pasted.
    Cut,
    /// Into a folder picked now.
    CopyTo,
    MoveTo,
    CopyPath,
    Terminal,
    Share,
}

impl Action {
    pub const ALL: [Action; 8] = [
        Action::Zip,
        Action::Copy,
        Action::Cut,
        Action::CopyTo,
        Action::MoveTo,
        Action::CopyPath,
        Action::Terminal,
        Action::Share,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::Zip => "Zip",
            Action::Copy => "Copy",
            Action::Cut => "Cut",
            Action::CopyTo => "Copy To",
            Action::MoveTo => "Move To",
            Action::CopyPath => "Copy Path",
            Action::Terminal => "Terminal",
            Action::Share => "Share",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Action::Zip => "into one archive",
            Action::Copy | Action::Cut | Action::CopyPath => "to the clipboard",
            Action::CopyTo | Action::MoveTo => "to a folder",
            Action::Terminal => "open here",
            Action::Share => "to your phone",
        }
    }

    pub fn from_label(label: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.label() == label)
    }

    /// Whether this desktop has what the action needs.
    pub fn available(self) -> bool {
        match self {
            Action::Zip => true,
            Action::Copy => FILE_CLIPBOARD.get().is_some() || has("wl-copy"),
            // Only a clipboard that carries KDE's cut mark can say the files are to be moved.
            Action::Cut => FILE_CLIPBOARD.get().is_some_and(|c| c.can_cut),
            // Windows has its own folder picker and clipboard.
            Action::CopyTo | Action::MoveTo => cfg!(windows) || picker().is_some(),
            Action::CopyPath => {
                cfg!(windows) || has("wl-copy") || has("dbus-send") || has("pbcopy")
            }
            Action::Terminal => terminal().is_some(),
            Action::Share => has("kdeconnect-cli"),
        }
    }
}

/// What the card says the action is about to do.
pub fn preview(files: &[PathBuf], action: Action) -> String {
    let subject = subject(files);
    match action {
        Action::Zip => format!("{subject}  ›  {}", zip_name(files)),
        Action::Copy => trf("Copy {} to the clipboard", &[&subject]),
        Action::Cut => trf("Cut {}, to paste where it goes", &[&subject]),
        Action::CopyTo => trf("Copy {} to…", &[&subject]),
        Action::MoveTo => trf("Move {} to…", &[&subject]),
        Action::CopyPath if files.len() > 1 => trf("Copy {} paths", &[&files.len()]),
        Action::CopyPath => trf("Copy the path of {}", &[&subject]),
        Action::Terminal => trf(
            "Terminal in {}",
            &[&catalog::display_name(&work_dir(files))],
        ),
        Action::Share => trf("Send {} to your phone", &[&subject]),
    }
}

/// Does `action` with `files` and says how it went.
pub fn run(files: &[PathBuf], action: Action, progress: &Progress) -> io::Result<String> {
    let first = files
        .first()
        .ok_or_else(|| io::Error::other("nothing to work on"))?;
    match action {
        Action::Zip => {
            let parent = parent_of(first);
            let items: Vec<(PathBuf, String)> = files
                .iter()
                .map(|f| (f.clone(), catalog::display_name(f)))
                .collect();
            let mut part = Scratch::file(parent)?;
            archive::pack(&items, Format::Zip, part.create()?, progress)?;
            let name = zip_name(files);
            let made = part.finish(parent, name.strip_suffix(".zip").unwrap_or(&name), ".zip")?;
            Ok(trf("Saved as {}", &[&catalog::display_name(&made)]))
        }
        Action::Copy | Action::Cut => {
            let cut = action == Action::Cut;
            files_to_clipboard(files, cut)?;
            Ok(match (cut, files.len()) {
                (false, 1) => trf("Copied; paste it anywhere", &[]),
                (false, n) => trf("{} files copied; paste them anywhere", &[&n]),
                (true, 1) => trf("Cut; paste it where it goes", &[]),
                (true, n) => trf("{} files cut; paste them where they go", &[&n]),
            })
        }
        Action::CopyTo | Action::MoveTo => {
            let (title, done) = if action == Action::CopyTo {
                ("Copy {} to", "Copied to {}")
            } else {
                ("Move {} to", "Moved to {}")
            };
            let dest = pick_folder(&trf(title, &[&subject(files)]), parent_of(first))?;
            transfer(files, &dest, action, progress)?;
            Ok(trf(done, &[&catalog::display_name(&dest)]))
        }
        Action::CopyPath => {
            let text = files
                .iter()
                .map(|f| f.to_string_lossy())
                .collect::<Vec<_>>()
                .join(if cfg!(windows) { "\r\n" } else { "\n" });
            set_clipboard(&text)?;
            Ok(if files.len() > 1 {
                trf("{} paths copied", &[&files.len()])
            } else {
                trf("Path copied", &[])
            })
        }
        Action::Terminal => {
            let (name, args) = terminal().ok_or_else(|| io::Error::other("no terminal found"))?;
            let dir = work_dir(files);
            let mut child = quiet(&mut Command::new(name))
                .args(args.iter().map(|a| {
                    if *a == "{dir}" {
                        dir.as_os_str()
                    } else {
                        std::ffi::OsStr::new(a)
                    }
                }))
                .current_dir(&dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            // Reaped when it closes, so no zombie is left behind.
            std::thread::spawn(move || child.wait());
            Ok(trf("Opened {}", &[&pretty_terminal(name)]))
        }
        Action::Share => share(files),
    }
}

/// "photo.jpg" or "3 files".
fn subject(files: &[PathBuf]) -> String {
    match files {
        [one] => catalog::display_name(one),
        _ => trf("{} files", &[&files.len()]),
    }
}

/// Like Finder: one item becomes "name.zip", several "Archive.zip".
fn zip_name(files: &[PathBuf]) -> String {
    match files {
        [one] => format!("{}.zip", catalog::display_name(one)),
        _ => "Archive.zip".into(),
    }
}

/// The folder a terminal opens in: the dropped folder itself, or the one holding the file.
fn work_dir(files: &[PathBuf]) -> PathBuf {
    match files.first() {
        Some(f) if f.is_dir() => f.clone(),
        Some(f) => parent_of(f).to_owned(),
        None => PathBuf::from("."),
    }
}

/// Whether `tool` is on the PATH (as `tool.exe` on Windows, where Windows Terminal's is a link
/// only `symlink_metadata` can look at).
pub(crate) fn has(tool: &str) -> bool {
    let tool = if cfg!(windows) && Path::new(tool).extension().is_none() {
        format!("{tool}.exe")
    } else {
        tool.to_owned()
    };
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let path = dir.join(&tool);
            path.is_file() || cfg!(windows) && path.symlink_metadata().is_ok_and(|m| !m.is_dir())
        })
    })
}

/// Keeps a console program from opening a window of its own on Windows, where the app has no
/// console to lend it.
pub(crate) fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        command.creation_flags(0x0800_0000);
    }
    command
}

/// The first terminal found, with the arguments that open it in `{dir}`.
fn terminal() -> Option<(&'static str, &'static [&'static str])> {
    static FOUND: OnceLock<Option<(&str, &[&str])>> = OnceLock::new();
    *FOUND.get_or_init(|| {
        const KNOWN: &[(&str, &[&str])] = &[
            #[cfg(target_os = "macos")]
            ("open", &["-a", "Terminal", "{dir}"]),
            #[cfg(windows)]
            ("wt", &["-d", "{dir}"]),
            // A new Command Prompt in the folder the hidden one runs in.
            #[cfg(windows)]
            ("cmd", &["/c", "start", "cmd.exe"]),
            ("konsole", &["--workdir", "{dir}"]),
            ("ptyxis", &["--new-window", "--working-directory", "{dir}"]),
            ("kgx", &["--working-directory", "{dir}"]),
            ("gnome-terminal", &["--working-directory", "{dir}"]),
            ("xfce4-terminal", &["--working-directory", "{dir}"]),
            ("kitty", &["--directory", "{dir}"]),
            ("alacritty", &["--working-directory", "{dir}"]),
            ("wezterm", &["start", "--cwd", "{dir}"]),
            ("foot", &["--working-directory", "{dir}"]),
            ("xterm", &[]),
        ];
        KNOWN.iter().find(|(name, _)| has(name)).copied()
    })
}

fn pretty_terminal(name: &str) -> &str {
    match name {
        "konsole" => "Konsole",
        "kgx" | "gnome-terminal" | "ptyxis" | "open" => "Terminal",
        "wt" => "Windows Terminal",
        "cmd" => crate::i18n::tr("Command Prompt"),
        _ => name,
    }
}

fn picker() -> Option<&'static str> {
    static FOUND: OnceLock<Option<&str>> = OnceLock::new();
    *FOUND.get_or_init(|| {
        ["kdialog", "zenity", "osascript"]
            .into_iter()
            .find(|t| has(t))
    })
}

/// Asks for a folder; `Err` if none is picked.
#[cfg(windows)]
fn pick_folder(title: &str, start: &Path) -> io::Result<PathBuf> {
    crate::desktop::pick_folder(title, start)
}

/// Asks for a folder; `Err` if none is picked.
#[cfg(not(windows))]
fn pick_folder(title: &str, start: &Path) -> io::Result<PathBuf> {
    let tool =
        picker().ok_or_else(|| io::Error::other("no folder picker (kdialog or zenity) found"))?;
    let start = start.to_string_lossy();
    let mut command = Command::new(tool);
    match tool {
        "kdialog" => command.args(["--title", title, "--getexistingdirectory", &start]),
        // The title and folder go in as arguments, so nothing in them is read as AppleScript.
        "osascript" => command.args([
            "-e",
            "on run argv",
            "-e",
            "activate",
            "-e",
            "POSIX path of (choose folder with prompt (item 1 of argv) \
             default location (POSIX file (item 2 of argv)))",
            "-e",
            "end run",
            title,
            &start,
        ]),
        _ => command.args([
            "--file-selection",
            "--directory",
            &format!("--title={title}"),
            &format!("--filename={start}/"),
        ]),
    };
    let out = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    let chosen = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_owned();
    if !out.status.success() || chosen.is_empty() {
        return Err(io::Error::other("cancelled"));
    }
    Ok(PathBuf::from(chosen))
}

/// Copies or moves each item into `dest` under a free name, folders whole.
fn transfer(files: &[PathBuf], dest: &Path, action: Action, progress: &Progress) -> io::Result<()> {
    let keep = action == Action::CopyTo;
    let total: u64 = files.iter().map(|f| size_of(f)).sum();
    progress.begin(total);
    // Compared as real paths: /home may be a link to /var/home.
    let real_dest = dest.canonicalize()?;
    for file in files {
        progress.check()?;
        let name = catalog::display_name(file);
        let (base, ext) = split_name(&name, file.is_dir());
        let real = file.canonicalize()?;
        if real_dest.starts_with(&real) {
            return Err(io::Error::other(trf("{} can't go inside itself", &[&name])));
        }
        // Moving something to where it already is: nothing to do.
        if !keep && real.parent() == Some(real_dest.as_path()) {
            progress.add(size_of(file));
            continue;
        }
        // Copies go to a hidden name first and are renamed once complete, like conversions.
        let copy = |progress: &Progress| -> io::Result<()> {
            let part = Scratch::dir(dest)?;
            let staged = part.path().join("item");
            copy_tree(file, &staged, progress)?;
            move_to_free_name(&staged, dest, base, ext).map(drop)
        };
        if keep {
            copy(progress)?;
        } else {
            match move_to_free_name(file, dest, base, ext) {
                Ok(_) => progress.add(size_of(file)),
                // Another disk: copy, make sure it's written, then remove the original.
                Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
                    copy(progress)?;
                    sync(dest)?;
                    if file.is_dir() {
                        fs::remove_dir_all(file)?
                    } else {
                        fs::remove_file(file)?
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

fn size_of(path: &Path) -> u64 {
    archive::walk_size(path)
}

/// Flushes the file system holding `dir` to disk (the copies made into it).
fn sync(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        // syncfs-like: syncing every copied file would mean walking them again.
        #[cfg(target_os = "linux")]
        let status = Command::new("sync").arg("-f").arg(dir).status()?;
        // Elsewhere `sync` takes no folder and flushes everything.
        #[cfg(not(target_os = "linux"))]
        let status = {
            let _ = dir;
            Command::new("sync").status()?
        };
        if !status.success() {
            return Err(io::Error::other("couldn't make sure the copy is on disk"));
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Copies a file, a link or a whole folder, keeping times; links stay links, and pipes or
/// devices inside a folder are left out (reading one could wait forever).
fn copy_tree(from: &Path, to: &Path, progress: &Progress) -> io::Result<()> {
    progress.check()?;
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(fs::read_link(from)?, to)?;
        return Ok(());
    }
    if !meta.is_dir() && !meta.is_file() {
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()), progress)?;
        }
        fs::set_permissions(to, meta.permissions())?;
    } else {
        fs::copy(from, to)?;
        progress.add(meta.len());
    }
    if let Ok(t) = meta.modified() {
        let _ = filetime::set_file_mtime(to, filetime::FileTime::from_system_time(t));
    }
    Ok(())
}

/// Puts `files` on the clipboard, and says whether they were cut.
pub type PutFiles = Box<dyn Fn(&[PathBuf], bool) -> io::Result<()> + Send + Sync>;

/// The platform's way of putting files on the clipboard.
pub struct FileClipboard {
    pub put: PutFiles,
    /// It can mark files as cut, so pasting moves them.
    pub can_cut: bool,
}

static FILE_CLIPBOARD: OnceLock<FileClipboard> = OnceLock::new();

/// How this platform puts files on the clipboard; set once, before the wheel offers Copy.
pub fn serve_files_with(clipboard: FileClipboard) {
    let _ = FILE_CLIPBOARD.set(clipboard);
}

/// The files on the clipboard, copied or cut, through the platform's way; wl-copy can copy them
/// as a URI list otherwise.
fn files_to_clipboard(files: &[PathBuf], cut: bool) -> io::Result<()> {
    if let Some(clipboard) = FILE_CLIPBOARD.get() {
        return (clipboard.put)(files, cut);
    }
    if cfg!(windows) || cut || !has("wl-copy") {
        return Err(io::Error::other("couldn't reach the clipboard"));
    }
    let uris: String = files
        .iter()
        .map(|f| f.canonicalize().map(|p| format!("{}\r\n", file_uri(&p))))
        .collect::<io::Result<_>>()?;
    let mut child = Command::new("wl-copy")
        .args(["--type", "text/uri-list"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(uris.as_bytes())?;
    if child.wait()?.success() {
        Ok(())
    } else {
        Err(io::Error::other("couldn't reach the clipboard"))
    }
}

/// `path` (absolute) as a `file://` URI, with anything but plain characters escaped.
pub(crate) fn file_uri(path: &Path) -> String {
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
    // Windows paths as URIs: file:///C:/folder/name.
    #[cfg(not(unix))]
    let bytes = format!("/{}", path.to_string_lossy().replace('\\', "/")).into_bytes();
    let mut uri = String::from("file://");
    for &byte in bytes.iter() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// wl-copy where it exists (any Wayland desktop), else KDE's Klipper over D-Bus; pbcopy on macOS.
#[cfg(windows)]
fn set_clipboard(text: &str) -> io::Result<()> {
    crate::desktop::put_text(text)
}

/// wl-copy where it exists (any Wayland desktop), else KDE's Klipper over D-Bus; pbcopy on macOS.
#[cfg(not(windows))]
fn set_clipboard(text: &str) -> io::Result<()> {
    for tool in ["wl-copy", "pbcopy"].into_iter().filter(|t| has(t)) {
        let mut child = Command::new(tool)
            // pbcopy reads the text in the locale's encoding, which an app opened from Finder
            // doesn't have.
            .env("LC_CTYPE", "UTF-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(text.as_bytes())?;
        // wl-copy stays behind to serve the clipboard; it only has to have read the text.
        if child.wait()?.success() {
            return Ok(());
        }
    }
    if !has("dbus-send") {
        return Err(io::Error::other("couldn't reach the clipboard"));
    }
    let status = Command::new("dbus-send")
        .args([
            "--session",
            "--type=method_call",
            "--dest=org.kde.klipper",
            "/klipper",
        ])
        .arg("org.kde.klipper.klipper.setClipboardContents")
        .arg(format!("string:{text}"))
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("couldn't reach the clipboard"))
    }
}

/// Sends files to the first phone KDE Connect can reach.
fn share(files: &[PathBuf]) -> io::Result<String> {
    if files.iter().any(|f| f.is_dir()) {
        return Err(io::Error::other("folders can't be sent; zip them first"));
    }
    let out = quiet(&mut Command::new("kdeconnect-cli"))
        .args(["-a", "--id-name-only"])
        .stderr(Stdio::null())
        .output()?;
    let list = String::from_utf8_lossy(&out.stdout);
    let (id, name) = list
        .lines()
        .find_map(|line| line.split_once(' '))
        .ok_or_else(|| io::Error::other("no phone is connected (KDE Connect)"))?;
    for file in files {
        let status = quiet(&mut Command::new("kdeconnect-cli"))
            .args(["-d", id, "--share"])
            .arg(file)
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "{name} didn't take {}",
                catalog::display_name(file)
            )));
        }
    }
    Ok(trf("Sent to {}", &[&name]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_moves_keep_both_sides_straight() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        fs::create_dir_all(from.join("album/inner")).unwrap();
        fs::create_dir(&to).unwrap();
        fs::write(from.join("note.txt"), "n").unwrap();
        fs::write(from.join("album/inner/pic.jpg"), "p").unwrap();
        fs::write(to.join("note.txt"), "already here").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("inner/pic.jpg", from.join("album/link")).unwrap();

        let items = [from.join("note.txt"), from.join("album")];
        transfer(&items, &to, Action::CopyTo, &Progress::default()).unwrap();
        assert_eq!(
            fs::read_to_string(to.join("note.txt")).unwrap(),
            "already here"
        );
        assert_eq!(fs::read_to_string(to.join("note 2.txt")).unwrap(), "n");
        assert_eq!(
            fs::read_to_string(to.join("album/inner/pic.jpg")).unwrap(),
            "p"
        );
        #[cfg(unix)]
        assert_eq!(
            fs::read_link(to.join("album/link")).unwrap(),
            Path::new("inner/pic.jpg")
        );
        assert!(from.join("album").is_dir(), "copying keeps the original");

        transfer(&items, &to, Action::MoveTo, &Progress::default()).unwrap();
        assert!(!from.join("album").exists() && !from.join("note.txt").exists());
        assert!(to.join("album 2/inner/pic.jpg").is_file());
        assert_eq!(fs::read_to_string(to.join("note 3.txt")).unwrap(), "n");
        let hidden: Vec<_> = fs::read_dir(&to)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(hidden.is_empty(), "scratch left behind");
    }

    #[test]
    fn a_folder_cant_go_into_itself() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("album");
        fs::create_dir_all(album.join("sub")).unwrap();
        assert!(
            transfer(
                std::slice::from_ref(&album),
                &album.join("sub"),
                Action::CopyTo,
                &Progress::default()
            )
            .is_err()
        );
    }

    #[test]
    fn moving_to_where_it_is_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("note.txt");
        fs::write(&note, "n").unwrap();
        transfer(
            std::slice::from_ref(&note),
            dir.path(),
            Action::MoveTo,
            &Progress::default(),
        )
        .unwrap();
        assert!(note.is_file());
        assert!(!dir.path().join("note 2.txt").exists());
        // Copying there makes a duplicate, as a file manager would.
        transfer(
            std::slice::from_ref(&note),
            dir.path(),
            Action::CopyTo,
            &Progress::default(),
        )
        .unwrap();
        assert!(dir.path().join("note 2.txt").is_file() && note.is_file());
    }
}
