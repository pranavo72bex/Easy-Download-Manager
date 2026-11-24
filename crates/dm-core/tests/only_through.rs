// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Downloads held to one network (Options › Advanced › Network › Only through, gaps row W12),
//! with a made-up adapter list standing for the computer's: a client goes out from the
//! adapter's address alone, and from one the computer lacks it reaches nothing; the adapter
//! taken away, a running download stops and waits saying why, and goes on by itself once it
//! is back. The adapter held to is the program's own, so these tests are a program of their
//! own, taking turns.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::response::Response;
use dm_core::{set_adapter_lister, set_only_through, DownloadList, Interface, State as Row};
use futures_util::StreamExt;

mod common;
use common::tempdir;

static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A second address of this computer's own to hold downloads to: another loopback address
/// where the system has the whole of 127/8 (Linux, Windows); macOS has 127.0.0.1 alone.
#[cfg(not(target_os = "macos"))]
const OURS: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
#[cfg(target_os = "macos")]
const OURS: Ipv4Addr = Ipv4Addr::LOCALHOST;

/// How big the file served is.
const SIZE: usize = 10 * 1024;

/// What the server saw: each request's address it came from.
#[derive(Clone, Default)]
struct Seen {
    from: Arc<Mutex<Vec<IpAddr>>>,
    asked: Arc<AtomicUsize>,
    /// Whether the file is now sent at once rather than trickled.
    fast: Arc<AtomicBool>,
}

/// Serves `SIZE` bytes: a hundred bytes every fifth of a second (twenty seconds in all, long
/// enough to take the network away under it) until told to send it at once.
async fn serve() -> (SocketAddr, Seen) {
    let seen = Seen::default();
    async fn handler(
        State(seen): State<Seen>,
        ConnectInfo(from): ConnectInfo<SocketAddr>,
    ) -> Response {
        seen.from.lock().unwrap().push(from.ip());
        seen.asked.fetch_add(1, Ordering::SeqCst);
        let answer = Response::builder().header("content-length", SIZE);
        if seen.fast.load(Ordering::SeqCst) {
            return answer.body(Body::from(vec![7u8; SIZE])).unwrap();
        }
        let chunks: Vec<_> = vec![7u8; SIZE]
            .chunks(100)
            .map(bytes::Bytes::copy_from_slice)
            .collect();
        let slow = futures_util::stream::iter(chunks).then(|c| async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok::<_, std::io::Error>(c)
        });
        answer.body(Body::from_stream(slow)).unwrap()
    }
    let app = axum::Router::new()
        .fallback(handler)
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    (addr, seen)
}

/// The adapter list the tests change: `dm-test0` at `address`, or none.
fn adapters(address: Option<Ipv4Addr>) -> Arc<Mutex<Vec<Interface>>> {
    let found = Arc::new(Mutex::new(Vec::new()));
    put(&found, address);
    found
}

fn put(found: &Arc<Mutex<Vec<Interface>>>, address: Option<Ipv4Addr>) {
    *found.lock().unwrap() = address
        .map(|a| Interface {
            name: "dm-test0".into(),
            address: IpAddr::V4(a),
        })
        .into_iter()
        .collect();
    // Handed again, so the adapter is looked for at once rather than at the next look's time.
    let listed = Arc::clone(found);
    set_adapter_lister(Some(Arc::new(move || listed.lock().unwrap().clone())));
}

fn let_go() {
    set_only_through(None);
    set_adapter_lister(None);
}

fn queue(dir: &std::path::Path) -> DownloadList {
    DownloadList::new(
        dm_core::client(),
        dm_core::Options {
            connections: 1,
            attempts: 0,
            ..common::options(dir)
        },
        1,
    )
}

#[tokio::test]
async fn a_client_held_to_one_address_goes_out_from_it_alone() {
    let _turn = TURN.lock().await;
    let (addr, seen) = serve().await;
    let _found = adapters(Some(OURS));
    set_only_through(Some("dm-test0"));
    let reached = dm_core::client()
        .get(format!("http://{addr}/second"))
        .send()
        .await;
    // An address this computer does not have (one of the documentation's): nothing goes out.
    set_only_through(None);
    let _gone = adapters(Some(Ipv4Addr::new(192, 0, 2, 7)));
    set_only_through(Some("dm-test0"));
    let refused = dm_core::client()
        .get(format!("http://{addr}/second"))
        .send()
        .await;
    let_go();
    assert!(reached.is_ok(), "{reached:?}");
    assert!(
        refused.is_err(),
        "reached the server from an address not this computer's"
    );
    assert_eq!(*seen.from.lock().unwrap(), vec![IpAddr::V4(OURS)]);
}

#[tokio::test]
async fn the_network_taken_away_holds_the_row_saying_why_and_back_it_goes_on() {
    let _turn = TURN.lock().await;
    let (addr, seen) = serve().await;
    let found = adapters(Some(OURS));
    set_only_through(Some("dm-test0"));
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    let running = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    // Under way over the adapter.
    for _ in 0..100 {
        if q.get(id).await.unwrap().progress.downloaded > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        q.get(id).await.unwrap().progress.downloaded > 0,
        "it never started"
    );

    // The adapter taken away: the row stops and waits for it, saying why.
    put(&found, None);
    let mut why = None;
    for _ in 0..200 {
        if let Some(retry) = q.retrying().await.get(&id) {
            why = Some(retry.2.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let why = why.expect("the row did not say it waits for the network");
    assert!(
        why.contains("dm-test0") && why.contains("only through"),
        "{why}"
    );
    assert_eq!(q.get(id).await.unwrap().state, Row::Running);
    let asked = seen.asked.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        seen.asked.load(Ordering::SeqCst),
        asked,
        "a request went out while the network was gone"
    );

    // Back: it goes on by itself, and is whole.
    seen.fast.store(true, Ordering::SeqCst);
    put(&found, Some(OURS));
    let ended = tokio::time::timeout(Duration::from_secs(30), running).await;
    let_go();
    ended
        .expect("it did not go on once the network was back")
        .unwrap();
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, Row::Completed);
    assert_eq!(row.progress.downloaded, SIZE as u64);
    assert!(
        seen.from
            .lock()
            .unwrap()
            .iter()
            .all(|from| *from == IpAddr::V4(OURS)),
        "a request came from another address"
    );
}

/// Woken from sleep, a running download asks its server again at once and goes on from what it
/// has, rather than waiting for its dead connection's time-out (gaps row K6).
#[tokio::test]
async fn woken_from_sleep_a_running_download_reconnects_at_once() {
    let _turn = TURN.lock().await;
    let (addr, seen) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/woke.bin")).await.unwrap();
    let running = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    for _ in 0..100 {
        if q.get(id).await.unwrap().progress.downloaded > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let asked = seen.asked.load(Ordering::SeqCst);
    assert!(asked >= 1, "it never started");
    seen.fast.store(true, Ordering::SeqCst);
    dm_core::woke();
    let mut whole = false;
    for _ in 0..200 {
        if q.get(id).await.unwrap().state == Row::Completed {
            whole = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    running.abort();
    assert!(
        seen.asked.load(Ordering::SeqCst) > asked,
        "it did not ask again on waking"
    );
    assert!(whole, "it did not finish once it asked again");
}
