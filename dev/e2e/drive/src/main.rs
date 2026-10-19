//! Drives KWin's fake input from a script on stdin (one command per line):
//!   move X Y | glide X Y STEPS MS | press | release | key CODE 0|1 | sleep MS | wiggle MS | shell CMD | log TEXT
use std::io::BufRead;
use std::thread::sleep;
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

struct S;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for S {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<OrgKdeKwinFakeInput, ()> for S {
    fn event(_: &mut Self, _: &OrgKdeKwinFakeInput, _: <OrgKdeKwinFakeInput as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

const BTN_LEFT: u32 = 0x110;

fn main() {
    let conn = Connection::connect_to_env().expect("wayland");
    let (globals, mut queue) = registry_queue_init::<S>(&conn).expect("registry");
    let qh = queue.handle();
    let fi: OrgKdeKwinFakeInput = globals.bind(&qh, 4..=5, ()).expect("org_kde_kwin_fake_input");
    fi.authenticate("mcgyver-test".into(), "drag test".into());
    let mut s = S;
    queue.roundtrip(&mut s).unwrap();
    let start = Instant::now();
    let mut pos = (0.0f64, 0.0f64);
    let sync = |queue: &mut wayland_client::EventQueue<S>| { queue.roundtrip(&mut S).unwrap(); };
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| w[i].parse::<f64>().unwrap();
        match w.first().copied() {
            Some("move") => { pos = (num(1), num(2)); fi.pointer_motion_absolute(pos.0, pos.1); }
            // `movefrom NAME`: to the "x y" a `shell` step wrote to $OUT/NAME.
            Some("movefrom") => {
                let file = std::path::Path::new(&std::env::var("OUT").unwrap()).join(w[1]);
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                let xy: Vec<f64> = text.split_whitespace().filter_map(|n| n.parse().ok()).collect();
                if let [x, y] = xy[..] {
                    pos = (x, y);
                    fi.pointer_motion_absolute(x, y);
                } else {
                    println!("[drive] no position in {}", file.display());
                }
            }
            Some("glide") => {
                let (x, y, steps, ms) = (num(1), num(2), num(3) as usize, num(4) as u64);
                let from = pos;
                for k in 1..=steps {
                    let t = k as f64 / steps as f64;
                    pos = (from.0 + (x - from.0) * t, from.1 + (y - from.1) * t);
                    fi.pointer_motion_absolute(pos.0, pos.1);
                    sync(&mut queue);
                    sleep(Duration::from_millis(ms));
                }
            }
            Some("wiggle") => {
                let end = Instant::now() + Duration::from_millis(num(1) as u64);
                let mut k = 0;
                while Instant::now() < end {
                    fi.pointer_motion_absolute(pos.0 + (k % 2) as f64, pos.1);
                    sync(&mut queue);
                    k += 1;
                    sleep(Duration::from_millis(30));
                }
                fi.pointer_motion_absolute(pos.0, pos.1);
            }
            Some("press") => fi.button(BTN_LEFT, 1),
            Some("release") => fi.button(BTN_LEFT, 0),
            Some("key") => fi.keyboard_key(num(1) as u32, num(2) as u32),
            Some("sleep") => { sync(&mut queue); sleep(Duration::from_millis(num(1) as u64)); }
            Some("shell") => {
                let cmd = line.trim_start().strip_prefix("shell").unwrap_or("");
                let _ = std::process::Command::new("sh").arg("-c").arg(cmd).status();
            }
            Some("log") => println!("[drive {:>6} ms] {}", start.elapsed().as_millis(), w[1..].join(" ")),
            _ => {}
        }
        sync(&mut queue);
    }
}
