//! Icons on Windows: what Explorer shows for a shortcut, a program or a Start menu app, asked
//! of the shell, which gives a bitmap that is read back as pixels.

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::core::PCWSTR;

use super::{Icon, fit};

/// The icon of an app, asked of the shell on a thread of its own: it needs COM set up, which
/// the caller's thread may have done differently.
pub fn app(id: &str, px: u32) -> Option<Icon> {
    if px == 0 {
        return None;
    }
    let id = id.to_owned();
    std::thread::spawn(move || {
        // SAFETY: balanced with `CoUninitialize` below, on this thread, only if it succeeded.
        if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
            return None;
        }
        let icon = shell_icon(&id, px);
        // SAFETY: the thread's COM was initialised above.
        unsafe { CoUninitialize() };
        icon
    })
    .join()
    .ok()?
}

/// The shell's icon for a path, or for an app id of the Start menu, about `px` square.
fn shell_icon(id: &str, px: u32) -> Option<Icon> {
    let name = if Path::new(id).is_absolute() {
        id.to_owned()
    } else {
        format!("shell:AppsFolder\\{id}")
    };
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
    let side = px as i32;
    // SAFETY: the factory is a live COM object; the bitmap it returns is ours to delete.
    let bitmap = unsafe {
        factory.GetImage(
            SIZE { cx: side, cy: side },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )
    }
    .ok()?;
    let pixels = read_bitmap(bitmap);
    // SAFETY: the bitmap is ours and isn't used past this.
    unsafe {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    let (w, h, rgba) = pixels?;
    fit(image::RgbaImage::from_raw(w, h, rgba)?, px)
}

/// A bitmap's width, height and pixels as straight RGBA. A bitmap whose alpha is all zero has
/// none (some icons come that way), so it counts as opaque.
fn read_bitmap(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = BITMAP::default();
    // SAFETY: `info` is a BITMAP, the size given.
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0),
            size_of::<BITMAP>() as i32,
            Some(&raw mut info as *mut c_void),
        )
    };
    let (w, h) = (info.bmWidth, info.bmHeight);
    if got == 0 || w <= 0 || h <= 0 {
        return None;
    }
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            // Negative: rows from the top.
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bytes = vec![0u8; w as usize * h as usize * 4];
    // SAFETY: the screen's device context is released after use; `bytes` holds `h` rows of `w`
    // 32-bit pixels, as asked of `GetDIBits`.
    let lines = unsafe {
        let dc = GetDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            h as u32,
            Some(bytes.as_mut_ptr() as *mut c_void),
            &mut header,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        lines
    };
    if lines == 0 {
        return None;
    }
    let opaque = bytes.as_chunks::<4>().0.iter().all(|p| p[3] == 0);
    for p in bytes.as_chunks_mut::<4>().0 {
        let (b, g, r) = (p[0], p[1], p[2]);
        let a = if opaque { 255 } else { p[3] };
        let straight = |c: u8| {
            if opaque || a == 0 {
                c
            } else {
                ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
            }
        };
        p.copy_from_slice(&[straight(r), straight(g), straight(b), a]);
    }
    Some((w as u32, h as u32, bytes))
}
