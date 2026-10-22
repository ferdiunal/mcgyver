//! Apps' icons as pixels, for the wheel and the settings window to draw. Where an icon lives is
//! the platform's to say: the icon theme on Linux (`linux`), AppKit on macOS (`mac`) and the
//! shell on Windows (`windows`).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

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

/// An icon's pixels: straight (not premultiplied) RGBA, row by row.
#[derive(Clone)]
pub struct Icon {
    pub size: [usize; 2],
    pub rgba: Arc<Vec<u8>>,
}

impl PartialEq for Icon {
    fn eq(&self, other: &Icon) -> bool {
        self.size == other.size && Arc::ptr_eq(&self.rgba, &other.rgba)
    }
}

impl fmt::Debug for Icon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Icon({}x{})", self.size[0], self.size[1])
    }
}

type Cache = Mutex<HashMap<(String, u32), Option<Icon>>>;

/// The icon of an app, about `px` pixels square (as close as found, never upscaled from tiny
/// bitmaps if a larger or scalable one exists), or `None`. `id` is what the platform knows the
/// app by: on Linux a desktop file id ("org.kde.dolphin.desktop", also without ".desktop", as
/// KWin names windows' apps); on macOS an .app bundle's path; on Windows an app id from
/// `Get-StartApps` (e.g. "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App" or
/// "{6D809377-...}\\Mozilla Firefox\\firefox.exe") or an absolute path to a .lnk or .exe.
/// Looked up once per (id, px): later calls, found or not, come from a cache.
pub fn app(id: &str, px: u32) -> Option<Icon> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (id.to_owned(), px);
    if let Some(known) = cache.lock().ok()?.get(&key) {
        return known.clone();
    }
    // Looked up outside the lock: two threads asking for the same icon at once just both look.
    let found = system::app(id, px);
    if let Ok(mut known) = cache.lock() {
        known.insert(key, found.clone());
    }
    found
}

/// Pixels as an icon, `None` for an image with no size.
#[cfg(any(target_os = "linux", windows))]
fn icon_of(image: image::RgbaImage) -> Option<Icon> {
    let size = [image.width() as usize, image.height() as usize];
    (size[0] > 0 && size[1] > 0).then(|| Icon {
        size,
        rgba: Arc::new(image.into_raw()),
    })
}

/// A picture scaled to fit in `px`×`px`, keeping its proportions and centred, as an icon.
#[cfg(any(target_os = "linux", windows))]
fn fit(picture: image::RgbaImage, px: u32) -> Option<Icon> {
    use image::imageops::{FilterType, overlay, resize};
    let (w, h) = picture.dimensions();
    if w == 0 || h == 0 || px == 0 {
        return None;
    }
    if (w, h) == (px, px) {
        return icon_of(picture);
    }
    let scale = px as f32 / w.max(h) as f32;
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).clamp(1, px),
        ((h as f32 * scale).round() as u32).clamp(1, px),
    );
    let scaled = resize(&picture, nw, nh, FilterType::Lanczos3);
    if (nw, nh) == (px, px) {
        return icon_of(scaled);
    }
    let mut canvas = image::RgbaImage::new(px, px);
    overlay(
        &mut canvas,
        &scaled,
        ((px - nw) / 2) as i64,
        ((px - nh) / 2) as i64,
    );
    icon_of(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_compare_by_their_pixels_being_the_same_allocation() {
        let a = Icon {
            size: [1, 1],
            rgba: Arc::new(vec![0; 4]),
        };
        let b = Icon {
            size: [1, 1],
            rgba: Arc::new(vec![0; 4]),
        };
        assert_eq!(a, a.clone());
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "Icon(1x1)");
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_wide_picture_is_centred_in_the_square() {
        let wide = image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]));
        let icon = fit(wide, 20).unwrap();
        assert_eq!(icon.size, [20, 20]);
        assert_eq!(icon.rgba[3], 0, "the top row is empty");
        assert_eq!(
            icon.rgba[(10 * 20 + 10) * 4 + 3],
            255,
            "the middle is filled"
        );
    }
}
