//! Photoshop files (PSD and PSB), read for the merged image Photoshop stores after the layers,
//! with the colour profile from the image resources. Layers themselves aren't needed for that.

use std::io;

use image::{DynamicImage, ImageBuffer, Luma, LumaA, Rgb, Rgba};

use super::{MAX_BYTES, Picture};

const ICC_RESOURCE: u16 = 1039;

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let bytes = self
            .data
            .get(self.at..self.at.checked_add(n).ok_or_else(damaged)?)
            .ok_or_else(damaged)?;
        self.at += n;
        Ok(bytes)
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
}

fn damaged() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "the Photoshop file is damaged")
}

fn unsupported(what: &str) -> io::Error {
    io::Error::other(format!("Photoshop {what} isn't supported"))
}

pub fn decode(data: &[u8]) -> io::Result<Picture> {
    let mut r = Reader { data, at: 0 };
    if r.take(4)? != b"8BPS" {
        return Err(damaged());
    }
    let big = match r.u16()? {
        1 => false,
        2 => true,
        _ => return Err(damaged()),
    };
    r.take(6)?;
    let channels = r.u16()? as usize;
    let height = r.u32()? as usize;
    let width = r.u32()? as usize;
    let depth = r.u16()?;
    let mode = r.u16()?;
    if !matches!(depth, 8 | 16) {
        return Err(unsupported(&format!("{depth}-bit colour")));
    }
    if width == 0 || height == 0 || channels == 0 {
        return Err(damaged());
    }

    let palette_len = r.u32()? as usize;
    let palette = r.take(palette_len)?;
    let resources_len = r.u32()? as usize;
    let icc = icc(r.take(resources_len)?);
    let layers_len = if big {
        r.u64()? as usize
    } else {
        r.u32()? as usize
    };
    let layers = r.take(layers_len)?;
    // A negative layer count means the merged image's first extra channel is its transparency;
    // otherwise extra channels are saved selections.
    let transparent = {
        let mut l = Reader {
            data: layers,
            at: 0,
        };
        let info_len = if big {
            l.u64().ok().map(|n| n as usize)
        } else {
            l.u32().ok().map(|n| n as usize)
        };
        info_len.is_some_and(|n| n >= 2) && l.u16().is_ok_and(|count| (count as i16) < 0)
    };

    let bytes = depth as usize / 8;
    let plane_len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(bytes))
        .ok_or_else(damaged)?;
    // Check the actual decoded pixel size before allocating. Indexed colour expands one sample
    // into an RGB pixel, while the other supported modes produce up to four 16-bit channels.
    let output_bytes_per_pixel = match mode {
        3 if channels >= 4 && transparent => 8,
        3 if channels >= 3 => 6,
        1 if channels >= 2 && transparent => 4,
        1 => 2,
        4 if channels >= 4 => 6,
        2 if palette.len() >= 768 && bytes == 1 => 3,
        _ => return Err(unsupported("colour mode")),
    };
    let output = width as u128 * height as u128 * output_bytes_per_pixel;
    let plane_bytes = plane_len as u128 * channels as u128;
    let rows = channels.checked_mul(height).ok_or_else(damaged)?;
    let compression = r.u16()?;
    let count_bytes = if compression == 1 {
        rows as u128 * std::mem::size_of::<usize>() as u128
    } else {
        0
    };
    let left = (data.len() - r.at) as u128;
    // PackBits can at most grow 64-fold. Bound every channel and the row-count table too.
    if output > MAX_BYTES as u128
        || plane_bytes + count_bytes > MAX_BYTES as u128
        || plane_bytes > left * 64
    {
        return Err(io::Error::other("this picture is too big to convert"));
    }
    let planes = match compression {
        0 => (0..channels)
            .map(|_| r.take(plane_len).map(<[u8]>::to_vec))
            .collect::<io::Result<Vec<_>>>()?,
        1 => {
            let counts: Vec<usize> = (0..rows)
                .map(|_| {
                    if big {
                        r.u32().map(|n| n as usize)
                    } else {
                        r.u16().map(usize::from)
                    }
                })
                .collect::<io::Result<_>>()?;
            let row_len = width * bytes;
            let mut planes = Vec::with_capacity(channels);
            for _ in 0..channels {
                let mut plane = Vec::new();
                plane
                    .try_reserve_exact(plane_len)
                    .map_err(|_| io::Error::other("this picture is too big to convert"))?;
                planes.push(plane);
            }
            for (i, count) in counts.into_iter().enumerate() {
                unpack_bits(r.take(count)?, row_len, &mut planes[i / height])?;
            }
            planes
        }
        _ => return Err(unsupported("ZIP-compressed data")),
    };

    // One sample of channel `c` at pixel `i`, scaled to 16 bits.
    let sample = |c: usize, i: usize| -> u16 {
        let p = &planes[c];
        if bytes == 1 {
            u16::from(p[i]) * 257
        } else {
            u16::from_be_bytes([p[i * 2], p[i * 2 + 1]])
        }
    };
    // Photoshop mattes the merged colour against white where it is transparent; undo that.
    let unmatte = |c: u16, a: u16| -> u16 {
        if a == 0 {
            return 0;
        }
        let (c, a) = (c as u32, a as u32);
        ((c + a).saturating_sub(65535) * 65535 / a).min(65535) as u16
    };
    let n = width * height;
    let (w, h) = (width as u32, height as u32);
    let pixels = match (mode, channels) {
        (3, c) if c >= 4 && transparent => {
            DynamicImage::ImageRgba16(ImageBuffer::from_fn(w, h, |x, y| {
                let i = (y * w + x) as usize;
                let a = sample(3, i);
                Rgba([
                    unmatte(sample(0, i), a),
                    unmatte(sample(1, i), a),
                    unmatte(sample(2, i), a),
                    a,
                ])
            }))
        }
        (3, c) if c >= 3 => DynamicImage::ImageRgb16(ImageBuffer::from_fn(w, h, |x, y| {
            let i = (y * w + x) as usize;
            Rgb([sample(0, i), sample(1, i), sample(2, i)])
        })),
        (1, c) if c >= 2 && transparent => {
            DynamicImage::ImageLumaA16(ImageBuffer::from_fn(w, h, |x, y| {
                let i = (y * w + x) as usize;
                let a = sample(1, i);
                LumaA([unmatte(sample(0, i), a), a])
            }))
        }
        (1, _) => DynamicImage::ImageLuma16(ImageBuffer::from_fn(w, h, |x, y| {
            Luma([sample(0, (y * w + x) as usize)])
        })),
        // Stored as 1 − ink; a plain conversion without the profile.
        (4, c) if c >= 4 => DynamicImage::ImageRgb16(ImageBuffer::from_fn(w, h, |x, y| {
            let i = (y * w + x) as usize;
            let k = u32::from(sample(3, i));
            let ink = |c: usize| (u32::from(sample(c, i)) * k / 65535) as u16;
            Rgb([ink(0), ink(1), ink(2)])
        })),
        (2, _) if palette.len() >= 768 && bytes == 1 => {
            DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |x, y| {
                let v = planes[0][(y * w + x) as usize] as usize;
                Rgb([palette[v], palette[256 + v], palette[512 + v]])
            }))
        }
        _ => return Err(unsupported("colour mode")),
    };
    debug_assert!(planes.iter().all(|p| p.len() >= n * bytes));
    // A CMYK profile doesn't describe the RGB pixels made above.
    let icc = icc.filter(|_| mode != 4);
    Ok(Picture {
        pixels,
        icc,
        exif: None,
        lossy: false,
        jpeg: None,
    })
}

/// The ICC profile among the image resources ("8BIM" blocks), if any.
fn icc(resources: &[u8]) -> Option<Vec<u8>> {
    let mut r = Reader {
        data: resources,
        at: 0,
    };
    while r.at < resources.len() {
        if r.take(4).ok()? != b"8BIM" {
            return None;
        }
        let id = r.u16().ok()?;
        // A Pascal string, padded to an even length with its length byte.
        let name_len = *r.take(1).ok()?.first()? as usize;
        r.take(name_len + (name_len + 1) % 2).ok()?;
        let len = r.u32().ok()? as usize;
        let data = r.take(len).ok()?;
        r.take(len % 2).ok()?;
        if id == ICC_RESOURCE {
            return Some(data.to_vec());
        }
    }
    None
}

/// PackBits, row by row: a header byte n then n + 1 literal bytes, or one byte repeated
/// 1 − n times.
fn unpack_bits(mut src: &[u8], row_len: usize, out: &mut Vec<u8>) -> io::Result<()> {
    let end = out.len() + row_len;
    while out.len() < end {
        let (&n, rest) = src.split_first().ok_or_else(damaged)?;
        src = rest;
        let n = n as i8;
        if n >= 0 {
            let count = n as usize + 1;
            out.extend_from_slice(src.get(..count).ok_or_else(damaged)?);
            src = &src[count..];
        } else if n != -128 {
            let (&byte, rest) = src.split_first().ok_or_else(damaged)?;
            src = rest;
            out.extend(std::iter::repeat_n(byte, 1 + (-(n as isize)) as usize));
        }
    }
    out.truncate(end);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_matches_the_spec_example() {
        // From Apple's TN1023.
        let packed = [
            0xFE, 0xAA, 0x02, 0x80, 0x00, 0x2A, 0xFD, 0xAA, 0x03, 0x80, 0x00, 0x2A, 0x22, 0xF7,
            0xAA,
        ];
        let mut out = Vec::new();
        unpack_bits(&packed, 24, &mut out).unwrap();
        assert_eq!(
            out,
            [
                0xAA, 0xAA, 0xAA, 0x80, 0x00, 0x2A, 0xAA, 0xAA, 0xAA, 0xAA, 0x80, 0x00, 0x2A, 0x22,
                0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA
            ]
        );
    }
}
