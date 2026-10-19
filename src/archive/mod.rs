//! Compressed files: archives (ZIP, 7Z, RAR, TAR with or without compression) and single
//! compressed files (GZ, XZ, …). Everything here blocks; `jobs` runs it on a thread.

mod codec;
mod fs;
#[cfg(feature = "rar")]
mod rar_io;
mod sevenz_io;
mod tar_io;
mod zip_io;

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

pub use codec::Codec;
pub use fs::{Progress, tidy_modes};

use Codec::*;
use Format::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zip,
    SevenZip,
    Rar,
    /// A tar, compressed or not.
    Tar(Option<Codec>),
    /// One compressed file.
    Single(Codec),
}

/// Name suffixes. Compound ones come first so `.tar.gz` wins over `.gz`; the first suffix of
/// each format is its canonical extension.
const SUFFIXES: &[(&str, Format)] = &[
    ("tar.gz", Tar(Some(Gz))),
    ("tgz", Tar(Some(Gz))),
    ("taz", Tar(Some(Gz))),
    ("tar.bz2", Tar(Some(Bz2))),
    ("tbz2", Tar(Some(Bz2))),
    ("tbz", Tar(Some(Bz2))),
    ("tb2", Tar(Some(Bz2))),
    ("tar.xz", Tar(Some(Xz))),
    ("txz", Tar(Some(Xz))),
    ("tar.zst", Tar(Some(Zst))),
    ("tzst", Tar(Some(Zst))),
    ("tar.lz4", Tar(Some(Lz4))),
    ("tar.lzma", Tar(Some(Lzma))),
    ("tar.lz", Tar(Some(Lzip))),
    ("tlz", Tar(Some(Lzip))),
    ("tar.br", Tar(Some(Br))),
    ("tar", Tar(None)),
    ("cbt", Tar(None)),
    ("zip", Zip),
    ("cbz", Zip),
    ("jar", Zip),
    ("war", Zip),
    ("apk", Zip),
    ("xpi", Zip),
    ("whl", Zip),
    ("nupkg", Zip),
    ("7z", SevenZip),
    ("cb7", SevenZip),
    ("rar", Rar),
    ("cbr", Rar),
    ("gz", Single(Gz)),
    ("gzip", Single(Gz)),
    ("bz2", Single(Bz2)),
    ("bzip2", Single(Bz2)),
    ("bz", Single(Bz2)),
    ("xz", Single(Xz)),
    ("zst", Single(Zst)),
    ("zstd", Single(Zst)),
    ("lz4", Single(Lz4)),
    ("lzma", Single(Lzma)),
    ("lz", Single(Lzip)),
    ("br", Single(Br)),
];

impl Format {
    /// Canonical extension; upper-cased, it is the wheel's label.
    pub fn ext(self) -> &'static str {
        SUFFIXES
            .iter()
            .find(|(_, f)| *f == self)
            .map_or("", |(ext, _)| ext)
    }

    /// The format a canonical extension stands for ("tar.gz", "7z"…).
    pub fn from_ext(ext: &str) -> Option<Format> {
        SUFFIXES
            .iter()
            .find(|(s, f)| *s == ext && f.ext() == ext)
            .map(|(_, f)| *f)
    }

    /// By file name alone.
    pub fn from_name(path: &Path) -> Option<Format> {
        suffix(path).map(|(_, format)| format)
    }

    /// By content, for names that say nothing: magic numbers, and a peek inside compressed
    /// streams to tell `.tar.gz` from `.gz`.
    pub fn sniff(path: &Path) -> Option<Format> {
        let head = read_head(File::open(path).ok()?);
        let format = sniff_bytes(&head)?;
        if let Single(codec) = format {
            let file = BufReader::new(File::open(path).ok()?);
            if is_tar(&read_head(codec.decoder(file).ok()?)) {
                return Some(Tar(Some(codec)));
            }
        }
        Some(format)
    }

    pub fn detect(path: &Path) -> Option<Format> {
        Format::from_name(path).or_else(|| Format::sniff(path))
    }

    pub fn is_archive(self) -> bool {
        !matches!(self, Single(_))
    }

    pub fn can_write(self) -> bool {
        self != Rar
    }
}

/// The file name without its archive suffix: "photos" for photos.tar.gz.
pub fn stem(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match suffix(path) {
        Some((len, _)) => name[..name.len() - len - 1].to_owned(),
        None => Path::new(&name)
            .file_stem()
            .map_or(name.clone(), |s| s.to_string_lossy().into_owned()),
    }
}

/// Length of the matching suffix (without its dot) and its format.
fn suffix(path: &Path) -> Option<(usize, Format)> {
    let name = path.file_name()?.to_string_lossy();
    let name = name.as_bytes();
    SUFFIXES
        .iter()
        .find(|(s, _)| {
            name.len() > s.len() + 1
                && name[name.len() - s.len() - 1] == b'.'
                && name[name.len() - s.len()..].eq_ignore_ascii_case(s.as_bytes())
        })
        .map(|(s, f)| (s.len(), *f))
}

fn read_head(mut r: impl Read) -> Vec<u8> {
    let mut head = Vec::with_capacity(512);
    let _ = r.by_ref().take(512).read_to_end(&mut head);
    head
}

fn sniff_bytes(b: &[u8]) -> Option<Format> {
    let starts = |magic: &[u8]| b.starts_with(magic);
    Some(if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        Zip
    } else if starts(b"7z\xBC\xAF\x27\x1C") {
        SevenZip
    } else if starts(b"Rar!\x1A\x07") {
        Rar
    } else if starts(&[0x1F, 0x8B]) {
        Single(Gz)
    } else if starts(b"BZh") && b.get(3).is_some_and(|d| (b'1'..=b'9').contains(d)) {
        Single(Bz2)
    } else if starts(b"\xFD7zXZ\x00") {
        Single(Xz)
    } else if starts(&[0x28, 0xB5, 0x2F, 0xFD]) {
        Single(Zst)
    } else if starts(&[0x04, 0x22, 0x4D, 0x18]) {
        Single(Lz4)
    } else if starts(b"LZIP") {
        Single(Lzip)
    } else if is_tar(b) {
        Tar(None)
    } else {
        return None;
    })
}

fn is_tar(b: &[u8]) -> bool {
    b.get(257..262) == Some(b"ustar")
}

/// Unpacks `src` into the existing, empty folder `dst`. A single compressed file becomes one
/// file named after it: notes.txt from notes.txt.gz.
pub fn extract(src: &Path, format: Format, dst: &Path, progress: &Progress) -> io::Result<()> {
    unpack(src, format, dst, progress)?;
    fs::tidy_modes(dst);
    Ok(())
}

fn unpack(src: &Path, format: Format, dst: &Path, progress: &Progress) -> io::Result<()> {
    match format {
        Zip => zip_io::extract(src, dst, progress),
        SevenZip => sevenz_io::extract(src, dst, progress),
        #[cfg(feature = "rar")]
        Rar => rar_io::extract(src, dst, progress),
        #[cfg(not(feature = "rar"))]
        Rar => Err(io::Error::other("this build can't open RAR archives")),
        Tar(codec) => tar_io::extract(decoded(src, codec, progress)?, dst),
        Single(codec) => {
            let name = match stem(src) {
                s if s.is_empty() => "file".to_owned(),
                s => s,
            };
            let mut out = File::create_new(dst.join(name))?;
            io::copy(&mut decoded(src, Some(codec), progress)?, &mut out)?;
            Ok(())
        }
    }
}

/// Packs files and folders into `dst`, each under its name. A single compressed file takes
/// exactly one file.
pub fn pack(
    items: &[(PathBuf, String)],
    format: Format,
    dst: File,
    progress: &Progress,
) -> io::Result<()> {
    let items = fs::walk(items)?;
    progress.begin(items.iter().map(|i| i.size()).sum());
    match format {
        Zip => zip_io::pack(&items, dst, progress),
        SevenZip => sevenz_io::pack(&items, dst, progress),
        Rar => Err(io::Error::other("RAR archives can only be read")),
        Tar(codec) => tar_io::pack(&items, codec::encoder(codec, dst)?, progress)?.finish(),
        Single(codec) => {
            let [item] = &items[..] else {
                return Err(io::Error::other("a compressed file holds exactly one file"));
            };
            if !matches!(item.kind, fs::Kind::File) {
                return Err(io::Error::other("only a file can be compressed on its own"));
            }
            let mut out = codec::encoder(Some(codec), dst)?;
            io::copy(
                &mut fs::Counted::new(File::open(&item.path)?, progress),
                &mut out,
            )?;
            out.finish()
        }
    }
}

/// Bytes in the files under `path` (or of `path` itself); 0 if it can't be read.
pub fn walk_size(path: &Path) -> u64 {
    fs::walk(&[(path.to_owned(), String::new())])
        .map_or(0, |items| items.iter().map(|i| i.size()).sum())
}

/// Changes how a tar or a single file is compressed without unpacking it.
pub fn recompress(
    src: &Path,
    from: Option<Codec>,
    dst: File,
    to: Option<Codec>,
    progress: &Progress,
) -> io::Result<()> {
    let mut out = codec::encoder(to, dst)?;
    io::copy(&mut decoded(src, from, progress)?, &mut out)?;
    out.finish()
}

/// `src` decoded, counting the compressed bytes read as progress.
fn decoded<'a>(
    src: &Path,
    codec: Option<Codec>,
    progress: &'a Progress,
) -> io::Result<Box<dyn Read + 'a>> {
    let file = File::open(src)?;
    progress.begin(file.metadata()?.len());
    let counted = BufReader::new(fs::Counted::new(file, progress));
    match codec {
        Some(codec) => codec.decoder(counted),
        None => Ok(Box::new(counted)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_pick_the_longest_suffix() {
        let f = |n: &str| Format::from_name(Path::new(n));
        assert_eq!(f("a.tar.gz"), Some(Tar(Some(Gz))));
        assert_eq!(f("a.TGZ"), Some(Tar(Some(Gz))));
        assert_eq!(f("a.gz"), Some(Single(Gz)));
        assert_eq!(f("a.tar.lz"), Some(Tar(Some(Lzip))));
        assert_eq!(f("a.lz4"), Some(Single(Lz4)));
        assert_eq!(f("comic.cbr"), Some(Rar));
        assert_eq!(f(".zip"), None);
        assert_eq!(f("atar.gz"), Some(Single(Gz)));
        assert_eq!(f("notes.txt"), None);
    }

    #[test]
    fn stems_drop_the_whole_suffix() {
        assert_eq!(stem(Path::new("/x/photos.tar.gz")), "photos");
        assert_eq!(stem(Path::new("notes.txt.gz")), "notes.txt");
        assert_eq!(stem(Path::new("Backup.TAR.ZST")), "Backup");
        assert_eq!(stem(Path::new("plain.bin")), "plain");
    }

    #[test]
    fn canonical_extensions_round_trip() {
        for (_, format) in SUFFIXES {
            assert_eq!(Format::from_ext(format.ext()), Some(*format), "{format:?}");
        }
        assert_eq!(Format::from_ext("tgz"), None);
    }
}
