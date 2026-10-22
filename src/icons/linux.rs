//! Icons on Linux, found the way the freedesktop icon theme spec says: the current theme, the
//! themes it inherits, then hicolor, then the loose files in `pixmaps`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::{Icon, fit, icon_of};

/// The theme KDE uses when its settings don't name one.
const DEFAULT_THEME: &str = "breeze";

/// A folder of a theme and the sizes of icons the theme's `index.theme` says are in it.
#[derive(Debug, Clone)]
struct Folder {
    path: PathBuf,
    /// Pixels: the nominal size times the scale.
    size: u32,
    min: u32,
    max: u32,
}

impl Folder {
    /// How far the folder's icons are from `px`, for choosing: none when it covers it.
    fn distance(&self, px: u32) -> u32 {
        self.min.saturating_sub(px).max(px.saturating_sub(self.max))
    }
}

/// The icon folders, the best theme's first, and the folders that hold loose icons.
#[derive(Debug, Default)]
struct Themes {
    folders: Vec<Vec<Folder>>,
    loose: Vec<PathBuf>,
}

pub fn app(id: &str, px: u32) -> Option<Icon> {
    let name = crate::desk::icon_name(id)?;
    if Path::new(&name).is_absolute() {
        return load(Path::new(&name), px);
    }
    static THEMES: OnceLock<Themes> = OnceLock::new();
    let themes = THEMES.get_or_init(|| Themes::read(&base_dirs(), &current_theme()));
    load(&themes.find(&name, px)?, px)
}

/// Where icon themes are kept, from the environment.
fn base_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let mut dirs = Vec::new();
    match (var("XDG_DATA_HOME"), &home) {
        (Some(data), _) => dirs.push(PathBuf::from(data).join("icons")),
        (None, Some(home)) => dirs.push(home.join(".local/share/icons")),
        _ => {}
    }
    if let Some(home) = &home {
        dirs.push(home.join(".icons"));
    }
    let data_dirs = var("XDG_DATA_DIRS")
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join("icons")),
    );
    if let Some(home) = &home {
        dirs.push(home.join(".local/share/flatpak/exports/share/icons"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/icons"));
    dirs
}

/// The icon theme chosen in KDE's settings.
fn current_theme() -> String {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")));
    config
        .and_then(|dir| std::fs::read_to_string(dir.join("kdeglobals")).ok())
        .and_then(|text| {
            sections(&text)
                .into_iter()
                .filter(|(name, _)| name == "Icons")
                .flat_map(|(_, keys)| keys)
                .find(|(key, _)| key == "Theme")
                .map(|(_, value)| value)
        })
        .filter(|theme| !theme.is_empty())
        .unwrap_or_else(|| DEFAULT_THEME.to_owned())
}

/// An ini-style file as its sections, each with its keys in order.
fn sections(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut all: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            all.push((name.to_owned(), Vec::new()));
        } else if let Some((key, value)) = line.split_once('=')
            && let Some((_, keys)) = all.last_mut()
        {
            keys.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    all
}

impl Themes {
    /// The folders of `theme`, of the themes it inherits, and of hicolor, found in `bases`.
    fn read(bases: &[PathBuf], theme: &str) -> Themes {
        let mut order = vec![theme.to_owned()];
        let mut folders = Vec::new();
        let mut at = 0;
        while at < order.len() {
            let name = order[at].clone();
            at += 1;
            let (list, inherits) = read_theme(bases, &name);
            if !list.is_empty() {
                folders.push(list);
            }
            for parent in inherits {
                if !order.contains(&parent) {
                    order.push(parent);
                }
            }
            if at == order.len() && !order.iter().any(|t| t == "hicolor") {
                order.push("hicolor".to_owned());
            }
        }
        let mut loose: Vec<PathBuf> = bases.to_vec();
        loose.push(PathBuf::from("/usr/share/pixmaps"));
        Themes { folders, loose }
    }

    /// The file for the icon called `name`, best for `px`: a scalable one, else the smallest
    /// bitmap that is large enough, else the largest. The first theme that has it decides.
    fn find(&self, name: &str, px: u32) -> Option<PathBuf> {
        let name = ["png", "svg", "xpm"]
            .iter()
            .find_map(|ext| name.strip_suffix(&format!(".{ext}")))
            .unwrap_or(name);
        for folders in &self.folders {
            let mut vector: Option<(u32, &Folder, PathBuf)> = None;
            let mut bitmap: Option<(&Folder, PathBuf)> = None;
            for folder in folders {
                let svg = folder.path.join(format!("{name}.svg"));
                if svg.is_file() {
                    let better = vector
                        .as_ref()
                        .is_none_or(|(d, f, _)| (folder.distance(px), folder.size) < (*d, f.size));
                    if better {
                        vector = Some((folder.distance(px), folder, svg));
                    }
                }
                let png = folder.path.join(format!("{name}.png"));
                if png.is_file() && bitmap.as_ref().is_none_or(|(f, _)| closer(folder, f, px)) {
                    bitmap = Some((folder, png));
                }
            }
            if let Some((_, _, path)) = vector {
                return Some(path);
            }
            if let Some((_, path)) = bitmap {
                return Some(path);
            }
        }
        self.loose.iter().find_map(|dir| {
            ["png", "svg"]
                .iter()
                .map(|ext| dir.join(format!("{name}.{ext}")))
                .find(|p| p.is_file())
        })
    }
}

/// Whether `a`'s bitmaps suit `px` better than `b`'s: large enough beats too small, then the
/// nearer one, and of two too small the larger.
fn closer(a: &Folder, b: &Folder, px: u32) -> bool {
    let key = |f: &Folder| {
        if f.size >= px {
            (0, f.size - px)
        } else {
            (1, px - f.size)
        }
    };
    key(a) < key(b)
}

/// A theme's folders that hold apps' icons, and the themes it inherits, from its `index.theme`
/// in the first base that has one; the folders of the theme in every base.
fn read_theme(bases: &[PathBuf], name: &str) -> (Vec<Folder>, Vec<String>) {
    let Some(text) = bases
        .iter()
        .find_map(|b| std::fs::read_to_string(b.join(name).join("index.theme")).ok())
    else {
        return (Vec::new(), Vec::new());
    };
    let all = sections(&text);
    let value = |section: &[(String, String)], key: &str| {
        section
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    let header = all
        .iter()
        .find(|(n, _)| n == "Icon Theme")
        .map(|(_, keys)| keys.as_slice())
        .unwrap_or_default();
    let list = |key: &str| -> Vec<String> {
        value(header, key)
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let inherits = list("Inherits");
    let mut seen = HashSet::new();
    let mut folders = Vec::new();
    for dir in list("Directories")
        .into_iter()
        .chain(list("ScaledDirectories"))
    {
        let Some((_, keys)) = all.iter().find(|(n, _)| *n == dir) else {
            continue;
        };
        if let Some(context) = value(keys, "Context")
            && !context.eq_ignore_ascii_case("Applications")
        {
            continue;
        }
        let number = |key: &str| value(keys, key).and_then(|v| v.parse::<u32>().ok());
        let Some(nominal) = number("Size") else {
            continue;
        };
        let scale = number("Scale").unwrap_or(1).max(1);
        let size = nominal * scale;
        let (min, max) = match value(keys, "Type").as_deref() {
            Some("Scalable") => (
                number("MinSize").unwrap_or(nominal) * scale,
                number("MaxSize").unwrap_or(nominal) * scale,
            ),
            Some("Threshold") => {
                let threshold = number("Threshold").unwrap_or(2);
                (
                    nominal.saturating_sub(threshold) * scale,
                    (nominal + threshold) * scale,
                )
            }
            _ => (size, size),
        };
        for base in bases {
            let path = base.join(name).join(&dir);
            if seen.insert(path.clone()) && path.is_dir() {
                folders.push(Folder {
                    path,
                    size,
                    min,
                    max,
                });
            }
        }
    }
    (folders, inherits)
}

/// An icon file, as pixels about `px` square.
fn load(path: &Path, px: u32) -> Option<Icon> {
    let bytes = std::fs::read(path).ok()?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "svg" => render_svg(&bytes, px),
        "png" => {
            let decoded =
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
            fit(decoded.to_rgba8(), px)
        }
        _ => None,
    }
}

/// A vector picture drawn `px` square, in proportion and centred.
fn render_svg(bytes: &[u8], px: u32) -> Option<Icon> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let (w, h) = (tree.size().width(), tree.size().height());
    if w <= 0.0 || h <= 0.0 || px == 0 {
        return None;
    }
    let scale = px as f32 / w.max(h);
    let shift = |side: f32| (px as f32 - side * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_translate(shift(w), shift(h)).pre_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    icon_of(image::RgbaImage::from_raw(px, px, pixels)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">
        <rect width="64" height="64" fill="#336699"/></svg>"##;

    fn png(side: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(side, side, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn theme_file(dirs: &[(&str, u32, &str)], inherits: &str) -> String {
        let names: Vec<&str> = dirs.iter().map(|d| d.0).collect();
        let mut text = format!(
            "[Icon Theme]\nName=t\nInherits={inherits}\nDirectories={}\n",
            names.join(",")
        );
        for (dir, size, kind) in dirs {
            text += &format!("\n[{dir}]\nSize={size}\nContext=Applications\nType={kind}\n");
        }
        text
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// A base with a theme "mine" (apps at 16 and 48, scalable) that inherits "parent".
    fn base() -> tempfile::TempDir {
        let base = tempfile::tempdir().unwrap();
        let root = base.path();
        write(
            &root.join("mine/index.theme"),
            theme_file(
                &[
                    ("apps/16", 16, "Fixed"),
                    ("apps/48", 48, "Fixed"),
                    ("apps/scalable", 64, "Scalable"),
                ],
                "parent",
            )
            .as_bytes(),
        );
        write(
            &root.join("parent/index.theme"),
            theme_file(&[("apps/32", 32, "Fixed")], "").as_bytes(),
        );
        write(&root.join("mine/apps/16/small.png"), &png(16));
        write(&root.join("mine/apps/48/small.png"), &png(48));
        write(&root.join("mine/apps/16/both.png"), &png(16));
        write(&root.join("mine/apps/48/both.png"), &png(48));
        write(&root.join("mine/apps/scalable/both.svg"), SVG.as_bytes());
        write(&root.join("mine/apps/16/tiny.png"), &png(16));
        write(&root.join("parent/apps/32/inherited.png"), &png(32));
        write(&root.join("loose.png"), &png(24));
        base
    }

    fn picked(themes: &Themes, name: &str, px: u32) -> String {
        themes
            .find(name, px)
            .map(|p| {
                p.file_name().unwrap().to_string_lossy().into_owned()
                    + "@"
                    + p.parent().unwrap().file_name().unwrap().to_str().unwrap()
            })
            .unwrap_or_default()
    }

    #[test]
    fn the_smallest_bitmap_that_is_large_enough_wins() {
        let base = base();
        let themes = Themes::read(&[base.path().to_owned()], "mine");
        assert_eq!(picked(&themes, "small", 20), "small.png@48");
        assert_eq!(picked(&themes, "small", 16), "small.png@16");
        assert_eq!(
            picked(&themes, "small", 100),
            "small.png@48",
            "else the largest"
        );
        assert_eq!(picked(&themes, "tiny", 64), "tiny.png@16");
    }

    #[test]
    fn a_scalable_icon_beats_bitmaps() {
        let base = base();
        let themes = Themes::read(&[base.path().to_owned()], "mine");
        assert_eq!(picked(&themes, "both", 32), "both.svg@scalable");
    }

    #[test]
    fn inherited_themes_and_loose_files_are_searched() {
        let base = base();
        let themes = Themes::read(&[base.path().to_owned()], "mine");
        assert_eq!(picked(&themes, "inherited", 32), "inherited.png@32");
        assert_eq!(
            picked(&themes, "loose.png", 32).split('@').next(),
            Some("loose.png")
        );
        assert_eq!(picked(&themes, "nothing", 32), "");
    }

    #[test]
    fn icons_come_out_the_size_asked() {
        let base = base();
        let themes = Themes::read(&[base.path().to_owned()], "mine");
        for (name, px) in [("small", 20), ("both", 32), ("tiny", 64), ("inherited", 16)] {
            let icon = load(&themes.find(name, px).unwrap(), px).unwrap();
            assert_eq!(icon.size, [px as usize, px as usize], "{name}");
            assert_eq!(icon.rgba.len(), (px * px * 4) as usize);
        }
        let svg = load(&themes.find("both", 32).unwrap(), 32).unwrap();
        assert_eq!(
            &svg.rgba[..4],
            &[0x33, 0x66, 0x99, 255],
            "straight alpha, drawn in colour"
        );
    }

    #[test]
    fn the_theme_comes_from_kdeglobals_sections() {
        let text = "[General]\nTheme=wrong\n[Icons]\nTheme=papirus\n";
        let found = sections(text)
            .into_iter()
            .filter(|(n, _)| n == "Icons")
            .flat_map(|(_, keys)| keys)
            .find(|(k, _)| k == "Theme");
        assert_eq!(found.map(|(_, v)| v), Some("papirus".to_owned()));
    }

    /// Read-only look at the icons installed on this machine: `cargo test --lib icons -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn smoke_check_installed_apps() {
        for id in [
            "org.kde.dolphin",
            "org.kde.konsole",
            "firefox",
            "org.mozilla.firefox",
        ] {
            for px in [32, 64, 128] {
                let started = std::time::Instant::now();
                let icon = crate::icons::app(id, px);
                println!("{id} @{px}: {icon:?} in {:?}", started.elapsed());
            }
        }
    }
}
