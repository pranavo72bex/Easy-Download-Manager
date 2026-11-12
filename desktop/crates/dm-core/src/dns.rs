// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A name looked up once a minute, not once a connection (gaps row S19).
//!
//! Every client dm builds had reqwest look a host's name up again for each connection it
//! opened, through the blocking pool: eight connections to one server, a reconnect after a cut,
//! each halving's new request, all asked the system again. Here each name's addresses are kept
//! for [`KEEP`] after a lookup, and a lookup that fails is not kept, so a name that comes back
//! is found at the next try.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// How long a name's addresses are kept: long enough for a download's connections and its
/// reconnects, short enough that a server that moved is found again within a minute.
pub(crate) const KEEP: Duration = Duration::from_secs(60);

/// The most names kept at once; past it the oldest are let go.
const MOST: usize = 512;

/// Each name looked up lately: when, and its addresses.
type Names = HashMap<String, (Instant, Vec<SocketAddr>)>;

/// The names kept ([`Names`]).
static KEPT: LazyLock<Mutex<Names>> = LazyLock::new(Default::default);

/// The resolver every client is built with (`http_client::builder_with`).
pub(crate) fn resolver() -> Arc<Kept> {
    Arc::new(Kept)
}

/// Looks a name up through the system, keeping what it found for [`KEEP`].
pub(crate) struct Kept;

/// `name`'s addresses, where they were looked up less than [`KEEP`] ago.
fn kept(name: &str) -> Option<Vec<SocketAddr>> {
    let kept = crate::lock(&KEPT);
    kept.get(name)
        .filter(|(at, _)| at.elapsed() < KEEP)
        .map(|(_, addrs)| addrs.clone())
}

/// Keeps `addrs` for `name`, letting the oldest go past [`MOST`].
fn keep(name: &str, addrs: &[SocketAddr]) {
    let mut kept = crate::lock(&KEPT);
    if kept.len() >= MOST {
        kept.retain(|_, (at, _)| at.elapsed() < KEEP);
        if kept.len() >= MOST {
            if let Some(oldest) = kept
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(name, _)| name.clone())
            {
                kept.remove(&oldest);
            }
        }
    }
    kept.insert(name.to_string(), (Instant::now(), addrs.to_vec()));
}

impl Resolve for Kept {
    fn resolve(&self, name: Name) -> Resolving {
        let name = name.as_str().to_ascii_lowercase();
        if let Some(addrs) = kept(&name) {
            return Box::pin(async move { Ok(Box::new(addrs.into_iter()) as Addrs) });
        }
        Box::pin(async move {
            // The port is the connector's to set, as reqwest's own resolver leaves it.
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            if !found.is_empty() {
                keep(&name, &found);
            }
            Ok(Box::new(found.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name is looked up once and kept; the kept addresses are what a second ask gets.
    #[tokio::test]
    async fn a_name_is_looked_up_once_and_kept() {
        let first: Vec<SocketAddr> = Kept
            .resolve("localhost".parse().unwrap())
            .await
            .unwrap()
            .collect();
        assert!(!first.is_empty());
        assert_eq!(kept("localhost"), Some(first.clone()));
        let again: Vec<SocketAddr> = Kept
            .resolve("localhost".parse().unwrap())
            .await
            .unwrap()
            .collect();
        assert_eq!(again, first);
    }
}
