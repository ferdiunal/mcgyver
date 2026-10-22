//! A presentation made smaller: its pictures scaled down to what a screen shows and
//! recompressed, the rest of the file copied as it is.

use std::io::{self, Cursor, Read, Write};
use std::path::Path;

use image::{ImageDecoder, ImageEncoder};

use super::{failed, read};

/// Pictures are scaled down to at most this many pixels on their long side: a full-width
/// picture on a widescreen slide at 150 dpi.
const LONGEST: u32 = 1920;

/// Whether Compress can make a presentation of `ext` smaller here: PowerPoint's and
/// OpenDocument's, which are zip files of parts.
pub fn compresses(ext: &str) -> bool {
    matches!(ext, "pptx" | "odp")
}

/// The presentation with its JPEG and PNG pictures scaled down and recompressed. Each keeps its
/// format and its name, so nothing that points at it changes; every other part is copied byte
/// for byte. Fails when that saves nothing, rather than leaving a copy that is no smaller.
pub fn compress(input: &Path) -> io::Result<Vec<u8>> {
    let bytes = read(input)?;
    let before = bytes.len();
    let mut deck = zip::ZipArchive::new(Cursor::new(bytes)).map_err(failed)?;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..deck.len() {
        let name = deck
            .by_index_raw(i)
            .map_err(failed)?
            .name()
            .map_err(failed)?
            .into_owned();
        let picture = (name.starts_with("ppt/media/") || name.starts_with("Pictures/"))
            .then(|| {
                name.rsplit_once('.')
                    .map(|(_, ext)| ext.to_ascii_lowercase())
            })
            .flatten()
            .filter(|ext| matches!(ext.as_str(), "jpg" | "jpeg" | "png"));
        let Some(ext) = picture else {
            out.raw_copy_file(deck.by_index_raw(i).map_err(failed)?)
                .map_err(failed)?;
            continue;
        };
        let mut entry = deck.by_index(i).map_err(failed)?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        drop(entry);
        match smaller(&data, ext != "png") {
            Some(data) => {
                // Pictures are compressed already.
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                out.start_file(&name, options).map_err(failed)?;
                out.write_all(&data)?;
            }
            None => out
                .raw_copy_file(deck.by_index_raw(i).map_err(failed)?)
                .map_err(failed)?,
        }
    }
    let bytes = out.finish().map_err(failed)?.into_inner();
    if bytes.len() >= before {
        return Err(io::Error::other("this file is already as small as it gets"));
    }
    Ok(bytes)
}

/// The picture scaled down to [`LONGEST`] and saved again, as a JPEG at quality 80 or a PNG at
/// its best compression; `None` if it can't be read, or comes out no smaller. One that needn't
/// be scaled is saved again only for a tenth off: each new JPEG loses a little more of the
/// picture, so Compress done twice leaves it alone.
fn smaller(data: &[u8], jpeg: bool) -> Option<Vec<u8>> {
    let mut decoder = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    // A photo its Exif turns would come out unturned, in a frame on the slide that is the
    // turned size: it is left as it is.
    if decoder.orientation().ok()? != image::metadata::Orientation::NoTransforms {
        return None;
    }
    // The colour profile goes along, so the colours stay.
    let profile = decoder.icc_profile().ok().flatten();
    let picture = image::DynamicImage::from_decoder(decoder).ok()?;
    let scaled = picture.width().max(picture.height()) > LONGEST;
    let picture = if scaled {
        picture.resize(LONGEST, LONGEST, image::imageops::FilterType::Lanczos3)
    } else {
        picture
    };
    let mut out = Vec::new();
    if jpeg {
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
        if let Some(profile) = profile {
            encoder.set_icc_profile(profile).ok()?;
        }
        encoder.encode_image(&picture.to_rgb8()).ok()?;
    } else {
        let mut encoder = image::codecs::png::PngEncoder::new_with_quality(
            &mut out,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        );
        if let Some(profile) = profile {
            encoder.set_icc_profile(profile).ok()?;
        }
        let picture = picture.to_rgba8();
        encoder
            .write_image(
                picture.as_raw(),
                picture.width(),
                picture.height(),
                image::ExtendedColorType::Rgba8,
            )
            .ok()?;
    }
    let enough = if scaled {
        data.len()
    } else {
        data.len() / 10 * 9
    };
    (out.len() < enough).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_pictures_shrink_and_the_rest_is_copied_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("talk.pptx");
        // A noisy 3000 × 2000 photo, saved at the best JPEG quality, and a slide.
        let photo = image::RgbImage::from_fn(3000, 2000, |x, y| {
            let n = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) % 256;
            image::Rgb([n as u8, (x % 256) as u8, (y % 256) as u8])
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
            .encode_image(&photo)
            .unwrap();
        let slide = b"<p:sld>a slide</p:sld>".repeat(50);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&deck).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("ppt/slides/slide1.xml", options).unwrap();
        zip.write_all(&slide).unwrap();
        zip.start_file("ppt/media/image1.jpeg", options).unwrap();
        zip.write_all(&jpeg).unwrap();
        zip.finish().unwrap();

        let smaller = compress(&deck).unwrap();
        assert!(smaller.len() < std::fs::metadata(&deck).unwrap().len() as usize);
        let mut out = zip::ZipArchive::new(Cursor::new(smaller)).unwrap();
        let mut kept = Vec::new();
        out.by_name("ppt/slides/slide1.xml")
            .unwrap()
            .read_to_end(&mut kept)
            .unwrap();
        assert_eq!(kept, slide);
        let mut picture = Vec::new();
        out.by_name("ppt/media/image1.jpeg")
            .unwrap()
            .read_to_end(&mut picture)
            .unwrap();
        let picture = image::load_from_memory(&picture).unwrap();
        assert_eq!((picture.width(), picture.height()), (1920, 1280));

        // Done once, there is nothing left to save.
        let again = dir.path().join("again.pptx");
        std::fs::write(&again, compress(&deck).unwrap()).unwrap();
        assert_eq!(
            compress(&again).unwrap_err().to_string(),
            "this file is already as small as it gets"
        );
    }
}
