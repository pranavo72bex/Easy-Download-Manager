// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's tray item and the StatusNotifierWatcher that panels (GNOME's
//! extension, KDE, waybar) show items from.

use zbus::blocking::{fdo::DBusProxy, Connection, Proxy};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";

/// How long the watcher is given to answer as the window closes (`shown`) before it is
/// taken as not showing.
const ANSWER_WITHIN: std::time::Duration = std::time::Duration::from_millis(500);

/// How often the tray item is looked at where the bus will not say when the watcher changes.
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// The waits, in milliseconds, before each time a new watcher is asked whether it lists the
/// item: at once, half a second, then 1, 2 and 4 s later.
const REGISTER_AGAIN_MS: [u64; 5] = [0, 500, 1_000, 2_000, 4_000];

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
        .map(|b| b.method_timeout(ANSWER_WITHIN))
        .and_then(|b| b.build())
    else {
        return false;
    };
    check(&connection, register).unwrap_or(false)
}

/// For as long as dm runs, the tray item stays registered with whatever watcher there is:
/// registered again each time the watcher's name is taken (a panel started after dm, or
/// restarted), as the bus says it is (`NameOwnerChanged`), not looked at every few seconds
/// (gaps row S18). Where the bus will not say, looked at every few seconds as before.
pub fn keep_registered() {
    let _ = std::thread::Builder::new()
        .name("dm-tray-watch".into())
        .spawn(|| {
            if let Ok(connection) = Connection::session() {
                let _ = follow_the_watcher(&connection);
            }
            let mut connection = None;
            loop {
                std::thread::sleep(LOOK_EVERY);
                if connection.is_none() {
                    connection = Connection::session().ok();
                }
                if let Some(connection) = &connection {
                    let _ = check(connection, true);
                }
            }
        });
}

/// Registers the tray item again with each new owner of the watcher's name, for as long
/// as the bus is there to say so.
fn follow_the_watcher(connection: &Connection) -> zbus::Result<()> {
    let bus = DBusProxy::new(connection)?;
    let owners = bus.receive_name_owner_changed_with_args(&[(0, WATCHER)])?;
    for changed in owners {
        let Ok(args) = changed.args() else { continue };
        if args.new_owner().is_some() {
            // A watcher that has just taken its name may drop a registration before it
            // answers calls (`shown`): asked again, half a second, then 1, 2 and 4 s later,
            // until it lists the item.
            for wait in REGISTER_AGAIN_MS {
                std::thread::sleep(std::time::Duration::from_millis(wait));
                if check(connection, true).unwrap_or(false) {
                    break;
                }
            }
        }
    }
    Ok(())
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
