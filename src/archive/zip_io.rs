use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use zip::read::ZipFile;
use zip::result::ZipError;
use zip::write::FullFileOptions;
use zip::{CompressionMethod, DateTime, ExtraField, ZipArchive, ZipWriter};

use super::codec;
use super::fs::{self, Counted, Item, Kind, Progress, Unpacker};

/// Info-ZIP's extended timestamp: exact UTC times, unlike the local DOS time in the header.
const EXTENDED_TIMESTAMP: u16 = 0x5455;

pub fn extract(src: &Path, dst: &Path, progress: &Progress) -> io::Result<()> {
    let file = File::open(src)?;
    progress.begin(file.metadata()?.len());
    let mut zip = ZipArchive::new(Counted::new(BufReader::new(file), progress)).map_err(error)?;
    let mut out = Unpacker::new(dst);
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(error)?;
        let name = entry.name().map_err(error)?.into_owned();
        let mtime = mtime(&entry);
        if entry.is_dir() {
            out.dir(&name, entry.unix_mode(), mtime)?;
        } else if entry.is_symlink() {
            let target = fs::link_target(&mut entry)?;
            out.symlink(&name, &target, mtime)?;
        } else {
            let mode = entry.unix_mode();
            out.file(&name, &mut entry, mode, mtime)?;
        }
    }
    out.finish()
}

pub fn pack(items: &[Item], dst: File, progress: &Progress) -> io::Result<()> {
    let mut zip = ZipWriter::new(BufWriter::with_capacity(256 * 1024, dst));
    for item in items {
        let mut options = FullFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(item.mode())
            // Incompressible data grows a little, so switch to Zip64 a bit before 4 GiB.
            .large_file(item.size() >= u32::MAX as u64 / 8 * 7);
        if let Some(t) = item.mtime() {
            options = options.last_modified_time(dos_time(t));
            if let Some(secs) = t
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| u32::try_from(d.as_secs()).ok())
            {
                let mut field = vec![1u8];
                field.extend(secs.to_le_bytes());
                options
                    .add_extra_field(EXTENDED_TIMESTAMP, field, false)
                    .map_err(error)?;
            }
        }
        match &item.kind {
            Kind::Dir => zip.add_directory(&item.name, options).map_err(error)?,
            Kind::Symlink(target) => zip
                .add_symlink(&item.name, target.to_string_lossy(), options)
                .map_err(error)?,
            Kind::File => {
                zip.start_file(&item.name, options).map_err(error)?;
                io::copy(
                    &mut Counted::new(File::open(&item.path)?, progress),
                    &mut zip,
                )?;
            }
        }
    }
    codec::close(zip.finish().map_err(error)?)
}

fn mtime<R: Read>(entry: &ZipFile<'_, R>) -> Option<SystemTime> {
    for field in entry.extra_data_fields() {
        if let ExtraField::ExtendedTimestamp(ts) = field
            && let Some(secs) = ts.mod_time()
        {
            return Some(UNIX_EPOCH + Duration::from_secs(secs.into()));
        }
    }
    from_dos(entry.last_modified()?)
}

/// The header's DOS time is local time with no zone, as every zip tool writes it.
fn from_dos(t: DateTime) -> Option<SystemTime> {
    let civil = jiff::civil::DateTime::try_from(t).ok()?;
    Some(civil.to_zoned(TimeZone::system()).ok()?.timestamp().into())
}

fn dos_time(t: SystemTime) -> DateTime {
    Timestamp::try_from(t)
        .ok()
        .and_then(|ts| DateTime::try_from(ts.to_zoned(TimeZone::system()).datetime()).ok())
        .unwrap_or_default()
}

fn error(e: ZipError) -> io::Error {
    match e {
        ZipError::Io(e) => e,
        ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED) | ZipError::InvalidPassword => {
            io::Error::other("this archive is password-protected")
        }
        e => io::Error::other(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_times_round_trip_in_two_second_steps() {
        let t = UNIX_EPOCH + Duration::from_secs(1_600_000_001);
        let back = from_dos(dos_time(t)).unwrap();
        assert_eq!(back, UNIX_EPOCH + Duration::from_secs(1_600_000_000));
    }
}
