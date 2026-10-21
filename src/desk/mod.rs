//! The wheel without files, for the trigger key held on its own: the player that is playing,
//! everyday settings (keyboard layout, volume, brightness), and the quick apps picked in the
//! settings. Also sharing files with messaging apps, which needs the same knowledge of what is
//! installed. What the system has and does is in `linux`, `mac` and `windows`.

use std::path::PathBuf;
use std::sync::mpsc;

use crate::catalog::Mode;
use crate::i18n::{tr, trf};

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

pub use system::installed_apps;

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
    Layout(String),
    Volume(f32),
    Mute(bool),
    Brightness(f32),
    Launch(App),
    /// Opens the settings, to pick quick apps.
    Settings,
}

/// What the system has now, read when the wheel opens and again after each command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
    pub player: Option<Player>,
    pub layouts: Vec<Layout>,
    /// Which of `layouts` is in use.
    pub layout: Option<usize>,
    pub volume: Option<Volume>,
    /// 0 → 1.
    pub brightness: Option<f32>,
    /// The quick apps, in the order the settings keep them.
    pub apps: Vec<App>,
}

/// One wedge: its label, the caption while it is aimed at, what it does when picked (if
/// anything; some only hold options), and the options that pop out around it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub caption: String,
    pub command: Option<Command>,
    pub subs: Vec<Sub>,
}

/// One option popping out of a wedge, as a bubble.
#[derive(Clone, Debug, PartialEq)]
pub struct Sub {
    pub label: String,
    pub caption: String,
    pub command: Command,
    /// What is in use now: the layout typed with, the level nearest the one set.
    pub on: bool,
}

/// Volume and brightness levels offered as bubbles.
const VOLUME_STEPS: [f32; 4] = [0.25, 0.5, 0.75, 1.0];
const BRIGHTNESS_STEPS: [f32; 5] = [0.1, 0.25, 0.5, 0.75, 1.0];

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

impl Menu {
    /// Reads what the system has, with the quick apps the settings name (those still installed),
    /// the parts side by side. The layouts are read on this thread: macOS wants them read on
    /// the main one, and this may be it.
    pub fn read(quick_apps: &[String]) -> Menu {
        std::thread::scope(|s| {
            let player = s.spawn(system::player);
            let volume = s.spawn(system::volume);
            let brightness = s.spawn(system::brightness);
            let apps = s.spawn(|| quick_apps.iter().filter_map(|id| system::app(id)).collect());
            let (layouts, layout) = system::layouts();
            Menu {
                player: player.join().ok().flatten(),
                layouts,
                layout,
                volume: volume.join().ok().flatten(),
                brightness: brightness.join().ok().flatten(),
                apps: apps.join().unwrap_or_default(),
            }
        })
    }

    /// The modes it offers, in ring order: the player only while something plays or is paused,
    /// and the apps always (with a way to pick some when there are none yet).
    pub fn modes(&self) -> Vec<Mode> {
        let mut modes = Vec::new();
        if self.player.is_some() {
            modes.push(Mode::Media);
        }
        if !self.items(Mode::Everyday).is_empty() {
            modes.push(Mode::Everyday);
        }
        modes.push(Mode::Apps);
        modes
    }

    pub fn items(&self, mode: Mode) -> Vec<Item> {
        match mode {
            Mode::Media => self.player.as_ref().map(media).unwrap_or_default(),
            Mode::Everyday => self.everyday(),
            Mode::Apps if self.apps.is_empty() => vec![Item {
                label: tr("Choose apps…").into(),
                caption: tr("Pick quick apps in the settings").into(),
                command: Some(Command::Settings),
                subs: Vec::new(),
            }],
            Mode::Apps => self
                .apps
                .iter()
                .map(|app| Item {
                    label: app.name.clone(),
                    caption: trf("Open {}", &[&app.name]),
                    command: Some(Command::Launch(app.clone())),
                    subs: Vec::new(),
                })
                .collect(),
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
            Command::Media { .. } | Command::Launch(_) | Command::Settings => {}
        }
    }

    fn everyday(&self) -> Vec<Item> {
        let mut items = Vec::new();
        if self.layouts.len() > 1 {
            let current = self.layout.and_then(|i| self.layouts.get(i));
            let next = self.layout.map_or(0, |i| (i + 1) % self.layouts.len());
            items.push(Item {
                label: tr("Keyboard").into(),
                caption: current.map_or_else(String::new, |l| l.name.clone()),
                command: Some(Command::Layout(self.layouts[next].id.clone())),
                subs: self
                    .layouts
                    .iter()
                    .enumerate()
                    .map(|(i, l)| Sub {
                        label: l.short.clone(),
                        caption: l.name.clone(),
                        command: Command::Layout(l.id.clone()),
                        on: self.layout == Some(i),
                    })
                    .collect(),
            });
        }
        if let Some(volume) = self.volume {
            let mut subs = vec![Sub {
                label: tr(if volume.muted { "Unmute" } else { "Mute" }).into(),
                caption: tr(if volume.muted {
                    "Turn the sound back on"
                } else {
                    "Turn the sound off"
                })
                .into(),
                command: Command::Mute(!volume.muted),
                on: volume.muted,
            }];
            let near = nearest(&VOLUME_STEPS, volume.level);
            subs.extend(VOLUME_STEPS.iter().enumerate().map(|(i, &level)| Sub {
                label: percent(level),
                caption: trf("Volume {}", &[&percent(level)]),
                command: Command::Volume(level),
                on: !volume.muted && i == near,
            }));
            items.push(Item {
                label: tr("Volume").into(),
                caption: volume_caption(volume),
                command: Some(Command::Mute(!volume.muted)),
                subs,
            });
        }
        if let Some(now) = self.brightness {
            let near = nearest(&BRIGHTNESS_STEPS, now);
            items.push(Item {
                label: tr("Brightness").into(),
                caption: trf("Brightness {}", &[&percent(now)]),
                command: None,
                subs: BRIGHTNESS_STEPS
                    .iter()
                    .enumerate()
                    .map(|(i, &level)| Sub {
                        label: percent(level),
                        caption: trf("Brightness {}", &[&percent(level)]),
                        command: Command::Brightness(level),
                        on: i == near,
                    })
                    .collect(),
            });
        }
        items
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
        subs: Vec::new(),
    }];
    if player.can_next {
        items.push(Item {
            label: tr("Next").into(),
            caption: tr("Next track").into(),
            command: Some(key(MediaKey::Next)),
            subs: Vec::new(),
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
            subs: Vec::new(),
        });
    }
    items.push(Item {
        label: player.name.clone(),
        caption: trf("Open {}", &[&player.name]),
        command: Some(key(MediaKey::Raise)),
        subs: Vec::new(),
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
            subs: Vec::new(),
        });
    }
    if player.can_previous {
        items.push(Item {
            label: tr("Previous").into(),
            caption: tr("Previous track").into(),
            command: Some(key(MediaKey::Previous)),
            subs: Vec::new(),
        });
    }
    items
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
    jobs: mpsc::Sender<(Vec<Command>, Vec<String>, u64)>,
    next: u64,
}

impl Runner {
    /// `read` gets each batch's number and what the system has after it.
    pub fn new(read: impl Fn(u64, Menu) + Send + 'static) -> Runner {
        let (jobs, todo) = mpsc::channel::<(Vec<Command>, Vec<String>, u64)>();
        std::thread::Builder::new()
            .name("mcgyver desk".into())
            .spawn(move || {
                for (commands, quick_apps, number) in todo {
                    for command in &commands {
                        if let Err(e) = run(command) {
                            eprintln!("mcgyver: {e}");
                        }
                    }
                    std::thread::sleep(SETTLE_AFTER_COMMAND);
                    read(number, Menu::read(&quick_apps));
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
        let number = self.next;
        self.next += 1;
        let _ = self.jobs.send((commands, quick_apps, number));
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
        assert_eq!(quiet.modes(), vec![Mode::Apps]);
        let playing = Menu {
            player: Some(player(true)),
            ..Menu::default()
        };
        assert_eq!(playing.modes(), vec![Mode::Media, Mode::Apps]);
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
