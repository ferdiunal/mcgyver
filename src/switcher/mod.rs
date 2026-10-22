//! A task switcher for the wheel: the open windows of other apps, and bringing one forward. What
//! each system can tell and do is in `linux` (KWin's scripting, the only way a Wayland client can
//! see other windows), `mac` (one entry per running app) and `windows`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as system;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as system;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as system;
#[cfg(windows)]
pub(crate) use windows::modifiers_up;

/// A window of another app, as the switcher shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// How `activate` finds it again.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its app's name, as people know it ("Konsole", "Firefox").
    pub app: String,
    /// What the platform knows the app by, for its icon: the desktop file's name on Linux
    /// (KWin's desktopFileName, e.g. "org.kde.konsole", possibly empty), the .app bundle's path
    /// on macOS, the program's path on Windows.
    pub app_id: String,
    /// It has the keyboard now.
    pub active: bool,
}

/// The windows a task switcher would show (no docks, desktops, panels, skip-taskbar windows,
/// or this app's own windows), the one used most recently first. Empty when the system doesn't
/// answer in time.
pub fn windows() -> Vec<Window> {
    system::windows()
}

/// Brings the window forward and gives it the keyboard, unminimizing it.
pub fn activate(id: &str) -> Result<(), String> {
    system::activate(id)
}
