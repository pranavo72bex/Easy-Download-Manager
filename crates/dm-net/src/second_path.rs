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
/// Each download's ranges last fetched over another network than the system's route, by the
/// network (1 the second, 2 the third; gaps row K3).
static ON_SECOND: std::sync::Mutex<Option<HashMap<PathBuf, HashMap<usize, u8>>>> =
    std::sync::Mutex::new(None);

/// The third network's client, where one is chosen (gaps row K3).
static THIRD: RwLock<Option<reqwest::Client>> = RwLock::new(None);

/// Range `index` of the download staged in `parts` was taken by a connection on network
/// `network`: 0 the system's route, 1 the second, 2 the third (gaps row K3).
pub fn fetched_by(parts: &Path, index: usize, network: u8) {
    let mut held = crate::lock(&ON_SECOND);
    let all = held.get_or_insert_with(HashMap::new);
    if network > 0 {
        all.entry(parts.to_path_buf())
            .or_default()
            .insert(index, network);
    } else if let Some(ranges) = all.get_mut(parts) {
        ranges.remove(&index);
    }
}

/// The client for piece `index` of a stream staged in `parts`: every other
/// piece goes out on the second path where one is chosen, the rest (and all
/// of them where none is) on `first`. Records which, for `on_second`.
pub fn client_for_piece(first: &reqwest::Client, parts: &Path, index: usize) -> reqwest::Client {
    let (network, client) = network_for(first, index);
    fetched_by(parts, index, network);
    client
}

/// The networks downloads go out on, beyond the system's route: each its number (1 the second,
/// 2 the third) and its client, those chosen and connected alone (gaps row K3).
pub fn extra_networks() -> Vec<(u8, reqwest::Client)> {
    let mut found = Vec::new();
    if let Some(second) = second_client() {
        found.push((1, second));
    }
    if let Some(third) = THIRD.read().ok().and_then(|t| t.clone()) {
        found.push((2, third));
    }
    found
}

/// The network the `ordinal`th connection or piece goes out on, in turn over the system's route
/// (0, with `first`) and each network chosen beside it: its number and its client.
pub fn network_for(first: &reqwest::Client, ordinal: usize) -> (u8, reqwest::Client) {
    let extra = extra_networks();
    match ordinal % (extra.len() + 1) {
        0 => (0, first.clone()),
        n => extra
            .get(n - 1)
            .cloned()
            .unwrap_or_else(|| (0, first.clone())),
    }
}

/// Downloads' third network, as [`set_second_path`] sets the second (gaps row K3).
#[doc(hidden)]
pub fn set_third_path(name: Option<&str>) -> Result<(), String> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let client = match name {
        None => None,
        Some(name) => Some(bound_client(name)?),
    };
    if let Ok(mut slot) = THIRD.write() {
        *slot = client;
    }
    Ok(())
}

/// A client bound to the network `name`, or why it cannot be.
fn bound_client(name: &str) -> Result<reqwest::Client, String> {
    let Some(found) = interfaces().into_iter().find(|i| i.name == name) else {
        return Err(format!("the network {name} is not connected"));
    };
    let builder = crate::builder().local_address(found.address);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let builder = builder.interface(&found.name);
    builder
        .build()
        .map_err(|e| format!("the network {name} could not be used: {e}"))
}

/// The ranges of the download staged in `parts` last taken on the second
/// path, in this run.
pub fn on_second(parts: &Path) -> HashSet<usize> {
    networks_of(parts).into_keys().collect()
}

/// The ranges of the download staged in `parts` last taken on another network than the
/// system's route, each with its network (1 the second, 2 the third), in this run.
pub fn networks_of(parts: &Path) -> HashMap<usize, u8> {
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
/// path is left off. Put in force by dm-core's `configure`.
#[doc(hidden)]
pub fn set_second_path(name: Option<&str>) -> Result<(), String> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let Some(name) = name else {
        // None chosen: the path a name set goes. One given outright (a test's,
        // `set_second_client`) is not a setting's to take away: every save of the
        // settings comes here, and would take it from under that test.
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
#[doc(hidden)]
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
pub fn second_client() -> Option<reqwest::Client> {
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
        fetched_by(parts, 1, 1);
        fetched_by(parts, 3, 2);
        fetched_by(parts, 2, 0);
        assert_eq!(on_second(parts), HashSet::from([1, 3]));
        // Taken again by a connection on the first path.
        fetched_by(parts, 3, 0);
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

tokio::task_local! {
    /// Whether the connection running in this task goes out over the second network (gaps row
    /// K5): its reads then take from that network's own limit too.
    pub static ON_SECOND_TASK: u8;
}

/// The second network's own speed limit, shared by every connection over it (gaps row K5);
/// none while it has none.
static SECOND_LIMIT: std::sync::Mutex<Option<crate::limit::Limiter>> = std::sync::Mutex::new(None);

/// Sets the second network's own limit, in bytes a second; 0 is none.
pub fn set_second_limit(bytes_per_second: u64) {
    let mut held = crate::lock(&SECOND_LIMIT);
    *held = (bytes_per_second > 0).then(|| crate::limit::Limiter::new(bytes_per_second));
}

/// The limit this task's reads take from beside the download's own: the second network's,
/// where this task's connection goes out over it and it has one.
pub fn task_limit() -> Option<crate::limit::Limiter> {
    // The second network's limit is the second's alone (1), not the third's.
    if ON_SECOND_TASK.try_with(|on| *on).unwrap_or(0) != 1 {
        return None;
    }
    crate::lock(&SECOND_LIMIT).clone()
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    /// A connection over the second network takes from its own limit where one is set; one
    /// over the system's route, or any while none is set, does not (gaps row K5).
    #[tokio::test]
    async fn the_second_networks_limit_is_its_connections_alone() {
        set_second_limit(64 * 1024);
        let over_second = ON_SECOND_TASK
            .scope(1, async { task_limit().is_some() })
            .await;
        let over_first = ON_SECOND_TASK
            .scope(0, async { task_limit().is_some() })
            .await;
        let outside = task_limit().is_some();
        set_second_limit(0);
        let unset = ON_SECOND_TASK
            .scope(1, async { task_limit().is_some() })
            .await;
        assert!(over_second, "the second network's connection took no limit");
        assert!(
            !over_first && !outside,
            "a connection over the first took the second's limit"
        );
        assert!(!unset, "a limit of 0 still limited");
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;

    /// With a second and a third network chosen, connections and pieces go out over the
    /// system's route and each in turn; without them, all over the system's route (gaps row K3).
    #[test]
    fn connections_are_spread_over_every_network_chosen() {
        let first = crate::builder().build().unwrap();
        set_second_client(Some(crate::builder().build().unwrap()));
        if let Ok(mut third) = THIRD.write() {
            *third = Some(crate::builder().build().unwrap());
        }
        let turns: Vec<u8> = (0..6).map(|n| network_for(&first, n).0).collect();
        set_second_client(None);
        set_third_path(None).unwrap();
        let alone: Vec<u8> = (0..3).map(|n| network_for(&first, n).0).collect();
        assert_eq!(turns, [0, 1, 2, 0, 1, 2]);
        assert_eq!(alone, [0, 0, 0]);
    }
}
