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

/// How long a name stays held to public addresses after a public address led to it: a
/// download's redirects and the pieces its playlist names, and the lookups after them.
pub(crate) const HELD_PUBLIC: Duration = Duration::from_secs(600);

/// Names a public address led to ([`must_be_public`]), and when.
static PUBLIC_ONLY: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Default::default);

/// Holds `name` to public addresses for [`HELD_PUBLIC`]: a public page, playlist, mirror list or
/// redirect led to it (`redirects::inward`), so where it resolves to addresses inside alone
/// (this computer, its network), it is refused, as an inside address written there is. A name
/// that resolves to a public address when judged and to an inside one when connected to (DNS
/// rebinding) is caught here, at the connection (gaps row 200). At most [`MOST`] names.
pub(crate) fn must_be_public(name: &str) {
    let mut held = crate::lock(&PUBLIC_ONLY);
    if held.len() >= MOST {
        held.retain(|_, at| at.elapsed() < HELD_PUBLIC);
    }
    if held.len() < MOST {
        held.insert(name.to_ascii_lowercase(), Instant::now());
    }
}

/// Whether `name` was found lately to lead inside alone: a hop dm connected to inside.
pub(crate) fn leads_inside(name: &str) -> bool {
    kept(&name.to_ascii_lowercase()).is_some_and(|addrs| {
        !addrs.is_empty() && addrs.iter().all(|a| crate::redirects::inside_ip(a.ip()))
    })
}

/// Whether `name`'s addresses take a name held to public ones inside: every one of them inside.
fn led_inside(name: &str, addrs: &[SocketAddr]) -> bool {
    let held = crate::lock(&PUBLIC_ONLY)
        .get(name)
        .is_some_and(|at| at.elapsed() < HELD_PUBLIC);
    held && !addrs.is_empty() && addrs.iter().all(|a| crate::redirects::inside_ip(a.ip()))
}

/// The error a name led inside is refused with.
fn refused(name: &str) -> Box<dyn std::error::Error + Send + Sync> {
    format!("{name} leads to an address inside this computer's network, from a public one").into()
}

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
            if led_inside(&name, &addrs) {
                return Box::pin(async move { Err(refused(&name)) });
            }
            return Box::pin(async move { Ok(Box::new(addrs.into_iter()) as Addrs) });
        }
        Box::pin(async move {
            // The port is the connector's to set, as reqwest's own resolver leaves it.
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            if !found.is_empty() {
                keep(&name, &found);
            }
            if led_inside(&name, &found) {
                return Err(refused(&name));
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

    /// A name a public address led to, which resolves inside alone, is refused at the
    /// connection; one never led to, or that also resolves outside, is not (gaps row 200).
    #[tokio::test]
    async fn a_public_name_that_leads_inside_is_refused() {
        let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let public: SocketAddr = "93.184.215.14:0".parse().unwrap();
        keep("rebinds.example", &[loopback]);
        keep("home-nas.example", &[loopback]);
        keep("both.example", &[loopback, public]);
        let resolve = |name: &'static str| async move {
            Kept.resolve(name.parse().unwrap())
                .await
                .map(|addrs| addrs.collect::<Vec<_>>())
        };
        // Typed or handed over directly: a name of one's own network goes inside.
        assert!(resolve("home-nas.example").await.is_ok());
        // A public page led to it.
        let page = url::Url::parse("https://cdn.example/index.m3u8").unwrap();
        for name in ["rebinds.example", "both.example"] {
            let to = url::Url::parse(&format!("https://{name}/seg1.ts")).unwrap();
            assert!(!crate::redirects::inward(&to, std::slice::from_ref(&page)));
        }
        let why = resolve("rebinds.example").await.unwrap_err().to_string();
        assert!(why.contains("rebinds.example"), "{why}");
        assert_eq!(resolve("both.example").await.unwrap().len(), 2);
        // An intranet's own name, found inside, leads on to another inside.
        keep("files.intranet.example", &[loopback]);
        keep("mirror.intranet.example", &[loopback]);
        let from = url::Url::parse("https://files.intranet.example/a").unwrap();
        let to = url::Url::parse("https://mirror.intranet.example/a").unwrap();
        assert!(!crate::redirects::inward(&to, &[from]));
        assert!(resolve("mirror.intranet.example").await.is_ok());
    }
}
