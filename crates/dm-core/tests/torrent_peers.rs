// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's torrent engine against a made-up peer (`common/peer.rs`, gaps row H32a): what it sends,
//! and later what it does with wrong messages.

use std::net::SocketAddr;
use std::time::Duration;

use dm_core::{client, download_torrent};

mod common;
#[path = "common/peer.rs"]
mod peer;

/// A tracker on the loopback whose every announce names `peer` alone.
async fn tracker_returning(peer: SocketAddr) -> SocketAddr {
    use axum::{extract::State, routing::get, Router};
    async fn announce(State(peer): State<SocketAddr>) -> axum::body::Bytes {
        let std::net::IpAddr::V4(ip) = peer.ip() else {
            unreachable!("the made-up peer binds v4");
        };
        let mut body = b"d8:completei1e10:incompletei0e8:intervali60e5:peers6:".to_vec();
        body.extend_from_slice(&ip.octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
        body.push(b'e');
        body.into()
    }
    common::serve(
        Router::new()
            .route("/announce", get(announce))
            .with_state(peer),
    )
    .await
}

/// dm's engine, given a magnet whose tracker names the made-up peer, connects to it and greets
/// it with a handshake carrying the magnet's info hash (gaps row H32a).
#[tokio::test(flavor = "multi_thread")]
async fn a_made_up_peer_is_greeted_by_dms_engine_with_the_magnets_hash() {
    let hash = "0123456789abcdef0123456789abcdef01234567";
    let peer = peer::peer(Vec::new()).await;
    let tracker = tracker_returning(peer.addr).await;
    let dir = common::tempdir();
    let options = common::options(dir.path());
    let control = options.control.clone();
    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr=http://{tracker}/announce");
    let run =
        tokio::spawn(async move { download_torrent(&client(), &magnet, &options, None).await });
    let began = std::time::Instant::now();
    let greeting = loop {
        let got = peer.got.lock().unwrap().clone();
        if got.len() >= peer::HANDSHAKE {
            break got;
        }
        assert!(
            began.elapsed() < Duration::from_secs(20),
            "dm's engine never greeted the made-up peer"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    control.pause();
    let _ = tokio::time::timeout(Duration::from_secs(5), run).await;
    assert_eq!(greeting[0], 19);
    assert_eq!(&greeting[1..20], b"BitTorrent protocol");
    let wanted: Vec<u8> = (0..20)
        .map(|i| u8::from_str_radix(&hash[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    assert_eq!(&greeting[28..48], wanted.as_slice(), "another info hash");
}

/// A peer that sends a `ut_metadata` Data message straight after its handshake, before any
/// extended handshake, has that connection closed, and nothing else: the magnet still waits
/// for its metadata and a pause ends it as a pause, not a panic (gaps row H32b).
#[tokio::test(flavor = "multi_thread")]
async fn metadata_sent_before_the_peers_handshake_closes_that_peer_and_nothing_else() {
    let hash = "1123456789abcdef0123456789abcdef01234567";
    // An extended message (20), the id dm would give ut_metadata, a Data message for piece 0
    // of a 16 KB info dictionary, and a few bytes of it.
    let mut body = vec![20u8, 3];
    body.extend_from_slice(b"d8:msg_typei1e5:piecei0e10:total_sizei16384ee");
    body.extend_from_slice(&[0x55; 100]);
    let mut message = (body.len() as u32).to_be_bytes().to_vec();
    message.extend_from_slice(&body);
    let peer = peer::peer(vec![message]).await;
    let tracker = tracker_returning(peer.addr).await;
    let dir = common::tempdir();
    let options = common::options(dir.path());
    let control = options.control.clone();
    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr=http://{tracker}/announce");
    let run =
        tokio::spawn(async move { download_torrent(&client(), &magnet, &options, None).await });
    let began = std::time::Instant::now();
    while !peer.closed.load(std::sync::atomic::Ordering::SeqCst) {
        assert!(
            began.elapsed() < Duration::from_secs(20),
            "dm kept the peer that sent metadata unasked"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!run.is_finished(), "the magnet ended with the one bad peer");
    control.pause();
    let ended = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("the paused magnet was still running")
        .expect("the magnet's task panicked");
    assert!(
        matches!(ended, Err(dm_core::Error::Paused)),
        "{:?}",
        ended.err()
    );
}

/// `count` compact IPv4 addresses (BEP 23), none of them a peer: 127.`net`.x.y, port 9,
/// numbered from `from`.
fn addresses(net: u8, from: usize, count: usize) -> Vec<u8> {
    (from..from + count)
        .flat_map(|i| [127, net, (i / 250) as u8, (i % 250 + 1) as u8, 0, 9])
        .collect()
}

/// A PEX message's dictionary naming `addresses` under `key` ("added" or "dropped").
fn pex(key: &str, addresses: &[u8]) -> Vec<u8> {
    let mut dictionary = format!("d{}:{key}{}:", key.len(), addresses.len()).into_bytes();
    dictionary.extend_from_slice(addresses);
    dictionary.push(b'e');
    dictionary
}

/// Waits until the swarm's known peers reach `at_least`, then a little longer, and gives the
/// count then.
async fn known_after(known: &std::sync::atomic::AtomicU32, at_least: u32) -> u32 {
    let began = std::time::Instant::now();
    while known.load(std::sync::atomic::Ordering::SeqCst) < at_least {
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "the swarm knew {} peers, never {at_least}",
            known.load(std::sync::atomic::Ordering::SeqCst)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    known.load(std::sync::atomic::Ordering::SeqCst)
}

/// A peer sending PEX messages of 1,000 new addresses without end cannot grow the peers a
/// torrent knows past 5,000, and the addresses it says it dropped are never added (gaps rows
/// H10, H33, H33a): the attack refused is PEX growing the peer table until memory runs out.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_sending_endless_pex_cannot_grow_the_known_peers_past_the_cap() {
    use sha1::Digest;
    let mut info = b"d6:lengthi16384e4:name8:pex.data12:piece lengthi16384e6:pieces20:".to_vec();
    info.extend_from_slice(&[0x5a; 20]);
    info.push(b'e');
    let hash: String = sha1::Sha1::digest(&info)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let seeder = peer::seeder(info).await;
    let tracker = tracker_returning(seeder.addr).await;
    let dir = common::tempdir();
    let options = common::options(dir.path());
    let control = options.control.clone();
    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr=http://{tracker}/announce");
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let run =
        tokio::spawn(async move { download_torrent(&client(), &magnet, &options, Some(tx)).await });
    let known = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let seen = std::sync::Arc::clone(&known);
    tokio::spawn(async move {
        while let Some(update) = rx.recv().await {
            if let dm_core::Update::Swarm(swarm) = update {
                seen.store(swarm.known, std::sync::atomic::Ordering::SeqCst);
            }
        }
    });
    // A thousand dropped and a thousand added: the seeder and the thousand added are known.
    seeder
        .pex
        .send(pex("dropped", &addresses(2, 0, 1000)))
        .unwrap();
    seeder
        .pex
        .send(pex("added", &addresses(1, 0, 1000)))
        .unwrap();
    assert_eq!(
        known_after(&known, 1001).await,
        1001,
        "dropped peers were added"
    );
    // Five thousand more, past the cap.
    for batch in 1..6 {
        let message = pex("added", &addresses(1, batch * 1000, 1000));
        seeder.pex.send(message).unwrap();
    }
    let most = known_after(&known, 5000).await;
    assert_eq!(most, 5000, "the known peers grew past the cap");
    control.pause();
    let _ = tokio::time::timeout(Duration::from_secs(10), run).await;
}
