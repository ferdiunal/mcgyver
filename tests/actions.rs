//! The Actions wheel through its real entry point, with a stand-in folder picker: its own test
//! binary, so PATH can point at the stand-in before anything looks for a picker.

use std::fs;
use std::path::Path;

use mcgyver::actions::{self, Action};
use mcgyver::archive::Progress;

/// A `kdialog` that answers with the folder in $MCGYVER_TEST_PICK.
fn fake_picker(bin: &Path) {
    let script = bin.join("kdialog");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$MCGYVER_TEST_PICK\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn copy_keeps_the_original_and_move_takes_it() {
    let bin = tempfile::tempdir().unwrap();
    fake_picker(bin.path());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("dest");
    fs::create_dir(&dest).unwrap();
    // SAFETY: set before any other thread of this test binary reads the environment.
    unsafe {
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        std::env::set_var("MCGYVER_TEST_PICK", &dest);
    }
    let note = dir.path().join("note.txt");
    fs::write(&note, "n").unwrap();

    let said = actions::run(
        std::slice::from_ref(&note),
        Action::Copy,
        &Progress::default(),
    )
    .unwrap();
    assert_eq!(said, "Copied to dest");
    assert!(note.is_file(), "Copy removed the original");
    assert!(dest.join("note.txt").is_file());

    let said = actions::run(
        std::slice::from_ref(&note),
        Action::Move,
        &Progress::default(),
    )
    .unwrap();
    assert_eq!(said, "Moved to dest");
    assert!(!note.exists(), "Move kept the original");
    assert_eq!(fs::read_to_string(dest.join("note 2.txt")).unwrap(), "n");

    // Cancelling the picker (no folder printed) changes nothing.
    unsafe { std::env::set_var("MCGYVER_TEST_PICK", "") };
    let other = dir.path().join("other.txt");
    fs::write(&other, "o").unwrap();
    assert!(
        actions::run(
            std::slice::from_ref(&other),
            Action::Move,
            &Progress::default()
        )
        .is_err()
    );
    assert!(other.is_file());
}
