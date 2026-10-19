//! What the app left behind when it was called Zero-Click carries over to McGyver. Its own test
//! binary: it points `XDG_CONFIG_HOME` at a temporary folder for the whole process. Zero-Click
//! only ever ran on Linux, where starting at login is an XDG autostart entry.
#![cfg(target_os = "linux")]

use std::fs;

use mcgyver::settings::{self, Settings, Trigger};

#[test]
fn zero_click_settings_and_autostart_carry_over() {
    let config = tempfile::tempdir().unwrap();
    // SAFETY: the only test in this binary, so nothing else reads the environment meanwhile.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", config.path()) };
    let old = config.path().join("zero-click-converter");
    fs::create_dir_all(&old).unwrap();
    fs::write(
        old.join("settings.conf"),
        "# Zero-Click settings\nlanguage = tr\ntrigger = ctrl\n",
    )
    .unwrap();
    let autostart = config.path().join("autostart");
    fs::create_dir_all(&autostart).unwrap();
    fs::write(
        autostart.join("zero-click-converter.desktop"),
        "[Desktop Entry]\nExec=\"/old/zero-click-converter\"\n",
    )
    .unwrap();

    settings::migrate();
    let carried = Settings::load();
    assert_eq!(carried.trigger, Trigger::Ctrl);
    assert_eq!(carried.language.code(), "tr");
    assert!(!autostart.join("zero-click-converter.desktop").exists());
    assert!(settings::starts_at_login());
    let entry = fs::read_to_string(autostart.join("mcgyver.desktop")).unwrap();
    assert!(entry.contains("Name=McGyver"), "{entry}");
    // The old settings stay where they were.
    assert!(old.join("settings.conf").exists());

    // Settings made under the new name aren't overwritten by the old ones.
    let mut mine = carried;
    mine.trigger = Trigger::Alt;
    mine.save().unwrap();
    settings::migrate();
    assert_eq!(Settings::load().trigger, Trigger::Alt);
}
