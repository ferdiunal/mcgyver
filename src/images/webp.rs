//! WebP: lossy through libwebp for photos, lossless through `image` for graphics. libwebp's
//! simple encoder writes no metadata, so the colour profile and Exif are muxed in afterwards.

use std::io::{self, Write};

use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageEncoder};

use super::{Picture, failed, profile_for, with_metadata};

const QUALITY: f32 = 82.0;

pub fn encode(picture: &Picture, out: &mut impl Write) -> io::Result<()> {
    let image = &picture.pixels;
    let rgba = image.color().has_alpha();
    let image = if rgba {
        DynamicImage::ImageRgba8(image.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    };
    if !picture.lossy {
        let mut encoder = WebPEncoder::new_lossless(out);
        with_metadata(&mut encoder, picture, false);
        return encoder
            .write_image(
                image.as_bytes(),
                image.width(),
                image.height(),
                image.color().into(),
            )
            .map_err(failed);
    }
    let encoder = if rgba {
        webp::Encoder::from_rgba(image.as_bytes(), image.width(), image.height())
    } else {
        webp::Encoder::from_rgb(image.as_bytes(), image.width(), image.height())
    };
    let encoded = encoder
        .encode_simple(false, QUALITY)
        .map_err(|e| io::Error::other(format!("WebP: {e:?}")))?;
    out.write_all(&mux(
        &encoded,
        image.width(),
        image.height(),
        profile_for(picture, false),
        picture.exif.as_deref(),
    ))
}

/// Whether a WebP file holds lossy (VP8) rather than lossless (VP8L) data.
pub fn is_lossy(data: &[u8]) -> bool {
    chunks(data).any(|(id, _)| id == b"VP8 ")
}

/// The chunks after the RIFF/WEBP header.
fn chunks(data: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut at = 12;
    std::iter::from_fn(move || {
        let id = data.get(at..at + 4)?;
        let len = u32::from_le_bytes(data.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        let body = data.get(at + 8..at + 8 + len)?;
        at += 8 + len + len % 2;
        Some((id, body))
    })
}

/// Rebuilds a simple WebP as an extended one (VP8X) carrying ICCP and EXIF chunks.
fn mux(webp: &[u8], width: u32, height: u32, icc: Option<&[u8]>, exif: Option<&[u8]>) -> Vec<u8> {
    if icc.is_none() && exif.is_none() {
        return webp.to_vec();
    }
    let image: Vec<(&[u8], &[u8])> = chunks(webp).filter(|(id, _)| *id != b"VP8X").collect();
    let alpha = image.iter().any(|(id, _)| *id == b"ALPH");
    let mut flags = 0u8;
    if icc.is_some() {
        flags |= 0x20;
    }
    if alpha {
        flags |= 0x10;
    }
    if exif.is_some() {
        flags |= 0x08;
    }
    let mut vp8x = vec![flags, 0, 0, 0];
    vp8x.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    vp8x.extend_from_slice(&(height - 1).to_le_bytes()[..3]);

    let mut body = b"WEBP".to_vec();
    let mut chunk = |id: &[u8], data: &[u8]| {
        body.extend_from_slice(id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    };
    chunk(b"VP8X", &vp8x);
    if let Some(icc) = icc {
        chunk(b"ICCP", icc);
    }
    for (id, data) in &image {
        chunk(id, data);
    }
    if let Some(exif) = exif {
        chunk(b"EXIF", exif);
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn muxed_files_keep_the_image_and_gain_the_chunks() {
        let pixels = vec![200u8; 16 * 8 * 3];
        let plain = webp::Encoder::from_rgb(&pixels, 16, 8)
            .encode_simple(false, 80.0)
            .unwrap();
        assert!(is_lossy(&plain));
        let muxed = mux(&plain, 16, 8, Some(b"icc!"), Some(b"MM\0*exif"));
        let ids: Vec<&[u8]> = chunks(&muxed).map(|(id, _)| id).collect();
        assert_eq!(ids, [&b"VP8X"[..], b"ICCP", b"VP8 ", b"EXIF"]);
        let decoded = image::load_from_memory(&muxed).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 8));
    }
}
