//! HEIC, HEIF and AVIF through libheif, opened at run time: nothing is needed to build, and
//! without the library (or one of its codecs) only these formats are missing. Signatures and
//! constants follow libheif's C API (heif.h, stable since 1.17).

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::io;
use std::path::Path;
use std::ptr;
use std::sync::OnceLock;

use image::metadata::Orientation;
use image::{DynamicImage, RgbImage, RgbaImage};

use super::Picture;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Hevc,
    Av1,
}

impl Codec {
    fn id(self) -> c_uint {
        match self {
            Codec::Hevc => 1,
            Codec::Av1 => 4,
        }
    }
}

const COLORSPACE_RGB: c_uint = 1;
const CHROMA_RGB: c_uint = 10;
const CHROMA_RGBA: c_uint = 11;
const CHANNEL_INTERLEAVED: c_uint = 10;
const PROFILE_RICC: c_uint = u32::from_be_bytes(*b"rICC");
const PROFILE_PROF: c_uint = u32::from_be_bytes(*b"prof");
const HEIC_QUALITY: c_int = 80;
const AVIF_QUALITY: c_int = 70;

#[repr(C)]
struct Error {
    code: c_uint,
    subcode: c_uint,
    message: *const c_char,
}

type Ptr = *mut c_void;

struct Api {
    _lib: libloading::Library,
    init: unsafe extern "C" fn(Ptr) -> Error,
    context_alloc: unsafe extern "C" fn() -> Ptr,
    context_free: unsafe extern "C" fn(Ptr),
    read_from_file: unsafe extern "C" fn(Ptr, *const c_char, *const c_void) -> Error,
    primary_handle: unsafe extern "C" fn(Ptr, *mut Ptr) -> Error,
    handle_release: unsafe extern "C" fn(*const c_void),
    handle_width: unsafe extern "C" fn(*const c_void) -> c_int,
    handle_height: unsafe extern "C" fn(*const c_void) -> c_int,
    has_alpha: unsafe extern "C" fn(*const c_void) -> c_int,
    decode_image:
        unsafe extern "C" fn(*const c_void, *mut Ptr, c_uint, c_uint, *const c_void) -> Error,
    image_release: unsafe extern "C" fn(*const c_void),
    image_width: unsafe extern "C" fn(*const c_void, c_uint) -> c_int,
    image_height: unsafe extern "C" fn(*const c_void, c_uint) -> c_int,
    plane_readonly: unsafe extern "C" fn(*const c_void, c_uint, *mut c_int) -> *const u8,
    profile_type: unsafe extern "C" fn(*const c_void) -> c_uint,
    profile_size: unsafe extern "C" fn(*const c_void) -> usize,
    profile: unsafe extern "C" fn(*const c_void, *mut c_void) -> Error,
    metadata_count: unsafe extern "C" fn(*const c_void, *const c_char) -> c_int,
    metadata_ids: unsafe extern "C" fn(*const c_void, *const c_char, *mut u32, c_int) -> c_int,
    metadata_size: unsafe extern "C" fn(*const c_void, u32) -> usize,
    metadata: unsafe extern "C" fn(*const c_void, u32, *mut c_void) -> Error,
    have_decoder: unsafe extern "C" fn(c_uint) -> c_int,
    have_encoder: unsafe extern "C" fn(c_uint) -> c_int,
    get_encoder: unsafe extern "C" fn(Ptr, c_uint, *mut Ptr) -> Error,
    set_quality: unsafe extern "C" fn(Ptr, c_int) -> Error,
    encoder_release: unsafe extern "C" fn(Ptr),
    image_create: unsafe extern "C" fn(c_int, c_int, c_uint, c_uint, *mut Ptr) -> Error,
    add_plane: unsafe extern "C" fn(Ptr, c_uint, c_int, c_int, c_int) -> Error,
    plane: unsafe extern "C" fn(Ptr, c_uint, *mut c_int) -> *mut u8,
    set_profile: unsafe extern "C" fn(Ptr, *const c_char, *const c_void, usize) -> Error,
    encode_image: unsafe extern "C" fn(Ptr, *const c_void, Ptr, *const c_void, *mut Ptr) -> Error,
    add_exif: unsafe extern "C" fn(Ptr, *const c_void, *const c_void, c_int) -> Error,
    write_to_file: unsafe extern "C" fn(Ptr, *const c_char) -> Error,
}

#[cfg(target_os = "macos")]
const NAMES: &[&str] = &[
    "libheif.1.dylib",
    "/opt/homebrew/lib/libheif.1.dylib",
    "/usr/local/lib/libheif.1.dylib",
];
#[cfg(windows)]
const NAMES: &[&str] = &["heif.dll", "libheif.dll"];
#[cfg(not(any(target_os = "macos", windows)))]
const NAMES: &[&str] = &["libheif.so.1", "libheif.so"];

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: libheif's initialisers have no requirements; every symbol below is cast to
        // the signature heif.h declares for it.
        unsafe {
            let lib = NAMES
                .iter()
                .find_map(|name| libloading::Library::new(*name).ok())?;
            macro_rules! sym {
                ($name:literal) => {
                    *lib.get(concat!($name, "\0").as_bytes()).ok()?
                };
            }
            let api = Api {
                init: sym!("heif_init"),
                context_alloc: sym!("heif_context_alloc"),
                context_free: sym!("heif_context_free"),
                read_from_file: sym!("heif_context_read_from_file"),
                primary_handle: sym!("heif_context_get_primary_image_handle"),
                handle_release: sym!("heif_image_handle_release"),
                handle_width: sym!("heif_image_handle_get_width"),
                handle_height: sym!("heif_image_handle_get_height"),
                has_alpha: sym!("heif_image_handle_has_alpha_channel"),
                decode_image: sym!("heif_decode_image"),
                image_release: sym!("heif_image_release"),
                image_width: sym!("heif_image_get_width"),
                image_height: sym!("heif_image_get_height"),
                plane_readonly: sym!("heif_image_get_plane_readonly"),
                profile_type: sym!("heif_image_handle_get_color_profile_type"),
                profile_size: sym!("heif_image_handle_get_raw_color_profile_size"),
                profile: sym!("heif_image_handle_get_raw_color_profile"),
                metadata_count: sym!("heif_image_handle_get_number_of_metadata_blocks"),
                metadata_ids: sym!("heif_image_handle_get_list_of_metadata_block_IDs"),
                metadata_size: sym!("heif_image_handle_get_metadata_size"),
                metadata: sym!("heif_image_handle_get_metadata"),
                have_decoder: sym!("heif_have_decoder_for_format"),
                have_encoder: sym!("heif_have_encoder_for_format"),
                get_encoder: sym!("heif_context_get_encoder_for_format"),
                set_quality: sym!("heif_encoder_set_lossy_quality"),
                encoder_release: sym!("heif_encoder_release"),
                image_create: sym!("heif_image_create"),
                add_plane: sym!("heif_image_add_plane"),
                plane: sym!("heif_image_get_plane"),
                set_profile: sym!("heif_image_set_raw_color_profile"),
                encode_image: sym!("heif_context_encode_image"),
                add_exif: sym!("heif_context_add_exif_metadata"),
                write_to_file: sym!("heif_context_write_to_file"),
                _lib: lib,
            };
            // Loads the codec plugins.
            check((api.init)(ptr::null_mut())).ok()?;
            Some(api)
        }
    })
    .as_ref()
}

pub fn can_encode(codec: Codec) -> bool {
    // SAFETY: a query with no pointers.
    api().is_some_and(|api| unsafe { (api.have_encoder)(codec.id()) } != 0)
}

fn can_decode(codec: Codec) -> bool {
    // SAFETY: a query with no pointers.
    api().is_some_and(|api| unsafe { (api.have_decoder)(codec.id()) } != 0)
}

const HEVC_PLUGINS: &str =
    "libheif's libde265 and x265 plugins (on Fedora: libheif-freeworld from RPM Fusion)";
const AV1_PLUGINS: &str = "libheif's dav1d and aom plugins";

/// What this system can't do with HEIC and AVIF, and what to install for it.
pub fn missing() -> Option<String> {
    if api().is_none() {
        return Some("opening and saving HEIC and AVIF needs libheif".into());
    }
    let gap = |name: &str, codec: Codec, plugins: &str| {
        let what = match (can_decode(codec), can_encode(codec)) {
            (true, true) => return None,
            (false, false) => "opening and saving",
            (false, true) => "opening",
            (true, false) => "saving",
        };
        Some(format!("{what} {name} needs {plugins}"))
    };
    let gaps: Vec<String> = [
        gap("HEIC", Codec::Hevc, HEVC_PLUGINS),
        gap("AVIF", Codec::Av1, AV1_PLUGINS),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!gaps.is_empty()).then(|| gaps.join("; "))
}

/// ISO media files with a HEIF or AVIF brand.
pub fn is_heif(head: &[u8]) -> bool {
    head.get(4..8) == Some(b"ftyp")
        && head.get(8..12).is_some_and(|brand| {
            [
                b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"msf1", b"avif",
                b"avis",
            ]
            .iter()
            .any(|b| *b == brand)
        })
}

fn check(e: Error) -> io::Result<()> {
    if e.code == 0 {
        return Ok(());
    }
    // SAFETY: libheif always sets a static, NUL-terminated message.
    let message = unsafe { CStr::from_ptr(e.message) }
        .to_string_lossy()
        .into_owned();
    Err(io::Error::new(io::ErrorKind::InvalidData, message))
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_encoded_bytes()).map_err(io::Error::other)
}

/// Frees what it holds when dropped, whichever way decoding or encoding ends.
struct Owned<'a> {
    api: &'a Api,
    ptr: Ptr,
    free: fn(&Api, Ptr),
}

impl Drop for Owned<'_> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            (self.free)(self.api, self.ptr);
        }
    }
}

fn context(api: &Api) -> Owned<'_> {
    // SAFETY: freed exactly once by `Owned`.
    let ptr = unsafe { (api.context_alloc)() };
    Owned {
        api,
        ptr,
        free: |api, p| unsafe { (api.context_free)(p) },
    }
}

fn handle(api: &Api, ptr: Ptr) -> Owned<'_> {
    Owned {
        api,
        ptr,
        free: |api, p| unsafe { (api.handle_release)(p) },
    }
}

fn image(api: &Api, ptr: Ptr) -> Owned<'_> {
    Owned {
        api,
        ptr,
        free: |api, p| unsafe { (api.image_release)(p) },
    }
}

pub fn decode(path: &Path) -> io::Result<Picture> {
    let api = api()
        .ok_or_else(|| io::Error::other("HEIC and AVIF need libheif, which isn't installed"))?;
    let name = c_path(path)?;
    let ctx = context(api);
    // SAFETY: every pointer passed is live for the call; outputs are owned right away.
    unsafe {
        check((api.read_from_file)(ctx.ptr, name.as_ptr(), ptr::null()))?;
        let mut h = ptr::null_mut();
        check((api.primary_handle)(ctx.ptr, &mut h))?;
        let h = handle(api, h);
        let alpha = (api.has_alpha)(h.ptr) != 0;
        let channels = if alpha { 4usize } else { 3usize };
        let (declared_w, declared_h) = ((api.handle_width)(h.ptr), (api.handle_height)(h.ptr));
        if declared_w <= 0 || declared_h <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "libheif returned invalid image dimensions",
            ));
        }
        let declared_bytes = declared_w as u128 * declared_h as u128 * channels as u128;
        if declared_bytes > super::MAX_BYTES as u128 {
            return Err(io::Error::other("this picture is too big to convert"));
        }
        let mut img = ptr::null_mut();
        let decoded = (api.decode_image)(
            h.ptr,
            &mut img,
            COLORSPACE_RGB,
            if alpha { CHROMA_RGBA } else { CHROMA_RGB },
            ptr::null(),
        );
        let img = image(api, img);
        if let Err(e) = check(decoded) {
            // The usual reason on Linux: a distribution's libheif without the patented codec.
            let text = e.to_string();
            let missing = text.contains("No decoding plugin");
            return Err(if missing && text.contains("HEVC") {
                io::Error::other(format!("opening HEIC needs {HEVC_PLUGINS}"))
            } else if missing && text.contains("AV1") {
                io::Error::other(format!("opening AVIF needs {AV1_PLUGINS}"))
            } else {
                e
            });
        }
        let (w, h_px) = (
            (api.image_width)(img.ptr, CHANNEL_INTERLEAVED),
            (api.image_height)(img.ptr, CHANNEL_INTERLEAVED),
        );
        if w <= 0 || h_px <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "libheif returned invalid image dimensions",
            ));
        }
        let row_bytes = w as u128 * channels as u128;
        let image_bytes = row_bytes * h_px as u128;
        if image_bytes > super::MAX_BYTES as u128 {
            return Err(io::Error::other("this picture is too big to convert"));
        }
        let (row, total) = (row_bytes as usize, image_bytes as usize);
        let mut stride = 0;
        let data = (api.plane_readonly)(img.ptr, CHANNEL_INTERLEAVED, &mut stride);
        if data.is_null() || stride <= 0 || (stride as usize) < row {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "libheif returned no pixels",
            ));
        }
        let mut pixels = Vec::with_capacity(total);
        for y in 0..h_px as usize {
            pixels.extend_from_slice(std::slice::from_raw_parts(
                data.add(y * stride as usize),
                row,
            ));
        }
        let pixels = if alpha {
            DynamicImage::ImageRgba8(
                RgbaImage::from_raw(w as u32, h_px as u32, pixels).unwrap_or_default(),
            )
        } else {
            DynamicImage::ImageRgb8(
                RgbImage::from_raw(w as u32, h_px as u32, pixels).unwrap_or_default(),
            )
        };

        let icc = match (api.profile_type)(h.ptr) {
            PROFILE_PROF | PROFILE_RICC => {
                let mut icc = vec![0u8; (api.profile_size)(h.ptr)];
                check((api.profile)(h.ptr, icc.as_mut_ptr().cast()))
                    .ok()
                    .map(|()| icc)
            }
            _ => None,
        };
        Ok(Picture {
            pixels,
            icc,
            exif: exif(api, h.ptr),
            lossy: true,
            jpeg: None,
        })
    }
}

/// The first Exif block, as TIFF data. libheif already turned the pixels upright, so the
/// orientation tag goes.
unsafe fn exif(api: &Api, h: Ptr) -> Option<Vec<u8>> {
    let kind = c"Exif";
    // SAFETY: `h` is a live handle; buffers are sized from libheif's own answers.
    unsafe {
        if (api.metadata_count)(h, kind.as_ptr()) < 1 {
            return None;
        }
        let mut id = 0u32;
        (api.metadata_ids)(h, kind.as_ptr(), &mut id, 1);
        let mut data = vec![0u8; (api.metadata_size)(h, id)];
        check((api.metadata)(h, id, data.as_mut_ptr().cast())).ok()?;
        // The block starts with the offset of the TIFF header after these four bytes.
        let offset = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
        let mut tiff = data.get(4 + offset..)?.to_vec();
        let _ = Orientation::remove_from_exif_chunk(&mut tiff);
        Some(tiff)
    }
}

pub fn encode(picture: &Picture, codec: Codec, path: &Path) -> io::Result<()> {
    let api = api()
        .ok_or_else(|| io::Error::other("HEIC and AVIF need libheif, which isn't installed"))?;
    if !can_encode(codec) {
        return Err(io::Error::other(missing().unwrap_or_default()));
    }
    let alpha = picture.pixels.color().has_alpha();
    let (pixels, channels) = if alpha {
        (picture.pixels.to_rgba8().into_raw(), 4)
    } else {
        (picture.pixels.to_rgb8().into_raw(), 3)
    };
    let (w, h) = (
        picture.pixels.width() as c_int,
        picture.pixels.height() as c_int,
    );
    let name = c_path(path)?;
    let ctx = context(api);
    // SAFETY: as in `decode`; the plane is written within the stride libheif reports.
    unsafe {
        let mut enc = ptr::null_mut();
        check((api.get_encoder)(ctx.ptr, codec.id(), &mut enc))?;
        let enc = Owned {
            api,
            ptr: enc,
            free: |api, p| (api.encoder_release)(p),
        };
        check((api.set_quality)(
            enc.ptr,
            if codec == Codec::Hevc {
                HEIC_QUALITY
            } else {
                AVIF_QUALITY
            },
        ))?;

        let mut img = ptr::null_mut();
        check((api.image_create)(
            w,
            h,
            COLORSPACE_RGB,
            if alpha { CHROMA_RGBA } else { CHROMA_RGB },
            &mut img,
        ))?;
        let img = image(api, img);
        check((api.add_plane)(img.ptr, CHANNEL_INTERLEAVED, w, h, 8))?;
        let mut stride = 0;
        let plane = (api.plane)(img.ptr, CHANNEL_INTERLEAVED, &mut stride);
        if plane.is_null() {
            return Err(io::Error::other("libheif gave no image to fill"));
        }
        let row = w as usize * channels;
        for y in 0..h as usize {
            ptr::copy_nonoverlapping(
                pixels.as_ptr().add(y * row),
                plane.add(y * stride as usize),
                row,
            );
        }
        if let Some(icc) = super::profile_for(picture, false) {
            check((api.set_profile)(
                img.ptr,
                c"prof".as_ptr(),
                icc.as_ptr().cast(),
                icc.len(),
            ))?;
        }
        let mut out = ptr::null_mut();
        check((api.encode_image)(
            ctx.ptr,
            img.ptr,
            enc.ptr,
            ptr::null(),
            &mut out,
        ))?;
        let out = handle(api, out);
        if let Some(exif) = &picture.exif {
            check((api.add_exif)(
                ctx.ptr,
                out.ptr,
                exif.as_ptr().cast(),
                exif.len() as c_int,
            ))?;
        }
        check((api.write_to_file)(ctx.ptr, name.as_ptr()))?;
    }
    std::fs::File::open(path)?.sync_all()
}
