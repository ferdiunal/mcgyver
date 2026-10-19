//! Dolphin through its KDE D-Bus interface: select generated files in an existing window, and
//! find out what the active window has selected.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

const DOLPHIN_PREFIX: &str = "org.kde.dolphin-";
const DOLPHIN_OBJECT: &str = "/dolphin/Dolphin_1";
const DOLPHIN_INTERFACE: &str = "org.kde.dolphin.MainWindow";
const COPY_ACTION: &str = "/dolphin/Dolphin_1/actions/edit_copy";

struct DolphinWindow {
    service: String,
    is_active: bool,
    open_parent_count: usize,
}

/// Selects all paths from one completed batch in a single existing Dolphin window.
pub fn select_items(paths: Vec<PathBuf>) {
    let spawned = thread::Builder::new()
        .name("mcgyver file selection".into())
        .spawn(move || {
            if let Err(error) = show_items(&paths) {
                log::warn!(
                    "couldn't select converted files in an existing Dolphin window: {error}"
                );
            }
        });
    if let Err(error) = spawned {
        log::warn!("couldn't start file selection: {error}");
    }
}

fn show_items(paths: &[PathBuf]) -> io::Result<()> {
    let items = paths
        .iter()
        .map(|path| file_uri(path))
        .collect::<io::Result<Vec<_>>>()?;
    if items.is_empty() {
        return Ok(());
    }

    let windows = dolphin_windows(paths, &items)?;
    let target = windows
        .into_iter()
        .filter(|window| window.is_active || window.open_parent_count > 0)
        .max_by_key(|window| {
            (
                window.is_active && window.open_parent_count > 0,
                window.open_parent_count,
                window.is_active,
            )
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no active Dolphin window matches the output folders",
            )
        })?;

    // Call Dolphin's per-window API directly. FileManager1.ShowItems cannot name a target window
    // and Dolphin may open a separate window for it, especially under Wayland.
    let item_list = format!("array:string:{}", items.join(","));
    dbus_call(
        &target.service,
        DOLPHIN_OBJECT,
        &format!("{DOLPHIN_INTERFACE}.openFiles"),
        &[item_list, "boolean:false".into()],
    )?;
    Ok(())
}

/// The D-Bus service of the active Dolphin window, if it has files selected (its Copy action is
/// enabled only then).
pub fn active_with_selection() -> Option<String> {
    dolphin_services().ok()?.into_iter().find(|service| {
        call_bool(service, &format!("{DOLPHIN_INTERFACE}.isActiveWindow")).unwrap_or(false)
            && dbus_call(
                service,
                COPY_ACTION,
                "org.freedesktop.DBus.Properties.Get",
                &[
                    "string:org.qtproject.Qt.QAction".into(),
                    "string:enabled".into(),
                ],
            )
            .ok()
            .and_then(|reply| parse_bool(&reply))
            .unwrap_or(false)
    })
}

/// Has that Dolphin window copy its selection to the clipboard, as Ctrl+C would.
pub fn copy_selection(service: &str) -> io::Result<()> {
    dbus_call(
        service,
        COPY_ACTION,
        "org.qtproject.Qt.QAction.trigger",
        &[],
    )
    .map(drop)
}

fn dolphin_services() -> io::Result<Vec<String>> {
    let names = dbus_call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus.ListNames",
        &[],
    )?;
    Ok(names
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("string \"")
                .and_then(|name| name.strip_suffix('"'))
        })
        .filter(|name| name.starts_with(DOLPHIN_PREFIX))
        .map(str::to_owned)
        .collect())
}

fn dolphin_windows(paths: &[PathBuf], items: &[String]) -> io::Result<Vec<DolphinWindow>> {
    let services = dolphin_services()?;
    if services.is_empty() {
        return Ok(Vec::new());
    }

    let mut folders_to_check = Vec::new();
    let mut seen_folders = HashSet::new();
    for (path, item) in paths.iter().zip(items) {
        if let Some(parent) = path.parent() {
            let folder = file_uri(parent)?;
            if seen_folders.insert(folder.clone()) {
                folders_to_check.push((item.clone(), folder));
            }
        }
    }
    let mut windows = Vec::with_capacity(services.len());
    for service in services {
        let is_active =
            call_bool(&service, &format!("{DOLPHIN_INTERFACE}.isActiveWindow")).unwrap_or(false);
        let mut open_parent_count = 0;
        for (item, parent) in &folders_to_check {
            let visible = call_bool_arg(
                &service,
                &format!("{DOLPHIN_INTERFACE}.isItemVisibleInAnyView"),
                item,
            )
            .unwrap_or(false);
            let folder_open = if visible {
                false
            } else {
                call_bool_arg(&service, &format!("{DOLPHIN_INTERFACE}.isUrlOpen"), parent)
                    .unwrap_or(false)
            };
            if visible || folder_open {
                open_parent_count += 1;
            }
        }
        windows.push(DolphinWindow {
            service,
            is_active,
            open_parent_count,
        });
    }
    Ok(windows)
}

fn call_bool(service: &str, method: &str) -> io::Result<bool> {
    let reply = dbus_call(service, DOLPHIN_OBJECT, method, &[])?;
    parse_bool(&reply).ok_or_else(|| io::Error::other("Dolphin returned no boolean result"))
}

fn call_bool_arg(service: &str, method: &str, argument: &str) -> io::Result<bool> {
    let reply = dbus_call(
        service,
        DOLPHIN_OBJECT,
        method,
        &[format!("string:{argument}")],
    )?;
    parse_bool(&reply).ok_or_else(|| io::Error::other("Dolphin returned no boolean result"))
}

fn parse_bool(reply: &str) -> Option<bool> {
    let mut words = reply.split_whitespace();
    while let Some(word) = words.next() {
        if word == "boolean" {
            return match words.next()? {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    None
}

fn dbus_call(
    destination: &str,
    object: &str,
    method: &str,
    arguments: &[String],
) -> io::Result<String> {
    let output = Command::new("dbus-send")
        .args(["--session", "--type=method_call", "--print-reply"])
        .arg(format!("--dest={destination}"))
        .arg(object)
        .arg(method)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(io::Error::other(format!(
            "D-Bus request exited with {}",
            output.status
        )))
    }
}

fn file_uri(path: &Path) -> io::Result<String> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(crate::actions::file_uri(&absolute))
}
