//! How busy the computer is: the processor, the memory and the system's disk. Through sysinfo,
//! and on Windows from Win32 itself.

use std::sync::Mutex;

#[cfg(not(windows))]
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

use super::Usage;

/// Kept between readings: the processor's use is worked out from the time since the last one.
#[cfg(not(windows))]
static SYSTEM: Mutex<Option<System>> = Mutex::new(None);

#[cfg(not(windows))]
pub fn usage() -> Option<Usage> {
    let mut kept = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
    let system = kept.get_or_insert_with(|| {
        let mut system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        );
        // The first reading has nothing to compare with: take another a moment later.
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        system.refresh_cpu_usage();
        system
    });
    system.refresh_cpu_usage();
    system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
    let total = system.total_memory();
    let memory = if total > 0 {
        system.used_memory() as f32 / total as f32
    } else {
        0.0
    };
    let processor = system.global_cpu_usage() / 100.0;
    drop(kept);
    let disk = root_disk().unwrap_or(0.0);
    Some(Usage {
        processor: processor.clamp(0.0, 1.0),
        memory: memory.clamp(0.0, 1.0),
        disk: disk.clamp(0.0, 1.0),
    })
}

/// How full the disk the system is on is: asked of `/` alone, so other mounts (a network one
/// that hangs) don't come into it.
#[cfg(not(windows))]
fn root_disk() -> Option<f32> {
    // SAFETY: a zeroed statvfs for the call to fill, and a NUL-terminated path.
    let mut fs: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c"/".as_ptr(), &mut fs) } != 0 || fs.f_blocks == 0 {
        return None;
    }
    // Free to anyone, as `df` counts it.
    Some(1.0 - fs.f_bavail as f32 / fs.f_blocks as f32)
}

/// The processor's idle and total time at the last reading, in 100 ns steps.
#[cfg(windows)]
static TIMES: Mutex<Option<(u64, u64)>> = Mutex::new(None);

#[cfg(windows)]
pub fn usage() -> Option<Usage> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    let times = || -> Option<(u64, u64)> {
        let mut t = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 3];
        let [idle, kernel, user] = &mut t;
        // SAFETY: three FILETIMEs to write into.
        if unsafe { GetSystemTimes(idle, kernel, user) } == 0 {
            return None;
        }
        let n = |t: &FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        // The kernel's time counts the idle time too.
        Some((n(&t[0]), n(&t[1]) + n(&t[2])))
    };
    let mut last = TIMES.lock().unwrap_or_else(|e| e.into_inner());
    let before = match *last {
        Some(before) => before,
        None => {
            // The first reading has nothing to compare with: take another a moment later.
            let before = times()?;
            std::thread::sleep(std::time::Duration::from_millis(200));
            before
        }
    };
    let now = times()?;
    *last = Some(now);
    let (idle, total) = (
        now.0.saturating_sub(before.0),
        now.1.saturating_sub(before.1),
    );
    let processor = if total > 0 {
        1.0 - idle as f32 / total as f32
    } else {
        0.0
    };

    // SAFETY: zeroed, then told its own size as the call wants.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: as above.
    let memory = if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
        status.dwMemoryLoad as f32 / 100.0
    } else {
        0.0
    };

    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\";
    let drive: Vec<u16> = drive.encode_utf16().chain([0]).collect();
    let (mut free, mut size, mut all_free) = (0u64, 0u64, 0u64);
    // SAFETY: a NUL-terminated path, and three numbers to write into.
    let found = unsafe { GetDiskFreeSpaceExW(drive.as_ptr(), &mut free, &mut size, &mut all_free) };
    let disk = if found != 0 && size > 0 {
        1.0 - all_free as f32 / size as f32
    } else {
        0.0
    };
    Some(Usage {
        processor: processor.clamp(0.0, 1.0),
        memory: memory.clamp(0.0, 1.0),
        disk: disk.clamp(0.0, 1.0),
    })
}
