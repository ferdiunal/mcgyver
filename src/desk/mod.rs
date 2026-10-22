//! The wheel without files, for the trigger key held on its own: the apps open (to switch to)
//! and the quick apps picked in the settings, the player that is playing, everyday settings
//! (keyboard layout, language, volume, brightness, screenshots, how busy the computer is) and
//! the system (lock, dark mode, staying awake, the microphone, the trash, sleep). Also sharing
//! files with messaging apps, which needs the same knowledge of what is installed. What the
//! system has and does is in `linux`, `mac` and `windows`.

use std::path::PathBuf;
use std::sync::mpsc;

use crate::catalog::Mode;
use crate::i18n::{tr, trf};

mod languages;
mod monitor;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as system;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as system;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as system;

#[cfg(target_os = "linux")]
pub use linux::{blend_changes, switched_dark};
pub use system::installed_apps;

/// The icon an app's desktop file names (a theme icon's name or a path), for `icons`.
#[cfg(target_os = "linux")]
pub(crate) fn icon_name(id: &str) -> Option<String> {
    linux::icon_of(id)
}

/// What is playing, as the player tells it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    /// How the system knows it (an MPRIS bus name, an app's name).
    pub id: String,
    /// The player's own name, as "Spotify".
    pub name: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub can_next: bool,
    pub can_previous: bool,
    /// `None` where the player doesn't say.
    pub shuffle: Option<bool>,
    pub repeat: Option<Repeat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Off,
    Track,
    All,
}

impl Repeat {
    /// The next one a press goes to: off, all, one track, off…
    fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::Track,
            Repeat::Track => Repeat::Off,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// How the system switches to it.
    pub id: String,
    /// "EN", "TR".
    pub short: String,
    /// "English (US)".
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    /// 0 → 1.
    pub level: f32,
    pub muted: bool,
}

/// An installed app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// What the settings keep: a desktop file's id, an app bundle's or shortcut's path.
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
    /// Brings the player's window forward.
    Raise,
}

/// What a screenshot or a recording takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shot {
    /// A part picked with the pointer.
    Region,
    Screen,
    Window,
}

/// Something a wedge or a bubble does.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Media {
        player: String,
        key: MediaKey,
    },
    Shuffle {
        player: String,
        on: bool,
    },
    Repeat {
        player: String,
        to: Repeat,
    },
    /// Types with this keyboard layout; one not set up yet (its id starts with `+`) is added.
    Layout(String),
    Volume(f32),
    Mute(bool),
    Brightness(f32),
    Launch(App),
    /// Brings a window (`switcher::Window::id`) forward.
    Activate(String),
    Screenshot(Shot),
    Record(Shot),
    /// Makes this the language the computer speaks, from the next login.
    Language(String),
    MuteMic(bool),
    DarkMode(bool),
    StayAwake(bool),
    Lock,
    Sleep,
    EmptyTrash,
    ShowDesktop,
    /// Opens the system's monitor app.
    Monitor,
    /// Opens the settings, to pick quick apps.
    Settings,
}

impl Command {
    /// Whether only a click does it, not letting go of the key on it: what can't be undone, or
    /// changes the system's settings for good.
    pub fn needs_click(&self) -> bool {
        match self {
            Command::EmptyTrash | Command::Sleep | Command::Language(_) => true,
            Command::Layout(id) => id.starts_with('+'),
            _ => false,
        }
    }

    /// Whether it opens or changes something the wheel would be in the way of, so the wheel
    /// closes first (and a screenshot waits for it to have gone).
    pub fn leaves(&self) -> bool {
        matches!(
            self,
            Command::Launch(_)
                | Command::Activate(_)
                | Command::Settings
                | Command::Media {
                    key: MediaKey::Raise,
                    ..
                }
                | Command::Screenshot(_)
                | Command::Record(_)
                | Command::Lock
                | Command::Sleep
                | Command::ShowDesktop
                | Command::Monitor
        )
    }
}

/// A language the computer can speak.
#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    /// How the system names it ("tr_TR", "en-US").
    pub id: String,
    /// "TR", "EN".
    pub short: String,
    /// Its own name: "Türkçe", "English".
    pub name: String,
}

/// How busy the computer is, each 0 → 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usage {
    pub processor: f32,
    pub memory: f32,
    /// The disk the system is on.
    pub disk: f32,
}

/// What the system has now, read when the wheel opens and again after each command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
    pub player: Option<Player>,
    pub layouts: Vec<Layout>,
    /// Which of `layouts` is in use.
    pub layout: Option<usize>,
    /// Layouts not set up yet that the computer's languages would want (ids start with `+`).
    pub more_layouts: Vec<Layout>,
    /// The languages the computer may speak, and which it speaks now.
    pub languages: Vec<Language>,
    pub language: Option<usize>,
    pub volume: Option<Volume>,
    /// 0 → 1.
    pub brightness: Option<f32>,
    /// Whether the microphone is muted, where that can be read.
    pub mic_muted: Option<bool>,
    /// Whether the desktop is dark, where that can be switched.
    pub dark: Option<bool>,
    /// Whether the computer is kept awake, where it can be.
    pub awake: Option<bool>,
    /// What screenshots and recordings can be taken of; with one, there is nothing to pick.
    pub shots: Vec<Shot>,
    pub records: Vec<Shot>,
    pub usage: Option<Usage>,
    /// Other apps' windows, the one used last first.
    pub windows: Vec<crate::switcher::Window>,
    /// The quick apps, in the order the settings keep them.
    pub apps: Vec<App>,
}

/// One wedge: its label, the caption while it is aimed at, what it does when picked (if
/// anything; some only hold options), and the options that pop out around it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub label: String,
    pub caption: String,
    pub command: Option<Command>,
    pub subs: Vec<Sub>,
    /// The app whose icon it shows (as `icons::app` takes it).
    pub icon: Option<String>,
    /// A switch: on or off.
    pub on: Option<bool>,
}

/// One option popping out of a wedge, as a bubble.
#[derive(Clone, Debug, PartialEq)]
pub struct Sub {
    pub label: String,
    pub caption: String,
    pub command: Command,
    /// What is in use now: the layout typed with, the level nearest the one set.
    pub on: bool,
    /// The app whose icon it shows instead of its label.
    pub icon: Option<String>,
    /// A gauge around it, full at 1.
    pub gauge: Option<f32>,
}

impl Sub {
    fn new(label: impl Into<String>, caption: impl Into<String>, command: Command) -> Sub {
        Sub {
            label: label.into(),
            caption: caption.into(),
            command,
            on: false,
            icon: None,
            gauge: None,
        }
    }

    fn on(self, on: bool) -> Sub {
        Sub { on, ..self }
    }
}

/// How often the desk is read again while it shows what changes by itself: how busy the
/// computer is, and the switches.
pub const LIVE: std::time::Duration = std::time::Duration::from_secs(1);

/// Volume and brightness levels offered as bubbles.
const VOLUME_STEPS: [f32; 4] = [0.25, 0.5, 0.75, 1.0];
const BRIGHTNESS_STEPS: [f32; 5] = [0.1, 0.25, 0.5, 0.75, 1.0];
/// More wedges than this get too narrow to aim at.
const MOST_WEDGES: usize = 8;
/// Wedges kept for quick apps that aren't open, when that many are.
const QUICK_ROOM: usize = 3;

fn percent(level: f32) -> String {
    format!("{}%", (level * 100.0).round() as i32)
}

/// Which of `steps` is nearest `level`.
fn nearest(steps: &[f32], level: f32) -> usize {
    (0..steps.len())
        .min_by(|&a, &b| {
            (steps[a] - level)
                .abs()
                .total_cmp(&(steps[b] - level).abs())
        })
        .unwrap_or(0)
}

/// What an app id comes to for telling whether a window is that app's: its last path part,
/// without `.desktop`, in lower case.
fn app_key(id: &str) -> String {
    let id = id.trim_end_matches(".desktop");
    id.rsplit(['/', '\\']).next().unwrap_or(id).to_lowercase()
}

impl Menu {
    /// This menu with what changes by itself read again (how busy the computer is, the volume,
    /// the brightness, the switches), the rest as it was: for a desk left open.
    pub fn read_live(&self) -> Menu {
        std::thread::scope(|s| {
            let volume = s.spawn(system::volume);
            let brightness = s.spawn(system::brightness);
            let mic = s.spawn(system::mic_muted);
            let dark = s.spawn(system::dark);
            let usage = s.spawn(monitor::usage);
            Menu {
                volume: volume.join().ok().flatten(),
                brightness: brightness.join().ok().flatten(),
                mic_muted: mic.join().ok().flatten(),
                dark: dark.join().ok().flatten(),
                awake: system::awake(),
                usage: usage.join().ok().flatten(),
                ..self.clone()
            }
        })
    }

    /// Reads what the system has, with the quick apps the settings name (those still installed),
    /// the parts side by side. The layouts are read on this thread: macOS wants them read on
    /// the main one, and this may be it.
    pub fn read(quick_apps: &[String]) -> Menu {
        std::thread::scope(|s| {
            let player = s.spawn(system::player);
            let volume = s.spawn(system::volume);
            let brightness = s.spawn(system::brightness);
            let languages = s.spawn(system::languages);
            let mic = s.spawn(system::mic_muted);
            let dark = s.spawn(system::dark);
            let windows = s.spawn(crate::switcher::windows);
            let usage = s.spawn(monitor::usage);
            let apps = s.spawn(|| quick_apps.iter().filter_map(|id| system::app(id)).collect());
            let (layouts, layout) = system::layouts();
            let (languages, language) = languages.join().unwrap_or_default();
            let more_layouts = more_layouts(&layouts, language.and_then(|i| languages.get(i)));
            let (shots, records) = system::captures();
            Menu {
                player: player.join().ok().flatten(),
                layouts,
                layout,
                more_layouts,
                languages,
                language,
                volume: volume.join().ok().flatten(),
                brightness: brightness.join().ok().flatten(),
                mic_muted: mic.join().ok().flatten(),
                dark: dark.join().ok().flatten(),
                awake: system::awake(),
                shots,
                records,
                usage: usage.join().ok().flatten(),
                windows: windows.join().unwrap_or_default(),
                apps: apps.join().unwrap_or_default(),
            }
        })
    }

    /// The modes it offers, in ring order: the apps (open ones to switch to, and the quick
    /// ones), the player only while something plays or is paused, everyday settings and the
    /// system.
    pub fn modes(&self) -> Vec<Mode> {
        let mut modes = vec![Mode::Apps];
        if self.player.is_some() {
            modes.push(Mode::Media);
        }
        for mode in [Mode::Everyday, Mode::System] {
            if !self.items(mode).is_empty() {
                modes.push(mode);
            }
        }
        modes
    }

    pub fn items(&self, mode: Mode) -> Vec<Item> {
        match mode {
            Mode::Media => self.player.as_ref().map(media).unwrap_or_default(),
            Mode::Everyday => self.everyday(),
            Mode::Apps => self.dock(),
            Mode::System => self.system(),
            Mode::Convert | Mode::Tools | Mode::Actions => Vec::new(),
        }
    }

    /// The hub's text while nothing is aimed at.
    pub fn info(&self, mode: Mode) -> String {
        match (mode, &self.player) {
            (Mode::Media, Some(player)) if player.playing => tr("Playing").into(),
            (Mode::Media, Some(_)) => tr("Paused").into(),
            (Mode::Everyday, _) => self
                .layout
                .and_then(|i| self.layouts.get(i))
                .map(|l| l.short.clone())
                .or_else(|| self.volume.map(|v| percent(v.level)))
                .unwrap_or_default(),
            (Mode::System, _) => tr("System").into(),
            _ => tr("Apps").into(),
        }
    }

    /// The caption while nothing is aimed at.
    pub fn summary(&self, mode: Mode) -> String {
        match (mode, &self.player) {
            (Mode::Media, Some(player)) => now_playing(player),
            (Mode::Everyday, _) => {
                let mut parts = Vec::new();
                if let Some(volume) = self.volume {
                    parts.push(volume_caption(volume));
                }
                if let Some(level) = self.brightness {
                    parts.push(trf("Brightness {}", &[&percent(level)]));
                }
                parts.join(" · ")
            }
            (Mode::System, _) => self.usage.map(usage_caption).unwrap_or_default(),
            _ if !self.windows.is_empty() => tr("Switch to an app").into(),
            _ => tr("Quick Apps").into(),
        }
    }

    /// What `command` will have changed, shown at once rather than when the system is read
    /// again: a second click on Mute must unmute.
    pub fn apply(&mut self, command: &Command) {
        match command {
            Command::Media {
                key: MediaKey::PlayPause,
                ..
            } => {
                if let Some(player) = &mut self.player {
                    player.playing = !player.playing;
                }
            }
            Command::Shuffle { on, .. } => {
                if let Some(player) = &mut self.player {
                    player.shuffle = Some(*on);
                }
            }
            Command::Repeat { to, .. } => {
                if let Some(player) = &mut self.player {
                    player.repeat = Some(*to);
                }
            }
            Command::Layout(id) => {
                if let Some(i) = self.layouts.iter().position(|l| &l.id == id) {
                    self.layout = Some(i);
                }
            }
            Command::Language(id) => {
                if let Some(i) = self.languages.iter().position(|l| &l.id == id) {
                    self.language = Some(i);
                }
            }
            Command::Volume(level) => {
                self.volume = Some(Volume {
                    level: *level,
                    muted: false,
                });
            }
            Command::Mute(on) => {
                if let Some(volume) = &mut self.volume {
                    volume.muted = *on;
                }
            }
            Command::Brightness(level) => {
                if self.brightness.is_some() {
                    self.brightness = Some(*level);
                }
            }
            Command::MuteMic(on) => self.mic_muted = Some(*on),
            Command::DarkMode(on) => self.dark = Some(*on),
            Command::StayAwake(on) => self.awake = Some(*on),
            _ => {}
        }
    }

    /// The open apps to switch to, the one used last first and the one in front last, each
    /// with its windows as bubbles when it has several; then the quick apps that aren't open.
    fn dock(&self) -> Vec<Item> {
        let mut groups: Vec<(String, Vec<&crate::switcher::Window>)> = Vec::new();
        for window in &self.windows {
            let key = if window.app_id.is_empty() {
                window.app.to_lowercase()
            } else {
                app_key(&window.app_id)
            };
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, windows)) => windows.push(window),
                None => groups.push((key, vec![window])),
            }
        }
        // The app in front is where you are; the others are where you'd go.
        groups.sort_by_key(|(_, windows)| windows.iter().any(|w| w.active));
        // Room is kept for a few quick apps that aren't open.
        let closed = self
            .apps
            .iter()
            .filter(|app| {
                let key = app_key(&app.id);
                !groups
                    .iter()
                    .any(|(k, w)| *k == key || w[0].app.eq_ignore_ascii_case(&app.name))
            })
            .count();
        // Past that, the one in front goes first (it is where you are), then those used least
        // lately.
        groups.truncate(MOST_WEDGES - closed.min(QUICK_ROOM));
        let mut items: Vec<Item> = groups
            .iter()
            .map(|(_, windows)| {
                let first = windows[0];
                let subs = if windows.len() > 1 {
                    windows
                        .iter()
                        .enumerate()
                        .map(|(i, w)| {
                            Sub::new(
                                (i + 1).to_string(),
                                w.title.clone(),
                                Command::Activate(w.id.clone()),
                            )
                            .on(w.active)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                Item {
                    label: first.app.clone(),
                    caption: first.title.clone(),
                    command: Some(Command::Activate(first.id.clone())),
                    subs,
                    icon: Some(first.app_id.clone()).filter(|id| !id.is_empty()),
                    on: None,
                }
            })
            .collect();
        let open: Vec<String> = groups.iter().map(|(key, _)| key.clone()).collect();
        let names: Vec<String> = items.iter().map(|i| i.label.to_lowercase()).collect();
        for app in &self.apps {
            if open.contains(&app_key(&app.id)) || names.contains(&app.name.to_lowercase()) {
                continue;
            }
            items.push(Item {
                label: app.name.clone(),
                caption: trf("Open {}", &[&app.name]),
                command: Some(Command::Launch(app.clone())),
                icon: Some(app.id.clone()),
                ..Item::default()
            });
        }
        items.truncate(MOST_WEDGES);
        if items.is_empty() {
            items.push(Item {
                label: tr("Choose apps…").into(),
                caption: tr("Pick quick apps in the settings").into(),
                command: Some(Command::Settings),
                ..Item::default()
            });
        }
        items
    }

    fn everyday(&self) -> Vec<Item> {
        let mut items = Vec::new();
        let switchable = self.layouts.len() + self.more_layouts.len();
        if switchable > 1 {
            let current = self.layout.and_then(|i| self.layouts.get(i));
            let next = self.layout.map_or(0, |i| (i + 1) % self.layouts.len());
            let mut subs: Vec<Sub> = self
                .layouts
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    Sub::new(
                        l.short.clone(),
                        l.name.clone(),
                        Command::Layout(l.id.clone()),
                    )
                    .on(self.layout == Some(i))
                })
                .collect();
            subs.extend(self.more_layouts.iter().map(|l| {
                Sub::new(
                    l.short.clone(),
                    trf("Add {}", &[&l.name]),
                    Command::Layout(l.id.clone()),
                )
            }));
            items.push(Item {
                label: tr("Keyboard").into(),
                caption: current.map_or_else(String::new, |l| l.name.clone()),
                command: (self.layouts.len() > 1)
                    .then(|| Command::Layout(self.layouts[next].id.clone())),
                subs,
                ..Item::default()
            });
        }
        if self.languages.len() > 1 {
            let current = self.language.and_then(|i| self.languages.get(i));
            items.push(Item {
                label: tr("Language").into(),
                caption: current.map_or_else(String::new, |l| l.name.clone()),
                subs: self
                    .languages
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let caption = if self.language == Some(i) {
                            l.name.clone()
                        } else {
                            trf("{} after you log in again", &[&l.name])
                        };
                        Sub::new(l.short.clone(), caption, Command::Language(l.id.clone()))
                            .on(self.language == Some(i))
                    })
                    .collect(),
                ..Item::default()
            });
        }
        if let Some(volume) = self.volume {
            let mut subs = vec![
                Sub::new(
                    tr(if volume.muted { "Unmute" } else { "Mute" }),
                    tr(if volume.muted {
                        "Turn the sound back on"
                    } else {
                        "Turn the sound off"
                    }),
                    Command::Mute(!volume.muted),
                )
                .on(volume.muted),
            ];
            let near = nearest(&VOLUME_STEPS, volume.level);
            subs.extend(VOLUME_STEPS.iter().enumerate().map(|(i, &level)| {
                Sub::new(
                    percent(level),
                    trf("Volume {}", &[&percent(level)]),
                    Command::Volume(level),
                )
                .on(!volume.muted && i == near)
            }));
            items.push(Item {
                label: tr("Volume").into(),
                caption: volume_caption(volume),
                command: Some(Command::Mute(!volume.muted)),
                subs,
                ..Item::default()
            });
        }
        if let Some(now) = self.brightness {
            let near = nearest(&BRIGHTNESS_STEPS, now);
            items.push(Item {
                label: tr("Brightness").into(),
                caption: trf("Brightness {}", &[&percent(now)]),
                subs: BRIGHTNESS_STEPS
                    .iter()
                    .enumerate()
                    .map(|(i, &level)| {
                        Sub::new(
                            percent(level),
                            trf("Brightness {}", &[&percent(level)]),
                            Command::Brightness(level),
                        )
                        .on(i == near)
                    })
                    .collect(),
                ..Item::default()
            });
        }
        let pick = |shot: Shot| match shot {
            Shot::Region => ("Area", "Pick an area"),
            Shot::Screen => ("Screen", "The whole screen"),
            Shot::Window => ("Window", "The window in front"),
        };
        let choices = |shots: &[Shot], command: fn(Shot) -> Command| -> Vec<Sub> {
            if shots.len() < 2 {
                return Vec::new();
            }
            shots
                .iter()
                .map(|&shot| {
                    let (label, caption) = pick(shot);
                    Sub::new(tr(label), tr(caption), command(shot))
                })
                .collect()
        };
        if let Some(&first) = self.shots.first() {
            items.push(Item {
                label: tr("Screenshot").into(),
                caption: tr(pick(first).1).into(),
                command: Some(Command::Screenshot(first)),
                subs: choices(&self.shots, Command::Screenshot),
                ..Item::default()
            });
        }
        if let Some(&first) = self.records.first() {
            items.push(Item {
                label: tr("Record").into(),
                caption: tr("Record the screen").into(),
                command: Some(Command::Record(first)),
                subs: choices(&self.records, Command::Record),
                ..Item::default()
            });
        }
        if let Some(usage) = self.usage {
            let gauge = |label: &str, caption: &str, level: f32| Sub {
                gauge: Some(level),
                ..Sub::new(
                    format!("{}\n{}", tr(label), percent(level)),
                    trf(caption, &[&percent(level)]),
                    Command::Monitor,
                )
            };
            items.push(Item {
                label: tr("Monitor").into(),
                caption: usage_caption(usage),
                command: Some(Command::Monitor),
                subs: vec![
                    gauge("CPU", "Processor {}", usage.processor),
                    gauge("RAM", "Memory {}", usage.memory),
                    gauge("Disk", "Disk {} full", usage.disk),
                ],
                ..Item::default()
            });
        }
        items
    }

    /// Switches (shown on or off) and actions for the whole computer.
    fn system(&self) -> Vec<Item> {
        let switch = |label: &str, on: bool, command: Command| {
            let state = tr(if on { "On" } else { "Off" });
            Item {
                label: tr(label).into(),
                caption: format!("{} · {state}", tr(label)),
                command: Some(command),
                on: Some(on),
                ..Item::default()
            }
        };
        let action = |label: &str, caption: &str, command: Command| Item {
            label: tr(label).into(),
            caption: tr(caption).into(),
            command: Some(command),
            ..Item::default()
        };
        let mut items = vec![action("Lock", "Lock the screen", Command::Lock)];
        if let Some(dark) = self.dark {
            items.push(switch("Dark Mode", dark, Command::DarkMode(!dark)));
        }
        if let Some(awake) = self.awake {
            items.push(switch("Stay Awake", awake, Command::StayAwake(!awake)));
        }
        if let Some(muted) = self.mic_muted {
            let mut mic = switch("Microphone", !muted, Command::MuteMic(!muted));
            mic.caption = tr(if muted {
                "The microphone is muted"
            } else {
                "The microphone is on"
            })
            .into();
            items.push(mic);
        }
        items.push(action(
            "Empty Trash",
            "Empty the trash",
            Command::EmptyTrash,
        ));
        if cfg!(not(target_os = "macos")) {
            items.push(action("Desktop", "Show the desktop", Command::ShowDesktop));
        }
        items.push(action("Sleep", "Put the computer to sleep", Command::Sleep));
        items
    }
}

/// "CPU 12% · Memory 48% · Disk 70%".
fn usage_caption(usage: Usage) -> String {
    [
        trf("Processor {}", &[&percent(usage.processor)]),
        trf("Memory {}", &[&percent(usage.memory)]),
        trf("Disk {} full", &[&percent(usage.disk)]),
    ]
    .join(" · ")
}

/// The keyboard layout the language the computer speaks would want, if it isn't set up yet.
fn more_layouts(layouts: &[Layout], language: Option<&Language>) -> Vec<Layout> {
    if !system::ADDS_LAYOUTS {
        return Vec::new();
    }
    let Some(language) = language else {
        return Vec::new();
    };
    let code = languages::code(&language.id);
    match languages::layout_for(&code) {
        Some(layout) if !layouts.iter().any(|l| l.short.eq_ignore_ascii_case(layout)) => {
            vec![Layout {
                id: format!("+{layout}"),
                short: layout.to_uppercase(),
                name: language.name.clone(),
            }]
        }
        _ => Vec::new(),
    }
}

fn volume_caption(volume: Volume) -> String {
    if volume.muted {
        tr("Muted").into()
    } else {
        trf("Volume {}", &[&percent(volume.level)])
    }
}

/// "Title — Artist", or what there is of it.
fn now_playing(player: &Player) -> String {
    match (player.title.is_empty(), player.artist.is_empty()) {
        (false, false) => format!("{} — {}", player.title, player.artist),
        (false, true) => player.title.clone(),
        (true, _) => player.name.clone(),
    }
}

/// The player's wedges, clockwise from the top: play or pause, next, repeat, the player
/// itself at the bottom, shuffle, previous.
fn media(player: &Player) -> Vec<Item> {
    let id = player.id.clone();
    let key = |key| Command::Media {
        player: id.clone(),
        key,
    };
    let playing = now_playing(player);
    let mut items = vec![Item {
        label: tr(if player.playing { "Pause" } else { "Play" }).into(),
        caption: playing.clone(),
        command: Some(key(MediaKey::PlayPause)),
        ..Item::default()
    }];
    if player.can_next {
        items.push(Item {
            label: tr("Next").into(),
            caption: tr("Next track").into(),
            command: Some(key(MediaKey::Next)),
            ..Item::default()
        });
    }
    if let Some(repeat) = player.repeat {
        items.push(Item {
            label: tr("Repeat").into(),
            caption: tr(match repeat {
                Repeat::Off => "Repeat is off",
                Repeat::All => "Repeating all",
                Repeat::Track => "Repeating this track",
            })
            .into(),
            command: Some(Command::Repeat {
                player: id.clone(),
                to: repeat.next(),
            }),
            on: Some(repeat != Repeat::Off),
            ..Item::default()
        });
    }
    items.push(Item {
        label: player.name.clone(),
        caption: trf("Open {}", &[&player.name]),
        command: Some(key(MediaKey::Raise)),
        ..Item::default()
    });
    if let Some(on) = player.shuffle {
        items.push(Item {
            label: tr("Shuffle").into(),
            caption: tr(if on {
                "Shuffle is on"
            } else {
                "Shuffle is off"
            })
            .into(),
            command: Some(Command::Shuffle {
                player: id.clone(),
                on: !on,
            }),
            on: Some(on),
            ..Item::default()
        });
    }
    if player.can_previous {
        items.push(Item {
            label: tr("Previous").into(),
            caption: tr("Previous track").into(),
            command: Some(key(MediaKey::Previous)),
            ..Item::default()
        });
    }
    items
}

/// Whether files are selected in the file manager in front: holding Shift over them is for
/// selecting more, not for the wheel.
pub fn selecting() -> bool {
    system::selecting()
}

/// Lists the apps and reads what is slow to read ahead, on a thread, so the first wheel
/// without files opens as quickly as the next.
pub fn warm() {
    std::thread::spawn(|| {
        let _ = system::installed_apps();
        let _ = system::brightness();
    });
}

/// Does `command`; an error says why it couldn't. The settings are the platform's to open
/// (see `Runner::run`), so they aren't done here.
pub fn run(command: &Command) -> Result<(), String> {
    match command {
        Command::Settings => Err("the settings are opened by the app".into()),
        Command::Screenshot(_) | Command::Record(_) => {
            // Not with the wheel in the picture: it has folded away by then.
            std::thread::sleep(std::time::Duration::from_millis(350));
            system::run(command)
        }
        command => system::run(command),
    }
}

/// A command gets this long to take effect before the system is read again.
const SETTLE_AFTER_COMMAND: std::time::Duration = std::time::Duration::from_millis(150);

/// Runs what the desk is asked to do on a thread of its own, one batch after another in the
/// order asked, and after each batch reads the system again for the wheel. Each batch has a
/// number, larger than the last; the reading comes back with it, so a reading for a wheel that
/// has closed since can be told apart.
pub struct Runner {
    jobs: mpsc::Sender<(Job, u64)>,
    next: u64,
}

/// What the desk's thread is asked to do.
enum Job {
    /// Run these, then read the system with these quick apps.
    Commands(Vec<Command>, Vec<String>),
    /// Read again what changes by itself on this menu (see `Menu::read_live`).
    Live(Box<Menu>),
}

impl Runner {
    /// `read` gets each batch's number and what the system has after it.
    pub fn new(read: impl Fn(u64, Menu) + Send + 'static) -> Runner {
        let (jobs, todo) = mpsc::channel::<(Job, u64)>();
        std::thread::Builder::new()
            .name("mcgyver desk".into())
            .spawn(move || {
                for (job, number) in todo {
                    let menu = match job {
                        Job::Commands(commands, quick_apps) => {
                            for command in &commands {
                                if let Err(e) = run(command) {
                                    eprintln!("mcgyver: {e}");
                                }
                            }
                            if !commands.is_empty() {
                                std::thread::sleep(SETTLE_AFTER_COMMAND);
                            }
                            Menu::read(&quick_apps)
                        }
                        Job::Live(menu) => menu.read_live(),
                    };
                    read(number, menu);
                }
            })
            .expect("the desk's thread starts");
        Runner { jobs, next: 1 }
    }

    /// The number the next batch will have: readings below it are for batches already asked.
    pub fn next(&self) -> u64 {
        self.next
    }

    /// Queues `commands`; returns the batch's number.
    pub fn run(&mut self, commands: Vec<Command>, quick_apps: Vec<String>) -> u64 {
        self.queue(Job::Commands(commands, quick_apps))
    }

    /// Queues reading again what changes by itself on `menu`; returns the reading's number.
    pub fn live(&mut self, menu: Menu) -> u64 {
        self.queue(Job::Live(Box::new(menu)))
    }

    fn queue(&mut self, job: Job) -> u64 {
        let number = self.next;
        self.next += 1;
        let _ = self.jobs.send((job, number));
        number
    }
}

/// The words of a name or id: its runs of letters and digits, in lower case.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// An app among `apps` with `word` as a whole word of its name or id, ignoring case: "teams"
/// finds Microsoft Teams, not TeamSpeak.
pub fn find_app<'a>(apps: &'a [App], word: &str) -> Option<&'a App> {
    let word = word.to_lowercase();
    apps.iter()
        .find(|app| words(&app.name).chain(words(&app.id)).any(|w| w == word))
}

/// Messaging apps the Actions wheel can hand files to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Messenger {
    Telegram,
    WhatsApp,
    Teams,
    Slack,
}

impl Messenger {
    pub const ALL: [Messenger; 4] = [
        Messenger::Telegram,
        Messenger::WhatsApp,
        Messenger::Teams,
        Messenger::Slack,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Messenger::Telegram => "Telegram",
            Messenger::WhatsApp => "WhatsApp",
            Messenger::Teams => "Teams",
            Messenger::Slack => "Slack",
        }
    }

    /// Whether it is installed.
    pub fn installed(self) -> bool {
        system::messenger(self).is_some()
    }

    /// The app, if installed.
    pub fn app(self) -> Option<App> {
        system::messenger(self)
    }

    /// Hands it `files`. Telegram opens its "share with" window for them; the others have no
    /// way in for files, so they are put on the clipboard and the app comes up to paste them in.
    /// Says what happened.
    pub fn send(self, files: &[PathBuf]) -> Result<String, String> {
        let app =
            system::messenger(self).ok_or_else(|| trf("{} isn't installed", &[&self.name()]))?;
        if self == Messenger::Telegram && system::telegram_send(&app, files)? {
            return Ok(tr("Pick a chat in Telegram").into());
        }
        crate::actions::put_files(files).map_err(|e| e.to_string())?;
        system::open_messenger(self, &app)?;
        Ok(trf("Copied: paste in {}", &[&self.name()]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(playing: bool) -> Player {
        Player {
            id: "org.mpris.MediaPlayer2.test".into(),
            name: "Test".into(),
            title: "Song".into(),
            artist: "Band".into(),
            playing,
            can_next: true,
            can_previous: true,
            shuffle: Some(false),
            repeat: Some(Repeat::Off),
        }
    }

    #[test]
    fn the_player_shows_only_while_there_is_one() {
        let quiet = Menu::default();
        assert_eq!(quiet.modes(), vec![Mode::Apps, Mode::System]);
        let playing = Menu {
            player: Some(player(true)),
            ..Menu::default()
        };
        assert_eq!(playing.modes(), vec![Mode::Apps, Mode::Media, Mode::System]);
        let items = playing.items(Mode::Media);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Pause", "Next", "Repeat", "Test", "Shuffle", "Previous"]
        );
        assert_eq!(playing.summary(Mode::Media), "Song — Band");
        assert_eq!(
            items[2].command,
            Some(Command::Repeat {
                player: "org.mpris.MediaPlayer2.test".into(),
                to: Repeat::All
            })
        );
        assert_eq!(
            Menu {
                player: Some(player(false)),
                ..Menu::default()
            }
            .items(Mode::Media)[0]
                .label,
            "Play"
        );
    }

    #[test]
    fn everyday_has_what_the_system_lets_change() {
        let mut menu = Menu {
            layouts: vec![Layout {
                id: "0".into(),
                short: "TR".into(),
                name: "Turkish".into(),
            }],
            layout: Some(0),
            volume: Some(Volume {
                level: 0.4,
                muted: false,
            }),
            brightness: Some(0.7),
            ..Menu::default()
        };
        // One layout is nothing to switch between.
        let labels = |m: &Menu| -> Vec<String> {
            m.items(Mode::Everyday)
                .into_iter()
                .map(|i| i.label)
                .collect()
        };
        assert_eq!(labels(&menu), ["Volume", "Brightness"]);
        menu.layouts.push(Layout {
            id: "1".into(),
            short: "US".into(),
            name: "English (US)".into(),
        });
        assert_eq!(labels(&menu), ["Keyboard", "Volume", "Brightness"]);
        let keyboard = &menu.items(Mode::Everyday)[0];
        assert_eq!(keyboard.command, Some(Command::Layout("1".into())));
        let shorts: Vec<&str> = keyboard.subs.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(shorts, ["TR", "US"]);
        let volume = &menu.items(Mode::Everyday)[1];
        assert_eq!(volume.subs[0].command, Command::Mute(true));
        assert_eq!(volume.subs[2].command, Command::Volume(0.5));
        // 40 % is nearest 50 %; the layout in use is marked.
        let on: Vec<bool> = volume.subs.iter().map(|s| s.on).collect();
        assert_eq!(on, [false, false, true, false, false]);
        assert!(keyboard.subs[0].on && !keyboard.subs[1].on);
        assert_eq!(menu.summary(Mode::Everyday), "Volume 40% · Brightness 70%");
        assert_eq!(menu.info(Mode::Everyday), "TR");
    }

    #[test]
    fn a_click_shows_at_once_what_it_changed() {
        let mut menu = Menu {
            player: Some(player(true)),
            volume: Some(Volume {
                level: 0.4,
                muted: false,
            }),
            ..Menu::default()
        };
        let mute = menu.items(Mode::Everyday)[0].command.clone().unwrap();
        menu.apply(&mute);
        // The next click on the same wedge unmutes.
        assert_eq!(
            menu.items(Mode::Everyday)[0].command,
            Some(Command::Mute(false))
        );
        let play_pause = menu.items(Mode::Media)[0].command.clone().unwrap();
        menu.apply(&play_pause);
        assert_eq!(menu.items(Mode::Media)[0].label, "Play");
    }

    #[test]
    fn the_apps_wedges_switch_to_open_apps_the_one_in_front_last() {
        let window = |id: &str, app: &str, app_id: &str, active: bool| crate::switcher::Window {
            id: id.into(),
            title: format!("{app} {id}"),
            app: app.into(),
            app_id: app_id.into(),
            active,
        };
        let menu = Menu {
            windows: vec![
                window("1", "Konsole", "org.kde.konsole", true),
                window("2", "Firefox", "org.mozilla.firefox", false),
                window("3", "Konsole", "org.kde.konsole", false),
                window("4", "Dolphin", "org.kde.dolphin", false),
            ],
            apps: vec![
                App {
                    id: "org.mozilla.firefox.desktop".into(),
                    name: "Firefox".into(),
                },
                App {
                    id: "org.gnome.Calculator.desktop".into(),
                    name: "Calculator".into(),
                },
            ],
            ..Menu::default()
        };
        let items = menu.items(Mode::Apps);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Firefox", "Dolphin", "Konsole", "Calculator"]);
        assert_eq!(items[0].command, Some(Command::Activate("2".into())));
        assert_eq!(items[0].icon.as_deref(), Some("org.mozilla.firefox"));
        // Konsole's two windows pop out, the one in front marked.
        let konsole = &items[2];
        assert_eq!(konsole.subs.len(), 2);
        assert!(konsole.subs[0].on && !konsole.subs[1].on);
        assert_eq!(konsole.subs[1].command, Command::Activate("3".into()));
        // A quick app not open starts.
        assert!(matches!(items[3].command, Some(Command::Launch(_))));
        assert_eq!(menu.summary(Mode::Apps), "Switch to an app");
    }

    #[test]
    fn the_system_wedges_show_their_switches() {
        let menu = Menu {
            dark: Some(true),
            awake: Some(false),
            mic_muted: Some(true),
            ..Menu::default()
        };
        let items = menu.items(Mode::System);
        let find = |label: &str| items.iter().find(|i| i.label == label).unwrap();
        assert_eq!(items[0].command, Some(Command::Lock));
        assert_eq!(find("Dark Mode").on, Some(true));
        assert_eq!(find("Dark Mode").command, Some(Command::DarkMode(false)));
        assert_eq!(find("Stay Awake").command, Some(Command::StayAwake(true)));
        // Muted, the microphone's switch is off; a click turns it on.
        assert_eq!(find("Microphone").on, Some(false));
        assert_eq!(find("Microphone").command, Some(Command::MuteMic(false)));
        assert!(Command::EmptyTrash.needs_click() && !Command::Lock.needs_click());
        // What changes the system's settings for good takes a click too.
        assert!(Command::Language("tr_TR".into()).needs_click());
        assert!(Command::Layout("+de".into()).needs_click());
        assert!(!Command::Layout("1".into()).needs_click());
        let mut menu = menu;
        menu.apply(&Command::DarkMode(false));
        assert_eq!(menu.dark, Some(false));
    }

    #[test]
    fn only_the_language_spoken_suggests_its_layout() {
        let us = Layout {
            id: "0".into(),
            short: "us".into(),
            name: "English (US)".into(),
        };
        let turkish = languages::language("tr_TR");
        let english = languages::language("en_US");
        let more = more_layouts(std::slice::from_ref(&us), Some(&turkish));
        if system::ADDS_LAYOUTS {
            assert_eq!(more.len(), 1);
            assert_eq!((more[0].id.as_str(), more[0].short.as_str()), ("+tr", "TR"));
        } else {
            assert!(more.is_empty());
        }
        assert!(more_layouts(std::slice::from_ref(&us), Some(&english)).is_empty());
        assert!(more_layouts(&[us], None).is_empty());
    }

    #[test]
    fn many_open_apps_leave_room_for_quick_apps() {
        let windows = (0..12)
            .map(|i| crate::switcher::Window {
                id: i.to_string(),
                title: format!("Window {i}"),
                app: format!("App {i}"),
                app_id: format!("app{i}"),
                active: i == 0,
            })
            .collect();
        let apps = (0..5)
            .map(|i| App {
                id: format!("quick{i}.desktop"),
                name: format!("Quick {i}"),
            })
            .collect();
        let menu = Menu {
            windows,
            apps,
            ..Menu::default()
        };
        let labels: Vec<String> = menu
            .items(Mode::Apps)
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(labels.len(), MOST_WEDGES);
        // Five open apps, the one in front left out; then three quick apps.
        assert_eq!(&labels[..2], ["App 1", "App 2"]);
        assert!(!labels.contains(&"App 0".to_owned()));
        assert_eq!(&labels[5..], ["Quick 0", "Quick 1", "Quick 2"]);
    }

    #[test]
    fn apps_are_found_by_whole_words() {
        let apps = [
            App {
                id: "com.teamspeak.TeamSpeak.desktop".into(),
                name: "TeamSpeak".into(),
            },
            App {
                id: "org.telegram.desktop.desktop".into(),
                name: "Telegram Desktop".into(),
            },
        ];
        assert!(find_app(&apps, "teams").is_none());
        assert_eq!(
            find_app(&apps, "telegram").unwrap().name,
            "Telegram Desktop"
        );
    }

    #[test]
    fn without_quick_apps_the_apps_wedge_leads_to_the_settings() {
        let menu = Menu::default();
        assert_eq!(menu.items(Mode::Apps)[0].command, Some(Command::Settings));
        let menu = Menu {
            apps: vec![App {
                id: "firefox.desktop".into(),
                name: "Firefox".into(),
            }],
            ..Menu::default()
        };
        let items = menu.items(Mode::Apps);
        assert_eq!(items[0].label, "Firefox");
        assert!(matches!(items[0].command, Some(Command::Launch(_))));
    }
}
