// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A name looked up once a minute, not once a connection (gaps row S19), and the addresses a
//! connection is made to judged after the lookup (gaps row 200).
//!
//! Every client dm builds had reqwest look a host's name up again for each connection it
//! opened, through the blocking pool: eight connections to one server, a reconnect after a cut,
//! each halving's new request, all asked the system again. Here each name's addresses are kept
//! for [`KEEP`] after a lookup, and a lookup that fails is not kept, so a name that comes back
//! is found at the next try.
//!
//! A name a public address led to ([`must_be_public`]) is connected to at its public addresses
//! alone, judged from the answer the connection is then made to, so a name that answers public
//! when judged and `127.0.0.1` when connected to (DNS rebinding) reaches nothing inside: the
//! resolver's answer is the only one reqwest connects to. As FTP connects its data connection
//! to the control connection's peer and not to the address the server names.
//!
//! A name under `.test` is asked of the benchmark's DNS server where one was given
//! ([`ask_test_names_of`], its `slowname` case), and kept as any other.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

mod ask;

pub use ask::ask_test_names_of;

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

/// The most names held to public addresses at once. Past it a public address leads nowhere new
/// until the oldest run out ([`must_be_public`]): letting one go would let a playlist naming
/// thousands of hosts free the one it means to point inside.
const MOST_HELD: usize = 4096;

/// Names a public address led to ([`must_be_public`]), and when.
static PUBLIC_ONLY: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Default::default);

/// Holds `name` to public addresses for [`HELD_PUBLIC`]: a public page, playlist, mirror list or
/// redirect led to it (`redirects::inward`), so a connection to it is made to its public
/// addresses alone, and where it has none it is refused, as an inside address written there is.
/// A name that resolves to a public address when judged and to an inside one when connected to
/// (DNS rebinding) is caught here, at the connection (gaps row 200).
///
/// False where `name` could not be held: [`MOST_HELD`] names are, none run out. The hop is then
/// refused as an inward one, rather than let through unheld.
#[must_use]
pub(crate) fn must_be_public(name: &str) -> bool {
    hold(&mut crate::lock(&PUBLIC_ONLY), name.to_ascii_lowercase())
}

/// Adds `name` to `held` ([`must_be_public`]), false where [`MOST_HELD`] names are held.
fn hold(held: &mut HashMap<String, Instant>, name: String) -> bool {
    if held.len() >= MOST_HELD && !held.contains_key(&name) {
        held.retain(|_, at| at.elapsed() < HELD_PUBLIC);
        if held.len() >= MOST_HELD {
            return false;
        }
    }
    held.insert(name, Instant::now());
    true
}

/// Whether `name` was found lately to lead inside alone: a hop dm connected to inside.
pub(crate) fn leads_inside(name: &str) -> bool {
    kept(&name.to_ascii_lowercase()).is_some_and(|addrs| {
        !addrs.is_empty() && addrs.iter().all(|a| crate::redirects::inside_ip(a.ip()))
    })
}

/// What a connection to `name` may use of `addrs`: all of them, unless a public address led to
/// it ([`must_be_public`]); then its public ones alone, the address connected to judged as an
/// inside address written there is, and none public is a refusal ([`LedInside`]).
fn judged(name: &str, addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, LedInside> {
    let held = crate::lock(&PUBLIC_ONLY)
        .get(name)
        .is_some_and(|at| at.elapsed() < HELD_PUBLIC);
    if !held || addrs.is_empty() {
        return Ok(addrs);
    }
    let public: Vec<SocketAddr> = addrs
        .into_iter()
        .filter(|a| !crate::redirects::inside_ip(a.ip()))
        .collect();
    if public.is_empty() {
        return Err(LedInside(name.to_string()));
    }
    Ok(public)
}

/// A name a public address led to that leads inside alone: refused at the lookup, so no
/// connection is made (gaps row 200). Said in the words of an inward redirect
/// (`redirects::execute`, `error::network`).
#[derive(Debug)]
pub struct LedInside(String);

impl std::fmt::Display for LedInside {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} leads to an address inside this computer's network, from a public one",
            self.0
        )
    }
}

impl std::error::Error for LedInside {}

/// Whether `error`, or anything under it, is the resolver refusing a name a public address led
/// inside ([`LedInside`]).
pub fn refused_inside(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(next) = cause {
        if next.is::<LedInside>() {
            return true;
        }
        cause = next.source();
    }
    false
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
            // The answer judged is the answer connected to: the kept one, not a second lookup.
            let answer: Result<Addrs, Failure> = judged(&name, addrs)
                .map(|addrs| Box::new(addrs.into_iter()) as Addrs)
                .map_err(Failure::from);
            return Box::pin(async move { answer });
        }
        Box::pin(async move {
            let found = look_up(&name).await?;
            if !found.is_empty() {
                keep(&name, &found);
            }
            let addrs = judged(&name, found)?;
            Ok::<Addrs, Failure>(Box::new(addrs.into_iter()))
        })
    }
}

/// Why a lookup failed, as reqwest takes it.
type Failure = Box<dyn std::error::Error + Send + Sync>;

/// `name`'s addresses, from the system.
async fn look_up(name: &str) -> std::io::Result<Vec<SocketAddr>> {
    // A test's own answer for a name, in place of the system's (`tests::answers`).
    #[cfg(test)]
    let answered = tests::answer(name);
    #[cfg(not(test))]
    let answered: Option<Vec<SocketAddr>> = None;
    if let Some(found) = answered {
        return Ok(found);
    }
    // The benchmark's DNS server, for a name under `.test` (`ask`, its `slowname` case).
    if let Some(server) = ask::server_for(name) {
        return ask::ask(server, name).await;
    }
    // The port is the connector's to set, as reqwest's own resolver leaves it.
    Ok(tokio::net::lookup_host((name, 0)).await?.collect())
}

#[cfg(test)]
mod tests;
