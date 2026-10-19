use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use tar::{Archive, Builder, EntryType, Header, HeaderMode};

use super::fs::{Counted, Exact, Item, Kind, Progress};

/// The tar crate does the unpacking: it keeps entries (links included) inside `dst`.
pub fn extract(reader: impl Read, dst: &Path) -> io::Result<()> {
    let mut archive = Archive::new(reader);
    archive.set_preserve_mtime(true);
    archive.set_unpack_xattrs(false);
    // Folders last, deepest first, so a read-only one doesn't block its children (as
    // `Archive::unpack` does, which can't report progress).
    let mut dirs = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type() == EntryType::Directory {
            dirs.push(entry);
        } else {
            entry.unpack_in(dst)?;
        }
    }
    dirs.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
    for mut dir in dirs {
        dir.unpack_in(dst)?;
    }
    // The tar ends before its compression does: read the rest so the codec checks its
    // checksum, or a damaged .tar.gz would pass.
    io::copy(&mut archive.into_inner(), &mut io::sink())?;
    Ok(())
}

pub fn pack<W: Write>(items: &[Item], out: W, progress: &Progress) -> io::Result<W> {
    let mut tar = Builder::new(out);
    for item in items {
        let mut header = Header::new_gnu();
        header.set_metadata_in_mode(&item.meta, HeaderMode::Complete);
        match &item.kind {
            Kind::Dir => tar.append_data(&mut header, &item.name, io::empty())?,
            Kind::Symlink(target) => {
                header.set_size(0);
                tar.append_link(&mut header, &item.name, target)?;
            }
            Kind::File => {
                let file = Counted::new(File::open(&item.path)?, progress);
                tar.append_data(&mut header, &item.name, Exact::new(file, item.size()))?;
            }
        }
    }
    tar.into_inner()
}
