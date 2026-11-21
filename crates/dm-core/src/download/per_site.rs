// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the options say for one address: the tries, the proxy and the route a download of it takes,
//! and the connections it opens.

use super::*;

/// Whether a connection of dm's own is refused under the computer's proxy
/// settings: they are chosen, they gave no address, the site is not
/// bypassed, and the computer has a proxy in force (asked last, as it may
/// run `scutil`).
pub(super) fn refuses_unrouted(
    system: bool,
    has_address: bool,
    bypassed: bool,
    in_force: impl FnOnce() -> bool,
) -> bool {
    system && !has_address && !bypassed && in_force()
}

impl Options {
    /// The attempts and the wait between them, as the retry loops take them.
    pub fn tries(&self) -> retry::Tries {
        retry::Tries {
            attempts: self.attempts,
            wait: (self.retry_wait > 0).then(|| std::time::Duration::from_secs(self.retry_wait)),
        }
    }

    /// The proxy a download of `url` goes through, for the tools and the
    /// FTP client: `proxy`, unless the site is on the bypass list; where a
    /// configuration script chooses, what it says for the site.
    pub fn proxy_for(&self, url: &str) -> Option<String> {
        let host = category::host_of(url);
        let bypassed = crate::proxy::Proxy {
            bypass: self.proxy_bypass.clone(),
            ..Default::default()
        }
        .bypasses(&host);
        if bypassed {
            None
        } else {
            crate::proxy::route_for(self.proxy.as_deref(), url)
        }
    }

    /// Why a connection of dm's own (FTP, a torrent's swarm) to `url` may
    /// not be made: the computer's proxy is in force and names no address
    /// such a connection could use, and the site is not on the bypass list.
    /// A computer with no proxy at all goes direct, so its FTP downloads
    /// and torrents are not refused.
    pub fn no_route_through_system_proxy(&self, url: &str, what: &str) -> Option<String> {
        let bypassed = crate::proxy::Proxy {
            bypass: self.proxy_bypass.clone(),
            ..Default::default()
        }
        .bypasses(&category::host_of(url));
        refuses_unrouted(
            self.system_proxy,
            self.proxy.is_some(),
            bypassed,
            crate::proxy::system_proxy_in_force,
        )
        .then(|| {
            format!(
                "dm cannot send {what} through the computer's proxy settings, and does not go \
                 around them — name the proxy's address in Options › Advanced"
            )
        })
    }

    /// The connections a download of `url` opens: the setting, or the
    /// site's own limit where it has one.
    pub fn connections_for(&self, url: &str) -> usize {
        let host = category::host_of(url);
        let capped = self
            .host_limits
            .iter()
            .filter(|limit| {
                let want = limit
                    .host
                    .trim()
                    .trim_start_matches('.')
                    .to_ascii_lowercase();
                !want.is_empty() && (host == want || host.ends_with(&format!(".{want}")))
            })
            .map(|limit| limit.connections.max(1))
            .min();
        capped.map_or(self.connections, |cap| cap.min(self.connections))
    }
}
