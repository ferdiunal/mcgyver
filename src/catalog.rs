//! What the wheel offers for a set of files. Purely descriptive: nothing here touches the files.

use std::path::{Path, PathBuf};

use crate::actions::Action;
use crate::archive::Format;
use crate::i18n::{tr, trf};
use crate::images;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Convert,
    Tools,
    Actions,
    /// The wheel without files (see `desk`): the player,
    Media,
    /// the keyboard layout, volume and brightness,
    Everyday,
    /// the apps open and the quick apps,
    Apps,
    /// and switches and actions for the whole computer.
    System,
}

impl Mode {
    /// The modes of a wheel of files.
    pub const ALL: [Mode; 3] = [Mode::Convert, Mode::Tools, Mode::Actions];
    /// The modes of the wheel without files, in ring order.
    pub const DESK: [Mode; 4] = [Mode::Apps, Mode::Media, Mode::Everyday, Mode::System];

    pub fn title(self) -> &'static str {
        match self {
            Mode::Convert => "CONVERT",
            Mode::Tools => "TOOLS",
            Mode::Actions => "ACTIONS",
            Mode::Media => "MEDIA",
            Mode::Everyday => "EVERYDAY",
            Mode::Apps => "APPS",
            Mode::System => "SYSTEM",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Audio,
    Video,
    Pdf,
    Document,
    Spreadsheet,
    Slides,
    Subtitle,
    /// Holds files: ZIP, 7Z, RAR, TAR (compressed or not).
    Archive,
    /// One compressed file: GZ, XZ, ZST…
    Compressed,
    Folder,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Image => "Image",
            Kind::Audio => "Audio",
            Kind::Video => "Video",
            Kind::Pdf => "PDF",
            Kind::Document => "Document",
            Kind::Spreadsheet => "Spreadsheet",
            Kind::Slides => "Slides",
            Kind::Subtitle => "Subtitles",
            Kind::Archive => "Archive",
            Kind::Compressed => "Compressed",
            Kind::Folder => "Folder",
        }
    }
}

/// The first wedge for archives and compressed files.
pub const UNZIP: &str = "Unzip";
/// Joins the files into one; offered only for two or more.
pub const MERGE: &str = "Merge";
pub const FLATTEN: &str = "Flatten";
/// Subtitles' re-timing, whose bubbles say how far.
pub const SHIFT: &str = "Shift";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub mode: Mode,
    pub label: &'static str,
    pub hint: &'static str,
}

/// Lowercase extension with common aliases folded together. Compressed files get their
/// format's canonical extension, whole: "tar.gz" for both .tar.gz and .tgz.
pub fn ext_of(path: &Path) -> String {
    if let Some(format) = Format::from_name(path) {
        return format.ext().into();
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpeg" | "jpe" | "jfif" => "jpg".into(),
        "tif" => "tiff".into(),
        "aif" => "aiff".into(),
        "htm" => "html".into(),
        "heif" | "hif" => "heic".into(),
        "dib" => "bmp".into(),
        "svgz" => "svg".into(),
        "markdown" => "md".into(),
        _ => ext,
    }
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    // Paths that don't exist (yet) are judged by name.
    let meta = std::fs::metadata(path).ok();
    match &meta {
        Some(m) if m.is_dir() => return Some(Kind::Folder),
        // A pipe or device named like a file is not one.
        Some(m) if !m.is_file() => return None,
        _ => {}
    }
    if let Some(format) = Format::from_name(path) {
        return Some(archive_kind(format));
    }
    let ext = ext_of(path);
    if images::EXTENSIONS.contains(&ext.as_str()) || images::is_raw(&ext) {
        return Some(Kind::Image);
    }
    Some(match ext.as_str() {
        "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aiff" | "wma" => Kind::Audio,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "m4v" => Kind::Video,
        "pdf" => Kind::Pdf,
        "docx" | "doc" | "pages" | "md" | "txt" | "epub" | "rtf" | "rtfd" | "odt" | "html" => {
            Kind::Document
        }
        "xlsx" | "xls" | "ods" | "csv" | "tsv" | "numbers" => Kind::Spreadsheet,
        "pptx" | "ppt" | "key" | "odp" => Kind::Slides,
        "srt" | "vtt" => Kind::Subtitle,
        // An unknown name may still be an archive (a backup without an extension, an .ipa…).
        _ if meta.is_some() => return Format::sniff(path).map(archive_kind),
        _ => return None,
    })
}

fn archive_kind(format: Format) -> Kind {
    if format.is_archive() {
        Kind::Archive
    } else {
        Kind::Compressed
    }
}

/// The name without its extension; compressed files lose the whole suffix (photos.tar.gz →
/// photos), folders keep their name.
pub fn stem_of(path: &Path) -> String {
    if path.is_dir() {
        display_name(path)
    } else {
        crate::archive::stem(path)
    }
}

fn convert_labels(kind: Kind, ext: &str) -> Vec<&'static str> {
    match kind {
        // HEIC and AVIF only where libheif can write them.
        Kind::Image => images::Target::ALL
            .iter()
            .filter(|t| t.available())
            .map(|t| t.label())
            .collect(),
        Kind::Audio if crate::media::available() => {
            vec!["MP3", "M4A", "WAV", "FLAC", "OGG", "OPUS", "AIFF"]
        }
        Kind::Video if crate::media::available() => vec!["MP4", "MOV", "MKV", "WEBM", "GIF", "MP3"],
        Kind::Document | Kind::Spreadsheet | Kind::Slides => {
            let mut labels: Vec<&'static str> = crate::office::Family::of(ext)
                .map(|family| family.targets().to_vec())
                .unwrap_or_default();
            if crate::markdown::reads(ext) {
                labels.push("MD");
            }
            labels
        }
        Kind::Audio | Kind::Video => Vec::new(),
        Kind::Pdf => vec!["DOCX", "PPTX", "XLSX", "JPG", "PNG", "EPUB", "TXT", "MD"],
        Kind::Subtitle => vec!["SRT", "VTT", "TXT"],
        Kind::Archive => vec![
            UNZIP, "ZIP", "7Z", "TAR.GZ", "TAR.XZ", "TAR.ZST", "TAR.BZ2", "TAR",
        ],
        Kind::Folder => vec!["ZIP", "7Z", "TAR.GZ", "TAR.XZ", "TAR.ZST", "TAR.BZ2", "TAR"],
        Kind::Compressed => vec![UNZIP, "GZ", "XZ", "ZST", "BZ2", "LZ4", "ZIP", "7Z"],
    }
}

fn convert_hint(label: &str) -> &'static str {
    match label {
        "JPG" => "photos",
        "PNG" => "lossless",
        "WEBP" => "for the web",
        "HEIC" => "Apple photos",
        "AVIF" => "smallest",
        "TIFF" => "print",
        "PDF" => "share & print",
        "MP3" => "plays anywhere",
        "M4A" => "AAC",
        "WAV" | "AIFF" => "uncompressed",
        "FLAC" => "lossless",
        "OGG" | "OPUS" => "open codec",
        "MP4" => "plays anywhere",
        "MOV" => "QuickTime",
        "MKV" => "Matroska",
        "WEBM" => "for the web",
        "GIF" => "256 colours",
        "BMP" => "uncompressed",
        "ICO" => "app icon",
        UNZIP => "unpack here",
        "DOCX" => "Word",
        "PPTX" => "PowerPoint",
        "XLSX" => "Excel",
        "PAGES" => "Apple Pages",
        "EPUB" => "e-book",
        "MD" => "Markdown",
        "TXT" => "plain text",
        "HTML" => "web page",
        "ODS" | "ODT" => "OpenDocument",
        "RTF" => "rich text",
        "CSV" | "TSV" => "raw table",
        "ZIP" => "opens anywhere",
        "7Z" => "smallest",
        "TAR.GZ" | "GZ" => "gzip",
        "TAR.XZ" | "XZ" => "small",
        "TAR.ZST" | "ZST" => "fast and small",
        "TAR.BZ2" | "BZ2" => "bzip2",
        "TAR" => "no compression",
        "LZ4" => "fastest",
        _ => "",
    }
}

fn tools(kind: Kind) -> &'static [(&'static str, &'static str)] {
    match kind {
        Kind::Image => &[
            ("Compress", "smaller file"),
            ("Resize", "fit a size"),
            ("Crop", "trim edges"),
            ("Rotate", "90°"),
            ("Strip EXIF", "remove photo details"),
        ],
        // FFmpeg does the work.
        Kind::Audio | Kind::Video if !crate::media::available() => &[],
        Kind::Audio => &[
            ("Trim", "cut silence"),
            ("Normalize", "even volume"),
            ("Compress", "smaller file"),
            (MERGE, "join tracks"),
        ],
        Kind::Video => &[
            ("Compress", "smaller file"),
            ("Trim", "cut black ends"),
            ("Mute", "drop audio"),
            ("Rotate", "90°"),
            ("Frames", "export stills"),
        ],
        Kind::Pdf => &[
            ("Compress", "smaller file"),
            ("Split", "one per page"),
            (MERGE, "join PDFs"),
            ("Rotate", "90°"),
        ],
        Kind::Document | Kind::Spreadsheet => &[(MERGE, "join files")],
        Kind::Slides => &[("Compress", "smaller file")],
        Kind::Subtitle => &[(SHIFT, "re-time")],
        Kind::Archive | Kind::Compressed => &[],
        Kind::Folder => &[(FLATTEN, "pull files up")],
    }
}

/// Options common to every recognised file in `files`.
///
/// Unrecognised files are ignored for conversions and tools, the way a mixed selection
/// should still work. A file already in the target format counts as compatible.
/// The modes with something to offer for these files, in ring order.
pub fn modes(files: &[PathBuf]) -> Vec<Mode> {
    Mode::ALL
        .into_iter()
        .filter(|mode| !choices(files, *mode).is_empty())
        .collect()
}

/// The options a choice pops out as bubbles: where Share can send the files, and how far Shift
/// moves subtitles.
pub fn subs(choice: Choice) -> Vec<Choice> {
    if choice.mode == Mode::Tools && choice.label == SHIFT {
        return crate::subtitles::SHIFTS
            .iter()
            .map(|&(label, _)| Choice {
                mode: Mode::Tools,
                label,
                hint: "",
            })
            .collect();
    }
    if choice.mode != Mode::Actions || Action::from_label(choice.label) != Some(Action::Share) {
        return Vec::new();
    }
    Action::shares()
        .into_iter()
        .map(|a| Choice {
            mode: Mode::Actions,
            label: a.label(),
            hint: a.hint(),
        })
        .collect()
}

pub fn choices(files: &[PathBuf], mode: Mode) -> Vec<Choice> {
    let known: Vec<(String, Kind)> = files
        .iter()
        .filter_map(|f| kind_of(f).map(|k| (ext_of(f), k)))
        .collect();
    match mode {
        // The messaging apps pop out of Share (see `subs`).
        Mode::Actions => Action::ALL
            .into_iter()
            .filter(|a| match a {
                Action::Send(_) => false,
                Action::Share => !Action::shares().is_empty(),
                a => a.available(),
            })
            .map(|a| Choice {
                mode,
                label: a.label(),
                hint: a.hint(),
            })
            .collect(),
        Mode::Convert => {
            let mut pool: Vec<&'static str> = Vec::new();
            for (ext, kind) in &known {
                for label in convert_labels(*kind, ext) {
                    if !pool.contains(&label) {
                        pool.push(label);
                    }
                }
            }
            pool.retain(|label| {
                known.iter().all(|(src, kind)| {
                    src.to_uppercase() == *label || convert_labels(*kind, src).contains(label)
                }) && known.iter().any(|(src, _)| src.to_uppercase() != *label)
            });
            pool.into_iter()
                .map(|label| Choice {
                    mode,
                    label,
                    hint: convert_hint(label),
                })
                .collect()
        }
        Mode::Tools => {
            let Some((_, first)) = known.first() else {
                return Vec::new();
            };
            tools(*first)
                .iter()
                .filter(|(label, _)| *label != MERGE || known.len() > 1)
                .filter(|(label, _)| {
                    known.iter().all(|(ext, kind)| {
                        tools(*kind).iter().any(|(l, _)| l == label)
                            && (*label != MERGE
                                || crate::convert::merges_file(*kind, ext) && kind == first)
                            && (*kind != Kind::Slides || crate::office::slides::compresses(ext))
                            && (*kind != Kind::Image
                                || images::Tool::from_label(label)
                                    .and_then(|tool| images::tool_target(ext, tool))
                                    .is_some())
                    })
                })
                .map(|&(label, hint)| Choice { mode, label, hint })
                .collect()
        }
        Mode::Media | Mode::Everyday | Mode::Apps | Mode::System => Vec::new(),
    }
}

/// "holiday.jpg · Image" or "3 files · Image".
pub fn describe(files: &[PathBuf]) -> String {
    let kinds: Vec<Option<Kind>> = files.iter().map(|f| kind_of(f)).collect();
    let shared = kinds
        .first()
        .copied()
        .flatten()
        .filter(|k| kinds.iter().all(|other| *other == Some(*k)));
    let subject = match files {
        [one] => display_name(one),
        _ => trf("{} files", &[&files.len()]),
    };
    match shared {
        Some(kind) => format!("{subject} · {}", tr(kind.name())),
        None => subject,
    }
}

pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Extension a conversion label produces.
pub fn extension_for(label: &str) -> String {
    label.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_needs_more_than_one_file() {
        let one = [PathBuf::from("report.docx")];
        let two = [PathBuf::from("report.docx"), PathBuf::from("notes.docx")];
        let labels = |files: &[PathBuf]| -> Vec<&str> {
            choices(files, Mode::Tools)
                .iter()
                .map(|c| c.label)
                .collect()
        };
        assert!(!labels(&one).contains(&MERGE));
        assert!(labels(&two).contains(&MERGE));
        // With Merge gone, a lone document has no tools, so the ring leaves Tools out.
        assert!(!modes(&one).contains(&Mode::Tools));
        let pdf = [PathBuf::from("scan.pdf")];
        assert!(!labels(&pdf).contains(&MERGE));
        assert!(labels(&pdf).contains(&"Split"));
    }
}
