//! The desk on Windows: the player through the system's media controls (the ones the volume
//! flyout shows), the volume through the default speakers, keyboard layouts through the input
//! languages, the brightness through WMI (built-in screens only), and the apps the Start menu
//! lists.

use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Sessions,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Media::MediaPlaybackAutoRepeatMode as AutoRepeat;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows_sys::Win32::Globalization::{
    GetLocaleInfoEx, GetUserDefaultUILanguage, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED;
use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetSuspendState,
    SetThreadExecutionState,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW,
};
use windows_sys::Win32::System::Shutdown::LockWorkStation;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, HKL, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VK_LWIN, VK_SHIFT, VK_SNAPSHOT,
};
use windows_sys::Win32::UI::Shell::{
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHEmptyRecycleBinW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, HWND_BROADCAST, PostMessageW, SMTO_ABORTIFHUNG,
    SW_SHOWNORMAL, SendMessageTimeoutW, WM_INPUTLANGCHANGEREQUEST, WM_SETTINGCHANGE,
};

use super::{App, Command, Language, Layout, MediaKey, Messenger, Player, Repeat, Shot, Volume};
use crate::actions::quiet;

/// How long the list of installed apps is trusted before the Start menu is read again.
const APPS_FRESH: Duration = Duration::from_secs(60);
/// How long the brightness read is shown before it is read again, behind the scenes.
const BRIGHTNESS_FRESH: Duration = Duration::from_secs(5);

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Joins this thread to COM's free-threaded side; again is harmless.
fn com() {
    // SAFETY: no reserved pointer; a thread already joined just says so.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// What a PowerShell script prints, if it is done within `limit`; otherwise it is stopped.
fn powershell_within(script: &str, limit: Duration) -> Option<String> {
    let mut child = quiet(
        Process::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )
    .spawn()
    .ok()?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(15)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut said = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut said).ok()?;
    status.success().then(|| said.trim().to_owned())
}

fn powershell(script: &str) -> Option<String> {
    let out = quiet(
        Process::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::null())
            .stderr(Stdio::null()),
    )
    .output()
    .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn sessions() -> Vec<Session> {
    com();
    Sessions::RequestAsync()
        .and_then(|op| op.join())
        .and_then(|manager| manager.GetSessions())
        .map(|list| list.into_iter().collect())
        .unwrap_or_default()
}

fn session(id: &str) -> Option<Session> {
    sessions().into_iter().find(|s| {
        s.SourceAppUserModelId()
            .is_ok_and(|a| a.to_string_lossy() == id)
    })
}

/// The player playing, or else one paused.
pub fn player() -> Option<Player> {
    let mut players: Vec<Player> = sessions().iter().filter_map(read_session).collect();
    players.sort_by_key(|p| !p.playing);
    players.into_iter().next()
}

fn read_session(session: &Session) -> Option<Player> {
    let info = session.GetPlaybackInfo().ok()?;
    let status = info.PlaybackStatus().ok()?;
    if status != Status::Playing && status != Status::Paused {
        return None;
    }
    let id = session.SourceAppUserModelId().ok()?.to_string_lossy();
    let media = session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| op.join())
        .ok();
    let title = media.as_ref().and_then(|m| m.Title().ok());
    let artist = media.as_ref().and_then(|m| m.Artist().ok());
    let controls = info.Controls().ok();
    let can = |ask: fn(&_) -> windows::core::Result<bool>| {
        controls.as_ref().is_some_and(|c| ask(c).unwrap_or(false))
    };
    let shuffle = can(|c| c.IsShuffleEnabled())
        .then(|| info.IsShuffleActive().and_then(|v| v.Value()).ok())
        .flatten();
    let repeat = can(|c| c.IsRepeatEnabled())
        .then(|| info.AutoRepeatMode().and_then(|v| v.Value()).ok())
        .flatten()
        .map(|mode| match mode {
            AutoRepeat::Track => Repeat::Track,
            AutoRepeat::List => Repeat::All,
            _ => Repeat::Off,
        });
    Some(Player {
        name: app_name(&id),
        title: title.map(|s| s.to_string_lossy()).unwrap_or_default(),
        artist: artist.map(|s| s.to_string_lossy()).unwrap_or_default(),
        playing: status == Status::Playing,
        can_next: can(|c| c.IsNextEnabled()),
        can_previous: can(|c| c.IsPreviousEnabled()),
        shuffle,
        repeat,
        id,
    })
}

/// A player's name from its app id: the Start menu's name for it, or the id's most telling
/// part ("Spotify.exe", "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" → "Spotify").
fn app_name(id: &str) -> String {
    if let Some(app) = installed_apps()
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(id))
    {
        return app.name;
    }
    let tail = id.rsplit(['\\', '!']).next().unwrap_or(id);
    let tail = tail
        .strip_suffix(".exe")
        .or_else(|| tail.strip_suffix(".EXE"))
        .unwrap_or(tail);
    let word = if tail.eq_ignore_ascii_case("app") {
        id.split(['_', '!']).next().unwrap_or(id)
    } else {
        tail
    };
    word.rsplit('.').next().unwrap_or(word).to_owned()
}

/// The default speakers' volume.
fn endpoint() -> Option<IAudioEndpointVolume> {
    default_device(eRender)
}

/// The default speakers' (`eRender`) or microphone's (`eCapture`) volume.
fn default_device(flow: EDataFlow) -> Option<IAudioEndpointVolume> {
    com();
    // SAFETY: COM calls on interfaces COM handed out, on a thread joined to COM.
    unsafe {
        let devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = devices.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
        device.Activate(CLSCTX_ALL, None).ok()
    }
}

pub fn volume() -> Option<Volume> {
    let endpoint = endpoint()?;
    // SAFETY: as in `endpoint`.
    unsafe {
        Some(Volume {
            level: endpoint.GetMasterVolumeLevelScalar().ok()?,
            muted: endpoint.GetMute().ok()?.as_bool(),
        })
    }
}

/// The last brightness read, and when.
static BRIGHTNESS: Mutex<Option<(Instant, Option<f32>)>> = Mutex::new(None);
static READING_BRIGHTNESS: AtomicBool = AtomicBool::new(false);

/// An Explorer window is in front with files selected. (Explorer is asked only when it is the
/// window in front.)
pub fn selecting() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    // SAFETY: asks for the window in front and writes its class name into the buffer given.
    let (front, class) = unsafe {
        let front = GetForegroundWindow();
        let mut class = [0u16; 64];
        let len = GetClassNameW(front, class.as_mut_ptr(), class.len() as i32);
        (
            front,
            String::from_utf16_lossy(&class[..len.max(0) as usize]),
        )
    };
    if class != "CabinetWClass" {
        return false;
    }
    powershell_within(
        &format!(
            "(New-Object -ComObject Shell.Application).Windows() | Where-Object {{ $_.HWND -eq {} }} \
             | ForEach-Object {{ $_.Document.SelectedItems().Count }}",
            front as isize
        ),
        Duration::from_millis(1500),
    )
    .and_then(|count| count.lines().next()?.trim().parse::<u32>().ok())
    .is_some_and(|count| count > 0)
}

/// The built-in screen's brightness. WMI is slow to ask (PowerShell starts for it), so after
/// the first time this answers with the last reading and reads again behind the scenes.
pub fn brightness() -> Option<f32> {
    let last = *BRIGHTNESS.lock().unwrap_or_else(|e| e.into_inner());
    match last {
        None => {
            let level = read_brightness();
            *BRIGHTNESS.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), level));
            level
        }
        Some((at, level)) => {
            if at.elapsed() > BRIGHTNESS_FRESH && !READING_BRIGHTNESS.swap(true, Ordering::AcqRel) {
                std::thread::spawn(|| {
                    let level = read_brightness();
                    *BRIGHTNESS.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((Instant::now(), level));
                    READING_BRIGHTNESS.store(false, Ordering::Release);
                });
            }
            level
        }
    }
}

fn read_brightness() -> Option<f32> {
    let said = powershell(
        "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction Stop \
         | Select-Object -First 1).CurrentBrightness",
    )?;
    Some(said.lines().next()?.trim().parse::<f32>().ok()? / 100.0)
}

/// The keyboard layouts the user added, and which one the window in front types with.
pub fn layouts() -> (Vec<Layout>, Option<usize>) {
    // SAFETY: the list is written into a buffer of the length given.
    let list = unsafe {
        let mut list: Vec<HKL> = vec![std::ptr::null_mut(); 32];
        let n = GetKeyboardLayoutList(list.len() as i32, list.as_mut_ptr());
        list.truncate(n.max(0) as usize);
        list
    };
    // SAFETY: plain queries about the window in front.
    let current = unsafe {
        GetKeyboardLayout(GetWindowThreadProcessId(
            GetForegroundWindow(),
            std::ptr::null_mut(),
        ))
    };
    let index = list.iter().position(|&hkl| hkl == current);
    (list.into_iter().map(layout).collect(), index)
}

fn layout(hkl: HKL) -> Layout {
    // The low word is the language.
    let language = (hkl as usize & 0xffff) as u32;
    let mut locale = [0u16; 85];
    // SAFETY: written into a buffer of the length given, NUL included in what it returns.
    let len = unsafe { LCIDToLocaleName(language, locale.as_mut_ptr(), locale.len() as i32, 0) };
    let code = String::from_utf16_lossy(&locale[..(len.max(1) - 1) as usize]);
    let mut display = [0u16; 128];
    // SAFETY: `locale` is NUL-terminated; written into a buffer of the length given.
    let len = unsafe {
        GetLocaleInfoEx(
            locale.as_ptr(),
            LOCALE_SLOCALIZEDDISPLAYNAME,
            display.as_mut_ptr(),
            display.len() as i32,
        )
    };
    let name = String::from_utf16_lossy(&display[..(len.max(1) - 1) as usize]);
    let short = code.split('-').next().unwrap_or_default().to_uppercase();
    Layout {
        id: format!("{:x}", hkl as usize),
        short: if short.is_empty() { "?".into() } else { short },
        name: if name.is_empty() { code } else { name },
    }
}

/// Asks the window in front to type with another layout, as the language switcher does.
fn select_layout(id: &str) -> Result<(), String> {
    let hkl = usize::from_str_radix(id, 16).map_err(|e| e.to_string())?;
    // SAFETY: posts a message to the window in front.
    let posted = unsafe {
        PostMessageW(
            GetForegroundWindow(),
            WM_INPUTLANGCHANGEREQUEST,
            0,
            hkl as isize,
        )
    };
    if posted != 0 {
        Ok(())
    } else {
        Err("couldn't switch the layout".into())
    }
}

pub fn run(command: &Command) -> Result<(), String> {
    let answer = |done: windows::core::Result<bool>| match done {
        Ok(true) => Ok(()),
        Ok(false) => Err("the player didn't do it".to_owned()),
        Err(e) => Err(e.message()),
    };
    let player = |id: &str| session(id).ok_or_else(|| "the player is gone".to_owned());
    match command {
        Command::Media { player: id, key: MediaKey::Raise } => raise(id),
        Command::Media { player: id, key } => {
            let session = player(id)?;
            answer(match key {
                MediaKey::PlayPause => session.TryTogglePlayPauseAsync().and_then(|op| op.join()),
                MediaKey::Next => session.TrySkipNextAsync().and_then(|op| op.join()),
                _ => session.TrySkipPreviousAsync().and_then(|op| op.join()),
            })
        }
        Command::Shuffle { player: id, on } => {
            answer(player(id)?.TryChangeShuffleActiveAsync(*on).and_then(|op| op.join()))
        }
        Command::Repeat { player: id, to } => {
            let mode = match to {
                Repeat::Off => AutoRepeat::None,
                Repeat::Track => AutoRepeat::Track,
                Repeat::All => AutoRepeat::List,
            };
            answer(player(id)?.TryChangeAutoRepeatModeAsync(mode).and_then(|op| op.join()))
        }
        Command::Layout(id) => select_layout(id),
        Command::Volume(level) => {
            let endpoint = endpoint().ok_or("no speakers")?;
            // SAFETY: as in `endpoint`.
            unsafe {
                endpoint
                    .SetMute(false, std::ptr::null())
                    .and_then(|()| endpoint.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null()))
                    .map_err(|e| e.message())
            }
        }
        Command::Mute(on) => {
            let endpoint = endpoint().ok_or("no speakers")?;
            // SAFETY: as in `endpoint`.
            unsafe { endpoint.SetMute(*on, std::ptr::null()).map_err(|e| e.message()) }
        }
        Command::Brightness(level) => powershell(&format!(
            "Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop \
             | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=1; Brightness={}}} \
             | Out-Null",
            (level * 100.0).round()
        ))
        .map(|_| {
            *BRIGHTNESS.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), Some(*level)));
        })
        .ok_or_else(|| "couldn't set the brightness".to_owned()),
        Command::Launch(app) => launch(&app.id),
        Command::Activate(id) => crate::switcher::activate(id),
        // Snipping Tool's own keys: its bar to pick an area, the screen saved to Pictures, its
        // recorder.
        Command::Screenshot(Shot::Screen) => keys(&[VK_LWIN, VK_SNAPSHOT]),
        Command::Screenshot(_) => keys(&[VK_LWIN, VK_SHIFT, u16::from(b'S')]),
        Command::Record(_) => keys(&[VK_LWIN, VK_SHIFT, u16::from(b'R')]),
        Command::Language(id) => set_language(id),
        Command::MuteMic(on) => {
            let mic = default_device(eCapture).ok_or("no microphone")?;
            // SAFETY: as in `default_device`.
            unsafe { mic.SetMute(*on, std::ptr::null()).map_err(|e| e.message()) }
        }
        Command::DarkMode(on) => dark_mode(*on),
        Command::StayAwake(on) => {
            stay_awake(*on);
            Ok(())
        }
        // SAFETY: plain calls without arguments that matter.
        Command::Lock => match unsafe { LockWorkStation() } {
            0 => Err("couldn't lock the screen".into()),
            _ => Ok(()),
        },
        Command::Sleep => {
            // SAFETY: as above.
            if unsafe { SetSuspendState(false, false, false) } {
                Ok(())
            } else {
                Err("couldn't put the computer to sleep".into())
            }
        }
        Command::EmptyTrash => {
            // SAFETY: every drive's bin, no window to own a dialog, and no dialog. An empty bin
            // answers with an error, which isn't one here.
            unsafe {
                SHEmptyRecycleBinW(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI,
                );
            }
            Ok(())
        }
        Command::ShowDesktop => keys(&[VK_LWIN, u16::from(b'D')]),
        Command::Monitor => shell_open("taskmgr.exe"),
        Command::Settings => unreachable!("the settings are opened by `desk::run`"),
    }
}

/// Presses the keys in order and lets them go in reverse, as one would, once the trigger key
/// has come up (with it still down, they would be another shortcut).
fn keys(vks: &[u16]) -> Result<(), String> {
    if !crate::switcher::modifiers_up() {
        return Err("a key is still held down".into());
    }
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs: Vec<INPUT> = vks
        .iter()
        .map(|&vk| key(vk, false))
        .chain(vks.iter().rev().map(|&vk| key(vk, true)))
        .collect();
    // SAFETY: the inputs, their count and the size of one.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("couldn't press the keys".into())
    }
}

/// Windows adds keyboard languages in its own settings.
pub const ADDS_LAYOUTS: bool = false;

/// Snipping Tool takes an area (its bar also offers a window) or the screen; Windows 11's
/// records an area.
pub fn captures() -> (Vec<Shot>, Vec<Shot>) {
    let build: u32 = registry_text(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        "CurrentBuildNumber",
    )
    .and_then(|b| b.parse().ok())
    .unwrap_or(0);
    let records = if build >= 22000 {
        vec![Shot::Region]
    } else {
        Vec::new()
    };
    (vec![Shot::Region, Shot::Screen], records)
}

/// A text value from the registry.
fn registry_text(root: HKEY, key: &str, name: &str) -> Option<String> {
    let (key, name) = (wide(key), wide(name));
    let mut buffer = [0u16; 256];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: reads a string into the buffer, of the size given in bytes.
    let found = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    } == 0;
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    found.then(|| String::from_utf16_lossy(&buffer[..len]))
}

/// The display languages installed, the one Windows speaks marked.
pub fn languages() -> (Vec<Language>, Option<usize>) {
    let installed = wide(r"SYSTEM\CurrentControlSet\Control\MUI\UILanguages");
    let mut ids = Vec::new();
    // SAFETY: the key is opened to read, its subkeys' names are written into a buffer of the
    // length given, and it is closed.
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            installed.as_ptr(),
            0,
            KEY_READ,
            &mut key,
        ) == 0
        {
            for i in 0.. {
                let mut name = [0u16; 86];
                let mut len = name.len() as u32;
                let status = RegEnumKeyExW(
                    key,
                    i,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                if status != 0 {
                    break;
                }
                ids.push(String::from_utf16_lossy(&name[..len as usize]));
            }
            RegCloseKey(key);
        }
    }
    let mut locale = [0u16; 85];
    // SAFETY: written into a buffer of the length given, NUL included in what it returns.
    let len = unsafe {
        LCIDToLocaleName(
            u32::from(GetUserDefaultUILanguage()),
            locale.as_mut_ptr(),
            locale.len() as i32,
            0,
        )
    };
    let current = String::from_utf16_lossy(&locale[..(len.max(1) - 1) as usize]);
    if !current.is_empty() && !ids.iter().any(|id| id.eq_ignore_ascii_case(&current)) {
        ids.insert(0, current.clone());
    }
    let languages: Vec<Language> = ids
        .iter()
        .map(|id| super::languages::language(id))
        .collect();
    let index = languages
        .iter()
        .position(|l| l.id.eq_ignore_ascii_case(&current));
    (languages, index)
}

/// Has Windows speak `id` from the next sign-in.
fn set_language(id: &str) -> Result<(), String> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("no language {id}"));
    }
    powershell(&format!("Set-WinUILanguageOverride -Language '{id}'"))
        .map(drop)
        .ok_or_else(|| "couldn't change the language".to_owned())
}

/// Whether the default microphone is muted.
pub fn mic_muted() -> Option<bool> {
    let mic = default_device(eCapture)?;
    // SAFETY: as in `default_device`.
    unsafe { mic.GetMute().ok().map(|muted| muted.as_bool()) }
}

/// Whether apps are dark.
pub fn dark() -> Option<bool> {
    Some(crate::settings::system_dark_now())
}

/// Turns apps and the taskbar dark or light, and tells the windows open.
fn dark_mode(on: bool) -> Result<(), String> {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let light: u32 = (!on).into();
    for name in ["AppsUseLightTheme", "SystemUsesLightTheme"] {
        let name = wide(name);
        // SAFETY: writes a DWORD of the size given.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_DWORD,
                (&light as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if status != 0 {
            return Err("couldn't switch dark mode".into());
        }
    }
    std::thread::spawn(|| {
        let what = wide("ImmersiveColorSet");
        let mut result = 0;
        // SAFETY: a NUL-terminated string for every top window; one that hangs is skipped.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                what.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                500,
                &mut result,
            );
        }
    });
    Ok(())
}

/// The thread keeping the computer awake; dropping it lets the computer sleep again.
static AWAKE: Mutex<Option<std::sync::mpsc::Sender<()>>> = Mutex::new(None);

pub fn awake() -> Option<bool> {
    Some(AWAKE.lock().unwrap_or_else(|e| e.into_inner()).is_some())
}

/// Windows keeps the screen on while a thread asks it to, until that thread stops asking.
fn stay_awake(on: bool) {
    let mut held = AWAKE.lock().unwrap_or_else(|e| e.into_inner());
    *held = None;
    if on {
        let (tell, told) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            // SAFETY: flags only.
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)
            };
            let _ = told.recv();
            // SAFETY: as above.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        });
        *held = Some(tell);
    }
}

/// Brings a player forward by starting it again, which an app already running answers by
/// showing itself.
fn raise(id: &str) -> Result<(), String> {
    let apps = installed_apps();
    let name = app_name(id);
    match apps
        .iter()
        .find(|a| a.id.eq_ignore_ascii_case(id))
        .or_else(|| super::find_app(&apps, &name))
    {
        Some(app) => launch(&app.id),
        None => launch(id),
    }
}

/// Opens a file, a link or `shell:` place the way Explorer would, on a thread of its own that
/// is set up for the shell's extensions.
fn shell_open(target: &str) -> Result<(), String> {
    let target = target.to_owned();
    std::thread::spawn(move || {
        // SAFETY: joins this new thread to COM the way the shell wants, then asks the shell to
        // open a NUL-terminated target.
        let result = unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(
                std::ptr::null(),
                COINIT_APARTMENTTHREADED as u32,
            );
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide(&target).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as usize > 32 {
            Ok(())
        } else {
            Err(format!("couldn't open {target}"))
        }
    })
    .join()
    .unwrap_or_else(|_| Err("the shell failed".into()))
}

/// Starts an app the Start menu lists: by its app id, or a shortcut's path.
fn launch(id: &str) -> Result<(), String> {
    if Path::new(id).is_absolute() || id.ends_with(':') || id.contains("://") {
        shell_open(id)
    } else {
        shell_open(&format!("shell:AppsFolder\\{id}"))
    }
}

/// The Start menu's apps, Store ones included, by name.
pub fn installed_apps() -> Vec<App> {
    static CACHE: Mutex<Option<(Instant, Vec<App>)>> = Mutex::new(None);
    static LISTING: AtomicBool = AtomicBool::new(false);
    let cached = CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match cached {
        // Listing takes PowerShell a second or two: after the first time, the last list is
        // used and a new one made behind the scenes.
        Some((at, apps)) => {
            if at.elapsed() > APPS_FRESH && !LISTING.swap(true, Ordering::AcqRel) {
                std::thread::spawn(|| {
                    let apps = list_apps();
                    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), apps));
                    LISTING.store(false, Ordering::Release);
                });
            }
            apps
        }
        None => {
            let apps = list_apps();
            *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), apps.clone()));
            apps
        }
    }
}

fn list_apps() -> Vec<App> {
    let listed = powershell(
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; \
         Get-StartApps | ForEach-Object { $_.Name + [char]9 + $_.AppID }",
    );
    let mut apps: Vec<App> = listed
        .iter()
        .flat_map(|said| said.lines())
        .filter_map(|line| {
            let (name, id) = line.split_once('\t')?;
            Some(App {
                id: id.trim().to_owned(),
                name: name.trim().to_owned(),
            })
        })
        .collect();
    if apps.is_empty() {
        apps = start_menu_shortcuts();
    }
    apps.retain(|a| !a.name.is_empty() && !a.name.to_lowercase().contains("uninstall"));
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

/// The Start menu's shortcuts, for when PowerShell can't list its apps.
fn start_menu_shortcuts() -> Vec<App> {
    fn walk(dir: &Path, out: &mut Vec<App>) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
                && let Some(name) = path.file_stem()
            {
                out.push(App {
                    id: path.to_string_lossy().into_owned(),
                    name: name.to_string_lossy().into_owned(),
                });
            }
        }
    }
    let mut apps = Vec::new();
    for root in ["APPDATA", "ProgramData"] {
        if let Some(base) = std::env::var_os(root) {
            walk(
                &PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"),
                &mut apps,
            );
        }
    }
    apps
}

pub fn app(id: &str) -> Option<App> {
    installed_apps().into_iter().find(|a| a.id == id)
}

/// Whether a kind of link (`whatsapp:`) has an app to open it.
fn handles(scheme: &str) -> bool {
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: opens a key by a NUL-terminated name, and closes it if it was there.
    unsafe {
        let found = RegOpenKeyExW(
            HKEY_CLASSES_ROOT,
            wide(scheme).as_ptr(),
            0,
            KEY_READ,
            &mut key,
        ) == 0;
        if found {
            RegCloseKey(key);
        }
        found
    }
}

fn telegram_exe() -> Option<PathBuf> {
    let exe = PathBuf::from(std::env::var_os("APPDATA")?).join(r"Telegram Desktop\Telegram.exe");
    exe.is_file().then_some(exe)
}

/// The messaging app, found where its installer puts it or by the links it opens; looking
/// takes no PowerShell, as the Actions wheel asks while it opens.
pub fn messenger(which: Messenger) -> Option<App> {
    let (id, scheme) = match which {
        Messenger::Telegram => {
            if let Some(exe) = telegram_exe() {
                return Some(App {
                    id: exe.to_string_lossy().into_owned(),
                    name: which.name().into(),
                });
            }
            ("tg:", "tg")
        }
        Messenger::WhatsApp => ("whatsapp:", "whatsapp"),
        Messenger::Teams => ("msteams:", "msteams"),
        Messenger::Slack => ("slack://open", "slack"),
    };
    handles(scheme).then(|| App {
        id: id.into(),
        name: which.name().into(),
    })
}

/// Telegram's own program takes `-sendpath`, also while it runs (it hands the files over).
pub fn telegram_send(app: &App, files: &[PathBuf]) -> Result<bool, String> {
    if !app.id.to_lowercase().ends_with(".exe") {
        return Ok(false);
    }
    let mut child = Process::new(&app.id)
        .arg("-sendpath")
        .args(files)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || child.wait());
    Ok(true)
}

pub fn open_messenger(_which: Messenger, app: &App) -> Result<(), String> {
    launch(&app.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_players_name_comes_from_its_app_id() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(
            app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"),
            "Spotify"
        );
        assert_eq!(
            app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!App"),
            "ZuneMusic"
        );
    }
}
