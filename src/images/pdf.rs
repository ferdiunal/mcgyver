//! One picture on one PDF page, sized at 96 dpi. A plain JPEG goes in unchanged; other lossy
//! sources are stored as JPEG, lossless ones losslessly (Flate), with transparency as a mask.

use std::io::{self, Write};

use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageEncoder};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};

use super::{Picture, failed, flatten, profile_for};

const POINTS_PER_PIXEL: f32 = 72.0 / 96.0;

pub fn write(picture: &Picture, out: &mut impl Write) -> io::Result<()> {
    let image = &picture.pixels;
    let (w, h) = (image.width(), image.height());
    let gray = matches!(flatten(image), DynamicImage::ImageLuma8(_));

    // The pixels, as the PDF stores them.
    let (filter, data) = match &picture.jpeg {
        Some(jpeg) => (Filter::DctDecode, jpeg.clone()),
        None if picture.lossy => {
            let flat = flatten(image);
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 92)
                .write_image(flat.as_bytes(), w, h, flat.color().into())
                .map_err(failed)?;
            (Filter::DctDecode, jpeg)
        }
        None => {
            let raw = if gray {
                image.to_luma8().into_raw()
            } else {
                image.to_rgb8().into_raw()
            };
            (Filter::FlateDecode, deflate(&raw)?)
        }
    };
    // JPEG has no transparency, so a mask only goes with lossless data.
    let mask = (filter == Filter::FlateDecode && image.color().has_alpha())
        .then(|| deflate(&image.to_rgba8().pixels().map(|p| p[3]).collect::<Vec<_>>()))
        .transpose()?;
    let channels = if gray { 1 } else { 3 };
    let icc = profile_for(picture, gray);

    let (catalog, pages, page, picture_id, mask_id, icc_id, content_id) = (
        Ref::new(1),
        Ref::new(2),
        Ref::new(3),
        Ref::new(4),
        Ref::new(5),
        Ref::new(6),
        Ref::new(7),
    );
    let name = Name(b"Im1");
    let (pw, ph) = (w as f32 * POINTS_PER_PIXEL, h as f32 * POINTS_PER_PIXEL);
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(pages);
    pdf.pages(pages).kids([page]).count(1);
    let mut p = pdf.page(page);
    p.media_box(Rect::new(0.0, 0.0, pw, ph));
    p.parent(pages);
    p.contents(content_id);
    p.resources().x_objects().pair(name, picture_id);
    p.finish();

    let mut xobject = pdf.image_xobject(picture_id, &data);
    xobject.filter(filter);
    xobject.width(w as i32);
    xobject.height(h as i32);
    xobject.bits_per_component(8);
    match icc {
        Some(_) => xobject.color_space().icc_based(icc_id),
        None if gray => xobject.color_space().device_gray(),
        None => xobject.color_space().device_rgb(),
    }
    if mask.is_some() {
        xobject.s_mask(mask_id);
    }
    xobject.finish();
    if let Some(mask) = &mask {
        let mut s = pdf.image_xobject(mask_id, mask);
        s.filter(Filter::FlateDecode);
        s.width(w as i32);
        s.height(h as i32);
        s.color_space().device_gray();
        s.bits_per_component(8);
    }
    if let Some(icc) = icc {
        let compressed = deflate(icc)?;
        let mut profile = pdf.icc_profile(icc_id, &compressed);
        profile.filter(Filter::FlateDecode);
        profile.n(channels);
        if gray {
            profile.alternate().device_gray();
        } else {
            profile.alternate().device_rgb();
        }
    }
    let mut content = Content::new();
    content.save_state();
    content.transform([pw, 0.0, 0.0, ph, 0.0, 0.0]);
    content.x_object(name);
    content.restore_state();
    pdf.stream(content_id, &content.finish());
    out.write_all(&pdf.finish())
}

fn deflate(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(data)?;
    z.finish()
}

/// The number of colour components of a JPEG, from its frame header; 4 means CMYK, which a
/// PDF would need told apart, so only 1 and 3 are embedded as they are.
pub fn jpeg_components(data: &[u8]) -> Option<u8> {
    let mut at = 2;
    while at + 9 < data.len() {
        if data[at] != 0xFF {
            return None;
        }
        let marker = data[at + 1];
        let len = u16::from_be_bytes([data[at + 2], data[at + 3]]) as usize;
        // Start of frame: every SOFn but DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            return data.get(at + 9).copied();
        }
        at += 2 + len;
    }
    None
}
