//! McGyver: drag files, hold a modifier, drop them on a wheel of options.

pub mod actions;
pub mod archive;
pub mod catalog;
#[cfg(target_os = "linux")]
mod clipboard;
pub mod convert;
mod file_manager;
pub mod glass;
pub mod i18n;
mod icon;
pub mod images;
pub mod jobs;
mod markdown;
pub mod media;
pub mod office;
pub mod overlay;
pub mod settings;
#[cfg(target_os = "linux")]
mod sound;
pub mod wheel;

#[cfg(target_os = "linux")]
pub mod kde;
#[cfg(target_os = "linux")]
pub mod service;
#[cfg(target_os = "linux")]
pub mod settings_window;
#[cfg(target_os = "linux")]
mod tap;
#[cfg(target_os = "linux")]
mod tray;
