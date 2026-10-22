//! The tray icon (a StatusNotifierItem, which Plasma shows in its system tray): a status line,
//! then pause (or resume), settings and quit. Clicking the icon opens the settings too.

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, ToolTip};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::i18n::{tr, trf};
use crate::service::Command;
use crate::settings::{APP_ID, APP_NAME, Trigger, status_line};

/// Sizes handed to the tray, which picks the closest.
const SIZES: [u32; 5] = [16, 22, 24, 32, 48];

pub struct Tray {
    pub paused: bool,
    pub trigger: Trigger,
    commands: Sender<Command>,
    icon: Vec<Icon>,
    icon_paused: Vec<Icon>,
}

/// Shows the icon; `None` if that fails (the app works without one).
pub fn spawn(commands: Sender<Command>, trigger: Trigger) -> Option<Handle<Tray>> {
    let tray = Tray {
        paused: false,
        trigger,
        commands,
        icon: icons(1.0),
        icon_paused: icons(0.4),
    };
    // Plasma's tray may come up after us at login, or restart; the icon shows up when it does.
    match tray.assume_sni_available(true).spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("mcgyver: no tray icon: {e}");
            None
        }
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        APP_ID.into()
    }

    fn title(&self) -> String {
        APP_NAME.into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        if self.paused {
            self.icon_paused.clone()
        } else {
            self.icon.clone()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        let description = if self.paused {
            tr("Paused").to_owned()
        } else {
            trf(
                "Hold {} while dragging files, or tap it with files selected in Dolphin",
                &[&self.trigger.label()],
            )
        };
        ToolTip {
            title: APP_NAME.into(),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.commands.send(Command::ShowSettings);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: status_line(self.paused),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            // An item with an icon like the others: Plasma lines a checkbox up apart from them.
            StandardItem {
                label: tr(if self.paused { "Resume" } else { "Pause" }).into(),
                icon_name: if self.paused {
                    "media-playback-start"
                } else {
                    "media-playback-pause"
                }
                .into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.commands.send(Command::TogglePause);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr("Settings…").into(),
                icon_name: "configure".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.commands.send(Command::ShowSettings);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: tr("Quit").into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.commands.send(Command::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// The app icon at each of `SIZES`, at `opacity`, as ARGB32 in network byte order.
fn icons(opacity: f32) -> Vec<Icon> {
    SIZES
        .iter()
        .filter_map(|&size| {
            let rgba = crate::icon::rgba(size)?;
            let data = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, a]| [(a as f32 * opacity).round() as u8, r, g, b])
                .collect();
            Some(Icon {
                width: size as i32,
                height: size as i32,
                data,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, translate};

    #[test]
    fn the_menu_has_turkish() {
        for text in [
            "{} — ready",
            "{} — paused",
            "Pause",
            "Resume",
            "Settings…",
            "Quit",
        ] {
            assert_ne!(translate(Lang::Turkish, text), text);
        }
    }

    #[test]
    fn the_tray_gets_every_size_and_a_faded_one_when_paused() {
        let (icons, faded) = (icons(1.0), icons(0.4));
        assert_eq!(icons.len(), SIZES.len());
        for (icon, faded) in icons.iter().zip(&faded) {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
            let middle = ((icon.height / 2 * icon.width + icon.width / 2) * 4) as usize;
            assert_eq!(icon.data[middle], 255);
            assert_eq!(faded.data[middle], 102);
        }
    }
}
