// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Downloads held to one network (Options › Advanced › Network › Only through, gaps row
//! W12): a VPN's adapter, say, so that when the VPN drops a download does not go on over the
//! plain line.
//!
//! Every client dm builds while it is chosen is bound to the adapter's address
//! ([`crate::with_tls`], which every client is built through, asks [`bind`]), as the second
//! path's client is (`second_path.rs`), and the sockets dm opens itself for FTP connect from
//! it ([`connect_tcp`]); a torrent's sockets are bound to the adapter by name (librqbit's
//! `bind_device_name`, gaps row H23) on macOS, Linux and Android, and on Windows, where the
//! library cannot, a torrent refuses to start while it is chosen, as a live stream does
//! everywhere (dm-core's `torrent` and `live`). Where the adapter
//! has no address (the VPN is down) a client is still bound to the last address it had, or
//! to one no computer has, so nothing can leave by another route; a download waits for it
//! before it goes out ([`before_download`]), saying why on its row, and goes on by itself
//! when the address is back. The adapter is looked for again every few seconds, as the
//! second network's list is read again.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::second_path::Interface;
use crate::{Control, Error};

/// What lists the computer's adapters: the system's (`second_path::interfaces`), or a test's.
pub type Lister = Arc<dyn Fn() -> Vec<Interface> + Send + Sync>;

/// The adapter downloads are held to, as last looked at.
struct Held {
    /// Its name, as the adapter list gives it.
    name: String,
    /// Its address at the last look; `None` while it has none.
    address: Option<IpAddr>,
    /// The last address it had, bound to while it has none: a socket bound to an address
    /// this computer no longer has cannot connect at all.
    last: Option<IpAddr>,
    /// When it was last looked for; `None` to look at the next chance.
    looked: Option<Instant>,
}

static HELD: Mutex<Option<Held>> = Mutex::new(None);

/// A test's own adapter list, in place of the system's.
static LISTER: Mutex<Option<Lister>> = Mutex::new(None);

/// Bumped each time the address clients are bound to changes, so a client built before is
/// built again ([`generation`]).
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Bumped each time what is running must start again: the adapter chosen anew (what runs
/// was started unbound, or bound to another), or found gone (what runs over it waits for it).
/// The queue stops its running rows and puts them back in line ([`restarts`]).
static RESTARTS: AtomicU64 = AtomicU64::new(0);

/// Every adapter's address at the last look, and when that was (`None`: look at the next
/// chance), for [`moves`].
struct Seen {
    looked: Option<Instant>,
    addresses: Vec<IpAddr>,
}

static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

/// Bumped each time the computer's network changed, held to an adapter or not ([`moves`]).
static MOVES: AtomicU64 = AtomicU64::new(0);

/// How often the adapter is looked for again.
pub const LOOK_EVERY: Duration = Duration::from_secs(2);

/// How often a download waiting for the adapter looks at its own control.
const STOP_LOOK: Duration = Duration::from_millis(200);

/// What a client is bound to while the adapter has never had an address since dm started:
/// one of the documentation's addresses (RFC 5737), which no computer has, so a connection
/// from it fails rather than goes out by another route.
const NOWHERE: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));

/// The words a download waiting for the adapter says on its row (dm-desktop-base's `REASONS`
/// knows them by "that downloads go only through").
pub fn waiting_words(name: &str) -> String {
    format!("the network {name} that downloads go only through is not connected")
}

/// Why `what` (a torrent, a live stream), whose sockets dm cannot bind, is not started while
/// downloads are held to the network `name`.
pub fn refused_words(what: &str, name: &str) -> String {
    format!(
        "{what} cannot be held to one network, and downloads go only through {name}, so dm \
         did not start it: choose Any network under Options › Advanced › Network to fetch it"
    )
}

fn adapters() -> Vec<Interface> {
    let lister = crate::lock(&LISTER).clone();
    match lister {
        Some(list) => list(),
        None => crate::second_path::interfaces(),
    }
}

/// The adapter's address in `found`, where it is there.
fn address_of(found: &[Interface], name: &str) -> Option<IpAddr> {
    found.iter().find(|i| i.name == name).map(|i| i.address)
}

/// Holds downloads to the adapter named, or (`None`, or an empty name) lets them go by any
/// network. Put in force by dm-core's `configure`; called alone by tests. An adapter with no
/// address now is held to all the same: its downloads wait for it.
#[doc(hidden)]
pub fn set_only_through(name: Option<&str>) {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let mut held = crate::lock(&HELD);
    if held.as_ref().map(|h| h.name.as_str()) == name {
        return;
    }
    *held = name.map(|name| {
        let address = address_of(&adapters(), name);
        Held {
            name: name.to_string(),
            address,
            last: address,
            looked: Some(Instant::now()),
        }
    });
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if held.is_some() {
        RESTARTS.fetch_add(1, Ordering::SeqCst);
    }
}

/// A test's own adapter list (`None`: the system's again), looked at from the next look on.
#[doc(hidden)]
pub fn set_lister(lister: Option<Lister>) {
    *crate::lock(&LISTER) = lister;
    if let Some(held) = crate::lock(&HELD).as_mut() {
        held.looked = None;
    }
    if let Some(seen) = crate::lock(&SEEN).as_mut() {
        seen.looked = None;
    }
}

/// Says the computer's network changed, where the system says so before the next look finds
/// it (a phone's, moving between Wi-Fi and mobile data): [`moves`] changes at once.
pub fn network_changed() {
    MOVES.fetch_add(1, Ordering::SeqCst);
}

/// Says the computer woke from sleep (gaps row K6): every running download's connections died
/// while it slept and would be found dead only by their time-outs, so what runs is stopped and
/// put back in line at once, each to go on from its ledger, as a new network's restart does.
pub fn woke() {
    MOVES.fetch_add(1, Ordering::SeqCst);
    RESTARTS.fetch_add(1, Ordering::SeqCst);
}

/// How much more wall-clock time than a 5 s wait may pass before it counts as a sleep.
const SLEPT_PAST: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether a wait of `waited` by the monotonic clock took `passed` by the wall clock long
/// enough that the computer slept between (the monotonic clock stops while it sleeps).
pub fn slept(waited: std::time::Duration, passed: std::time::Duration) -> bool {
    passed > waited + SLEPT_PAST
}

/// Watches for the computer's sleep from a thread of its own, once a process (gaps row K6):
/// it waits 5 s at a time and calls [`woke`] where far more wall time passed. The same on
/// every system, with no system notice to subscribe to.
pub fn watch_sleep() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("dm-sleep-watch".into())
            .spawn(|| loop {
                let wait = std::time::Duration::from_secs(5);
                let (began, wall) = (Instant::now(), std::time::SystemTime::now());
                std::thread::sleep(wait);
                let waited = began.elapsed();
                let passed = wall.elapsed().unwrap_or_default();
                if slept(waited, passed) {
                    woke();
                }
            });
    });
}

/// Changes each time the computer's network does (gaps row N8): an adapter's address came,
/// went or changed (looked at, at most every [`LOOK_EVERY`]), the system said so
/// ([`network_changed`]), or the adapter downloads are held to was chosen anew or has a new
/// address ([`generation`]). What keeps sockets of its own, a torrent's session, announces
/// itself again on the new network when this changes.
pub fn moves() -> u64 {
    {
        let mut seen = crate::lock(&SEEN);
        let due = seen
            .as_ref()
            .is_none_or(|s| s.looked.is_none_or(|at| at.elapsed() >= LOOK_EVERY));
        if due {
            let mut now: Vec<IpAddr> = adapters().into_iter().map(|i| i.address).collect();
            now.sort();
            now.dedup();
            if seen.as_ref().is_some_and(|s| s.addresses != now) {
                MOVES.fetch_add(1, Ordering::SeqCst);
            }
            *seen = Some(Seen {
                looked: Some(Instant::now()),
                addresses: now,
            });
        }
    }
    MOVES
        .load(Ordering::SeqCst)
        .wrapping_add(GENERATION.load(Ordering::SeqCst))
}

/// The adapter downloads are held to, where one is.
pub fn only_through() -> Option<String> {
    crate::lock(&HELD).as_ref().map(|h| h.name.clone())
}

/// Changes each time the address clients are bound to changes: a client built at another
/// generation is to be built again.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// How many times what runs has had to start again (the adapter chosen anew, or found gone):
/// the queue stops what was running when this changes and puts it back in line, where it
/// goes out bound to the adapter, or waits for it.
pub fn restarts() -> u64 {
    RESTARTS.load(Ordering::SeqCst)
}

/// The address every connection goes out from while downloads are held to an adapter: its
/// own, the last it had while it has none, or one no computer has. `None` while they are not.
pub fn bound_to() -> Option<IpAddr> {
    let held = crate::lock(&HELD);
    let held = held.as_ref()?;
    Some(held.address.or(held.last).unwrap_or(NOWHERE))
}

/// `builder`, bound to the adapter downloads are held to, where they are.
pub fn bind(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    match bound_to() {
        Some(address) => builder.local_address(address),
        None => builder,
    }
}

/// Looks for the adapter again, at most every [`LOOK_EVERY`]: why downloads wait for it,
/// in words, while it has no address; `None` while it has one or none is chosen.
pub fn look() -> Option<String> {
    let mut held = crate::lock(&HELD);
    let found = held.as_mut()?;
    if found.looked.is_none_or(|at| at.elapsed() >= LOOK_EVERY) {
        found.looked = Some(Instant::now());
        let now = address_of(&adapters(), &found.name);
        if now != found.address {
            if now.is_none() {
                RESTARTS.fetch_add(1, Ordering::SeqCst);
            }
            found.address = now;
            if now.is_some() && now != found.last {
                found.last = now;
                GENERATION.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    found.address.is_none().then(|| waiting_words(&found.name))
}

/// Waits, before a download goes out, for the adapter downloads are held to: `Ok` at once
/// where none is chosen or it has an address; else `waiting` hears why (and the how-many-th
/// look this is) every [`LOOK_EVERY`] until it has one, and `Paused` or `Cancelled` comes
/// back as soon as the download is asked to stop.
pub async fn before_download(
    control: &Control,
    waiting: impl Fn(u32, String, Duration) + Send,
) -> Result<(), Error> {
    let mut looks = 0u32;
    while let Some(why) = look() {
        looks = looks.saturating_add(1);
        waiting(looks, why, LOOK_EVERY);
        let until = Instant::now() + LOOK_EVERY;
        while Instant::now() < until {
            control.check()?;
            tokio::time::sleep(STOP_LOOK).await;
        }
    }
    Ok(())
}

/// A TCP connection to `host:port` from the adapter downloads are held to, where they are;
/// otherwise from any. For the sockets dm opens itself (FTP's and a proxy's for FTP, RTMP's,
/// RTSP's), the name looked up and judged as an HTTP client's is (`dns::addresses`, gaps row
/// H38).
pub async fn connect_tcp(host: &str, port: u16) -> std::io::Result<tokio::net::TcpStream> {
    let to = crate::dns::addresses(host, port).await?;
    let Some(from) = bound_to() else {
        return tokio::net::TcpStream::connect(to.as_slice()).await;
    };
    let mut last = None;
    for to in to {
        // From the adapter's own address only: one of the other family cannot be reached
        // from it, and is not tried by another route.
        if to.is_ipv4() != from.is_ipv4() {
            continue;
        }
        let socket = if from.is_ipv4() {
            tokio::net::TcpSocket::new_v4()?
        } else {
            tokio::net::TcpSocket::new_v6()?
        };
        let made = match socket.bind(SocketAddr::new(from, 0)) {
            Ok(()) => socket.connect(to).await,
            Err(e) => Err(e),
        };
        match made {
            Ok(stream) => return Ok(stream),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("{host} has no address the network chosen can reach"),
        )
    }))
}

#[cfg(test)]
mod sleep_tests {
    use std::time::Duration;

    /// A 5 s wait that took the wall clock minutes was a sleep; one a little late was not.
    #[test]
    fn a_wait_the_wall_clock_overran_by_far_was_a_sleep() {
        let waited = Duration::from_secs(5);
        assert!(super::slept(waited, Duration::from_secs(600)));
        assert!(!super::slept(waited, Duration::from_secs(6)));
        assert!(!super::slept(waited, Duration::from_secs(34)));
    }
}
