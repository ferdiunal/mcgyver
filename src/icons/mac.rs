//! Icons on macOS: what Finder shows for an .app bundle, drawn by AppKit into a bitmap of the
//! size asked, which picks the picture of the icon that suits it best.

use std::ffi::c_void;
use std::ptr::null_mut;

use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use super::Icon;

// Grand Central Dispatch, to run on the main thread.
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_sync_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    fn pthread_main_np() -> i32;
}

/// Runs `f` on the main thread, where AppKit is to be used, and waits for it.
fn on_main<T: Send, F: FnOnce() -> T + Send>(f: F) -> T {
    // SAFETY: only asks which thread this is.
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }
    struct Job<F, T> {
        f: Option<F>,
        out: Option<T>,
    }
    extern "C" fn work<F: FnOnce() -> T, T>(context: *mut c_void) {
        // SAFETY: `context` is the `Job` below, alive until `dispatch_sync_f` returns.
        let job = unsafe { &mut *(context as *mut Job<F, T>) };
        job.out = job.f.take().map(|f| f());
    }
    let mut job = Job {
        f: Some(f),
        out: None,
    };
    // SAFETY: the main queue runs `work` once with the job, before this returns.
    unsafe {
        dispatch_sync_f(
            &raw const _dispatch_main_q,
            &mut job as *mut Job<F, T> as *mut c_void,
            work::<F, T>,
        );
    }
    job.out.expect("the main queue ran the job")
}

pub fn app(path: &str, px: u32) -> Option<Icon> {
    if px == 0 || !std::path::Path::new(path).exists() {
        return None;
    }
    let path = path.to_owned();
    on_main(move || draw(&path, px))
}

/// The icon of the file at `path`, drawn `px` pixels square.
fn draw(path: &str, px: u32) -> Option<Icon> {
    let side = px as isize;
    let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path));
    image.setSize(NSSize::new(px as f64, px as f64));
    // SAFETY: AppKit makes the bitmap's own pixels (no planes given), 8 bits of red, green,
    // blue and alpha, premultiplied, rows top to bottom.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            null_mut(),
            side,
            side,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            32,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    image.drawInRect_fromRect_operation_fraction(
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(px as f64, px as f64)),
        NSRect::ZERO,
        NSCompositingOperation::SourceOver,
        1.0,
    );
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let stride = bitmap.bytesPerRow() as usize;
    let data = bitmap.bitmapData();
    if data.is_null() || stride < px as usize * 4 {
        return None;
    }
    // SAFETY: the bitmap holds `px` rows of `stride` bytes at `data`, alive as long as `bitmap`.
    let bytes = unsafe { std::slice::from_raw_parts(data, stride * px as usize) };
    let mut rgba = Vec::with_capacity((px * px * 4) as usize);
    for row in bytes.chunks(stride) {
        for p in row[..px as usize * 4].as_chunks::<4>().0 {
            let a = p[3] as u32;
            let straight = |c: u8| {
                (c as u32 * 255 + a / 2)
                    .checked_div(a)
                    .map_or(0, |v| v.min(255) as u8)
            };
            rgba.extend_from_slice(&[straight(p[0]), straight(p[1]), straight(p[2]), p[3]]);
        }
    }
    Some(Icon {
        size: [px as usize, px as usize],
        rgba: std::sync::Arc::new(rgba),
    })
}
