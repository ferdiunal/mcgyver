//! The clipboard through `ext-data-control`, which lets a client without focus read and set it.
//! Used to borrow it for a moment: save what is there, let Dolphin copy its selection, read that,
//! and put the saved contents back; and to put files on it, copied or cut, as Dolphin does.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, event_created_child, globals::GlobalList,
    protocol::wl_seat::WlSeat,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

/// Contents larger than this in all aren't saved, so they aren't put back either.
const MAX_SAVED: usize = 64 << 20;
/// Types per clipboard entry worth saving; apps offer a handful of aliases at most.
const MAX_TYPES: usize = 32;
/// How long the owner of the clipboard gets to hand over one type.
const READ_TIMEOUT: Duration = Duration::from_millis(500);
/// How long saving may take in all. Apps encode some types on request (Qt offers an image in
/// every format it can write), so the ones that matter most go first.
const SAVE_BUDGET: Duration = Duration::from_millis(400);

#[derive(Clone)]
pub struct Clipboard {
    manager: ExtDataControlManagerV1,
    device: ExtDataControlDeviceV1,
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    current: Mutex<Selection>,
    changed: Condvar,
}

#[derive(Default)]
struct Selection {
    /// Counts changes, so a waiter can tell a new entry from the one it saw.
    generation: u64,
    offer: Option<ExtDataControlOfferV1>,
}

pub struct ManagerData;
pub struct DeviceData(Arc<Shared>);
/// The types an offer announced.
#[derive(Default)]
pub struct OfferData(Mutex<Vec<String>>);
/// What a source we set hands out, by type.
pub struct SourceData(Arc<Vec<(String, Vec<u8>)>>);

/// The clipboard as it was: `None` if it was empty.
pub struct Saved(Option<Vec<(String, Vec<u8>)>>);

impl Clipboard {
    pub fn bind<State>(globals: &GlobalList, qh: &QueueHandle<State>, seat: &WlSeat) -> Option<Self>
    where
        State: Dispatch<ExtDataControlManagerV1, ManagerData>
            + Dispatch<ExtDataControlDeviceV1, DeviceData>
            + 'static,
    {
        let manager: ExtDataControlManagerV1 = globals.bind(qh, 1..=1, ManagerData).ok()?;
        let shared = Arc::new(Shared::default());
        let device = manager.get_data_device(seat, qh, DeviceData(shared.clone()));
        Some(Self {
            manager,
            device,
            shared,
        })
    }

    /// Reads the types the clipboard holds now, within `SAVE_BUDGET`, and which change that
    /// was.
    pub fn save(&self, conn: &Connection) -> (u64, Saved) {
        let (generation, offer) = {
            let current = self.shared.current.lock().unwrap();
            (current.generation, current.offer.clone())
        };
        let Some(offer) = offer else {
            return (generation, Saved(None));
        };
        let mut types = mimes(&offer);
        types.sort_by_key(|mime| match mime.as_str() {
            m if m.starts_with("text/") => 0,
            "image/png" => 1,
            m if m.starts_with("image/") => 3,
            _ => 2,
        });
        let deadline = Instant::now() + SAVE_BUDGET;
        let mut saved = Vec::new();
        let mut left = MAX_SAVED;
        for mime in types.into_iter().take(MAX_TYPES) {
            let Some(time) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let Some(bytes) = read(conn, &offer, &mime, left, time.min(READ_TIMEOUT)) else {
                continue;
            };
            left -= bytes.len();
            saved.push((mime, bytes));
        }
        (generation, Saved(Some(saved)))
    }

    /// Counts the changes seen so far.
    pub fn generation(&self) -> u64 {
        self.shared.current.lock().unwrap().generation
    }

    /// Waits for the clipboard to change after `generation`, then reads `mime` from it.
    pub fn next(
        &self,
        conn: &Connection,
        generation: u64,
        mime: &str,
        timeout: Duration,
    ) -> Option<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        let mut current = self.shared.current.lock().unwrap();
        while current.generation == generation {
            let left = deadline.checked_duration_since(Instant::now())?;
            current = self.shared.changed.wait_timeout(current, left).unwrap().0;
        }
        let offer = current.offer.clone()?;
        drop(current);
        if !mimes(&offer).iter().any(|m| m == mime) {
            return None;
        }
        read(conn, &offer, mime, MAX_SAVED, READ_TIMEOUT)
    }

    /// Puts saved contents back. They are served from this process until something else
    /// takes the clipboard.
    pub fn restore<State>(&self, conn: &Connection, qh: &QueueHandle<State>, saved: Saved)
    where
        State: Dispatch<ExtDataControlSourceV1, SourceData> + 'static,
    {
        match saved.0 {
            Some(types) if !types.is_empty() => self.offer(conn, qh, types),
            // Unreadable contents can't come back; an empty clipboard stays empty.
            Some(_) => {}
            None => {
                self.device.set_selection(None);
                let _ = conn.flush();
            }
        }
    }

    /// Takes the clipboard with `types` (each a MIME type and its contents), served from this
    /// process until something else takes it.
    pub fn offer<State>(
        &self,
        conn: &Connection,
        qh: &QueueHandle<State>,
        types: Vec<(String, Vec<u8>)>,
    ) where
        State: Dispatch<ExtDataControlSourceV1, SourceData> + 'static,
    {
        let types = Arc::new(types);
        let source = self
            .manager
            .create_data_source(qh, SourceData(types.clone()));
        for (mime, _) in types.iter() {
            source.offer(mime.clone());
        }
        self.device.set_selection(Some(&source));
        let _ = conn.flush();
    }
}

/// Files as Dolphin puts them on the clipboard: their URIs, KDE's mark that they were cut (a
/// paste then moves them), GNOME's form of both, and their paths as text.
pub fn file_types(uris: &[String], paths: &[String], cut: bool) -> Vec<(String, Vec<u8>)> {
    let uri_list: String = uris.iter().map(|u| format!("{u}\r\n")).collect();
    let gnome = format!("{}\n{}", if cut { "cut" } else { "copy" }, uris.join("\n"));
    let text = paths.join("\n");
    vec![
        ("text/uri-list".into(), uri_list.into_bytes()),
        (
            "application/x-kde-cutselection".into(),
            if cut { b"1".to_vec() } else { b"0".to_vec() },
        ),
        ("x-special/gnome-copied-files".into(), gnome.into_bytes()),
        ("text/plain;charset=utf-8".into(), text.clone().into_bytes()),
        ("text/plain".into(), text.into_bytes()),
    ]
}

fn mimes(offer: &ExtDataControlOfferV1) -> Vec<String> {
    offer
        .data::<OfferData>()
        .map(|data| data.0.lock().unwrap().clone())
        .unwrap_or_default()
}

/// One type from an offer, at most `limit` bytes, waiting at most `timeout`.
fn read(
    conn: &Connection,
    offer: &ExtDataControlOfferV1,
    mime: &str,
    limit: usize,
    timeout: Duration,
) -> Option<Vec<u8>> {
    let (reader, writer) = std::io::pipe().ok()?;
    offer.receive(mime.to_owned(), writer.as_fd());
    drop(writer);
    conn.flush().ok()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = reader.take(limit as u64 + 1).read_to_end(&mut bytes);
        let _ = tx.send(read.ok().map(|_| bytes));
    });
    rx.recv_timeout(timeout)
        .ok()
        .flatten()
        .filter(|bytes| bytes.len() <= limit)
}

impl<State> Dispatch2<ExtDataControlManagerV1, State> for ManagerData {
    fn event(
        &self,
        _: &mut State,
        _: &ExtDataControlManagerV1,
        _: <ExtDataControlManagerV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl<State> Dispatch2<ExtDataControlDeviceV1, State> for DeviceData
where
    State: Dispatch<ExtDataControlOfferV1, OfferData> + 'static,
{
    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, OfferData::default())
    ]);

    fn event(
        &self,
        _: &mut State,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            ext_data_control_device_v1::Event::Selection { id } => {
                let mut current = self.0.current.lock().unwrap();
                if let Some(old) = std::mem::replace(&mut current.offer, id) {
                    old.destroy();
                }
                current.generation += 1;
                self.0.changed.notify_all();
            }
            ext_data_control_device_v1::Event::PrimarySelection { id: Some(offer) } => {
                offer.destroy();
            }
            _ => {}
        }
    }
}

impl<State> Dispatch2<ExtDataControlOfferV1, State> for OfferData {
    fn event(
        &self,
        _: &mut State,
        _: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            self.0.lock().unwrap().push(mime_type);
        }
    }
}

impl<State> Dispatch2<ExtDataControlSourceV1, State> for SourceData {
    fn event(
        &self,
        _: &mut State,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                let types = self.0.clone();
                // Writing blocks until the reader takes it all; keep that off the event loop.
                std::thread::spawn(move || {
                    if let Some((_, bytes)) = types.iter().find(|(m, _)| *m == mime_type) {
                        let _ = File::from(fd).write_all(bytes);
                    }
                });
            }
            ext_data_control_source_v1::Event::Cancelled => source.destroy(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_go_on_the_clipboard_as_dolphin_puts_them() {
        let uris = [
            "file:///home/a/x%20y.jpg".to_owned(),
            "file:///home/a/z".to_owned(),
        ];
        let paths = ["/home/a/x y.jpg".to_owned(), "/home/a/z".to_owned()];
        let get = |types: &[(String, Vec<u8>)], mime: &str| {
            let (_, bytes) = types.iter().find(|(m, _)| m == mime).unwrap();
            String::from_utf8(bytes.clone()).unwrap()
        };
        let cut = file_types(&uris, &paths, true);
        assert_eq!(
            get(&cut, "text/uri-list"),
            "file:///home/a/x%20y.jpg\r\nfile:///home/a/z\r\n"
        );
        assert_eq!(get(&cut, "application/x-kde-cutselection"), "1");
        assert_eq!(
            get(&cut, "x-special/gnome-copied-files"),
            "cut\nfile:///home/a/x%20y.jpg\nfile:///home/a/z"
        );
        assert_eq!(get(&cut, "text/plain"), "/home/a/x y.jpg\n/home/a/z");

        let copy = file_types(&uris, &paths, false);
        assert_eq!(get(&copy, "application/x-kde-cutselection"), "0");
        assert!(get(&copy, "x-special/gnome-copied-files").starts_with("copy\n"));
    }
}
