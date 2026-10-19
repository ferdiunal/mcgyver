use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sevenz_rust2::encoder_options::Lzma2Options;
use sevenz_rust2::{ArchiveEntry, ArchiveReader, ArchiveWriter, Password, SourceReader};

use super::codec;
use super::fs::{self, Counted, Item, Kind, Progress, Unpacker};

/// 7-Zip keeps Unix modes in the top half of the Windows attributes, flagged with this bit.
const UNIX_EXTENSION: u32 = 0x8000;
const DIRECTORY: u32 = 0x10;
const S_IFMT: u32 = 0o170_000;
const S_IFLNK: u32 = 0o120_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
/// Files are compressed together (solid) in blocks of at most this much.
const BLOCK: u64 = 4 << 30;
/// LZMA2 splits a block into chunks this big so threads can share it, as `xz -T` does.
const CHUNK: u64 = 24 << 20;

pub fn extract(src: &Path, dst: &Path, progress: &Progress) -> io::Result<()> {
    let file = File::open(src)?;
    progress.begin(file.metadata()?.len());
    let source = Counted::new(BufReader::new(file), progress);
    let mut archive = ArchiveReader::new(source, Password::empty()).map_err(error)?;
    let mut out = Unpacker::new(dst);
    archive
        .for_each_entries(|entry, data| {
            unpack(&mut out, entry, data)?;
            Ok(true)
        })
        .map_err(error)?;
    out.finish()
}

fn unpack(out: &mut Unpacker, entry: &ArchiveEntry, data: &mut dyn Read) -> io::Result<()> {
    let mode = (entry.has_windows_attributes && entry.windows_attributes & UNIX_EXTENSION != 0)
        .then_some(entry.windows_attributes >> 16);
    let mtime = entry
        .has_last_modified_date
        .then(|| SystemTime::from(entry.last_modified_date()));
    if entry.is_anti_item() {
        // Marks a deletion in an update; nothing to write.
    } else if entry.is_directory() {
        out.dir(entry.name(), mode, mtime)?;
    } else if mode.is_some_and(|m| m & S_IFMT == S_IFLNK) {
        let target = fs::link_target(data)?;
        out.symlink(entry.name(), &target, mtime)?;
    } else {
        out.file(entry.name(), data, mode, mtime)?;
    }
    // Solid blocks are one stream: whatever wasn't read has to be skipped.
    io::copy(data, &mut io::sink())?;
    Ok(())
}

pub fn pack(items: &[Item], dst: File, progress: &Progress) -> io::Result<()> {
    let mut archive =
        ArchiveWriter::new(BufWriter::with_capacity(256 * 1024, dst)).map_err(error)?;
    archive.set_content_methods(vec![
        Lzma2Options::from_level_mt(6, codec::workers(), CHUNK).into(),
    ]);
    let mut entries = Vec::new();
    let mut readers = Vec::new();
    let mut block = 0;
    for item in items {
        let mut entry = ArchiveEntry::from_path(&item.path, item.name.clone());
        entry.is_directory = matches!(item.kind, Kind::Dir);
        let kind = match item.kind {
            Kind::Dir => S_IFDIR,
            Kind::Symlink(_) => S_IFLNK,
            Kind::File => S_IFREG,
        };
        entry.has_windows_attributes = true;
        entry.windows_attributes = (kind | item.mode()) << 16
            | UNIX_EXTENSION
            | if entry.is_directory { DIRECTORY } else { 0 };
        match &item.kind {
            Kind::Dir => {
                archive
                    .push_archive_entry::<&[u8]>(entry, None)
                    .map_err(error)?;
            }
            Kind::Symlink(target) => {
                let target = target.to_string_lossy();
                archive
                    .push_archive_entry(entry, Some(target.as_bytes()))
                    .map_err(error)?;
            }
            Kind::File if item.size() == 0 => {
                archive
                    .push_archive_entry::<&[u8]>(entry, None)
                    .map_err(error)?;
            }
            Kind::File => {
                if block + item.size() > BLOCK && !entries.is_empty() {
                    archive
                        .push_archive_entries(
                            std::mem::take(&mut entries),
                            std::mem::take(&mut readers),
                        )
                        .map_err(error)?;
                    block = 0;
                }
                block += item.size();
                entries.push(entry);
                readers.push(SourceReader::new(Lazy::new(&item.path, progress)));
            }
        }
    }
    if !entries.is_empty() {
        archive
            .push_archive_entries(entries, readers)
            .map_err(error)?;
    }
    codec::close(archive.finish()?)
}

/// Opens its file on the first read and closes it at the end, so a solid block over
/// thousands of files doesn't hold them all open.
struct Lazy<'a> {
    path: PathBuf,
    file: Option<File>,
    done: bool,
    progress: &'a Progress,
}

impl<'a> Lazy<'a> {
    fn new(path: &Path, progress: &'a Progress) -> Self {
        Self {
            path: path.to_owned(),
            file: None,
            done: false,
            progress,
        }
    }
}

impl Read for Lazy<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        let file = match &mut self.file {
            Some(file) => file,
            None => self.file.insert(File::open(&self.path)?),
        };
        let n = file.read(buf)?;
        if n == 0 {
            self.file = None;
            self.done = true;
        }
        self.progress.add(n as u64);
        Ok(n)
    }
}

fn error(e: sevenz_rust2::Error) -> io::Error {
    match e {
        sevenz_rust2::Error::Io(e, _) | sevenz_rust2::Error::FileOpen(e, _) => e,
        sevenz_rust2::Error::PasswordRequired | sevenz_rust2::Error::MaybeBadPassword(_) => {
            io::Error::other("this archive is password-protected")
        }
        e => io::Error::other(e.to_string()),
    }
}
