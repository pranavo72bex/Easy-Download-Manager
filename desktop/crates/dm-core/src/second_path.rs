//! A second network path for a download's ranges: another interface (an
//! iPhone over USB or its hotspot, a second Wi-Fi, a wired port) beside the
//! one the system routes by. Every other connection of a ranged download
//! goes out on it; the ranges are taken as each connection finishes one, so
//! the faster path takes more of them by itself.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// A network interface a download could go out on: its name (`en5`) and
/// its address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
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
        unix_interfaces()
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

#[cfg(unix)]
fn unix_interfaces() -> Vec<Interface> {
    let mut found = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` fills `list` with a linked list the system owns
    // until `freeifaddrs`; each node and its address are read only while it
    // lives, and the address is read as IPv4 only where its family says so.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return found;
        }
        let mut at = list;
        while !at.is_null() {
            let node = &*at;
            at = node.ifa_next;
            let flags = node.ifa_flags as libc::c_int;
            let up = flags & libc::IFF_UP != 0 && flags & libc::IFF_RUNNING != 0;
            if !up || flags & libc::IFF_LOOPBACK != 0 || node.ifa_addr.is_null() {
                continue;
            }
            if libc::c_int::from((*node.ifa_addr).sa_family) != libc::AF_INET {
                continue;
            }
            let v4 = &*(node.ifa_addr as *const libc::sockaddr_in);
            let address = IpAddr::V4(std::net::Ipv4Addr::from(u32::from_be(v4.sin_addr.s_addr)));
            if address.is_unspecified() {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(node.ifa_name)
                .to_string_lossy()
                .into_owned();
            found.push(Interface { name, address });
        }
        libc::freeifaddrs(list);
    }
    found
}

/// The name the system gives an interface, where it gives one ("iPhone
/// USB", "Wi-Fi", "Thunderbolt Ethernet"): on macOS, `networksetup`'s
/// hardware ports, read at most every few seconds (a phone plugged in
/// shows up by its name at the next look). `None` elsewhere.
pub fn system_name(interface: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static PORTS: Mutex<Option<(Instant, HashMap<String, String>)>> = Mutex::new(None);
        let mut held = crate::lock(&PORTS);
        let stale = held
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(5));
        if stale {
            let listed = std::process::Command::new("/usr/sbin/networksetup")
                .arg("-listallhardwareports")
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .map(|o| ports_of(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or_default();
            *held = Some((Instant::now(), listed));
        }
        held.as_ref()
            .and_then(|(_, ports)| ports.get(interface).cloned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = interface;
        None
    }
}

/// Each device's hardware port, from `networksetup -listallhardwareports`
/// ("Hardware Port: iPhone USB" then "Device: en5").
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn ports_of(listing: &str) -> HashMap<String, String> {
    let mut ports = HashMap::new();
    let mut port: Option<String> = None;
    for line in listing.lines() {
        if let Some(name) = line.strip_prefix("Hardware Port:") {
            port = Some(name.trim().to_string());
        } else if let Some(device) = line.strip_prefix("Device:") {
            if let Some(name) = port.take() {
                ports.insert(device.trim().to_string(), name);
            }
        }
    }
    ports
}

/// Downloads' second path: the interface named, or none for `None` or an
/// empty name. A name with no address now is refused in words, and the
/// path is left off.
pub fn set_second_path(name: Option<&str>) -> Result<(), String> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let Some(name) = name else {
        set_second_client(None);
        return Ok(());
    };
    let Some(found) = interfaces().into_iter().find(|i| i.name == name) else {
        set_second_client(None);
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
    set_second_client(Some(client));
    Ok(())
}

/// The second path's client as given: for a test, one bound to an address.
pub fn set_second_client(client: Option<reqwest::Client>) {
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

    #[test]
    fn the_listed_interfaces_are_up_and_not_the_loopback() {
        for i in interfaces() {
            assert!(!i.address.is_loopback(), "{i:?}");
            assert!(!i.name.is_empty());
        }
    }

    #[test]
    fn a_device_is_named_by_its_hardware_port() {
        let listing = "\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 00:00\n\nHardware Port: iPhone USB\nDevice: en5\nEthernet Address: N/A\n";
        let ports = ports_of(listing);
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
