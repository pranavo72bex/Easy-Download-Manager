// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The interfaces of macOS and Linux, for the second path's list.

use super::Interface;
use std::net::IpAddr;

/// The interfaces that are up, have an IPv4 address and are not the loopback.
pub(super) fn interfaces() -> Vec<Interface> {
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
