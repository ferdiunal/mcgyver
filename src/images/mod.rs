//! Pictures: reading every common still-image format and writing the wheel's targets.
//!
//! Pixels pass through `image::DynamicImage`, with the colour profile and Exif carried along.
//! Orientation is applied to the pixels and reset in the Exif, so nothing is turned twice.
//! HEIC, HEIF and AVIF go through libheif, loaded at run time (`heif`). Everything else is
//! Rust, plus the C libwebp that the `webp` crate bundles for lossy WebP.

mod heif;
mod pdf;
mod psd;
mod webp;

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Component, Path};
use std::sync::{Arc, OnceLock};

use image::codecs::bmp::BmpEncoder;
use image::codecs::gif::GifEncoder;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{
    DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Rgb, RgbImage, RgbaImage,
};

/// Decoded pixels may take at most this much memory: 16k × 16k RGBA, 8-bit. A small file can
/// claim any size in its header, and a failed allocation would end the whole app.
pub const MAX_BYTES: u64 = 1 << 30;

/// A decoded picture and what travels with it.
pub struct Picture {
    pub pixels: DynamicImage,
    pub icc: Option<Vec<u8>>,
    /// Exif as TIFF data, its orientation reset because `pixels` are already upright.
    pub exif: Option<Vec<u8>>,
    /// The source was lossy (JPEG, HEIC…), so a lossy target doesn't lose much more.
    pub lossy: bool,
    /// The source file when it is a plain, upright JPEG that a PDF can carry unchanged.
    pub jpeg: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Jpg,
    Png,
    Webp,
    Heic,
    Avif,
    Tiff,
    Gif,
    Bmp,
    Ico,
    Pdf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Compress,
    Resize,
    Crop,
    Rotate,
    StripExif,
}

impl Tool {
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "Compress" => Self::Compress,
            "Resize" => Self::Resize,
            "Crop" => Self::Crop,
            "Rotate" => Self::Rotate,
            "Strip EXIF" => Self::StripExif,
            _ => return None,
        })
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Self::Compress => "compressed",
            Self::Resize => "resized",
            Self::Crop => "cropped",
            Self::Rotate => "rotated",
            Self::StripExif => "exif removed",
        }
    }
}

impl Target {
    pub const ALL: [Target; 10] = [
        Target::Jpg,
        Target::Png,
        Target::Webp,
        Target::Heic,
        Target::Avif,
        Target::Tiff,
        Target::Gif,
        Target::Bmp,
        Target::Ico,
        Target::Pdf,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Target::Jpg => "JPG",
            Target::Png => "PNG",
            Target::Webp => "WEBP",
            Target::Heic => "HEIC",
            Target::Avif => "AVIF",
            Target::Tiff => "TIFF",
            Target::Gif => "GIF",
            Target::Bmp => "BMP",
            Target::Ico => "ICO",
            Target::Pdf => "PDF",
        }
    }

    pub fn from_label(label: &str) -> Option<Target> {
        Target::ALL.into_iter().find(|t| t.label() == label)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Target::Jpg => "jpg",
            Target::Png => "png",
            Target::Webp => "webp",
            Target::Heic => "heic",
            Target::Avif => "avif",
            Target::Tiff => "tiff",
            Target::Gif => "gif",
            Target::Bmp => "bmp",
            Target::Ico => "ico",
            Target::Pdf => "pdf",
        }
    }

    /// HEIC and AVIF need libheif with an encoder for them; the rest always work. Asked once:
    /// the wheel checks this for every file it shows.
    pub fn available(self) -> bool {
        static HEIF: OnceLock<(bool, bool)> = OnceLock::new();
        let (hevc, av1) = *HEIF.get_or_init(|| {
            (
                heif::can_encode(heif::Codec::Hevc),
                heif::can_encode(heif::Codec::Av1),
            )
        });
        match self {
            Target::Heic => hevc,
            Target::Avif => av1,
            _ => true,
        }
    }
}

/// The target format used by an image tool. Compression exports WebP; tools that edit pixels
/// keep the source format when it has a matching writer and use PNG otherwise.
pub fn tool_target(ext: &str, tool: Tool) -> Option<Target> {
    let ext = match ext {
        "jpeg" | "jpe" | "jfif" => "jpg",
        "tif" => "tiff",
        "dib" => "bmp",
        "heif" | "hif" => "heic",
        other => other,
    };
    let target = if tool == Tool::Compress {
        Target::Webp
    } else {
        match ext {
            "jpg" => Target::Jpg,
            "png" | "apng" => Target::Png,
            "webp" => Target::Webp,
            "heic" => Target::Heic,
            "avif" => Target::Avif,
            "tiff" => Target::Tiff,
            "gif" => Target::Gif,
            "bmp" => Target::Bmp,
            "ico" => Target::Ico,
            _ => Target::Png,
        }
    };
    target.available().then_some(target)
}

/// Runs one of the picture tools and writes a new image. The original is never changed.
pub fn run_tool(input: &Path, tool: Tool, output: &Path) -> io::Result<()> {
    const MAX_EDGE: u32 = 2048;
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let target = tool_target(&ext, tool)
        .ok_or_else(|| io::Error::other("this image format cannot be written on this system"))?;
    let mut picture = decode(input)?;
    match tool {
        Tool::Compress => {}
        Tool::Resize => {
            let (width, height) = (picture.pixels.width(), picture.pixels.height());
            if width.max(height) > MAX_EDGE {
                picture.pixels = picture
                    .pixels
                    .resize(MAX_EDGE, MAX_EDGE, FilterType::Lanczos3);
                picture.jpeg = None;
            }
        }
        Tool::Crop => {
            let side = picture.pixels.width().min(picture.pixels.height());
            let x = (picture.pixels.width() - side) / 2;
            let y = (picture.pixels.height() - side) / 2;
            picture.pixels = picture.pixels.crop_imm(x, y, side, side);
            picture.jpeg = None;
        }
        Tool::Rotate => {
            picture.pixels = picture.pixels.rotate90();
            picture.jpeg = None;
        }
        Tool::StripExif => picture.exif = None,
    }
    encode(&picture, target, output)
}

/// What this system can't do with HEIC and AVIF, and what to install for it.
pub fn missing_codecs() -> Option<String> {
    heif::missing()
}

/// Extensions read as pictures, lower case. Camera RAW formats come on top (`is_raw`).
pub const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "bmp", "dib", "tif", "tiff", "ico",
    "cur", "tga", "qoi", "ppm", "pgm", "pbm", "pam", "pnm", "hdr", "exr", "dds", "ff", "heic",
    "heif", "hif", "avif", "jxl", "svg", "svgz", "psd", "psb",
];

pub fn is_raw(ext: &str) -> bool {
    let ext = ext.to_uppercase();
    rawler::decoders::supported_extensions()
        .iter()
        .any(|e| *e == ext)
}

pub fn decode(path: &Path) -> io::Result<Picture> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut head = [0u8; 16];
    let n = File::open(path)?.read(&mut head)?;
    let head = &head[..n];
    if heif::is_heif(head) {
        return heif::decode(path);
    }
    // A bare codestream or the ISO container.
    if ext == "jxl" || head.starts_with(&[0xFF, 0x0A]) || head.starts_with(b"\0\0\0\x0cJXL ") {
        return jxl(path);
    }
    match ext.as_str() {
        "svg" | "svgz" => svg(path),
        "psd" | "psb" => psd::decode(&std::fs::read(path)?),
        _ if is_raw(&ext) => raw(path),
        _ => common(path),
    }
}

/// Formats the `image` crate reads, recognised by content first and name second.
fn common(path: &Path) -> io::Result<Picture> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let format = reader.format();
    let decoder = reader.into_decoder().map_err(failed)?;
    let lossy = match format {
        Some(ImageFormat::Jpeg) => true,
        Some(ImageFormat::WebP) => webp::is_lossy(&std::fs::read(path)?),
        _ => false,
    };
    let jpeg = (format == Some(ImageFormat::Jpeg)).then_some(path);
    let mut picture = from_decoder(decoder, lossy, jpeg, true)?;
    // HDR and EXR hold linear light; everything downstream expects sRGB-encoded values.
    if matches!(format, Some(ImageFormat::Hdr | ImageFormat::OpenExr)) {
        picture.pixels = tone_map(picture.pixels);
    }
    // image's TIFF decoder misses the profile; the tiff crate finds it.
    if picture.icc.is_none() && format == Some(ImageFormat::Tiff) {
        picture.icc = tiff::decoder::Decoder::new(BufReader::new(File::open(path)?))
            .ok()
            .and_then(|mut d| d.get_tag_u8_vec(tiff::tags::Tag::IccProfile).ok());
    }
    Ok(picture)
}

/// `orient`: apply the Exif orientation (JPEG, TIFF, WebP); JPEG XL is already upright.
fn from_decoder(
    mut decoder: impl ImageDecoder,
    lossy: bool,
    jpeg: Option<&Path>,
    orient: bool,
) -> io::Result<Picture> {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_BYTES);
    decoder.set_limits(limits).map_err(failed)?;
    if decoder.total_bytes() > MAX_BYTES {
        return Err(too_big());
    }
    let orientation = if orient {
        decoder.orientation().unwrap_or(Orientation::NoTransforms)
    } else {
        Orientation::NoTransforms
    };
    let icc = decoder.icc_profile().ok().flatten();
    let exif = decoder.exif_metadata().ok().flatten().map(|mut exif| {
        let _ = Orientation::remove_from_exif_chunk(&mut exif);
        exif
    });
    let mut pixels = DynamicImage::from_decoder(decoder).map_err(failed)?;
    pixels.apply_orientation(orientation);
    let jpeg = match jpeg {
        Some(path) if orientation == Orientation::NoTransforms => {
            let data = std::fs::read(path)?;
            matches!(pdf::jpeg_components(&data), Some(1 | 3)).then_some(data)
        }
        _ => None,
    };
    // Float samples (EXR, HDR, some JPEG XL) are kept as 16 bits from here on.
    let pixels = match pixels {
        DynamicImage::ImageRgb32F(_) => DynamicImage::ImageRgb16(pixels.to_rgb16()),
        DynamicImage::ImageRgba32F(_) => DynamicImage::ImageRgba16(pixels.to_rgba16()),
        other => other,
    };
    Ok(Picture {
        pixels,
        icc,
        exif,
        lossy,
        jpeg,
    })
}

fn too_big() -> io::Error {
    io::Error::other("this picture is too big to convert")
}

/// JPEG XL: jxl-oxide turns the pixels upright itself. Its Exif box starts with the offset of
/// the TIFF header, which goes, and so does the orientation.
fn jxl(path: &Path) -> io::Result<Picture> {
    let decoder = jxl_oxide::integration::JxlDecoder::new(BufReader::new(File::open(path)?))
        .map_err(failed)?;
    let mut picture = from_decoder(decoder, true, None, false)?;
    picture.exif = picture.exif.take().and_then(|exif| {
        let offset = u32::from_be_bytes(exif.get(..4)?.try_into().ok()?) as usize;
        let mut tiff = exif.get(4 + offset..)?.to_vec();
        let _ = Orientation::remove_from_exif_chunk(&mut tiff);
        Some(tiff)
    });
    Ok(picture)
}

/// Turns linear light into sRGB-encoded values, 16 bits per sample.
fn tone_map(pixels: DynamicImage) -> DynamicImage {
    let encode = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    match pixels {
        DynamicImage::ImageRgb16(img) => {
            let mut img = DynamicImage::ImageRgb16(img).into_rgb32f();
            img.pixels_mut().for_each(|p| p.0 = p.0.map(encode));
            DynamicImage::ImageRgb16(DynamicImage::ImageRgb32F(img).to_rgb16())
        }
        DynamicImage::ImageRgba16(img) => {
            let mut img = DynamicImage::ImageRgba16(img).into_rgba32f();
            img.pixels_mut().for_each(|p| {
                let [r, g, b, a] = p.0;
                p.0 = [encode(r), encode(g), encode(b), a];
            });
            DynamicImage::ImageRgba16(DynamicImage::ImageRgba32F(img).to_rgba16())
        }
        other => other,
    }
}

/// SVGs render at their own size, but small ones (icons) at least 1024 px on the long side.
fn svg(path: &Path) -> io::Result<Picture> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    });
    // Pictures an SVG links to may only come from its own folder: an SVG from anywhere
    // shouldn't read the user's other files (or a pipe, and wait forever).
    let folder = path.parent().and_then(|p| p.canonicalize().ok());
    let default = resvg::usvg::ImageHrefResolver::default_string_resolver();
    let resolve_string: resvg::usvg::ImageHrefStringResolverFn = {
        let folder = folder.clone();
        Box::new(move |href: &str, options: &resvg::usvg::Options| {
            let folder = folder.as_ref()?;
            let relative = Path::new(href);
            if !relative
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
            {
                return None;
            }
            let file = folder.join(relative).canonicalize().ok()?;
            let meta = std::fs::metadata(&file).ok()?;
            (file.starts_with(folder) && meta.is_file() && meta.len() <= 64 << 20)
                .then(|| default(href, options))?
        })
    };
    let options = resvg::usvg::Options {
        fontdb: fonts.clone(),
        resources_dir: folder,
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: resvg::usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string,
        },
        ..Default::default()
    };
    if std::fs::metadata(path)?.len() > 64 << 20 {
        return Err(too_big());
    }
    let tree = resvg::usvg::Tree::from_data(&std::fs::read(path)?, &options).map_err(failed)?;
    let size = tree.size();
    let long = size.width().max(size.height());
    // At least 1024 px on the long side, at most 16384.
    let scale = (1024.0 / long).max(1.0).min(16384.0 / long);
    let (w, h) = (
        (size.width() * scale).ceil() as u32,
        (size.height() * scale).ceil() as u32,
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))
        .ok_or_else(|| failed("the SVG has no size"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let pixels =
        RgbaImage::from_raw(pixmap.width(), pixmap.height(), rgba).ok_or_else(|| failed("SVG"))?;
    Ok(Picture {
        pixels: DynamicImage::ImageRgba8(pixels),
        icc: None,
        exif: None,
        lossy: false,
        jpeg: None,
    })
}

/// Camera RAW, developed with rawler's default steps (demosaic, white balance, sRGB).
fn raw(path: &Path) -> io::Result<Picture> {
    let raw = rawler::decode_file(path).map_err(failed)?;
    let developed = rawler::imgop::develop::RawDevelop::default()
        .develop_intermediate(&raw)
        .map_err(failed)?;
    let mut pixels = developed
        .to_dynamic_image()
        .ok_or_else(|| failed("the RAW file develops to nothing"))?;
    let orientation =
        Orientation::from_exif(raw.orientation.to_u16() as u8).unwrap_or(Orientation::NoTransforms);
    pixels.apply_orientation(orientation);
    Ok(Picture {
        pixels,
        icc: None,
        exif: None,
        lossy: true,
        jpeg: None,
    })
}

/// Writes `picture` as `target` into a new file at `path`.
pub fn encode(picture: &Picture, target: Target, path: &Path) -> io::Result<()> {
    if matches!(target, Target::Heic | Target::Avif) {
        let codec = if target == Target::Heic {
            heif::Codec::Hevc
        } else {
            heif::Codec::Av1
        };
        return heif::encode(picture, codec, path);
    }
    let mut out = BufWriter::with_capacity(256 * 1024, File::create_new(path)?);
    let image = &picture.pixels;
    match target {
        Target::Jpg => {
            let flat = flatten(image);
            let mut encoder = JpegEncoder::new_with_quality(&mut out, 90);
            with_metadata(&mut encoder, picture, is_gray(&flat));
            match flat {
                DynamicImage::ImageLuma8(gray) => encoder.write_image(
                    &gray,
                    gray.width(),
                    gray.height(),
                    image::ExtendedColorType::L8,
                ),
                other => {
                    let rgb = other.to_rgb8();
                    encoder.write_image(
                        &rgb,
                        rgb.width(),
                        rgb.height(),
                        image::ExtendedColorType::Rgb8,
                    )
                }
            }
            .map_err(failed)?;
        }
        Target::Png => {
            let mut encoder = PngEncoder::new(&mut out);
            let image = keep_depth(image);
            with_metadata(&mut encoder, picture, is_gray(&image));
            encoder
                .write_image(
                    image.as_bytes(),
                    image.width(),
                    image.height(),
                    image.color().into(),
                )
                .map_err(failed)?;
        }
        Target::Webp => webp::encode(picture, &mut out)?,
        Target::Tiff => tiff(picture, &mut out)?,
        Target::Gif => {
            let mut encoder = GifEncoder::new_with_speed(&mut out, 10);
            encoder
                .encode_frame(image::Frame::new(image.to_rgba8()))
                .map_err(failed)?;
        }
        Target::Bmp => {
            let image = if image.color().has_alpha() {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            };
            BmpEncoder::new(&mut out)
                .write_image(
                    image.as_bytes(),
                    image.width(),
                    image.height(),
                    image.color().into(),
                )
                .map_err(failed)?;
        }
        Target::Ico => icon(image, &mut out)?,
        Target::Pdf => pdf::write(picture, &mut out)?,
        Target::Heic | Target::Avif => unreachable!("handled above"),
    }
    out.flush()?;
    out.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()
}

fn with_metadata(encoder: &mut impl ImageEncoder, picture: &Picture, gray: bool) {
    if let Some(icc) = profile_for(picture, gray) {
        let _ = encoder.set_icc_profile(icc.to_vec());
    }
    if let Some(exif) = &picture.exif {
        let _ = encoder.set_exif_metadata(exif.clone());
    }
}

fn is_gray(image: &DynamicImage) -> bool {
    !image.color().has_color()
}

/// The picture's colour profile, if it describes the colour model being written: a CMYK
/// profile no longer fits pixels decoded to RGB, nor an RGB one a grey picture.
pub fn profile_for(picture: &Picture, gray: bool) -> Option<&[u8]> {
    let icc = picture.icc.as_deref()?;
    (icc.get(16..20) == Some(if gray { b"GRAY" } else { b"RGB " })).then_some(icc)
}

/// 16 bits stay 16 bits where the format allows; floats and odd layouts become 8 or 16 bit.
fn keep_depth(image: &DynamicImage) -> DynamicImage {
    match image {
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageRgb8(_)
        | DynamicImage::ImageRgba8(_) => image.clone(),
        DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageLumaA16(_)
        | DynamicImage::ImageRgb16(_)
        | DynamicImage::ImageRgba16(_) => image.clone(),
        _ if image.color().has_alpha() => DynamicImage::ImageRgba16(image.to_rgba16()),
        _ => DynamicImage::ImageRgb16(image.to_rgb16()),
    }
}

/// Formats without transparency get the picture on white, not on whatever the hidden colour
/// values happen to be.
pub fn flatten(image: &DynamicImage) -> DynamicImage {
    if !image.color().has_alpha() {
        return match image {
            DynamicImage::ImageLuma8(_) => image.clone(),
            DynamicImage::ImageLuma16(_) => DynamicImage::ImageLuma8(image.to_luma8()),
            _ => DynamicImage::ImageRgb8(image.to_rgb8()),
        };
    }
    let rgba = image.to_rgba8();
    let mut rgb = RgbImage::new(rgba.width(), rgba.height());
    for (out, px) in rgb.pixels_mut().zip(rgba.pixels()) {
        let a = px[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        *out = Rgb([blend(px[0]), blend(px[1]), blend(px[2])]);
    }
    DynamicImage::ImageRgb8(rgb)
}

/// Lossless (Deflate) TIFF with the colour profile, 16 bits kept.
fn tiff<W: Write + Seek>(picture: &Picture, out: &mut W) -> io::Result<()> {
    use tiff::encoder::{Compression, DeflateLevel, Predictor, TiffEncoder, colortype};
    use tiff::tags::Tag;
    let mut encoder = TiffEncoder::new(out)
        .map_err(failed)?
        .with_compression(Compression::Deflate(DeflateLevel::Balanced))
        .with_predictor(Predictor::Horizontal);
    // Grey with alpha is written as RGBA, so it takes an RGB profile.
    let image = match keep_depth(&picture.pixels) {
        gray_alpha @ (DynamicImage::ImageLumaA8(_) | DynamicImage::ImageLumaA16(_)) => {
            DynamicImage::ImageRgba16(gray_alpha.to_rgba16())
        }
        other => other,
    };
    let (w, h) = (image.width(), image.height());
    macro_rules! write {
        ($color:ty, $data:expr) => {{
            let mut img = encoder.new_image::<$color>(w, h).map_err(failed)?;
            if let Some(icc) = profile_for(picture, is_gray(&image)) {
                img.encoder()
                    .write_tag(Tag::IccProfile, icc)
                    .map_err(failed)?;
            }
            img.write_data($data).map_err(failed)
        }};
    }
    match &image {
        DynamicImage::ImageLuma8(i) => write!(colortype::Gray8, i.as_raw()),
        DynamicImage::ImageRgb8(i) => write!(colortype::RGB8, i.as_raw()),
        DynamicImage::ImageRgba8(i) => write!(colortype::RGBA8, i.as_raw()),
        DynamicImage::ImageLuma16(i) => write!(colortype::Gray16, i.as_raw()),
        DynamicImage::ImageRgb16(i) => write!(colortype::RGB16, i.as_raw()),
        DynamicImage::ImageRgba16(i) => write!(colortype::RGBA16, i.as_raw()),
        other if other.color().has_alpha() => write!(colortype::RGBA8, other.to_rgba8().as_raw()),
        other => write!(colortype::RGB8, other.to_rgb8().as_raw()),
    }
}

/// An icon in the usual sizes up to 256 px, each a square with the picture centred.
fn icon(image: &DynamicImage, out: &mut impl Write) -> io::Result<()> {
    let long = image.width().max(image.height());
    let frames = [16, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .filter(|s| *s <= long.max(16))
        .map(|size| {
            let scaled = image.resize(size, size, FilterType::Lanczos3).to_rgba8();
            let mut square = RgbaImage::new(size, size);
            let (x, y) = ((size - scaled.width()) / 2, (size - scaled.height()) / 2);
            image::imageops::overlay(&mut square, &scaled, x.into(), y.into());
            IcoFrame::as_png(square.as_raw(), size, size, image::ExtendedColorType::Rgba8)
                .map_err(failed)
        })
        .collect::<io::Result<Vec<_>>>()?;
    IcoEncoder::new(out).encode_images(&frames).map_err(failed)
}

fn failed(e: impl std::fmt::Display) -> io::Error {
    let message = e.to_string();
    // Decoders word their errors for developers; most of them mean the file is damaged.
    if message.contains("nsupported") {
        io::Error::other(format!(
            "this picture uses a variant that isn't supported ({message})"
        ))
    } else {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }
}
