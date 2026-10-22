//! KDE Plasma (Wayland) backend.
//!
//! Plasma lets any client watch the modifier keys (`org_kde_kwin_keystate`) and put a surface
//! above everything (wlr layer shell). When the trigger key (Shift unless the settings say
//! otherwise) goes down we map an invisible
//! fullscreen overlay. If a file drag is in progress it now passes over our surface and the
//! compositor tells us where; otherwise the pointer enters the overlay and we take it away
//! again within a frame.
//!
//! Tapping the trigger key alone (see `tap`) opens the wheel without a drag, for the files selected in the
//! active Dolphin window; it stays open until a click picks something or dismisses it.
//!
//! Holding the trigger key a moment with no drag opens the desk (see `desk`) around the
//! pointer: clicks do what its wedges do while the key is held, and letting go does what it
//! aims at. Plasma doesn't let anyone see the other keys and the clicks, so a hold that was
//! meant for a Shift-click or Shift-arrow opens it too; the settings can turn it off.

use std::ffi::c_void;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use egui::{Pos2, Rect, ViewportId, pos2, vec2};
use glutin::api::egl::{
    config::Config, context::PossiblyCurrentContext, display::Display, surface::Surface,
};
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextAttributesBuilder, PossiblyCurrentGlContext};
use glutin::display::GlDisplay;
use glutin::surface::{GlSurface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    data_device_manager::{
        DataDeviceManagerState,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
    },
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            EventLoop,
            channel::{self, Sender},
        },
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_output, wl_pointer,
        wl_seat, wl_shm, wl_surface,
    },
};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;
use wayland_protocols_plasma::keystate::client::org_kde_kwin_keystate::{self, OrgKdeKwinKeystate};

use crate::catalog::Mode;
use crate::clipboard::Clipboard;
use crate::desk::{self, Menu};
use crate::overlay::{CardSpot, Overlay};
use crate::service::Command;
use crate::settings::{Settings, Trigger};

const STATE_PRESSED: u32 = 3;
const URI_LIST: &str = "text/uri-list";
/// A probe that hears nothing for this long is probably sitting under a drag (see `nudge`).
const NUDGE_AFTER: Duration = Duration::from_millis(15);
/// Nudges per probe, `NUDGE_AFTER` apart, in case KWin mapped the overlay late.
const NUDGES: u8 = 3;
/// Its own ID, so it doesn't hide the menu entry a package installs as `mcgyver.desktop`.
pub(crate) const DESKTOP_FILE: &str = "mcgyver-kwin.desktop";
/// Where `install` puts the app icon, under the data folder, for its entry to name.
const ICON_FILE: &str = "icons/hicolor/scalable/apps/mcgyver.svg";
/// What `install` wrote before the app was called McGyver.
const OLD_DESKTOP_FILES: [&str; 2] = [
    "zero-click-converter.desktop",
    "zero-click-converter-kwin.desktop",
];
/// How long KWin gets to notice the KWin entry (see `wait_for_kwin`).
const KWIN_CATCH_UP: Duration = Duration::from_secs(10);
/// How long to wait for the drag source to hand over its file list.
const READ_TIMEOUT: Duration = Duration::from_millis(300);
/// How long Dolphin gets to copy its selection after a tap.
const COPY_TIMEOUT: Duration = Duration::from_secs(1);
/// A selection that took longer than this to arrive no longer answers the tap.
const STALE_TAP: Duration = Duration::from_secs(2);
/// KGlobalAccel reports the key release that ends a drag as a tap too, about when keystate
/// reports the release; taps this soon after one are ignored.
const TAP_AFTER_DRAG: Duration = Duration::from_millis(300);

const BTN_LEFT: u32 = 0x110;
/// Height of the bands that approximate round glass in the blur region.
const BLUR_BAND: i32 = 2;
/// Samples per pixel for anti-aliasing.
const MSAA: u8 = 4;
/// The overlay draws at least this many pixels per logical pixel. On a 1× screen the
/// compositor averages each 2×2 block down to one, which supersamples everything, text too.
const MIN_RENDER_SCALE: i32 = 2;

/// The trigger key as `org_kde_kwin_keystate` numbers it.
fn keystate_key(trigger: Trigger) -> u32 {
    match trigger {
        Trigger::Alt => 3,
        Trigger::Ctrl => 4,
        Trigger::Shift => 5,
        // Not offered on Linux (see `Trigger`).
        Trigger::Cmd => 6,
    }
}

pub fn run() -> Result<(), String> {
    crate::settings::migrate();
    let settings = Settings::load();
    crate::i18n::set(settings.language);
    crate::glass::set_dark(settings.appearance.dark());
    let (command_tx, command_rx) = channel::channel::<Command>();
    // One app at a time: launching it again opens the settings of the one running.
    let granted = Arc::new(crate::service::Granted::new());
    let service = match crate::service::serve(command_tx.clone(), granted.clone()) {
        Ok(conn) => Some(conn),
        Err(crate::service::Error::Running) => {
            crate::service::call("ShowSettings");
            println!("McGyver is already running; opened its settings.");
            return Ok(());
        }
        Err(crate::service::Error::Bus(e)) => {
            eprintln!("McGyver: the session bus is unavailable ({e}); running without it");
            None
        }
    };
    // KWin looks the rights up when we connect, so a stale entry is fixed before.
    heal_install();
    let conn = Connection::connect_to_env().map_err(|e| format!("no Wayland session: {e}"))?;
    let (globals, queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();

    let keystate: OrgKdeKwinKeystate = globals.bind(&qh, 5..=5, ()).map_err(|_| {
        "org_kde_kwin_keystate v5 is missing; this needs KDE Plasma 6 on Wayland".to_owned()
    })?;
    keystate.fetchStates();

    // Optional: without it the glass just isn't blurred.
    let effects: Option<ExtBackgroundEffectManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    // Restricted: KWin offers it only to programs whose desktop file asks for it (`install`).
    let fake_input: Option<OrgKdeKwinFakeInput> = globals.bind(&qh, 4..=5, ()).ok();
    match &fake_input {
        Some(fake) => fake.authenticate(
            "McGyver".into(),
            "Opens the wheel under a drag without waiting for the mouse to move".into(),
        ),
        None => eprintln!(
            "McGyver: tip: run once with --install so the wheel opens without waiting for \
             the mouse to move"
        ),
    }
    if let Some(missing) = crate::images::missing_codecs() {
        eprintln!("McGyver: note: {missing}");
    }

    let shm = Shm::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let (picked_tx, picked_rx) = channel::channel::<(Vec<PathBuf>, Instant)>();
    let (desk_tx, desk_rx) = channel::channel::<DeskRead>();
    let (tap_tx, tap_rx) = channel::channel::<()>();
    granted
        .fast_open
        .store(fake_input.is_some(), Ordering::Relaxed);
    let tapping = match crate::tap::listen(tap_tx, settings.trigger, installed()) {
        Ok(true) => true,
        Ok(false) => {
            eprintln!(
                "McGyver: tip: run once with --install to open the wheel by tapping {}, with \
                 files selected in Dolphin",
                settings.trigger.label()
            );
            false
        }
        Err(e) => {
            eprintln!(
                "McGyver: tapping {} won't work: {e}",
                settings.trigger.label()
            );
            false
        }
    };
    let tray = crate::tray::spawn(command_tx, settings.trigger);
    let shown = tray.clone();
    crate::update::start(move || {
        if let Some(tray) = &shown {
            tray.update(|_| {});
        }
    });
    let mut app = App {
        trigger: settings.trigger,
        hold: settings.hold,
        hold_time: settings.hold_time(),
        appearance: settings.appearance,
        quick_apps: settings.quick_apps.clone(),
        paused: false,
        quit: false,
        exit: None,
        tapping,
        service,
        tray,
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).map_err(|e| e.to_string())?,
        layer_shell: LayerShell::bind(&globals, &qh)
            .map_err(|_| "zwlr_layer_shell_v1 is missing".to_owned())?,
        pool: SlotPool::new(1024 * 1024, &shm).map_err(|e| e.to_string())?,
        shm,
        data_devices: DataDeviceManagerState::bind(&globals, &qh).map_err(|e| e.to_string())?,
        data_device: None,
        clipboard: None,
        pointer: None,
        cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
        cursor: None,
        pointer_serial: 0,
        on_card: None,
        effects,
        can_blur: false,
        fake_input,
        nudge_at: None,
        nudges: 0,
        gl: Gl::new(&conn)?,
        egui: {
            let ctx = egui::Context::default();
            crate::glass::install_fonts(&ctx);
            ctx
        },
        overlay: Overlay::default(),
        clicker: crate::sound::Clicker::new(),
        layer: None,
        pressed: [false; 8],
        keystate_seen: false,
        probing: false,
        accepting: false,
        click_files: None,
        sticky: false,
        held_at: None,
        hold_at: None,
        desk_menu: None,
        desk: false,
        holds: 0,
        runner: {
            let read = desk_tx.clone();
            desk::Runner::new(move |number, menu| {
                let _ = read.send(DeskRead::Again(number, menu));
            })
        },
        desk_from: 0,
        live_at: None,
        live_read: None,
        welcome: None,
        desk_read: desk_tx.clone(),
        key_saw_drag: false,
        taps_ignored_until: None,
        selecting: Arc::new(AtomicBool::new(false)),
        picked: picked_tx,
        repaint_at: None,
        conn: conn.clone(),
        qh: qh.clone(),
        start: Instant::now(),
        debug: std::env::var_os("MCGYVER_DEBUG").is_some(),
    };
    // SCTK only reports seats that appear later through `new_seat`.
    if let Some(seat) = app.seat_state.seats().next() {
        app.data_device = Some(app.data_devices.get_data_device(&qh, &seat));
        app.clipboard = Clipboard::bind(&globals, &qh, &seat);
    }
    // Copy and Cut on the Actions wheel put files on the clipboard as Dolphin does.
    if let Some(clipboard) = app.clipboard.clone() {
        let (conn, qh) = (app.conn.clone(), qh.clone());
        crate::actions::serve_files_with(crate::actions::FileClipboard {
            put: Box::new(move |files, cut| {
                let paths = files
                    .iter()
                    .map(|f| f.canonicalize())
                    .collect::<std::io::Result<Vec<_>>>()?;
                let uris: Vec<String> = paths.iter().map(|p| crate::actions::file_uri(p)).collect();
                let texts: Vec<String> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                clipboard.offer(&conn, &qh, crate::clipboard::file_types(&uris, &texts, cut));
                Ok(())
            }),
            can_cut: true,
        });
    }
    granted.tapping.store(tapping, Ordering::Relaxed);
    granted
        .selection
        .store(app.clipboard.is_some(), Ordering::Relaxed);
    granted.ready.store(true, Ordering::Release);
    if tapping && app.clipboard.is_none() {
        eprintln!(
            "McGyver: tapping {} needs ext-data-control, which this compositor lacks",
            app.trigger.label()
        );
    }

    // The first time: the wheel shows itself, and how to open it.
    if !settings.welcomed {
        let apps = settings.quick_apps.clone();
        std::thread::spawn(move || {
            let _ = desk_tx.send(DeskRead::Welcome(Menu::read(&apps)));
        });
    }

    let mut event_loop: EventLoop<App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    WaylandSource::new(conn, queue)
        .insert(event_loop.handle())
        .map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    handle
        .insert_source(tap_rx, |event, _, app| {
            if let channel::Event::Msg(()) = event {
                app.key_tapped();
            }
        })
        .map_err(|e| format!("event loop: {e}"))?;
    handle
        .insert_source(command_rx, |event, _, app| {
            if let channel::Event::Msg(command) = event {
                app.command(command);
            }
        })
        .map_err(|e| format!("event loop: {e}"))?;
    handle
        .insert_source(picked_rx, |event, _, app| {
            if let channel::Event::Msg((files, tapped_at)) = event {
                app.open_for_click(files, tapped_at);
            }
        })
        .map_err(|e| format!("event loop: {e}"))?;
    handle
        .insert_source(desk_rx, |event, _, app| {
            if let channel::Event::Msg(read) = event {
                app.desk_read(read);
            }
        })
        .map_err(|e| format!("event loop: {e}"))?;
    let key = app.trigger.label();
    eprintln!(
        "McGyver: ready. Drag files and hold {key}, or select files in Dolphin and tap {key}."
    );
    while !app.quit {
        let timeout = app.timeout();
        event_loop
            .dispatch(timeout, &mut app)
            .map_err(|e| e.to_string())?;
        app.tick();
    }
    app.hide();
    if let Some(tray) = app.tray.take() {
        tray.shutdown().wait();
    }
    Ok(())
}

/// Asks KWin to offer this binary the fake-input protocol, used to nudge the pointer (see
/// `App::nudge`). KWin grants restricted protocols to executables whose desktop file requests
/// them, so this writes one, then refreshes KDE's service cache.
pub fn install() -> Result<(), String> {
    let path = write_kwin_entry()?;
    say(&format!(
        "Wrote {}. Restart mcgyver for KWin to pick it up.",
        path.display()
    ));
    // So a restart right after gets the rights.
    wait_for_kwin(true);
    crate::tap::install(Settings::load().trigger)
}

/// Prints for whoever ran `--install` or `--uninstall`; the settings window has no terminal,
/// or one that may be gone.
pub(crate) fn say(line: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// Whether `install` ran (and `uninstall` didn't since).
pub fn installed() -> bool {
    std::iter::once(DESKTOP_FILE)
        .chain(OLD_DESKTOP_FILES)
        .any(|name| desktop_file(name).is_ok_and(|p| p.exists()))
}

pub fn uninstall() -> Result<(), String> {
    let mut removed = false;
    for name in std::iter::once(DESKTOP_FILE).chain(OLD_DESKTOP_FILES) {
        let path = desktop_file(name)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                removed = true;
                say(&format!("Removed {}.", path.display()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    if let Ok(icon) = data_file(ICON_FILE)
        && std::fs::remove_file(&icon).is_ok()
    {
        say(&format!("Removed {}.", icon.display()));
    }
    if removed {
        refresh_service_cache();
        wait_for_kwin(false);
    }
    crate::tap::uninstall()
}

/// What `install` writes: KWin matches the first word of Exec against the running binary. The
/// icon is named by its path, which KWin finds without its icon theme catching up.
fn kwin_entry() -> Result<String, String> {
    let exe = crate::settings::own_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("can't find this program: {e}"))?;
    // A snap runs from /snap/<name>/<revision>, which changes with every refresh; KWin
    // resolves links, so the snap's `current` link keeps matching.
    let exe = match (
        std::env::var_os("SNAP"),
        std::env::var_os("SNAP_INSTANCE_NAME"),
    ) {
        (Some(snap), Some(name)) => match exe.strip_prefix(&snap) {
            Ok(inside) => PathBuf::from("/snap")
                .join(name)
                .join("current")
                .join(inside),
            Err(_) => exe,
        },
        _ => exe,
    };
    // Quoting covers spaces; anything that would need escaping is rare enough to refuse.
    let exe = exe
        .to_str()
        .filter(|s| !s.contains(['"', '\\', '`', '$', '%']))
        .ok_or_else(|| format!("move the program to a plainer path than {}", exe.display()))?;
    // A path the entry can hold as it is (a backslash would start an escape); else no icon.
    let icon = data_file(ICON_FILE)
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .filter(|p| !p.contains('\\') && !p.contains(char::is_control))
        .map(|p| format!("Icon={p}\n"))
        .unwrap_or_default();
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=McGyver\nExec=\"{exe}\"\n{icon}\
         NoDisplay=true\nX-KDE-Wayland-Interfaces=org_kde_kwin_fake_input\n"
    ))
}

fn write_kwin_entry() -> Result<PathBuf, String> {
    let entry = kwin_entry()?;
    let path = desktop_file(DESKTOP_FILE)?;
    write_file(&path, entry.as_bytes())?;
    for old in OLD_DESKTOP_FILES {
        if let Ok(old) = desktop_file(old) {
            let _ = std::fs::remove_file(old);
        }
    }
    refresh_service_cache();
    write_icon();
    Ok(path)
}

/// Puts the app icon where the KWin entry names it, unless it is there already. Without it the
/// settings window only has KWin's own icon; fake input doesn't need it.
fn write_icon() {
    let Ok(path) = data_file(ICON_FILE) else {
        return;
    };
    if std::fs::read(&path).is_ok_and(|icon| icon == crate::icon::SVG) {
        return;
    }
    if let Err(e) = write_file(&path, crate::icon::SVG) {
        eprintln!("McGyver: no icon for the KWin entry: {e}");
    }
}

fn write_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

/// Rewrites what `install` wrote if this binary has moved since (a new build elsewhere, the
/// old ID), so KWin keeps granting fake input, and puts back the icon it names. KWin reads the
/// icon by its path, so a new one needs neither the service cache nor a wait.
fn heal_install() {
    if !installed() {
        return;
    }
    write_icon();
    let current = desktop_file(DESKTOP_FILE)
        .and_then(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()));
    if current.ok() != kwin_entry().ok() {
        match write_kwin_entry() {
            Ok(path) => {
                eprintln!("McGyver: updated {} for this binary", path.display());
                wait_for_kwin(true);
            }
            Err(e) => eprintln!("McGyver: couldn't update the KWin entry: {e}"),
        }
    }
}

/// KWin decides which restricted protocols a client gets when it connects, from its service
/// cache, which catches up with a new desktop file a few seconds after `kbuildsycoca6`. Until
/// then a restart would connect without them. This waits, up to `KWIN_CATCH_UP`, until a new
/// connection is offered fake input (`offered`) or no longer is; false if it never was.
pub fn wait_for_kwin(offered: bool) -> bool {
    let until = Instant::now() + KWIN_CATCH_UP;
    loop {
        match fake_input_offered() {
            Some(now) if now == offered => return true,
            None => return false,
            Some(_) if Instant::now() >= until => return false,
            Some(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

/// Whether KWin offers this program fake input on a new connection; `None` without Wayland.
fn fake_input_offered() -> Option<bool> {
    struct Probe;
    impl Dispatch<wayland_client::protocol::wl_registry::WlRegistry, GlobalListContents> for Probe {
        fn event(
            _: &mut Self,
            _: &wayland_client::protocol::wl_registry::WlRegistry,
            _: wayland_client::protocol::wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    let conn = Connection::connect_to_env().ok()?;
    let (globals, _queue) = registry_queue_init::<Probe>(&conn).ok()?;
    Some(globals.contents().with_list(|list| {
        list.iter()
            .any(|global| global.interface == "org_kde_kwin_fake_input")
    }))
}

fn desktop_file(name: &str) -> Result<PathBuf, String> {
    data_file(&format!("applications/{name}"))
}

/// `path` in the user's data folder.
fn data_file(path: &str) -> Result<PathBuf, String> {
    // Per the XDG spec, an empty or relative XDG_DATA_HOME is ignored.
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or("neither XDG_DATA_HOME nor HOME is set")?;
    Ok(data.join(path))
}

fn refresh_service_cache() {
    let ok = std::process::Command::new("kbuildsycoca6")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !ok {
        eprintln!("McGyver: kbuildsycoca6 failed; run it yourself so KWin sees the change");
    }
}

/// One EGL context and egui painter, reused for every overlay surface.
struct Gl {
    display: Display,
    config: Config,
    context: PossiblyCurrentContext,
    painter: egui_glow::Painter,
}

impl Gl {
    fn new(conn: &Connection) -> Result<Self, String> {
        let display_ptr = NonNull::new(conn.backend().display_ptr() as *mut c_void)
            .ok_or("null Wayland display")?;
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display_ptr));
        let display = unsafe { Display::new(raw) }.map_err(|e| format!("EGL: {e}"))?;
        // Wedges are plain triangle meshes, which egui doesn't anti-alias; multisampling gives
        // them smooth, crisp edges.
        let pick = |samples: u8| {
            let template = ConfigTemplateBuilder::new().with_alpha_size(8);
            let template = if samples > 1 {
                template.with_multisampling(samples)
            } else {
                template
            };
            unsafe { display.find_configs(template.build()) }
                .ok()?
                .filter(|c| c.alpha_size() >= 8 && c.num_samples() >= samples)
                .min_by_key(|c| c.num_samples())
        };
        let config = pick(MSAA)
            .or_else(|| pick(0))
            .ok_or("no EGL config with an alpha channel")?;
        let context = unsafe {
            display.create_context(&config, &ContextAttributesBuilder::new().build(None))
        }
        .map_err(|e| format!("EGL context: {e}"))?
        .make_current_surfaceless()
        .map_err(|e| format!("EGL surfaceless context: {e}"))?;
        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name))
        };
        // Dithering keeps the soft shadows and gradients free of banding.
        let painter = egui_glow::Painter::new(Arc::new(gl), "", None, true)
            .map_err(|e| format!("egui_glow: {e}"))?;
        Ok(Self {
            display,
            config,
            context,
            painter,
        })
    }

    fn create_surface(
        &self,
        surface: &wl_surface::WlSurface,
        width: u32,
        height: u32,
    ) -> Result<Surface<WindowSurface>, String> {
        let ptr = NonNull::new(surface.id().as_ptr() as *mut c_void).ok_or("null surface")?;
        let raw = RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr));
        let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw,
            NonZeroU32::new(width).ok_or("zero width")?,
            NonZeroU32::new(height).ok_or("zero height")?,
        );
        let gl_surface = unsafe { self.display.create_window_surface(&self.config, &attrs) }
            .map_err(|e| format!("EGL surface: {e}"))?;
        self.bind(&gl_surface)?;
        // Pace frames on the compositor: swap blocks until the previous frame was shown.
        let _ = gl_surface.set_swap_interval(&self.context, SwapInterval::Wait(NonZeroU32::MIN));
        Ok(gl_surface)
    }

    fn bind(&self, gl_surface: &Surface<WindowSurface>) -> Result<(), String> {
        use glow::HasContext as _;
        self.context
            .make_current(gl_surface)
            .map_err(|e| format!("EGL make current: {e}"))?;
        // The context was first made current without a surface, which leaves the default
        // framebuffer's draw and read buffers at GL_NONE; NVIDIA keeps them there, so every
        // draw would go nowhere.
        unsafe {
            self.painter.gl().draw_buffer(glow::BACK);
            self.painter.gl().read_buffer(glow::BACK);
        }
        Ok(())
    }
}

struct OverlaySurface {
    layer: LayerSurface,
    /// Logical size from the compositor; zero until the first configure.
    size: (u32, u32),
    scale: i32,
    /// Created once there is something to draw; a bare probe uses a blank shm buffer.
    gl_surface: Option<Surface<WindowSurface>>,
    /// The input region last sent: `None` for the whole surface, else the cards' outlines.
    input: Option<Vec<[i32; 4]>>,
    blur: Option<ExtBackgroundEffectSurfaceV1>,
    /// Blur region last sent, as bands.
    blur_bands: Vec<[i32; 4]>,
}

impl OverlaySurface {
    fn render_scale(&self) -> i32 {
        self.scale.max(MIN_RENDER_SCALE)
    }
}

/// With the look set to System, the glass follows the desk's Dark Mode once it has switched
/// the desktop's colours, fading across with them. Where KWin fades the screen across itself,
/// the glass has turned already (see `run_desk_commands`).
fn follow_desk_dark(appearance: crate::settings::Appearance) {
    if appearance == crate::settings::Appearance::System
        && let Some(dark) = desk::switched_dark()
        && dark != crate::glass::dark()
    {
        crate::glass::turn_dark(dark);
        crate::settings::system_dark_is(dark);
    }
}

/// What a thread read of the system for the desk: to open it, or after a command.
enum DeskRead {
    /// For the hold with this number.
    Open(u64, Menu),
    /// After the batch of commands with this number.
    Again(u64, Menu),
    /// For the welcome, the first time the app starts.
    Welcome(Menu),
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    data_devices: DataDeviceManagerState,
    data_device: Option<DataDevice>,
    clipboard: Option<Clipboard>,
    /// The key held for a drag and tapped for Dolphin's selection.
    trigger: Trigger,
    /// Holding the key alone this long opens the desk, with these quick apps.
    hold: bool,
    hold_time: Duration,
    quick_apps: Vec<String>,
    /// Light or dark glass, looked at each time a wheel opens.
    appearance: crate::settings::Appearance,
    /// From the tray: the trigger key does nothing until resumed.
    paused: bool,
    quit: bool,
    /// Asked to quit or restart; waits until nothing is converting (see `tick`).
    exit: Option<Command>,
    /// KGlobalAccel tells us about taps (`--install` ran).
    tapping: bool,
    /// Holds the app's bus name.
    service: Option<zbus::blocking::Connection>,
    tray: Option<ksni::blocking::Handle<crate::tray::Tray>>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shapes: Option<CursorShapeManager>,
    /// Sets the arrow over the overlay, which would otherwise keep whatever cursor it had.
    cursor: Option<WpCursorShapeDeviceV1>,
    /// The pointer's last enter on the overlay, which a cursor change has to quote.
    pointer_serial: u32,
    /// What of a card the pointer was last on, to log when that changes.
    on_card: Option<CardSpot>,
    effects: Option<ExtBackgroundEffectManagerV1>,
    /// The compositor said it can blur behind surfaces.
    can_blur: bool,
    fake_input: Option<OrgKdeKwinFakeInput>,
    nudge_at: Option<Instant>,
    nudges: u8,
    gl: Gl,
    egui: egui::Context,
    overlay: Overlay,
    clicker: crate::sound::Clicker,
    layer: Option<OverlaySurface>,
    pressed: [bool; 8],
    keystate_seen: bool,
    /// The overlay is mapped to catch a drag.
    probing: bool,
    /// Whether we currently accept the drag (only while it points at a wedge).
    accepting: bool,
    /// Files from a tap, waiting for the pointer to show where to open the wheel.
    click_files: Option<Vec<PathBuf>>,
    /// The wheel was opened by a tap: clicks pick, and letting go of keys doesn't close it.
    sticky: bool,
    /// When the trigger key went down, and when it will have been held long enough with no
    /// drag for the desk.
    held_at: Option<Instant>,
    hold_at: Option<Instant>,
    /// What the desk will show, waiting for the pointer to say where.
    desk_menu: Option<Menu>,
    /// The wheel open is the desk: clicks do things, and letting go of the key ends it.
    desk: bool,
    desk_read: Sender<DeskRead>,
    /// Holds so far (and other modifiers pressed during one): a reading for an earlier one is
    /// dropped.
    holds: u64,
    runner: desk::Runner,
    /// The first batch of commands for the desk open now: readings after earlier ones are for
    /// a desk gone since.
    desk_from: u64,
    /// While the desk shows what changes by itself (the computer's load, the switches): when
    /// to read it again, and the reading asked for and not back yet.
    live_at: Option<Instant>,
    live_read: Option<u64>,
    /// The welcome's desk, waiting for the overlay to know its size.
    welcome: Option<Menu>,
    /// This press of the trigger key found a drag (see `TAP_AFTER_DRAG`).
    key_saw_drag: bool,
    taps_ignored_until: Option<Instant>,
    /// A thread is asking Dolphin for its selection.
    selecting: Arc<AtomicBool>,
    /// Where the thread sends what it found, and when the key was tapped.
    picked: Sender<(Vec<PathBuf>, Instant)>,
    repaint_at: Option<Instant>,
    conn: Connection,
    qh: QueueHandle<App>,
    start: Instant,
    debug: bool,
}

impl App {
    fn log(&self, msg: impl std::fmt::Display) {
        if self.debug {
            eprintln!("[{:>7} ms] {msg}", self.start.elapsed().as_millis());
        }
    }

    fn timeout(&self) -> Option<Duration> {
        let now = Instant::now();
        [self.nudge_at, self.repaint_at, self.hold_at, self.live_at]
            .into_iter()
            .flatten()
            .min()
            .map(|at| at.saturating_duration_since(now))
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if self.nudge_at.is_some_and(|at| now >= at) {
            self.nudge_at = None;
            self.nudge();
        }
        if self.repaint_at.is_some_and(|at| now >= at) {
            self.render();
        }
        if self.hold_at.is_some_and(|at| now >= at) {
            self.hold_at = None;
            self.held_long();
        }
        self.read_live(now);
        if self.layer.is_some()
            && !self.probing
            && self.click_files.is_none()
            && self.desk_menu.is_none()
            && self.welcome.is_none()
            && self.overlay.is_idle()
        {
            self.hide();
        }
        // Ending the process would cut a conversion or a clipboard borrow off halfway.
        let settled = self.layer.is_none()
            && self.overlay.is_idle()
            && !self.selecting.load(Ordering::Acquire);
        if settled {
            match self.exit.take() {
                Some(Command::Restart) => self.restart(),
                Some(_) => self.quit = true,
                None => {}
            }
        }
    }

    /// What the tray, the settings window or another launch asked for.
    fn command(&mut self, command: Command) {
        self.log(format!("command: {command:?}"));
        match command {
            Command::ShowSettings => self.show_settings(),
            Command::Reload => self.reload(),
            Command::Restart | Command::Quit => {
                self.fold();
                // Quit wins over a restart.
                if self.exit != Some(Command::Quit) {
                    self.exit = Some(command);
                }
            }
            Command::TogglePause => {
                self.paused = !self.paused;
                if self.paused {
                    self.fold();
                }
                let paused = self.paused;
                if let Some(tray) = &self.tray {
                    tray.update(|tray| tray.paused = paused);
                }
            }
        }
    }

    /// Puts away whatever the trigger key started; finished cards stay.
    fn fold(&mut self) {
        self.click_files = None;
        self.hold_at = None;
        self.desk_menu = None;
        if self.probing {
            self.end_probe("folded");
        }
        if self.overlay.wants_input() {
            self.overlay.cancel(Instant::now());
            self.update_accept();
            self.repaint_now();
        }
    }

    /// Opens the settings window, unless one is open already. It is a process of its own (see
    /// `settings_window`), started through `sh` so it isn't our child: it outlives a restart
    /// and never waits to be reaped.
    fn show_settings(&mut self) {
        if self
            .service
            .as_ref()
            .is_some_and(crate::service::settings_open)
        {
            return;
        }
        let spawned = crate::settings::own_exe().and_then(|exe| {
            if exe == std::path::Path::new("/proc/self/exe") {
                // An upgrade deleted this binary; `sh` would resolve that link to itself.
                let mut child = std::process::Command::new(exe)
                    .arg("--settings")
                    .stdin(std::process::Stdio::null())
                    .spawn()?;
                std::thread::spawn(move || child.wait());
                return Ok(());
            }
            std::process::Command::new("sh")
                .args(["-c", "\"$0\" --settings </dev/null &"])
                .arg(exe)
                .status()
                .map(drop)
        });
        if let Err(e) = spawned {
            eprintln!("McGyver: couldn't open the settings: {e}");
        }
    }

    /// The settings window saved: take the language, trigger key and desk from the file.
    fn reload(&mut self) {
        let settings = Settings::load();
        crate::i18n::set(settings.language);
        self.hold = settings.hold;
        self.hold_time = settings.hold_time();
        self.appearance = settings.appearance;
        crate::glass::turn_dark(self.appearance.dark());
        self.quick_apps = settings.quick_apps;
        if settings.trigger != self.trigger {
            self.fold();
            self.trigger = settings.trigger;
            self.key_saw_drag = false;
            if self.tapping {
                let trigger = self.trigger;
                std::thread::spawn(move || {
                    if let Err(e) = crate::tap::rebind(trigger) {
                        eprintln!("McGyver: {e}");
                    }
                });
            }
        }
        let trigger = self.trigger;
        if let Some(tray) = &self.tray {
            // Also redraws the menu in the new language.
            tray.update(|tray| tray.trigger = trigger);
        }
        self.repaint_now();
    }

    /// Starts over as a new process, which picks up the rights `install` asked KWin for.
    fn restart(&mut self) {
        let exe = match crate::settings::own_exe() {
            Ok(exe) => exe,
            Err(e) => {
                eprintln!("McGyver: can't restart: {e}");
                return;
            }
        };
        self.hide();
        // Both names have to be free before the new process asks for them; it keeps this PID,
        // which the tray's name contains.
        if let Some(tray) = self.tray.take() {
            tray.shutdown().wait();
        }
        if let Some(service) = self.service.take() {
            let _ = service.release_name(crate::service::NAME);
        }
        use std::os::unix::process::CommandExt;
        let e = std::process::Command::new(exe).exec();
        eprintln!("McGyver: can't restart: {e}");
        self.quit = true;
    }

    fn key_changed(&mut self, key: u32, pressed: bool) {
        if key != keystate_key(self.trigger) {
            // Another modifier with it is a shortcut, not a hold (the locks don't count); a
            // reading on its way is for the hold that ended.
            if pressed && key >= 3 {
                self.holds += 1;
                if self.hold_at.is_some() || self.desk_menu.is_some() {
                    self.log("another modifier went down, no desk");
                }
                self.hold_at = None;
                self.desk_menu = None;
            }
            return;
        }
        let now = Instant::now();
        if pressed {
            self.log("trigger down");
            self.key_saw_drag = false;
            self.holds += 1;
            self.held_at = Some(now);
            self.hold_at = None;
            self.desk_menu = None;
            if self.paused {
                return;
            }
            // A wheel opened by a tap stays as it is; the tap that follows closes it.
            if !self.probing && !self.overlay.wants_input() {
                self.log("probing for a drag");
                self.probing = true;
                self.map_overlay();
            }
        } else {
            self.log("trigger up");
            self.held_at = None;
            self.hold_at = None;
            self.desk_menu = None;
            // A probe still waiting means a drag is probably holding the pointer still.
            if self.key_saw_drag || self.probing {
                self.taps_ignored_until = Some(now + TAP_AFTER_DRAG);
            }
            if std::mem::take(&mut self.desk) && self.overlay.desk_open() {
                let did = self.overlay.release(now);
                self.log(format!(
                    "trigger released on the desk, did something: {did}"
                ));
                self.run_desk_commands();
                self.repaint_now();
            }
            // Letting go of the key folds the wheel; the drag carries on below it.
            if self.probing {
                self.end_probe("trigger released before a drag");
            }
            if self.overlay.wants_input() && !self.sticky {
                self.log("trigger released, closing the wheel");
                self.overlay.cancel(now);
                self.update_accept();
                self.repaint_now();
            }
        }
    }

    /// Maps the overlay with input over the whole screen, or takes input back if it is up.
    fn map_overlay(&mut self) {
        if self.layer.is_some() {
            // The card is showing; taking input back is enough.
            self.repaint_now();
            self.arm_nudge();
            return;
        }
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("mcgyver"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0);
        layer.commit();
        self.layer = Some(OverlaySurface {
            layer,
            size: (0, 0),
            scale: 1,
            gl_surface: None,
            input: None,
            blur: None,
            blur_bands: Vec::new(),
        });
    }

    fn end_probe(&mut self, why: &str) {
        self.nudge_at = None;
        if std::mem::take(&mut self.probing) {
            self.log(format!("probe over: {why}"));
            if self.overlay.is_idle() {
                self.hide();
            } else {
                self.repaint_now();
            }
        }
    }

    fn hide(&mut self) {
        if let Some(OverlaySurface {
            layer,
            gl_surface,
            blur,
            ..
        }) = self.layer.take()
        {
            if let Some(blur) = blur {
                blur.destroy();
            }
            // The EGL window wraps the wl_surface, so it has to go first.
            if let Some(gl_surface) = gl_surface {
                let _ = self.gl.context.make_current_surfaceless();
                drop(gl_surface);
            }
            drop(layer);
            self.log("overlay hidden");
        }
        self.accepting = false;
        self.click_files = None;
        self.repaint_at = None;
        self.nudge_at = None;
    }

    /// The trigger key has been held long enough with no drag: reads what the desk shows on a
    /// thread (players and the volume take a few round trips), to open it once that's in.
    fn held_long(&mut self) {
        if !self.hold
            || self.paused
            || self.exit.is_some()
            || self.probing
            || self.click_files.is_some()
            || self.overlay.wants_input()
            || !self.pressed[keystate_key(self.trigger) as usize]
            || self.other_modifiers()
        {
            return;
        }
        self.log("trigger held alone, reading the desk");
        // Not a tap, however it ends.
        self.key_saw_drag = true;
        let (apps, read, hold) = (self.quick_apps.clone(), self.desk_read.clone(), self.holds);
        std::thread::spawn(move || {
            // Shift held over files selected in the file manager is for selecting more. Asked
            // alongside the reading, which it would otherwise hold up.
            let menu = std::thread::scope(|s| {
                let selecting = s.spawn(desk::selecting);
                let menu = Menu::read(&apps);
                (!selecting.join().unwrap_or(false)).then_some(menu)
            });
            if let Some(menu) = menu {
                let _ = read.send(DeskRead::Open(hold, menu));
            }
        });
    }

    /// While the desk shows the computer's load or its switches, reads it again every
    /// `desk::LIVE` (once the last reading is back).
    fn read_live(&mut self, now: Instant) {
        let live = self.desk
            && matches!(
                self.overlay.desk_mode(),
                Some(crate::catalog::Mode::Everyday | crate::catalog::Mode::System)
            );
        if !live {
            self.live_at = None;
            return;
        }
        match self.live_at {
            None => self.live_at = Some(now + desk::LIVE),
            Some(at) if now >= at && self.live_read.is_none() => {
                self.live_at = None;
                if let Some(menu) = self.overlay.desk_menu() {
                    self.live_read = Some(self.runner.live(menu));
                }
            }
            Some(at) if now >= at => self.live_at = None,
            Some(_) => {}
        }
    }

    /// Whether a modifier other than the trigger is held (the locks don't count).
    fn other_modifiers(&self) -> bool {
        let trigger = keystate_key(self.trigger) as usize;
        (3..self.pressed.len()).any(|key| key != trigger && self.pressed[key])
    }

    fn desk_read(&mut self, read: DeskRead) {
        match read {
            DeskRead::Open(hold, menu) => {
                let held = self.pressed[keystate_key(self.trigger) as usize];
                if hold != self.holds
                    || !held
                    || self.other_modifiers()
                    || self.held_at.is_none()
                    || self.paused
                    || self.probing
                    || self.click_files.is_some()
                    || self.overlay.wants_input()
                {
                    self.log("desk read, but the key is up or something else is open");
                    return;
                }
                self.log(format!("desk read: modes {:?}", menu.modes()));
                self.desk_menu = Some(menu);
                self.map_overlay();
            }
            DeskRead::Again(number, menu) => {
                if self.live_read.is_some_and(|asked| number >= asked) {
                    self.live_read = None;
                }
                if number >= self.desk_from {
                    self.overlay.refresh_desk(menu, Instant::now());
                    // Turned at the click, but the colours didn't switch: back as the desktop is.
                    if self.appearance == crate::settings::Appearance::System
                        && desk::switched_dark().is_none()
                        && let Some(dark) = self.overlay.desk_dark()
                        && dark != crate::glass::dark()
                    {
                        crate::glass::turn_dark(dark);
                        crate::settings::system_dark_is(dark);
                    }
                    self.repaint_now();
                }
            }
            DeskRead::Welcome(menu) => {
                if self.paused || self.layer.is_some() || !self.overlay.is_idle() {
                    return;
                }
                self.log("welcome");
                let mut settings = Settings::load();
                settings.welcomed = true;
                if let Err(e) = settings.save() {
                    eprintln!("McGyver: can't save the settings: {e}");
                }
                self.welcome = Some(menu);
                self.map_overlay();
            }
        }
    }

    /// Opens the welcome once the overlay knows the screen's size.
    fn show_welcome(&mut self) {
        let Some(size) = self.layer.as_ref().map(|s| s.size).filter(|s| s.0 > 0) else {
            return;
        };
        let Some(menu) = self.welcome.take() else {
            return;
        };
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        let hint = crate::i18n::trf(
            "Hold {} for your apps and settings, or while dragging files",
            &[&self.trigger.label()],
        );
        crate::glass::set_dark(self.appearance.dark());
        self.overlay.welcome(menu, screen, hint, Instant::now());
        self.repaint_now();
    }

    /// The probe found the pointer rather than a drag. Held on, the key opens the desk.
    fn no_drag(&mut self) {
        if self.probing && self.hold {
            self.hold_at = self.held_at.map(|at| at + self.hold_time);
        }
        self.end_probe("pointer entered, no drag");
    }

    /// Opens the desk at `at`, where the pointer showed up on the overlay.
    fn open_desk(&mut self, at: Pos2) {
        let Some(menu) = self.desk_menu.take() else {
            return;
        };
        self.nudge_at = None;
        let (w, h) = self.layer.as_ref().map_or((0, 0), |s| s.size);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
        crate::glass::set_dark(self.appearance.dark());
        self.overlay.open_desk(menu, at, screen, Instant::now());
        self.desk = true;
        self.desk_from = self.runner.next();
        self.sticky = false;
        self.log(format!("desk opened at ({:.0}, {:.0})", at.x, at.y));
        self.repaint_now();
    }

    /// Hands what the desk was asked to do to its thread, which reads the system again after;
    /// the settings this app opens itself.
    fn run_desk_commands(&mut self) {
        let mut commands = self.overlay.take_commands();
        if commands.is_empty() {
            return;
        }
        self.log(format!("desk: {commands:?}"));
        if let Some(at) = commands.iter().position(|c| *c == desk::Command::Settings) {
            commands.remove(at);
            self.show_settings();
        }
        // KWin fades the whole screen across when Plasma's colours change: it shows a still of
        // every window while they repaint, then blends it into what they show. Started here,
        // before the colours switch, the still has the wheel as it is, and the glass turns at
        // once beneath it to fade with everything else; plasma-apply-colorscheme's own start
        // then does nothing. A fade of the wheel's own would be caught halfway in the still.
        if self.appearance == crate::settings::Appearance::System
            && let Some(dark) = commands.iter().find_map(|c| match c {
                desk::Command::DarkMode(on) => Some(*on),
                _ => None,
            })
            && desk::blend_changes()
        {
            self.log("desk: KWin fades the colours across");
            crate::glass::set_dark(dark);
            crate::settings::system_dark_is(dark);
            self.repaint_now();
        }
        // Readings asked before these would undo what the click shows at once.
        self.desk_from = self.runner.run(commands, self.quick_apps.clone());
    }

    /// KWin re-targets a drag only when the pointer moves. If nothing answered the probe, a
    /// drag is probably holding the pointer: move it a pixel and back so the drag finds the
    /// overlay now rather than whenever the hand moves.
    fn nudge(&mut self) {
        if !self.probing && self.click_files.is_none() && self.desk_menu.is_none() {
            return;
        }
        if let Some(fake) = &self.fake_input {
            fake.pointer_motion(1.0, 0.0);
            fake.pointer_motion(-1.0, 0.0);
            let _ = self.conn.flush();
            self.nudges += 1;
            self.log("no answer yet, nudged the pointer");
            if self.nudges < NUDGES {
                self.nudge_at = Some(Instant::now() + NUDGE_AFTER);
            }
        }
    }

    fn arm_nudge(&mut self) {
        self.nudges = 0;
        self.nudge_at = self
            .fake_input
            .as_ref()
            .map(|_| Instant::now() + NUDGE_AFTER);
    }

    fn repaint_now(&mut self) {
        self.repaint_at = Some(Instant::now());
    }

    /// Maps a probe with a fully transparent buffer; GL takes over once there is a wheel.
    fn attach_blank(&mut self) {
        let Some(surface) = &self.layer else {
            return;
        };
        let (w, h) = (surface.size.0 as i32, surface.size.1 as i32);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        canvas.fill(0);
        let wl_surface = surface.layer.wl_surface();
        wl_surface.damage_buffer(0, 0, w, h);
        let _ = buffer.attach_to(wl_surface);
        surface.layer.commit();
        self.arm_nudge();
    }

    fn render(&mut self) {
        self.repaint_at = None;
        let Some(surface) = &mut self.layer else {
            return;
        };
        let (w, h) = surface.size;
        if w == 0 || h == 0 {
            return;
        }
        let scale = surface.render_scale();
        let (pw, ph) = (w * scale as u32, h * scale as u32);
        let wl_surface = surface.layer.wl_surface().clone();
        if surface.gl_surface.is_none() {
            wl_surface.set_buffer_scale(scale);
            match self.gl.create_surface(&wl_surface, pw, ph) {
                Ok(gl_surface) => surface.gl_surface = Some(gl_surface),
                Err(err) => {
                    eprintln!("McGyver: {err}");
                    return;
                }
            }
        }
        let Some(gl_surface) = &surface.gl_surface else {
            return;
        };
        if let Err(err) = self.gl.bind(gl_surface) {
            eprintln!("McGyver: {err}");
            return;
        }

        let now = Instant::now();
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32))),
            time: Some(now.duration_since(self.start).as_secs_f64()),
            predicted_dt: 1.0 / 60.0,
            ..Default::default()
        };
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(scale as f32);
        follow_desk_dark(self.appearance);
        let overlay = &mut self.overlay;
        let mut wants = None;
        let output = self.egui.run_ui(input, |ui| wants = overlay.paint(ui, now));
        // The whole screen while the key looks for a drag or a wheel is open; otherwise only
        // the cards, which can be moved and closed, and everything else goes to the windows
        // below. Committed with the frame, so it matches where the cards are drawn.
        let want_input = self.probing
            || self.click_files.is_some()
            || self.desk_menu.is_some()
            || self.overlay.wants_input();
        let input_region = (!want_input).then(|| {
            self.overlay
                .card_rects()
                .iter()
                .map(|r| {
                    let r = r.expand(1.0);
                    let (x, y) = (r.min.x.floor() as i32, r.min.y.floor() as i32);
                    [x, y, r.max.x.ceil() as i32 - x, r.max.y.ceil() as i32 - y]
                })
                .collect::<Vec<_>>()
        });
        if input_region != surface.input {
            match &input_region {
                None => wl_surface.set_input_region(None),
                Some(rects) => {
                    if let Ok(region) = Region::new(&self.compositor) {
                        for [x, y, w, h] in rects {
                            region.add(*x, *y, *w, *h);
                        }
                        wl_surface.set_input_region(Some(region.wl_region()));
                    }
                }
            }
            surface.input = input_region;
        }
        if self.overlay.take_click() {
            self.clicker.play();
        }
        let egui_wants = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.repaint_delay);
        let primitives = self.egui.tessellate(output.shapes, output.pixels_per_point);
        let mut textures = output.textures_delta;
        self.gl.painter.clear([pw, ph], [0.0; 4]);
        self.gl.painter.paint_and_update_textures(
            [pw, ph],
            output.pixels_per_point,
            &primitives,
            &mut textures,
        );
        if self.can_blur
            && let Some(effects) = &self.effects
        {
            let bands: Vec<[i32; 4]> = self
                .overlay
                .glass()
                .iter()
                .flat_map(|g| g.bands(BLUR_BAND))
                .collect();
            if bands != surface.blur_bands {
                let blur = surface.blur.get_or_insert_with(|| {
                    effects.get_background_effect(&wl_surface, &self.qh, ())
                });
                if bands.is_empty() {
                    blur.set_blur_region(None);
                } else if let Ok(region) = Region::new(&self.compositor) {
                    for [x, y, w, h] in &bands {
                        region.add(*x, *y, *w, *h);
                    }
                    blur.set_blur_region(Some(region.wl_region()));
                }
                surface.blur_bands = bands;
            }
        }
        if let Err(err) = gl_surface.swap_buffers(&self.gl.context) {
            eprintln!("McGyver: swap: {err}");
        }

        let delay = [wants, egui_wants].into_iter().flatten().min();
        self.repaint_at = delay
            .filter(|d| *d < Duration::from_secs(60))
            .map(|d| Instant::now() + d);
    }

    fn drag_offer(&self) -> Option<DragOffer> {
        self.data_device.as_ref()?.data().drag_offer()
    }

    fn is_ours(&self, surface: &wl_surface::WlSurface) -> bool {
        self.layer
            .as_ref()
            .is_some_and(|s| s.layer.wl_surface() == surface)
    }

    /// Accept the drop only while it points at a wedge, so the cursor tells the truth and
    /// releasing anywhere else cancels the drag.
    fn update_accept(&mut self) {
        let want = self.overlay.would_accept();
        if want == self.accepting {
            return;
        }
        if let Some(offer) = self.drag_offer() {
            if want {
                offer.accept_mime_type(offer.serial, Some(URI_LIST.into()));
            } else {
                offer.accept_mime_type(offer.serial, None);
            }
        }
        self.accepting = want;
    }

    /// Asks the drag source for its file list, waiting at most `READ_TIMEOUT`.
    fn read_files(&self, offer: &DragOffer) -> Vec<PathBuf> {
        let Ok(mut pipe) = offer.receive(URI_LIST.into()) else {
            return Vec::new();
        };
        let _ = self.conn.flush();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            let _ = tx.send(text);
        });
        rx.recv_timeout(READ_TIMEOUT)
            .map(|text| parse_uri_list(&text))
            .unwrap_or_default()
    }
}

/// Paths from a `text/uri-list`; non-file URIs are skipped.
fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|uri| uri.strip_prefix("file://"))
        // file://host/path: keep the path.
        .filter_map(|rest| rest.find('/').map(|slash| &rest[slash..]))
        .map(|path| {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(percent_decode(path)))
        })
        .collect()
}

fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).and_then(|b| hex(*b)),
                bytes.get(i + 2).and_then(|b| hex(*b)),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

impl Dispatch<OrgKdeKwinKeystate, ()> for App {
    fn event(
        app: &mut Self,
        _: &OrgKdeKwinKeystate,
        event: org_kde_kwin_keystate::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // KWin re-sends every key's state on each change; act on transitions only.
        let org_kde_kwin_keystate::Event::StateChanged { key, state } = event else {
            return;
        };
        if !app.keystate_seen {
            app.keystate_seen = true;
            app.log("keystate connected");
        }
        let Some(was) = app.pressed.get_mut(key as usize) else {
            return;
        };
        let now = state == STATE_PRESSED;
        if *was != now {
            *was = now;
            app.key_changed(key, now);
        }
    }
}

impl Dispatch<OrgKdeKwinFakeInput, ()> for App {
    fn event(
        _: &mut Self,
        _: &OrgKdeKwinFakeInput,
        _: <OrgKdeKwinFakeInput as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            app.can_blur = matches!(flags, WEnum::Value(f)
                if f.contains(ext_background_effect_manager_v1::Capability::Blur));
            app.log(format!("background blur available: {}", app.can_blur));
        }
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectSurfaceV1,
        _: <ExtBackgroundEffectSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.probing = false;
        self.overlay.cancel(Instant::now());
        self.sticky = false;
        self.desk = false;
        self.desk_menu = None;
        self.hide();
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.log(format!("overlay configured {:?}", configure.new_size));
        let Some(surface) = &mut self.layer else {
            return;
        };
        surface.size = configure.new_size;
        if self.welcome.is_some() {
            self.show_welcome();
        }
        let Some(surface) = &mut self.layer else {
            return;
        };
        match &surface.gl_surface {
            Some(gl_surface) => {
                let scale = surface.render_scale() as u32;
                if let (Some(w), Some(h)) = (
                    NonZeroU32::new(configure.new_size.0 * scale),
                    NonZeroU32::new(configure.new_size.1 * scale),
                ) {
                    gl_surface.resize(&self.gl.context, w, h);
                }
                self.repaint_now();
            }
            None => self.attach_blank(),
        }
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if matches!(
                event.kind,
                PointerEventKind::Enter { .. } | PointerEventKind::Leave { .. }
            ) {
                self.log(format!(
                    "pointer {} on {} surface at {:?}",
                    if matches!(event.kind, PointerEventKind::Enter { .. }) {
                        "enter"
                    } else {
                        "leave"
                    },
                    if self.is_ours(&event.surface) {
                        "our"
                    } else {
                        "another"
                    },
                    event.position,
                ));
            }
            if !self.is_ours(&event.surface) {
                continue;
            }
            let clicking = (self.sticky || self.desk) && self.overlay.wants_input();
            // Only the cards take the pointer: no wheel, nothing being looked for.
            let on_cards = !self.probing
                && self.click_files.is_none()
                && self.desk_menu.is_none()
                && !self.overlay.wants_input();
            let at = pos2(event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_serial = serial;
                    if let Some(cursor) = &self.cursor {
                        cursor.set_shape(serial, Shape::Default);
                    }
                    if self.click_files.is_some() {
                        self.open_click_wheel(at);
                    } else if self.desk_menu.is_some() {
                        self.open_desk(at);
                    } else if clicking {
                        self.overlay.drag_moved(at, Instant::now());
                        self.repaint_now();
                    } else {
                        // A plain pointer enter means no drag is going on.
                        self.no_drag();
                        if on_cards {
                            self.card_pointer(at);
                        }
                    }
                }
                // Already on the overlay (a card is up), the pointer moves rather than enters.
                PointerEventKind::Motion { .. } if self.probing => self.no_drag(),
                PointerEventKind::Leave { .. } => {
                    self.overlay.card_hover(None);
                    self.repaint_now();
                }
                PointerEventKind::Motion { .. } if self.click_files.is_some() => {
                    self.open_click_wheel(at);
                }
                PointerEventKind::Motion { .. } if self.desk_menu.is_some() => {
                    self.open_desk(at);
                }
                PointerEventKind::Motion { .. } if clicking => {
                    self.overlay.drag_moved(at, Instant::now());
                    self.repaint_now();
                }
                PointerEventKind::Press { button, .. } if clicking => {
                    let now = Instant::now();
                    if button == BTN_LEFT {
                        let picked = self.overlay.click(now);
                        self.log(format!("clicked, picked: {picked}"));
                        self.run_desk_commands();
                    } else {
                        self.overlay.cancel(now);
                    }
                    self.repaint_now();
                }
                PointerEventKind::Motion { .. } if on_cards => self.card_pointer(at),
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } if on_cards => {
                    if self.overlay.card_press(at) {
                        self.log("card pressed");
                        self.card_pointer(at);
                    }
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } if on_cards => {
                    self.overlay.card_release(at, Instant::now());
                    self.card_pointer(at);
                }
                _ => {}
            }
        }
    }
}

impl App {
    /// The pointer over the cards: the cursor says what a press would do there.
    fn card_pointer(&mut self, at: Pos2) {
        self.overlay.card_hover(Some(at));
        let spot = self.overlay.card_spot(at);
        if spot != self.on_card {
            self.on_card = spot;
            if let Some(close) = self.overlay.close_button_of(at) {
                self.log(format!(
                    "pointer on a card ({spot:?}), close button at ({:.0}, {:.0})",
                    close.x, close.y
                ));
            }
        }
        let shape = match self.overlay.card_spot(at) {
            Some(CardSpot::Close) => Shape::Pointer,
            _ => Shape::Default,
        };
        if let Some(cursor) = &self.cursor {
            cursor.set_shape(self.pointer_serial, shape);
        }
        self.repaint_now();
    }
}

impl DataDeviceHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataDevice,
        x: f64,
        y: f64,
        surface: &wl_surface::WlSurface,
    ) {
        self.log(format!(
            "data device enter at ({x:.0}, {y:.0}): our surface {}, probing {}, offer {}",
            self.is_ours(surface),
            self.probing,
            self.drag_offer().is_some(),
        ));
        if !self.is_ours(surface) {
            return;
        }
        self.drop_pending_tap();
        // A drag that comes by while the desk waits for the pointer gets the files' wheel.
        if self.desk_menu.take().is_some() {
            self.probing = true;
        }
        self.open_wheel(x, y);
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        self.log("data device leave");
        if self.overlay.wants_input() {
            self.log("drag left");
            self.overlay.cancel(Instant::now());
            self.accepting = false;
            self.repaint_now();
        }
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, x: f64, y: f64) {
        if self.overlay.wants_input() {
            self.overlay
                .drag_moved(pos2(x as f32, y as f32), Instant::now());
            self.update_accept();
            self.repaint_now();
        } else if self.probing {
            // The key went down again before the drag left the overlay. KWin only sends a
            // new enter after the drag leaves, so open from here.
            self.open_wheel(x, y);
        } else {
            self.drop_pending_tap();
        }
    }

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        let picked = self.overlay.drop(Instant::now());
        self.log(format!("dropped, picked: {picked}"));
        if let Some(offer) = self.drag_offer() {
            if picked {
                offer.finish();
            }
            offer.destroy();
        }
        self.accepting = false;
        self.repaint_now();
    }
}

impl App {
    /// Opens the wheel at (`x`, `y`) on the overlay for the drag over it.
    fn open_wheel(&mut self, x: f64, y: f64) {
        if !self.probing {
            return;
        }
        // Any drag, files or not: the key press that found it isn't a tap.
        self.key_saw_drag = true;
        let Some(offer) = self.drag_offer() else {
            return;
        };
        if !offer.with_mime_types(|m| m.iter().any(|m| m == URI_LIST)) {
            self.end_probe("drag carries no files");
            return;
        }
        let files = self.read_files(&offer);
        self.log(format!(
            "drag entered at ({x:.0}, {y:.0}) with {} file(s)",
            files.len()
        ));
        if files.is_empty() {
            self.end_probe("no file paths in the drag");
            return;
        }
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        offer.accept_mime_type(offer.serial, None);
        self.accepting = false;
        self.probing = false;
        self.nudge_at = None;
        let (w, h) = self.layer.as_ref().map_or((0, 0), |s| s.size);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
        crate::glass::set_dark(self.appearance.dark());
        self.overlay.open(
            files,
            pos2(x as f32, y as f32),
            screen,
            Mode::Convert,
            Instant::now(),
        );
        self.sticky = false;
        self.repaint_now();
    }

    /// KWin says the trigger key was tapped on its own: ask Dolphin for its selection on a
    /// thread (it takes a few D-Bus round trips and borrows the clipboard). Tapping again
    /// closes the wheel.
    fn key_tapped(&mut self) {
        let now = Instant::now();
        if self.paused {
            return;
        }
        // keystate may not have reported the release yet.
        let releasing_drag = self.pressed[keystate_key(self.trigger) as usize]
            && (self.key_saw_drag || self.probing);
        if releasing_drag || self.taps_ignored_until.is_some_and(|until| now < until) {
            self.log("trigger tapped, but it was held for a drag");
            return;
        }
        self.log("trigger tapped");
        if self.overlay.wants_input() {
            if self.sticky {
                self.overlay.cancel(now);
                self.repaint_now();
            }
            return;
        }
        let Some(clipboard) = self.clipboard.clone() else {
            self.log("no clipboard access (ext-data-control), can't read Dolphin's selection");
            return;
        };
        if self.click_files.is_some() || self.selecting.swap(true, Ordering::AcqRel) {
            return;
        }
        let (conn, qh, picked, busy) = (
            self.conn.clone(),
            self.qh.clone(),
            self.picked.clone(),
            self.selecting.clone(),
        );
        std::thread::spawn(move || {
            let files = dolphin_selection(&clipboard, &conn, &qh);
            let _ = picked.send((files, now));
            busy.store(false, Ordering::Release);
        });
    }

    /// Dolphin's selection arrived; map the overlay to find out where the pointer is.
    fn open_for_click(&mut self, files: Vec<PathBuf>, tapped_at: Instant) {
        if files.is_empty() {
            self.log("tap: no files selected in an active Dolphin window");
            return;
        }
        if self.paused
            || self.exit.is_some()
            || self.probing
            || self.overlay.wants_input()
            || tapped_at.elapsed() > STALE_TAP
        {
            self.log("tap: too late, something else is going on");
            return;
        }
        self.log(format!("tap: {} file(s) selected in Dolphin", files.len()));
        self.click_files = Some(files);
        self.map_overlay();
    }

    /// A drag reached the overlay while it waited to open a tap's wheel: let the drag through.
    fn drop_pending_tap(&mut self) {
        if self.click_files.take().is_some() {
            self.log("a drag came by, dropping the tap");
            self.repaint_now();
        }
    }

    fn open_click_wheel(&mut self, at: Pos2) {
        let Some(files) = self.click_files.take() else {
            return;
        };
        self.nudge_at = None;
        let (w, h) = self.layer.as_ref().map_or((0, 0), |s| s.size);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
        crate::glass::set_dark(self.appearance.dark());
        self.overlay
            .open(files, at, screen, Mode::Convert, Instant::now());
        self.sticky = true;
        self.log(format!(
            "wheel opened for clicks at ({:.0}, {:.0})",
            at.x, at.y
        ));
        self.repaint_now();
    }
}

/// The files selected in the active Dolphin window. Dolphin has no call that lists them, so
/// this has it copy them and puts back what the clipboard held before.
fn dolphin_selection(
    clipboard: &Clipboard,
    conn: &Connection,
    qh: &QueueHandle<App>,
) -> Vec<PathBuf> {
    let Some(service) = crate::file_manager::active_with_selection() else {
        return Vec::new();
    };
    let (generation, saved) = clipboard.save(conn);
    // Something else changed the clipboard meanwhile; leave it be.
    if clipboard.generation() != generation
        || crate::file_manager::copy_selection(&service).is_err()
    {
        return Vec::new();
    }
    let list = clipboard.next(conn, generation, URI_LIST, COPY_TIMEOUT);
    if clipboard.generation() != generation {
        clipboard.restore(conn, qh, saved);
    }
    list.map(|bytes| parse_uri_list(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default()
}

impl DataOfferHandler for App {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        offer: &mut DragOffer,
        _: DndAction,
    ) {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        wl_surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        let Some(surface) = &mut self.layer else {
            return;
        };
        if surface.layer.wl_surface() != wl_surface || surface.scale == factor {
            return;
        }
        surface.scale = factor;
        let scale = surface.render_scale();
        if let Some(gl_surface) = &surface.gl_surface {
            wl_surface.set_buffer_scale(scale);
            let (w, h) = surface.size;
            if let (Some(w), Some(h)) = (
                NonZeroU32::new(w * scale as u32),
                NonZeroU32::new(h * scale as u32),
            ) {
                gl_surface.resize(&self.gl.context, w, h);
            }
            self.repaint_now();
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if self.data_device.is_none() {
            self.data_device = Some(self.data_devices.get_data_device(qh, &seat));
        }
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
            if let (Some(pointer), Some(shapes)) = (&self.pointer, &self.cursor_shapes) {
                self.cursor = Some(shapes.get_shape_device(pointer, qh));
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kwin_entry_names_the_icon_by_its_path() {
        let entry = kwin_entry().unwrap();
        let icon = data_file(ICON_FILE).unwrap();
        assert!(
            entry
                .lines()
                .any(|line| line == format!("Icon={}", icon.display())),
            "{entry}"
        );
    }

    #[test]
    fn uri_list_decodes_paths() {
        let text = "# comment\r\nfile:///home/me/My%20Photos/a%C3%A7.jpg\r\nhttps://x.org/y\r\nfile://host/tmp/b.png\r\n";
        assert_eq!(
            parse_uri_list(text),
            vec![
                PathBuf::from("/home/me/My Photos/aç.jpg"),
                PathBuf::from("/tmp/b.png")
            ]
        );
    }
}
