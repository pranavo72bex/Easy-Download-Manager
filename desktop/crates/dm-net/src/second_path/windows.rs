// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows' adapters, for the second path's list.

use super::Interface;
use std::net::IpAddr;

/// Windows' adapters that are up, are not the loopback and have an IPv4 address, each by the
/// name Windows shows ("Wi-Fi", "Ethernet", "Cellular"): from `GetAdaptersAddresses`. Empty
/// where they cannot be listed.
pub(super) fn interfaces() -> Vec<Interface> {
    use windows_sys::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let mut found = Vec::new();
    let mut size: u32 = 16 * 1024;
    for _ in 0..4 {
        // Eight-byte words, so the list the call writes is aligned as its structures need.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let list = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: `list` points at `size` writable bytes, which is what the call is told; it
        // writes a linked list inside them and nothing outside.
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET),
                GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER,
                std::ptr::null(),
                list,
                &mut size,
            )
        };
        if result == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if result != 0 {
            return found;
        }
        let mut at = list.cast_const();
        while !at.is_null() {
            // SAFETY: each node, and the addresses and name it points to, live in `buffer`,
            // which outlives this loop; an address is read as IPv4 only where its family says.
            unsafe {
                let adapter = &*at;
                at = adapter.Next;
                if adapter.OperStatus != IfOperStatusUp
                    || adapter.IfType == IF_TYPE_SOFTWARE_LOOPBACK
                {
                    continue;
                }
                let name = if adapter.FriendlyName.is_null() {
                    String::new()
                } else {
                    let mut len = 0usize;
                    while *adapter.FriendlyName.add(len) != 0 {
                        len += 1;
                    }
                    String::from_utf16_lossy(std::slice::from_raw_parts(adapter.FriendlyName, len))
                };
                if name.is_empty() {
                    continue;
                }
                let mut unicast = adapter.FirstUnicastAddress.cast_const();
                while !unicast.is_null() {
                    let address = &*unicast;
                    unicast = address.Next;
                    let sockaddr = address.Address.lpSockaddr;
                    if sockaddr.is_null() || (*sockaddr).sa_family != AF_INET {
                        continue;
                    }
                    let inet = &*sockaddr.cast::<SOCKADDR_IN>();
                    let ip = std::net::Ipv4Addr::from(u32::from_be(inet.sin_addr.S_un.S_addr));
                    found.push(Interface {
                        name: name.clone(),
                        address: IpAddr::V4(ip),
                    });
                }
            }
        }
        break;
    }
    found
}
