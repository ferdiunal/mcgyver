//! Single-stream compression: what wraps a tar, or one file on its own.

use std::fs::File;
use std::io::{self, BufRead, BufWriter, Read, Write};
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Gz,
    Bz2,
    Xz,
    Zst,
    Lz4,
    Lzma,
    Lzip,
    Br,
}

/// A crafted .xz or .lzma can ask for a dictionary of gigabytes, and a failed allocation
/// aborts the whole app. Real files stay far below this (`xz -9` uses 64 MiB).
const MEM_LIMIT_KB: u32 = 1 << 20;

impl Codec {
    pub fn decoder<'a>(self, r: impl BufRead + 'a) -> io::Result<Box<dyn Read + 'a>> {
        Ok(match self {
            Codec::Gz => Box::new(flate2::bufread::MultiGzDecoder::new(r)),
            Codec::Bz2 => Box::new(bzip2::bufread::MultiBzDecoder::new(r)),
            Codec::Xz => Box::new(lzma_rust2::XzReader::new_mem_limit(r, true, MEM_LIMIT_KB)),
            Codec::Zst => Box::new(zstd::stream::read::Decoder::with_buffer(r)?),
            Codec::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(r)),
            Codec::Lzma => Box::new(lzma_rust2::LzmaReader::new_mem_limit(
                r,
                MEM_LIMIT_KB,
                None,
            )?),
            Codec::Lzip => Box::new(lzma_rust2::LzipReader::new(r)),
            Codec::Br => Box::new(brotli::Decompressor::new(r, 64 * 1024)),
        })
    }
}

/// A writer that has to be told when the data ends.
pub trait Encoder: Write {
    fn finish(self: Box<Self>) -> io::Result<()>;
}

/// Threads for one multi-threaded encoder. xz and 7z need ~100 MB each at level 6, and
/// several jobs may run at once, so this stays small.
pub fn workers() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(4) as u32)
}

/// Flushes and syncs a finished file, so "Saved" means on disk.
pub fn close(w: BufWriter<File>) -> io::Result<()> {
    w.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()
}

/// Writes `file` through `codec` (or as is).
pub fn encoder(codec: Option<Codec>, file: File) -> io::Result<Box<dyn Encoder>> {
    let w = BufWriter::with_capacity(256 * 1024, file);
    let workers = workers();
    Ok(match codec {
        None => Box::new(w),
        Some(Codec::Gz) => Box::new(flate2::write::GzEncoder::new(
            w,
            flate2::Compression::default(),
        )),
        Some(Codec::Bz2) => Box::new(bzip2::write::BzEncoder::new(w, bzip2::Compression::best())),
        Some(Codec::Xz) => {
            let mut options = lzma_rust2::XzOptions::with_preset(6);
            // Blocks of three dictionaries, as `xz -T` does, so the threads can share the work.
            options.block_size = NonZeroU64::new(3 * options.lzma_options.dict_size as u64);
            Box::new(lzma_rust2::XzWriterMt::new(w, options, workers)?)
        }
        Some(Codec::Zst) => {
            let mut encoder = zstd::stream::write::Encoder::new(w, 3)?;
            encoder.multithread(workers)?;
            Box::new(encoder)
        }
        Some(Codec::Lz4) => Box::new(lz4_flex::frame::FrameEncoder::new(w)),
        Some(Codec::Lzma) => {
            let options = lzma_rust2::LzmaOptions::with_preset(6);
            Box::new(lzma_rust2::LzmaWriter::new_use_header(w, &options, None)?)
        }
        Some(Codec::Lzip) => Box::new(lzma_rust2::LzipWriter::new(
            w,
            lzma_rust2::LzipOptions::with_preset(6),
        )),
        Some(Codec::Br) => Box::new(brotli::CompressorWriter::new(
            Sticky {
                inner: w,
                error: None,
            },
            64 * 1024,
            9,
            22,
        )),
    })
}

type Out = BufWriter<File>;

impl Encoder for Out {
    fn finish(self: Box<Self>) -> io::Result<()> {
        close(*self)
    }
}

/// Each encoder's own `finish`, then `close`, so a full disk shows up as an error.
macro_rules! finish {
    ($($ty:ty => |$e:ident| $finish:expr;)*) => {$(
        impl Encoder for $ty {
            fn finish(self: Box<Self>) -> io::Result<()> {
                let $e = *self;
                let out: Out = $finish;
                close(out)
            }
        }
    )*};
}

/// Keeps the first write error, which brotli's `into_inner` would otherwise drop.
struct Sticky {
    inner: Out,
    error: Option<io::Error>,
}

impl Write for Sticky {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf).inspect_err(|e| {
            self.error
                .get_or_insert_with(|| io::Error::new(e.kind(), e.to_string()));
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().inspect_err(|e| {
            self.error
                .get_or_insert_with(|| io::Error::new(e.kind(), e.to_string()));
        })
    }
}

finish! {
    flate2::write::GzEncoder<Out> => |e| e.finish()?;
    bzip2::write::BzEncoder<Out> => |e| e.finish()?;
    lzma_rust2::XzWriterMt<Out> => |e| e.finish()?;
    zstd::stream::write::Encoder<'static, Out> => |e| e.finish()?;
    lz4_flex::frame::FrameEncoder<Out> => |e| e.finish().map_err(io::Error::other)?;
    lzma_rust2::LzmaWriter<Out> => |e| e.finish()?;
    lzma_rust2::LzipWriter<Out> => |e| e.finish()?;
    brotli::CompressorWriter<Sticky> => |e| {
        let mut e = e;
        e.flush()?;
        let sticky = e.into_inner();
        if let Some(error) = sticky.error {
            return Err(error);
        }
        sticky.inner
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Codec; 8] = [
        Codec::Gz,
        Codec::Bz2,
        Codec::Xz,
        Codec::Zst,
        Codec::Lz4,
        Codec::Lzma,
        Codec::Lzip,
        Codec::Br,
    ];

    #[test]
    fn every_codec_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..300_000u32)
            .flat_map(|i| (i % 251).to_le_bytes())
            .collect();
        for codec in ALL {
            let path = dir.path().join(format!("{codec:?}"));
            let mut w = encoder(Some(codec), File::create(&path).unwrap()).unwrap();
            w.write_all(&data).unwrap();
            w.finish().unwrap();
            let mut back = Vec::new();
            let file = io::BufReader::new(File::open(&path).unwrap());
            codec.decoder(file).unwrap().read_to_end(&mut back).unwrap();
            assert!(back == data, "{codec:?}");
        }
    }
}
