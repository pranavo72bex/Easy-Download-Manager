// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Names looked up under "Only through" (gaps row H23d): asked of the computer's own name
//! servers by dm's DNS client (`ask`) from the held network's address, so the question goes out
//! by that network or not at all. The system's resolver cannot be told which network to use, so
//! a name asked of it left by the default route while every connection was held: the sites a
//! person downloaded from were told to the network they meant to avoid.
//!
//! The name servers are the computer's (`/etc/resolv.conf`; on Linux under systemd-resolved,
//! whose file names its own stub on 127.0.0.53, the servers it asks,
//! `/run/systemd/resolve/resolv.conf`). macOS and Linux alone: Windows keeps no such file and
//! Android none a program may read; there the system's route stays, as `docs/how-it-works.md`
//! says. IPv4 addresses alone, as the client asks: a held network with no IPv4 address is
//! asked through the system.

use std::net::{IpAddr, SocketAddr};

use super::ask;

/// The address lookups are asked from: the held network's IPv4 one, on macOS and Linux.
pub(super) fn asks_from() -> Option<IpAddr> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return None;
    }
    crate::only_through::bound_to().filter(IpAddr::is_ipv4)
}

/// `name`'s addresses, asked of each name server in turn from `from` until one answers.
pub(super) async fn look_up(name: &str, from: IpAddr) -> std::io::Result<Vec<SocketAddr>> {
    let servers = name_servers();
    if servers.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{name}: no name server is known to ask through the held network"),
        ));
    }
    let mut last = None;
    for server in servers {
        match ask::ask_from(SocketAddr::new(server, 53), name, Some(from)).await {
            Ok(found) => return Ok(found),
            Err(why) => last = Some(why),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no answer")))
}

/// The computer's IPv4 name servers.
fn name_servers() -> Vec<IpAddr> {
    let read = |path: &str| {
        std::fs::read_to_string(path)
            .map(|t| servers_in(&t))
            .unwrap_or_default()
    };
    let servers = read("/etc/resolv.conf");
    // systemd-resolved's stub asks by its own route: the servers it asks are asked instead.
    if servers.iter().any(IpAddr::is_loopback) {
        let upstream = read("/run/systemd/resolve/resolv.conf");
        if !upstream.is_empty() {
            return upstream;
        }
    }
    servers
}

/// The IPv4 `nameserver` lines of a `resolv.conf`.
fn servers_in(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|rest| rest.trim().parse::<IpAddr>().ok())
        .filter(IpAddr::is_ipv4)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resolv_confs_ipv4_name_servers_are_read() {
        let text = "# made by the system\nsearch lan\nnameserver 10.8.0.1\nnameserver fe80::1%en0\n  nameserver 1.1.1.1 \noptions edns0\n";
        assert_eq!(
            servers_in(text),
            [
                "10.8.0.1".parse::<IpAddr>().unwrap(),
                "1.1.1.1".parse().unwrap()
            ]
        );
    }

    /// A question asked from an address is sent from it: a server on the loopback sees the
    /// question come from 127.0.0.1 and its answer is read.
    #[tokio::test]
    async fn a_question_is_sent_from_the_address_given() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let at = server.local_addr().unwrap();
        let answering = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (n, from) = server.recv_from(&mut buf).await.unwrap();
            // The question back as an answer with one A record, 10.0.0.7.
            let mut answer = buf[..n].to_vec();
            answer[2] = 0x81;
            answer[3] = 0x80;
            answer[7] = 1;
            answer.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 0, 0, 7]);
            server.send_to(&answer, from).await.unwrap();
            from
        });
        let found = ask::ask_from(at, "example.com", Some("127.0.0.1".parse().unwrap()))
            .await
            .unwrap();
        assert_eq!(found[0].ip(), "10.0.0.7".parse::<IpAddr>().unwrap());
        assert_eq!(
            answering.await.unwrap().ip(),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
    }
}
