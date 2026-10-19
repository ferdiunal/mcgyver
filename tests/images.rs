//! Real picture conversions through `convert::run`, and the Actions wheel.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, GenericImageView, ImageEncoder, Rgba, RgbaImage};
use mcgyver::actions::{self, Action};
use mcgyver::archive::Progress;
use mcgyver::catalog::{self, Kind, Mode};
use mcgyver::convert::{self, Task};
use mcgyver::images::{self, Target};

/// 64×40, a colour gradient with a transparent band on the right: wider than tall, so a turn
/// shows, and with alpha, so flattening shows.
fn sample() -> RgbaImage {
    RgbaImage::from_fn(64, 40, |x, y| {
        let a = if x >= 56 { 0 } else { 255 };
        Rgba([(x * 4) as u8, (y * 6) as u8, 128, a])
    })
}

/// Minimal big-endian Exif: one IFD with Orientation (0x0112) and Make (0x010F, "Zero").
fn exif(orientation: u16) -> Vec<u8> {
    let mut e = b"MM\0\x2a\0\0\0\x08\0\x02".to_vec();
    e.extend([0x01, 0x0F, 0, 2, 0, 0, 0, 5, 0, 0, 0, 38]);
    e.extend([0x01, 0x12, 0, 3, 0, 0, 0, 1]);
    e.extend(orientation.to_be_bytes());
    e.extend([0, 0]);
    e.extend([0, 0, 0, 0]);
    e.extend(b"Zero\0");
    e
}

fn write_png(path: &Path, image: &RgbaImage, icc: Option<&[u8]>) {
    let mut out = fs::File::create(path).unwrap();
    let mut enc = PngEncoder::new(&mut out);
    if let Some(icc) = icc {
        enc.set_icc_profile(icc.to_vec()).unwrap();
    }
    enc.write_image(
        image,
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
}

fn write_jpeg(path: &Path, image: &RgbaImage, exif: Option<Vec<u8>>) {
    let rgb = DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let mut out = fs::File::create(path).unwrap();
    let mut enc = JpegEncoder::new_with_quality(&mut out, 95);
    if let Some(exif) = exif {
        enc.set_exif_metadata(exif).unwrap();
    }
    enc.write_image(
        &rgb,
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
}

fn convert(file: &Path, target: Target) -> PathBuf {
    let progress = Progress::default();
    let made = convert::run(file, Task::Picture(target), &progress)
        .unwrap_or_else(|e| panic!("{} → {target:?}: {e}", file.display()));
    assert_eq!(progress.fraction(), 1.0);
    made
}

fn close(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tolerance)
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok()
}

#[test]
fn a_png_becomes_every_target() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("pic.png");
    write_png(&png, &sample(), None);
    for target in Target::ALL.into_iter().filter(|t| t.available()) {
        let made = convert(&png, target);
        // PNG to PNG makes a second file, as the wheel never offers it.
        let ext = catalog::extension_for(target.label());
        let name = if target == Target::Png {
            "pic 2.png".to_owned()
        } else {
            format!("pic.{ext}")
        };
        assert_eq!(made, dir.path().join(name), "{target:?}");
        if target == Target::Pdf {
            let pdf = fs::read(&made).unwrap();
            assert!(pdf.starts_with(b"%PDF-"));
            assert!(
                pdf.windows(6).any(|w| w == b"/SMask"),
                "transparency kept as a mask"
            );
            continue;
        }
        let back = images::decode(&made)
            .unwrap_or_else(|e| panic!("reading {target:?} back: {e}"))
            .pixels;
        if target == Target::Ico {
            // Square frames; the largest is 64 px, the picture centred in it.
            assert_eq!(back.dimensions(), (64, 64));
            continue;
        }
        assert_eq!(back.dimensions(), (64, 40), "{target:?}");
        let px = back.to_rgba8().get_pixel(20, 10).0;
        let tolerance = if target == Target::Gif { 24 } else { 12 };
        assert!(
            close([px[0], px[1], px[2]], [80, 60, 128], tolerance),
            "{target:?}: {px:?}"
        );
        // Formats without transparency put it on white.
        let edge = back.to_rgba8().get_pixel(60, 10).0;
        if matches!(target, Target::Jpg) {
            assert!(
                close([edge[0], edge[1], edge[2]], [255, 255, 255], 12),
                "{target:?}: {edge:?}"
            );
        } else if back.color().has_alpha() {
            assert!(edge[3] < 16, "{target:?} lost transparency: {edge:?}");
        }
    }
}

#[test]
fn turned_photos_come_out_upright_without_the_turn_in_their_exif() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("turned.jpg");
    // Orientation 6: show turned 90° clockwise.
    write_jpeg(&jpg, &sample(), Some(exif(6)));
    for target in [Target::Png, Target::Webp, Target::Jpg, Target::Tiff] {
        let made = convert(&jpg, target);
        let back = images::decode(&made).unwrap();
        assert_eq!(back.pixels.dimensions(), (40, 64), "{target:?}");
        // Our TIFFs carry no Exif.
        if target != Target::Tiff {
            let exif = back
                .exif
                .unwrap_or_else(|| panic!("{target:?} dropped the Exif"));
            let left = image::metadata::Orientation::from_exif_chunk(&exif);
            assert!(
                matches!(
                    left,
                    None | Some(image::metadata::Orientation::NoTransforms)
                ),
                "{target:?} kept the turn: {left:?}"
            );
            assert!(
                exif.windows(4).any(|w| w == b"Zero"),
                "{target:?} lost the other tags"
            );
        }
    }
}

#[test]
fn colour_profiles_travel() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("p3.png");
    // Not a real profile, but it says RGB where a profile does, and is kept byte for byte.
    let mut icc: Vec<u8> = (0..400u32).map(|i| (i * 7) as u8).collect();
    icc[16..20].copy_from_slice(b"RGB ");
    write_png(&png, &sample(), Some(&icc));
    for target in [Target::Jpg, Target::Png, Target::Webp, Target::Tiff] {
        let back = images::decode(&convert(&png, target)).unwrap();
        assert_eq!(back.icc.as_deref(), Some(&icc[..]), "{target:?}");
    }
}

#[test]
fn a_jpeg_goes_into_a_pdf_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("photo.jpg");
    write_jpeg(&jpg, &sample(), None);
    let pdf = fs::read(convert(&jpg, Target::Pdf)).unwrap();
    let original = fs::read(&jpg).unwrap();
    assert!(
        pdf.windows(original.len()).any(|w| w == original),
        "the JPEG was re-encoded"
    );
    assert!(pdf.windows(10).any(|w| w == b"/DCTDecode"));
}

#[test]
fn svgs_render_large_enough() {
    let dir = tempfile::tempdir().unwrap();
    let svg = dir.path().join("icon.svg");
    fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16"><rect width="32" height="16" fill="#ff0000"/></svg>"##).unwrap();
    assert_eq!(catalog::kind_of(&svg), Some(Kind::Image));
    let png = convert(&svg, Target::Png);
    let back = image::open(&png).unwrap();
    assert_eq!(back.dimensions(), (1024, 512));
    assert_eq!(back.to_rgba8().get_pixel(500, 200).0, [255, 0, 0, 255]);
}

#[test]
fn magick_made_formats_are_read() {
    if !have("magick") {
        eprintln!("no ImageMagick, skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("src.png");
    write_png(&png, &sample(), None);
    let mut read = Vec::new();
    for ext in [
        "jxl", "psd", "tga", "qoi", "ppm", "dds", "exr", "bmp", "gif", "tiff", "ico", "webp",
    ] {
        let file = dir.path().join(format!("from-magick.{ext}"));
        let ok = Command::new("magick")
            .arg(&png)
            .arg(&file)
            .status()
            .unwrap()
            .success();
        if !ok {
            continue;
        }
        let back = images::decode(&file)
            .unwrap_or_else(|e| panic!("{ext}: {e}"))
            .pixels;
        assert_eq!(back.dimensions(), (64, 40), "{ext}");
        // EXR is linear light by convention, but ImageMagick stores the sRGB values as they are.
        if ext != "exr" {
            let px = back.to_rgba8().get_pixel(20, 10).0;
            assert!(
                close([px[0], px[1], px[2]], [80, 60, 128], 16),
                "{ext}: {px:?}"
            );
        }
        read.push(ext);
    }
    assert!(read.contains(&"jxl") && read.contains(&"psd"), "{read:?}");
    eprintln!("read from ImageMagick: {read:?}");
}

#[test]
fn what_we_write_magick_reads() {
    if !have("magick") {
        eprintln!("no ImageMagick, skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("pic.png");
    write_png(&png, &sample(), None);
    // This ImageMagick may lack HEIF support; libheif's own round trip is tested above.
    for target in Target::ALL
        .into_iter()
        .filter(|t| t.available() && !matches!(t, Target::Pdf | Target::Heic | Target::Avif))
    {
        let made = convert(&png, target);
        let out = Command::new("magick")
            .args(["identify", "-format", "%w %h\n"])
            .arg(&made)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "magick couldn't read our {target:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if target != Target::Ico {
            assert_eq!(text.lines().next(), Some("64 40"), "{target:?}");
        }
    }
}

#[test]
fn avif_round_trips_through_libheif_when_present() {
    if !Target::Avif.available() {
        eprintln!("libheif has no AV1 encoder here, skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("pic.png");
    write_png(&png, &sample(), None);
    let avif = convert(&png, Target::Avif);
    assert_eq!(catalog::kind_of(&avif), Some(Kind::Image));
    let png_again = convert(&avif, Target::Png);
    let back = image::open(png_again).unwrap();
    assert_eq!(back.dimensions(), (64, 40));
    let px = back.to_rgba8().get_pixel(20, 10).0;
    assert!(close([px[0], px[1], px[2]], [80, 60, 128], 20), "{px:?}");
}

#[test]
fn the_image_wheel_offers_what_this_machine_writes() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("a.jpg");
    write_jpeg(&jpg, &sample(), None);
    let labels: Vec<_> = catalog::choices(std::slice::from_ref(&jpg), Mode::Convert)
        .iter()
        .map(|c| c.label)
        .collect();
    let expected: Vec<_> = Target::ALL
        .into_iter()
        .filter(|t| t.available() && *t != Target::Jpg)
        .map(|t| t.label())
        .collect();
    assert_eq!(labels, expected);
    for choice in catalog::choices(std::slice::from_ref(&jpg), Mode::Convert) {
        assert!(
            matches!(convert::task(&jpg, choice), Some(Task::Picture(_))),
            "{} isn't real",
            choice.label
        );
    }
}

#[test]
fn zip_action_packs_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "a").unwrap();
    fs::write(&b, "b").unwrap();
    let said = actions::run(&[a.clone(), b], Action::Zip, &Progress::default()).unwrap();
    assert_eq!(said, "Saved as Archive.zip");
    let said = actions::run(std::slice::from_ref(&a), Action::Zip, &Progress::default()).unwrap();
    assert_eq!(said, "Saved as a.txt.zip");
    let unpacked = convert::run(
        &dir.path().join("Archive.zip"),
        Task::Extract,
        &Progress::default(),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(unpacked.join("b.txt")).unwrap(), "b");
}

#[test]
fn archives_offer_unzip_first() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    fs::write(&a, "a").unwrap();
    actions::run(std::slice::from_ref(&a), Action::Zip, &Progress::default()).unwrap();
    let zip = dir.path().join("a.txt.zip");
    let choices = catalog::choices(std::slice::from_ref(&zip), Mode::Convert);
    assert_eq!(choices[0].label, catalog::UNZIP);
    assert_eq!(convert::task(&zip, choices[0]), Some(Task::Extract));
    // Archives have no tools.
    assert!(catalog::choices(std::slice::from_ref(&zip), Mode::Tools).is_empty());
}

#[test]
fn a_profile_for_another_colour_model_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("cmyk-tagged.png");
    let mut icc = vec![0u8; 200];
    icc[16..20].copy_from_slice(b"CMYK");
    write_png(&png, &sample(), Some(&icc));
    let back = images::decode(&convert(&png, Target::Jpg)).unwrap();
    assert_eq!(back.icc, None);
}

#[test]
fn headers_claiming_huge_pictures_are_refused_not_allocated() {
    let dir = tempfile::tempdir().unwrap();
    // farbfeld: magic, then width and height; 16 bytes in all.
    let ff = dir.path().join("huge.ff");
    let mut data = b"farbfeld".to_vec();
    data.extend(0x0010_0000u32.to_be_bytes());
    data.extend(0x0010_0000u32.to_be_bytes());
    fs::write(&ff, data).unwrap();
    assert!(images::decode(&ff).is_err());
    // A PSB claiming 300000 × 300000, RGB, 8 bits, with nothing after the header.
    let psb = dir.path().join("huge.psb");
    let mut data = b"8BPS\0\x02\0\0\0\0\0\0\0\x03".to_vec();
    data.extend(300_000u32.to_be_bytes());
    data.extend(300_000u32.to_be_bytes());
    data.extend([0, 8, 0, 3]);
    data.extend([0; 4]);
    data.extend([0; 4]);
    data.extend([0; 8]);
    data.extend([0, 0]);
    fs::write(&psb, data).unwrap();
    assert!(images::decode(&psb).is_err());
}

#[test]
fn huge_svgs_are_drawn_smaller() {
    let dir = tempfile::tempdir().unwrap();
    let svg = dir.path().join("poster.svg");
    fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="20000" height="10000"><rect width="20000" height="10000" fill="#00ff00"/></svg>"##).unwrap();
    let back = image::open(convert(&svg, Target::Png)).unwrap();
    assert_eq!(back.dimensions(), (16384, 8192));
}

#[test]
fn svgs_only_use_pictures_from_their_own_folder() {
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.png");
    write_png(
        &secret,
        &RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])),
        None,
    );
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local.png");
    write_png(
        &local,
        &RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255])),
        None,
    );
    let svg = dir.path().join("collage.svg");
    fs::write(&svg, format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="2048" height="1024">
            <image x="0" y="0" width="1024" height="1024" xlink:href="{}"/>
            <image x="1024" y="0" width="1024" height="1024" xlink:href="local.png"/>
        </svg>"##,
        secret.display()
    ))
    .unwrap();
    let back = image::open(convert(&svg, Target::Png)).unwrap().to_rgba8();
    assert_eq!(back.get_pixel(500, 500)[3], 0, "an outside file was read");
    assert_eq!(
        back.get_pixel(1500, 500).0,
        [0, 0, 255, 255],
        "the local picture is missing"
    );
}

#[test]
fn turned_jpeg_xl_is_turned_once() {
    if !have("magick") {
        eprintln!("no ImageMagick, skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("src.png");
    write_png(&png, &sample(), None);
    let jxl = dir.path().join("turned.jxl");
    assert!(
        Command::new("magick")
            .arg(&png)
            .args(["-orient", "RightTop"])
            .arg(&jxl)
            .status()
            .unwrap()
            .success()
    );
    let back = images::decode(&convert(&jxl, Target::Png)).unwrap();
    assert_eq!(back.pixels.dimensions(), (40, 64));
}
