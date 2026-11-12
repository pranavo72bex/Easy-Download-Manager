// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the tray says, whichever window owns it: its menu's words and its
//! tooltip, which says how many downloads run and how fast while some do;
//! what its menu adds a download by, its speed presets and which is ticked.
//! The icon and its menu are each window's.

use std::sync::Arc;

use dm_core::{Entry, State};
use dm_shell::app::commands::Command;

use crate::i18n::{t, tn};

/// The speed presets the tray's Speed menu lists, in its order, the one in
/// force ticked ([`ticked`]); the limiter's Settings… follows them there.
pub const PRESETS: [Command; 4] = [
    Command::SpeedLow,
    Command::SpeedMedium,
    Command::SpeedHigh,
    Command::SpeedUnlimited,
];

/// What the tray adds a download by, under Show dm (gaps row U28): Add URL…,
/// which opens as the toolbar's button opens it, and the links on the
/// clipboard.
pub const ADDS: [Command; 2] = [Command::AddUrl, Command::AddFromClipboard];

/// Whether `preset` is the speed in force at a limit of `speed_limit` KB/s
/// (0 for none, which is Unlimited's): the one the tray ticks, as the menu
/// bar's Speed limiter does.
#[must_use]
pub fn ticked(preset: Command, speed_limit: u64) -> bool {
    preset.speed_preset() == Some(speed_limit)
}

/// Whether the tray's `command` brings the window forward first: one that
/// opens a dialog (Add URL…, Settings…, Scheduler…) needs the window shown;
/// a preset or Start Queue is carried out where dm is.
#[must_use]
pub fn shows_window(command: Command) -> bool {
    command.asks_more()
}

/// A command's words on the tray's menu, in the language in force: with "…"
/// where it opens a dialog that asks for more, as the menu bar says it.
#[must_use]
pub fn menu_words(command: Command) -> String {
    let words = t(command.label());
    if command.asks_more() && !words.ends_with('…') {
        format!("{words}…")
    } else {
        words.to_string()
    }
}

/// The tray's words in the language in force: its four menu items, then its
/// tooltip when nothing is downloading.
pub fn words() -> [&'static str; 5] {
    [
        t("Show dm"),
        t("Speed limiter"),
        t("Scheduler…"),
        t("Exit dm"),
        t("dm — downloads keep running here"),
    ]
}

/// What the tray's tooltip says while downloads run: how many and how fast,
/// as the status bar says it; `None` when nothing is downloading. A row with
/// every byte is only putting its file together, and is not counted.
pub fn activity(rows: &[Arc<Entry>]) -> Option<String> {
    let running: Vec<&Entry> = rows
        .iter()
        .map(|r| &**r)
        .filter(|r| r.state == State::Running && !dm_shell::app::rules::fetched_all(r))
        .collect();
    if running.is_empty() {
        return None;
    }
    let rate: f64 = running.iter().map(|r| r.progress.speed).sum();
    Some(tn(
        "{} downloading at {}||{} downloading at {}",
        running.len() as u64,
        &[&running.len(), &crate::format::rate(rate)],
    ))
}

/// What is said when the system will make no tray icon (a Linux desktop
/// with no StatusNotifier host, say), for every window alike: why, in its
/// first line, and what follows from it.
pub fn unavailable(why: &str) -> String {
    format!(
        "the tray icon could not be shown ({}); until it can, closing this window \
         quits dm rather than keeping it downloading in the background.",
        why.lines().next().unwrap_or("").trim()
    )
}

/// Whether a Wayland compositor named by `desktop` (XDG_CURRENT_DESKTOP)
/// has no minimize, so that minimizing a closed window does nothing a
/// person can see: there closing the window quits (asked first while
/// downloads run), whichever window dm draws. Their close key (Super+Q)
/// looked dead.
#[must_use]
pub fn minimize_is_ignored(desktop: &str) -> bool {
    let desktop = desktop.to_lowercase();
    ["sway", "hyprland", "river", "niri", "dwl", "i3", "qtile"]
        .iter()
        .any(|name| desktop.split([':', ';']).any(|d| d == *name))
}

/// Whether something is showing tray icons now: on Linux, a StatusNotifier
/// host may come later than dm, or never (GNOME without its AppIndicator
/// extension), and a tray item nobody shows would leave a window put away
/// there out of reach. Elsewhere the system always has one.
#[must_use]
pub fn host_showing() -> bool {
    #[cfg(target_os = "linux")]
    {
        sni::shown(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Keeps dm's tray item registered, every few seconds for as long as dm
/// runs, with whatever StatusNotifier watcher there is (one that starts
/// after dm, or restarts, as a panel does). Nothing to do elsewhere.
pub fn keep_registered() {
    #[cfg(target_os = "linux")]
    sni::keep_registered();
}

/// dm's tray item and the StatusNotifierWatcher that panels (GNOME's
/// extension, KDE, waybar) show items from.
#[cfg(target_os = "linux")]
mod sni {
    use zbus::blocking::{fdo::DBusProxy, Connection, Proxy};

    const WATCHER: &str = "org.kde.StatusNotifierWatcher";

    /// Whether the watcher lists this process's tray item. With `register`,
    /// one it does not list is registered again: a watcher that has just
    /// (re)started can take its bus name before it answers calls, and a
    /// registration that lands in that gap is dropped without a reply —
    /// seen in the Linux check, and what a panel restart (waybar's, often)
    /// would do. A watcher that will not say what it lists is believed.
    pub fn shown(register: bool) -> bool {
        // Asked on the window's thread, as the window closes: a watcher that
        // holds its name and does not answer within half a second is taken as
        // not showing (`timed_out`), not waited for.
        let Ok(connection) = zbus::blocking::connection::Builder::session()
            .map(|b| b.method_timeout(std::time::Duration::from_millis(500)))
            .and_then(|b| b.build())
        else {
            return false;
        };
        check(&connection, register).unwrap_or(false)
    }

    /// Every few seconds, for as long as dm runs: the tray item stays
    /// registered with whatever watcher there is.
    pub fn keep_registered() {
        let _ = std::thread::Builder::new()
            .name("dm-tray-watch".into())
            .spawn(|| {
                let mut connection = None;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    if connection.is_none() {
                        connection = Connection::session().ok();
                    }
                    if let Some(connection) = &connection {
                        let _ = check(connection, true);
                    }
                }
            });
    }

    /// Whether a call ran out of the connection's time (`method_timeout`).
    fn timed_out(e: &zbus::Error) -> bool {
        matches!(e, zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut)
    }

    fn check(connection: &Connection, register: bool) -> Option<bool> {
        let bus = DBusProxy::new(connection).ok()?;
        let watcher_up = bus
            .name_has_owner(zbus::names::BusName::try_from(WATCHER).ok()?)
            .ok()?;
        if !watcher_up {
            return Some(false);
        }
        let prefix = format!("org.kde.StatusNotifierItem-{}-", std::process::id());
        let ours: Vec<String> = bus
            .list_names()
            .ok()?
            .into_iter()
            .map(|n| n.to_string())
            .filter(|n| n.starts_with(&prefix))
            .collect();
        if ours.is_empty() {
            return Some(false);
        }
        let watcher = zbus::blocking::proxy::Builder::<Proxy>::new(connection)
            .destination(WATCHER)
            .ok()?
            .path("/StatusNotifierWatcher")
            .ok()?
            .interface(WATCHER)
            .ok()?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .ok()?;
        // A watcher that will not say what it lists is believed; one that
        // does not answer at all is not.
        let listed = match watcher.get_property::<Vec<String>>("RegisteredStatusNotifierItems") {
            Ok(listed) => listed,
            Err(e) if timed_out(&e) => return Some(false),
            Err(_) => return Some(true),
        };
        let mut all = true;
        for name in &ours {
            let owner = zbus::names::BusName::try_from(name.as_str())
                .ok()
                .and_then(|n| bus.get_name_owner(n).ok())
                .map(|o| o.to_string());
            // Listed as the name, the name and a path, or the connection
            // that owns it: watchers differ.
            let is_ours = |entry: &String| {
                [Some(name.as_str()), owner.as_deref()]
                    .into_iter()
                    .flatten()
                    .any(|id| entry == id || entry.starts_with(&format!("{id}/")))
            };
            if !listed.iter().any(is_ours) {
                all = false;
                if register {
                    let _ = watcher.call_method("RegisterStatusNotifierItem", &(name.as_str(),));
                }
            }
        }
        // Listed is not shown: a watcher with no panel drawing its items
        // left a hidden window nothing could bring back. A
        // watcher that will not say is believed.
        let host = match watcher.get_property::<bool>("IsStatusNotifierHostRegistered") {
            Ok(host) => host,
            Err(e) if timed_out(&e) => false,
            Err(_) => true,
        };
        Some(all && host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiling_desktops_have_no_minimize_and_others_do() {
        assert!(minimize_is_ignored("sway"));
        assert!(minimize_is_ignored("Hyprland"));
        assert!(minimize_is_ignored("niri:GNOME"));
        assert!(!minimize_is_ignored("KDE"));
        assert!(!minimize_is_ignored("ubuntu:GNOME"));
        assert!(!minimize_is_ignored(""));
    }

    #[test]
    fn nothing_running_says_nothing_and_a_download_is_counted() {
        assert_eq!(activity(&[]), None);
        let row: Entry = serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "http://e.com/f",
            "state": serde_json::to_value(State::Running).unwrap(),
            "progress": {"downloaded": 10, "total": 100, "speed": 2048.0, "eta": null},
            "filename": "f",
        }))
        .unwrap();
        let said = activity(&[Arc::new(row)]).unwrap();
        assert!(said.starts_with("1 downloading at"), "{said}");
        assert_eq!(words().len(), 5);
        assert!(unavailable("no host\nmore").contains("(no host);"));
    }

    /// The tray ticks the preset in force, Unlimited with no limit, and
    /// none for a limit no preset holds (gaps row U28).
    #[test]
    fn the_preset_in_force_is_the_one_ticked() {
        let ticks =
            |limit| -> Vec<Command> { PRESETS.into_iter().filter(|p| ticked(*p, limit)).collect() };
        let low = Command::SpeedLow.speed_preset().unwrap_or(0);
        assert_eq!(ticks(low), vec![Command::SpeedLow]);
        assert_eq!(ticks(0), vec![Command::SpeedUnlimited]);
        assert!(ticks(low + 1).is_empty());
        // Each preset holds a speed of its own, so one at most is ticked.
        for preset in PRESETS {
            let limit = preset.speed_preset().unwrap_or(u64::MAX);
            assert_eq!(ticks(limit), vec![preset]);
        }
    }

    /// The tray adds a download as the toolbar does: Add URL… opens its
    /// dialog with the window brought forward; a preset leaves it where it
    /// is (gaps row U28).
    #[test]
    fn adding_from_the_tray_brings_the_window_forward() {
        assert_eq!(ADDS[0], Command::AddUrl);
        for add in ADDS {
            assert!(shows_window(add), "{add:?}");
            assert!(menu_words(add).ends_with('…'), "{add:?}");
        }
        assert_eq!(menu_words(Command::AddUrl), "Add URL…");
        assert!(shows_window(Command::SpeedLimiterSettings));
        assert!(shows_window(Command::Scheduler));
        for quiet in PRESETS
            .into_iter()
            .chain([Command::StartQueue, Command::StopAll])
        {
            assert!(!shows_window(quiet), "{quiet:?}");
            assert!(!menu_words(quiet).ends_with('…'), "{quiet:?}");
        }
    }
}
