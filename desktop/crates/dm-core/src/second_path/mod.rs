// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A second network path for a download's ranges: another interface (an
//! iPhone over USB or its hotspot, a second Wi-Fi, a wired port) beside the
//! one the system routes by. Every other connection of a ranged download
//! goes out on it; the ranges are taken as each connection finishes one, so
//! the faster path takes more of them by itself.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// A network interface a download could go out on: its name (`en5`) and
/// its address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    /// The interface's name.
    pub name: String,
    /// The interface's address.
    pub address: IpAddr,
}

/// The client for the second path, where one is chosen.
static SECOND: RwLock<Option<reqwest::Client>> = RwLock::new(None);

/// Which ranges of each download (by its parts folder) were last taken by a
/// connection on the second path, in this run: what the progress window
/// shows each path's share from.
static ON_SECOND: std::sync::Mutex<Option<HashMap<PathBuf, HashSet<usize>>>> =
    std::sync::Mutex::new(None);

/// Range `index` of the download staged in `parts` was taken by a
/// connection on the second path, or (`second` false) on the first.
pub(crate) fn fetched_by(parts: &Path, index: usize, second: bool) {
    let mut held = crate::lock(&ON_SECOND);
    let all = held.get_or_insert_with(HashMap::new);
    if second {
        all.entry(parts.to_path_buf()).or_default().insert(index);
    } else if let Some(ranges) = all.get_mut(parts) {
        ranges.remove(&index);
    }
}

/// The client for piece `index` of a stream staged in `parts`: every other
/// piece goes out on the second path where one is chosen, the rest (and all
/// of them where none is) on `first`. Records which, for `on_second`.
pub(crate) fn client_for_piece(
    first: &reqwest::Client,
    parts: &Path,
    index: usize,
) -> reqwest::Client {
    match second_client() {
        Some(second) if index % 2 == 1 => {
            fetched_by(parts, index, true);
            second
        }
        _ => {
            fetched_by(parts, index, false);
            first.clone()
        }
    }
}

/// The ranges of the download staged in `parts` last taken on the second
/// path, in this run.
pub fn on_second(parts: &Path) -> HashSet<usize> {
    crate::lock(&ON_SECOND)
        .as_ref()
        .and_then(|all| all.get(parts).cloned())
        .unwrap_or_default()
}

/// The interfaces that are up, have an IPv4 address and are not the
/// loopback, in the system's order. Empty where they cannot be listed.
pub fn interfaces() -> Vec<Interface> {
    #[cfg(unix)]
    {
        unix::interfaces()
    }
    #[cfg(windows)]
    {
        windows::interfaces()
    }
    #[cfg(not(any(unix, windows)))]
    {
        Vec::new()
    }
}

/// The name the system gives an interface, where it gives one ("iPhone
/// USB", "Wi-Fi", "Thunderbolt Ethernet"): on macOS, `networksetup`'s
/// hardware ports, read at most every few seconds (a phone plugged in
/// shows up by its name at the next look). `None` elsewhere.
pub fn system_name(interface: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::system_name(interface)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = interface;
        None
    }
}

/// Downloads' second path: the interface named, or none for `None` or an
/// empty name. A name with no address now is refused in words, and the
/// path is left off.
pub fn set_second_path(name: Option<&str>) -> Result<(), String> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let Some(name) = name else {
        // None chosen: the path a name set goes. One given outright (a test's,
        // `set_second_client`) is not a setting's to take away: every save of the
        // settings came here, and took it from under that test (2026-11-13).
        if NAMED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            put(None);
        }
        return Ok(());
    };
    let Some(found) = interfaces().into_iter().find(|i| i.name == name) else {
        NAMED.store(false, std::sync::atomic::Ordering::SeqCst);
        put(None);
        return Err(format!("the network {name} is not connected"));
    };
    let builder = crate::builder().local_address(found.address);
    // Bound to the interface too where the system allows it, so the
    // connection goes out on it rather than by the default route.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let builder = builder.interface(&found.name);
    let client = builder
        .build()
        .map_err(|e| format!("the network {name} could not be used: {e}"))?;
    put(Some(client));
    NAMED.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Whether the second path in force is one a name chose (`set_second_path`).
static NAMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The second path's client as given: for a test, one bound to an address.
pub fn set_second_client(client: Option<reqwest::Client>) {
    NAMED.store(false, std::sync::atomic::Ordering::SeqCst);
    put(client);
}

fn put(client: Option<reqwest::Client>) {
    if let Ok(mut slot) = SECOND.write() {
        *slot = client;
    }
}

/// The second path's client, where one is set.
pub(crate) fn second_client() -> Option<reqwest::Client> {
    SECOND.read().ok()?.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' own adapters are listed (gaps row 47): each named, none the loopback, each
    /// with an IPv4 address; a computer on any network has at least one.
    #[cfg(windows)]
    #[test]
    fn windows_lists_its_adapters_by_name() {
        let found = interfaces();
        assert!(!found.is_empty(), "no adapter listed");
        for interface in &found {
            assert!(!interface.name.trim().is_empty(), "{interface:?}");
            assert!(
                interface.address.is_ipv4() && !interface.address.is_loopback(),
                "{interface:?}"
            );
        }
    }

    #[test]
    fn the_listed_interfaces_are_up_and_not_the_loopback() {
        for i in interfaces() {
            assert!(!i.address.is_loopback(), "{i:?}");
            assert!(!i.name.is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_device_is_named_by_its_hardware_port() {
        let listing = "\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 00:00\n\nHardware Port: iPhone USB\nDevice: en5\nEthernet Address: N/A\n";
        let ports = macos::ports_of(listing);
        assert_eq!(ports.get("en0").map(String::as_str), Some("Wi-Fi"));
        assert_eq!(ports.get("en5").map(String::as_str), Some("iPhone USB"));
        assert_eq!(ports.len(), 2);
    }

    #[test]
    fn a_range_is_counted_to_the_path_that_took_it_last() {
        let parts = Path::new("/dm-test/second-path-share");
        fetched_by(parts, 1, true);
        fetched_by(parts, 3, true);
        fetched_by(parts, 2, false);
        assert_eq!(on_second(parts), HashSet::from([1, 3]));
        // Taken again by a connection on the first path.
        fetched_by(parts, 3, false);
        assert_eq!(on_second(parts), HashSet::from([1]));
        assert!(on_second(Path::new("/dm-test/another")).is_empty());
    }

    #[test]
    fn a_network_not_connected_is_refused_and_leaves_the_path_off() {
        let said = set_second_path(Some("dm-no-such-network0")).unwrap_err();
        assert!(said.contains("not connected"), "{said}");
        assert!(second_client().is_none());
        assert!(set_second_path(Some("  ")).is_ok());
        assert!(second_client().is_none());
    }
}
