//! McGyver: drag files, hold a modifier, drop them on a wheel of options.

pub mod actions;
pub mod archive;
pub mod catalog;
#[cfg(target_os = "linux")]
mod clipboard;
pub mod convert;
pub mod desk;
#[cfg(target_os = "linux")]
mod file_manager;
#[cfg(not(target_os = "linux"))]
mod file_manager {
    /// Selecting the results in the file manager is Dolphin's alone so far.
    pub fn select_items(_paths: Vec<std::path::PathBuf>) {}
}
pub mod glass;
pub mod i18n;
mod icon;
pub mod icons;
pub mod images;
pub mod jobs;
mod markdown;
pub mod media;
pub mod motion;
pub mod office;
pub mod overlay;
pub mod settings;
mod sound;
pub mod switcher;
mod textures;
pub mod wheel;

#[cfg(target_os = "linux")]
pub mod kde;
#[cfg(target_os = "linux")]
pub mod service;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub mod settings_window;
#[cfg(target_os = "linux")]
mod tap;
#[cfg(target_os = "linux")]
mod tray;

#[cfg(any(target_os = "macos", windows))]
pub mod desktop;
