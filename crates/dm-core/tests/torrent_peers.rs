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
