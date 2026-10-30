//! A second network path for a download's ranges: another interface (an
//! iPhone over USB or its hotspot, a second Wi-Fi, a wired port) beside the
//! one the system routes by. Every other connection of a ranged download
//! goes out on it; the ranges are taken as each connection finishes one, so
//! the faster path takes more of them by itself.

use std::net::IpAddr;
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
    fn a_network_not_connected_is_refused_and_leaves_the_path_off() {
        let said = set_second_path(Some("dm-no-such-network0")).unwrap_err();
        assert!(said.contains("not connected"), "{said}");
        assert!(second_client().is_none());
        assert!(set_second_path(Some("  ")).is_ok());
        assert!(second_client().is_none());
    }
}
