//! Moving the app into the Applications folder, as many Mac apps offer when they start from
//! anywhere else. From its disk image the app stops working once the image is put away, and
//! from Downloads macOS runs it from a random read-only copy ("App Translocation") that a login
//! item can't point at.
//!
//! This runs before winit, which panics if NSApplication was made before it, so the questions
//! are CoreFoundation's alerts, which need no NSApplication.

use std::ffi::{CString, OsString, c_void};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::ptr::NonNull;

use objc2_core_foundation::{
    CFOptionFlags, CFRetained, CFString, CFURL, CFUserNotification,
    kCFUserNotificationAlternateResponse, kCFUserNotificationCautionAlertLevel,
    kCFUserNotificationDefaultResponse, kCFUserNotificationNoteAlertLevel,
};
use objc2_foundation::{NSFileManager, NSString, NSURL};

use crate::i18n::{tr, trf};
use crate::settings::{APP_NAME, BUNDLE_ID};

/// Where the app belongs, when the user may add to it.
const APPLICATIONS: &str = "/Applications";
/// Left in the app's folder when the user said Do Not Move, so they aren't asked again.
const NOT_MOVED: &str = "not-moved";
/// How long the question waits, in seconds; unanswered, it is asked again at the next start.
const ALERT_TIMEOUT: f64 = 600.0;

/// What the administrator's shell runs, for an older copy at $2 that the user can't replace:
/// the app ($1) is copied beside it under a hidden name, without the quarantine, and given to
/// the user ($3, `uid:admin`); then the older copy is moved aside, the new one takes its place,
/// and the older one is removed. Not to the Trash: the user's may be closed to root (macOS's
/// privacy protections). A copy that failed halfway is removed, and the older one put back.
const INSTALL: &str = r#"set -e
dir=$(/usr/bin/dirname "$2")
name=$(/usr/bin/basename "$2")
new="$dir/.$name.$$.new"
old="$dir/.$name.$$.old"
trap '/bin/rm -rf "$new"' EXIT
/usr/bin/ditto "$1" "$new"
/usr/bin/xattr -d -r com.apple.quarantine "$new" 2>/dev/null || true
/usr/sbin/chown -R "$3" "$new"
if [ -e "$2" ]; then /bin/mv "$2" "$old"; fi
if ! /bin/mv "$new" "$2"; then
	if [ -e "$old" ]; then /bin/mv "$old" "$2"; fi
	exit 1
fi
/bin/rm -rf "$old"
"#;

/// Runs a shell script (item 2) and its arguments (the rest) as an administrator, asking for
/// the password with a prompt (item 1). AppleScript quotes every argument for the shell itself,
/// so no path is ever part of a script.
const AS_ADMIN: &str = r#"on run argv
	set cmd to "/bin/sh -c " & quoted form of item 2 of argv & " McGyver"
	repeat with arg in items 3 thru -1 of argv
		set cmd to cmd & " " & quoted form of (contents of arg)
	end repeat
	do shell script cmd with prompt (item 1 of argv) with administrator privileges
end run"#;

/// Run on its own once this process ($1) is gone, ten seconds at most: opens the moved app
/// ($2), then ejects the disk image it came from ($3, if any), which stays busy a moment while
/// macOS lets go of the app.
const START: &str = r#"i=0
while kill -0 "$1" 2>/dev/null && [ "$i" -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
/usr/bin/open -n "$2"
if [ -n "$3" ]; then
	for _ in 1 2 3 4 5; do /usr/bin/hdiutil detach -quiet "$3" && break; sleep 1; done
fi
"#;

/// Offers to move the app into the Applications folder when it runs from anywhere else, and
/// moves it: into /Applications, or ~/Applications for a user who may not add to that. True
/// when it moved and the copy there is starting, so this process should end.
pub(in crate::desktop) fn offer_move() -> bool {
    // A build being worked on runs where cargo put it, and a bare binary has no bundle.
    if cfg!(debug_assertions) {
        return false;
    }
    let Some(running) = super::bundle() else {
        return false;
    };
    let bundle = match original(&running) {
        Some(original) => original,
        // Translocated from somewhere the system doesn't say: nothing could be put away.
        None if running.to_string_lossy().contains("/AppTranslocation/") => return false,
        None => running.clone(),
    };
    let (Some(name), Some(home)) = (
        bundle.file_name(),
        std::env::var_os("HOME").map(PathBuf::from),
    ) else {
        return false;
    };
    let folder = if may_write(Path::new(APPLICATIONS)) {
        PathBuf::from(APPLICATIONS)
    } else {
        home.join("Applications")
    };
    let to = folder.join(name);
    // Already there, perhaps under another spelling (the folder's name in other letter case, a
    // link, /System/Volumes/Data), where "moving" it would put the app itself in the Trash.
    if in_applications(&bundle, &home)
        || same_file(&bundle, &to)
        || same_file(&bundle, &Path::new(APPLICATIONS).join(name))
    {
        follow_login_item(&home);
        return false;
    }
    if crate::desktop::running() {
        return false;
    }
    let Some(dir) = crate::settings::app_dir() else {
        return false;
    };
    if said_no(&dir) {
        return false;
    }
    crate::i18n::set(crate::settings::Settings::load().language);
    let icon = running.join("Contents/Resources/AppIcon.icns");
    match ask(&icon) {
        Some(true) => {}
        Some(false) => {
            remember_no(&dir);
            return false;
        }
        None => return false,
    }
    // The alert waits for as long as the user takes: a login item may have started the copy
    // in the Applications folder meanwhile, which moving would put in the Trash.
    if crate::desktop::running() {
        return false;
    }
    match move_to(&running, &bundle, &to) {
        Ok(true) => true,
        // The administrator's password was asked for, and not given: asking at every start
        // would do no better.
        Ok(false) => {
            remember_no(&dir);
            false
        }
        Err(e) => {
            eprintln!(
                "McGyver: couldn't move {} to {}: {e}",
                bundle.display(),
                folder.display()
            );
            let header = trf("Couldn't move {} to the Applications folder", &[&APP_NAME]);
            tell(&icon, &header, &e);
            false
        }
    }
}

/// Whether `bundle` is in /Applications or ~/Applications, however deep.
fn in_applications(bundle: &Path, home: &Path) -> bool {
    bundle.starts_with(APPLICATIONS) || bundle.starts_with(home.join("Applications"))
}

/// Whether `a` and `b` are the same file, however they are spelt.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// A login item made before the app moved starts the copy it was moved from. Settings writes
/// the item for the program that asks, so the moved copy writes it again once it runs, if it
/// starts anything else; there's no need to touch it at every start.
fn follow_login_item(home: &Path) {
    if !crate::settings::starts_at_login() {
        return;
    }
    let agent = home
        .join("Library/LaunchAgents")
        .join(format!("{BUNDLE_ID}.plist"));
    let exe = crate::settings::own_exe().and_then(|exe| exe.canonicalize());
    let (Ok(plist), Ok(exe)) = (std::fs::read_to_string(agent), exe) else {
        return;
    };
    if starts(&plist, &exe) {
        return;
    }
    if let Err(e) = crate::settings::set_start_at_login(true) {
        eprintln!(
            "McGyver: can't point the login item at {}: {e}",
            exe.display()
        );
    }
}

/// Whether the launch agent `plist` starts `exe`, written as settings writes it.
fn starts(plist: &str, exe: &Path) -> bool {
    let exe = exe
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    plist.contains(&format!("<string>{exe}</string>"))
}

/// Whether the user said Do Not Move before.
fn said_no(dir: &Path) -> bool {
    dir.join(NOT_MOVED).exists()
}

/// Remembers that the user would rather keep the app where it is.
fn remember_no(dir: &Path) {
    let written =
        std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(NOT_MOVED), ""));
    if let Err(e) = written {
        eprintln!("McGyver: can't remember not moving: {e}");
    }
}

/// Where the bundle really is when macOS runs it translocated: a quarantined app opened where
/// it was downloaded runs from a random read-only copy. None when it isn't, or when the system
/// doesn't say (the calls aren't in Apple's public headers, so they are looked up).
fn original(running: &Path) -> Option<PathBuf> {
    type IsTranslocated = unsafe extern "C" fn(*const CFURL, *mut bool, *mut c_void) -> u8;
    type OriginalPath = unsafe extern "C" fn(*const CFURL, *mut c_void) -> *mut CFURL;
    // SAFETY: the names are NUL-terminated; the framework is never closed, so what is looked up
    // stays valid. Both functions have had these signatures since macOS 10.12.
    let (is_translocated, original_path) = unsafe {
        let security = libc::dlopen(
            c"/System/Library/Frameworks/Security.framework/Security".as_ptr(),
            libc::RTLD_LAZY,
        );
        if security.is_null() {
            return None;
        }
        let is = libc::dlsym(security, c"SecTranslocateIsTranslocatedURL".as_ptr());
        let original = libc::dlsym(security, c"SecTranslocateCreateOriginalPathForURL".as_ptr());
        if is.is_null() || original.is_null() {
            return None;
        }
        (
            std::mem::transmute::<*mut c_void, IsTranslocated>(is),
            std::mem::transmute::<*mut c_void, OriginalPath>(original),
        )
    };
    let url = CFURL::from_directory_path(running)?;
    let mut translocated = false;
    // SAFETY: the URL is alive, `translocated` is room for the answer, and no error is asked
    // for.
    let asked = unsafe { is_translocated(&*url, &mut translocated, std::ptr::null_mut()) };
    if asked == 0 || !translocated {
        return None;
    }
    // SAFETY: as above.
    let original = NonNull::new(unsafe { original_path(&*url, std::ptr::null_mut()) })?;
    // SAFETY: a URL this process owns ("Create"), released when dropped.
    let original = unsafe { CFRetained::from_raw(original) };
    // Put together again from its parts, without the slash a folder's URL ends with.
    original
        .to_file_path()
        .map(|path| path.components().collect())
}

/// The mount point of the read-only disk image `bundle` sits at the top of, if it does: the
/// image to eject once the app has moved. Deeper in, or on a writable image, the bundle goes to
/// the Trash as it would from anywhere else.
fn disk_image(bundle: &Path) -> Option<PathBuf> {
    let volume = volume_of(bundle)?;
    if bundle.parent() != Some(volume.as_path()) {
        return None;
    }
    let info = Command::new("/usr/bin/hdiutil")
        .arg("info")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    read_only_image_at(&String::from_utf8_lossy(&info.stdout), &volume).then_some(volume)
}

/// `/Volumes/<name>` for a path inside a volume there.
fn volume_of(path: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix("/Volumes").ok()?;
    let Some(Component::Normal(name)) = rest.components().next() else {
        return None;
    };
    Some(Path::new("/Volumes").join(name))
}

/// Whether `hdiutil info` lists a read-only disk image mounted at `volume`. Each image has a
/// block of its own after a line of `=`: `key : value` lines, `writeable` among them, then a
/// line for each of its partitions with the device, its type and where it is mounted, tab
/// separated.
fn read_only_image_at(info: &str, volume: &Path) -> bool {
    let mut read_only = false;
    for line in info.lines() {
        if line.starts_with('=') {
            read_only = false;
        } else if line.starts_with("/dev/") {
            let at = line.rsplit('\t').next().unwrap_or_default();
            if Path::new(at.trim()) == volume {
                return read_only;
            }
        } else if let Some((key, value)) = line.split_once(':')
            && key.trim() == "writeable"
        {
            read_only = value.trim().eq_ignore_ascii_case("false");
        }
    }
    false
}

/// Asks whether to move: true for Move, false for Do Not Move, None when the alert went
/// unanswered (closed otherwise, or left for ten minutes) or couldn't be shown.
fn ask(icon: &Path) -> Option<bool> {
    let icon = CFURL::from_file_path(icon);
    let header = CFString::from_str(tr("Move to the Applications folder?"));
    let message = CFString::from_str(tr(
        "McGyver runs best from the Applications folder: there it keeps working after the disk \
         image is put away, and can start at login.",
    ));
    let move_it = CFString::from_str(tr("Move to Applications"));
    let keep = CFString::from_str(tr("Do Not Move"));
    let mut response: CFOptionFlags = 0;
    // SAFETY: every argument is a live CoreFoundation object or none, and `response` is room
    // for the answer.
    let failed = unsafe {
        CFUserNotification::display_alert(
            ALERT_TIMEOUT,
            kCFUserNotificationNoteAlertLevel,
            icon.as_deref(),
            None,
            None,
            Some(&header),
            Some(&message),
            Some(&move_it),
            Some(&keep),
            None,
            &mut response,
        )
    };
    if failed != 0 {
        eprintln!("McGyver: couldn't ask about moving to {APPLICATIONS} ({failed})");
        return None;
    }
    answer(response)
}

/// The button an alert's response names, in its lowest two bits: true for the default one
/// (Move), false for the alternate one (Do Not Move), None for none (closed, or timed out).
fn answer(response: CFOptionFlags) -> Option<bool> {
    let button = response & 0x3;
    if button == kCFUserNotificationDefaultResponse {
        Some(true)
    } else if button == kCFUserNotificationAlternateResponse {
        Some(false)
    } else {
        None
    }
}

/// Shows what went wrong, with an OK button. It doesn't wait: the app starts meanwhile.
fn tell(icon: &Path, header: &str, message: &str) {
    let icon = CFURL::from_file_path(icon);
    CFUserNotification::display_notice(
        0.0,
        kCFUserNotificationCautionAlertLevel,
        icon.as_deref(),
        None,
        None,
        Some(&CFString::from_str(header)),
        Some(&CFString::from_str(message)),
        None,
    );
}

/// Copies the running app to `to`, starts the copy once this process has ended, and puts away
/// the bundle it came from: the disk image is ejected, a bundle anywhere else goes to the Trash.
/// False when the user cancelled the administrator's password, and nothing was done.
fn move_to(running: &Path, bundle: &Path, to: &Path) -> Result<bool, String> {
    // Only an older McGyver is replaced, never another app that has the name.
    if to.symlink_metadata().is_ok() && bundle_id(to).as_deref() != Some(BUNDLE_ID) {
        return Err(format!("{} is another app", to.display()));
    }
    let image = disk_image(bundle);
    // The running copy is the one this process surely may read: a translocated app's original
    // is often in Downloads, a folder macOS asks about first.
    if !copy(running, to)? {
        return Ok(false);
    }
    start_after_exit(to, image.as_deref()).map_err(|e| e.to_string())?;
    // Running from the bundle doesn't keep it from the Trash: the program is loaded already. A
    // disk image is read-only, and is ejected instead.
    if image.is_none()
        && let Err(e) = trash(bundle)
    {
        eprintln!("McGyver: left {} where it was: {e}", bundle.display());
    }
    Ok(true)
}

/// The identifier in the Info.plist of the bundle at `app`, written as XML or not.
fn bundle_id(app: &Path) -> Option<String> {
    let read = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(app.join("Contents/Info.plist"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    read.status
        .success()
        .then(|| String::from_utf8_lossy(&read.stdout).trim().to_owned())
}

/// Puts a copy of `from` at `to`. It is made beside `to` under a hidden name, and only once
/// that worked does an older copy go to the Trash and the new one take its place: a copy that
/// fails halfway leaves the older one as it was. An older copy in /Applications that the user
/// can't put in the Trash (an administrator installed it) is replaced with an administrator's
/// password; false when that was cancelled.
fn copy(from: &Path, to: &Path) -> Result<bool, String> {
    // ~/Applications may not be there yet.
    if let Some(folder) = to.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    let mut hidden = OsString::from(".");
    hidden.push(to.file_name().unwrap_or_default());
    hidden.push(format!(".{}", std::process::id()));
    let staged = to.with_file_name(hidden);
    let discard = || {
        let _ = std::fs::remove_dir_all(&staged);
    };
    // One left by a start that crashed would be merged into, not replaced.
    discard();
    // ditto keeps the signature, the extended attributes and the links inside the bundle.
    let copied = match Command::new("/usr/bin/ditto")
        .arg(from)
        .arg(&staged)
        .stdin(Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(complaint(&out)),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = copied {
        discard();
        return Err(e);
    }
    // The user has opened the app already, and a quarantined copy would be translocated again.
    let _ = Command::new("/usr/bin/xattr")
        .args(["-d", "-r", "com.apple.quarantine"])
        .arg(&staged)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if to.symlink_metadata().is_ok()
        && let Err(e) = trash(to)
    {
        discard();
        if to.parent() != Some(Path::new(APPLICATIONS)) {
            return Err(e);
        }
        eprintln!(
            "McGyver: can't put the older {} in the Trash: {e}",
            to.display()
        );
        return copy_as_admin(from, to);
    }
    if let Err(e) = std::fs::rename(&staged, to) {
        discard();
        return Err(e.to_string());
    }
    Ok(true)
}

/// `copy` as an administrator, through AppleScript, which asks for the password: `INSTALL`.
/// False when the user cancelled it.
fn copy_as_admin(from: &Path, to: &Path) -> Result<bool, String> {
    // SAFETY: no arguments, and it can't fail.
    let uid = unsafe { libc::getuid() };
    let mut osascript = Command::new("/usr/bin/osascript");
    for line in AS_ADMIN.lines() {
        osascript.arg("-e").arg(line);
    }
    let done = osascript
        .arg(format!("{APP_NAME}: {}", tr("Move to Applications")))
        .arg(INSTALL)
        .arg(from)
        .arg(to)
        .arg(format!("{uid}:admin"))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if done.status.success() {
        return Ok(true);
    }
    let said = complaint(&done);
    // AppleScript's "User canceled." error.
    if said.contains("(-128)") {
        return Ok(false);
    }
    Err(script_error(&said).to_owned())
}

/// What a command that failed said, or how it ended when it said nothing.
fn complaint(out: &Output) -> String {
    let said = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    if said.is_empty() {
        out.status.to_string()
    } else {
        said
    }
}

/// The message in osascript's error, without where in the script it happened.
fn script_error(said: &str) -> &str {
    said.split_once("execution error: ")
        .map_or(said, |(_, message)| message)
}

/// Whether this user may add to `dir`.
fn may_write(dir: &Path) -> bool {
    let Ok(dir) = CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: a NUL-terminated path, only read.
    unsafe { libc::access(dir.as_ptr(), libc::W_OK) == 0 }
}

/// Puts `path` in the Trash, as Finder's Move to Trash does.
fn trash(path: &Path) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|e| e.localizedDescription().to_string())
    })
}

/// Starts `START` on its own: it outlives this process, which ends right after.
fn start_after_exit(app: &Path, image: Option<&Path>) -> std::io::Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(START)
        .arg(APP_NAME)
        .arg(std::process::id().to_string())
        .arg(app)
        .arg(image.unwrap_or(Path::new("")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // A group of its own: a terminal closing on this process doesn't end it too.
        .process_group(0)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applications_folders_at_any_depth() {
        let home = Path::new("/Users/me");
        for inside in [
            "/Applications/McGyver.app",
            "/Applications/Tools/McGyver.app",
            "/Users/me/Applications/McGyver.app",
        ] {
            assert!(in_applications(Path::new(inside), home), "{inside}");
        }
        for outside in [
            "/Users/me/Downloads/McGyver.app",
            "/Users/you/Applications/McGyver.app",
            "/ApplicationsOld/McGyver.app",
            "/Volumes/McGyver/McGyver.app",
        ] {
            assert!(!in_applications(Path::new(outside), home), "{outside}");
        }
    }

    #[test]
    fn volumes_are_named_by_their_first_folder() {
        assert_eq!(
            volume_of(Path::new("/Volumes/McGyver/McGyver.app")),
            Some(PathBuf::from("/Volumes/McGyver"))
        );
        assert_eq!(
            volume_of(Path::new("/Volumes/McGyver 1/McGyver.app")),
            Some(PathBuf::from("/Volumes/McGyver 1"))
        );
        assert_eq!(volume_of(Path::new("/Volumes")), None);
        assert_eq!(volume_of(Path::new("/Volumes/../Users/McGyver.app")), None);
        assert_eq!(volume_of(Path::new("/Users/me/McGyver.app")), None);
    }

    #[test]
    fn hdiutil_says_which_volumes_are_read_only_disk_images() {
        let info = "framework       : 671.100.4\n\
                    driver          : 671.100.4\n\
                    ================================================\n\
                    image-path      : /Users/me/Downloads/McGyver-1.0.0-universal.dmg\n\
                    image-type      : read-only disk image\n\
                    writeable       : FALSE\n\
                    /dev/disk4          \tGUID_partition_scheme          \t\n\
                    /dev/disk4s1        \tApple_APFS                     \t\n\
                    /dev/disk5          \tEF57347C-0000-11AA-AA11-0030654\t\n\
                    /dev/disk5s1        \t41504653-0000-11AA-AA11-0030654\t/Volumes/McGyver 1\n\
                    ================================================\n\
                    image-path      : /Users/me/Work.sparseimage\n\
                    writeable       : TRUE\n\
                    /dev/disk6          \tGUID_partition_scheme          \t\n\
                    /dev/disk6s1        \tApple_HFS                      \t/Volumes/Work\n\
                    ================================================\n\
                    image-path      : /Users/me/Old.dmg\n\
                    /dev/disk7s1        \tApple_HFS                      \t/Volumes/Old\n";
        assert!(read_only_image_at(info, Path::new("/Volumes/McGyver 1")));
        assert!(!read_only_image_at(info, Path::new("/Volumes/Work")));
        // A block without `writeable` isn't taken for read-only.
        assert!(!read_only_image_at(info, Path::new("/Volumes/Old")));
        assert!(!read_only_image_at(info, Path::new("/Volumes/McGyver")));
        assert!(!read_only_image_at("", Path::new("/Volumes/McGyver")));
    }

    #[test]
    fn only_do_not_move_is_a_no() {
        assert_eq!(answer(0), Some(true));
        assert_eq!(answer(1), Some(false));
        // The other button (there is none) and Cancel, which a timeout answers too.
        assert_eq!(answer(2), None);
        assert_eq!(answer(3), None);
        // Above the two bits, check boxes and pop-ups.
        assert_eq!(answer(0x100 | 1), Some(false));
    }

    #[test]
    fn login_items_are_read_as_settings_writes_them() {
        let plist = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <plist version=\"1.0\"><dict>\n\
                     <key>Label</key><string>io.github.ferdiunal.mcgyver</string>\n\
                     <key>ProgramArguments</key><array><string>/Users/me/R&amp;D/McGyver.app/\
                     Contents/MacOS/McGyver</string></array>\n\
                     </dict></plist>\n";
        assert!(starts(
            plist,
            Path::new("/Users/me/R&D/McGyver.app/Contents/MacOS/McGyver")
        ));
        assert!(!starts(
            plist,
            Path::new("/Applications/McGyver.app/Contents/MacOS/McGyver")
        ));
    }

    #[test]
    fn do_not_move_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("McGyver");
        assert!(!said_no(&app_dir));
        remember_no(&app_dir);
        assert!(said_no(&app_dir));
    }

    #[test]
    fn the_same_file_under_another_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("McGyver.app");
        std::fs::create_dir(&app).unwrap();
        assert!(same_file(&app, &dir.path().join(".").join("McGyver.app")));
        assert!(!same_file(&app, dir.path()));
        assert!(!same_file(&app, &dir.path().join("Other.app")));
    }

    #[test]
    fn osascript_errors_lose_where_they_happened() {
        assert_eq!(
            script_error(
                "0:263: execution error: ditto: /Applications/McGyver.app: Permission denied (1)"
            ),
            "ditto: /Applications/McGyver.app: Permission denied (1)"
        );
        assert_eq!(script_error("exit status: 1"), "exit status: 1");
    }

    /// Runs `INSTALL` from `from/McGyver.app` (made with a file `new` when `copyable`) over
    /// `Apps/McGyver.app` (made with a file `old`), giving the copy to the folder's owner.
    /// Whether it worked, and the folder the paths are in.
    fn install(copyable: bool) -> (bool, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (
            dir.path().join("from/McGyver.app"),
            dir.path().join("Apps/McGyver.app"),
        );
        std::fs::create_dir_all(to.join("Contents")).unwrap();
        std::fs::write(to.join("Contents/old"), "old").unwrap();
        if copyable {
            std::fs::create_dir_all(from.join("Contents")).unwrap();
            std::fs::write(from.join("Contents/new"), "new").unwrap();
        }
        let owner = std::fs::metadata(dir.path()).unwrap();
        let ran = Command::new("/bin/sh")
            .arg("-c")
            .arg(INSTALL)
            .arg(APP_NAME)
            .args([&from, &to])
            .arg(format!("{}:{}", owner.uid(), owner.gid()))
            .stderr(Stdio::null())
            .status()
            .unwrap();
        (ran.success(), dir)
    }

    fn names(dir: &Path) -> Vec<OsString> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_install_script_replaces_the_older_copy_once_the_new_one_is_made() {
        let (worked, dir) = install(true);
        assert!(worked);
        let to = dir.path().join("Apps/McGyver.app");
        assert!(to.join("Contents/new").exists());
        assert!(!to.join("Contents/old").exists());
        assert_eq!(names(&dir.path().join("Apps")), ["McGyver.app"]);
    }

    #[test]
    fn a_failed_copy_leaves_the_older_one() {
        let (worked, dir) = install(false);
        assert!(!worked);
        assert!(dir.path().join("Apps/McGyver.app/Contents/old").exists());
        assert_eq!(names(&dir.path().join("Apps")), ["McGyver.app"]);
    }
}
