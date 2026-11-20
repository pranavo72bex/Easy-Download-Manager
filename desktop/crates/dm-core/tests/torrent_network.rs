// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A torrent following the computer's network (gaps row N8): a finished torrent kept shared
//! announces itself to its tracker again when the computer's addresses change and when the
//! system says the network changed, and its sharing is held with the downloads (a metered
//! connection) and announced afresh when the hold ends. On loopback, against a tracker that
//! counts what it is told; a binary of its own, as the adapter list it changes is the
//! process's.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dm_core::{client, download_torrent, set_adapter_lister, DownloadList, Hold, Interface};
use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerMode, ListenerOptions, Session,
    SessionOptions,
};

mod common;
use common::{serve, tempdir};

/// The port of every announce the tracker was sent, in order.
type Announces = Arc<Mutex<Vec<u16>>>;

/// A tracker that names `peer` and keeps the port each announce gives.
async fn counting_tracker(peer: SocketAddr, seen: Announces) -> SocketAddr {
    use axum::{extract::RawQuery, routing::get, Router};
    let handler = move |RawQuery(query): RawQuery| {
        let seen = Arc::clone(&seen);
        async move {
            let port = query
                .unwrap_or_default()
                .split('&')
                .find_map(|pair| pair.strip_prefix("port="))
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);
            seen.lock().unwrap().push(port);
            let IpAddr::V4(v4) = peer.ip() else {
                unreachable!()
            };
            // An interval of an hour: an announce within the test is one dm asked for.
            let mut body = b"d8:completei1e10:incompletei0e8:intervali3600e5:peers6:".to_vec();
            body.extend_from_slice(&v4.octets());
            body.extend_from_slice(&peer.port().to_be_bytes());
            body.push(b'e');
            axum::body::Bytes::from(body)
        }
    };
    serve(Router::new().route("/announce", get(handler))).await
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The adapter list the process sees from now: one adapter with `address`.
fn adapters(address: Ipv4Addr) {
    let list = vec![Interface {
        name: "net0".into(),
        address: IpAddr::V4(address),
    }];
    set_adapter_lister(Some(Arc::new(move || list.clone())));
}

/// How many announces gave `port`.
fn count(seen: &Announces, port: u16) -> usize {
    seen.lock().unwrap().iter().filter(|p| **p == port).count()
}

/// Waits up to `secs` seconds for more than `than` announces giving `port`.
async fn more_than(seen: &Announces, port: u16, than: usize, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if count(seen, port) > than {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shared_torrent_announces_again_when_the_network_changes_and_waits_out_a_hold() {
    adapters(Ipv4Addr::new(10, 1, 1, 1));
    let _ = dm_core::network_moves();

    // A seeder on loopback, named by the tracker.
    let data: Vec<u8> = (0..200 * 1024u32)
        .map(|i| i.wrapping_mul(2654435761) as u8)
        .collect();
    let seed = tempdir();
    std::fs::write(seed.path().join("moves.bin"), &data).unwrap();
    let seeder_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, free_port()));
    let seeder = Session::new_with_opts(
        seed.path().to_path_buf(),
        SessionOptions {
            dht: None,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: seeder_addr,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let seen: Announces = Arc::default();
    let tracker = counting_tracker(seeder_addr, Arc::clone(&seen)).await;
    let torrent = librqbit::create_torrent(
        &seed.path().join("moves.bin"),
        CreateTorrentOptions {
            name: Some("moves.bin"),
            trackers: vec![format!("http://{tracker}/announce")],
            piece_length: Some(32 * 1024),
        },
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let bytes = torrent.as_bytes().unwrap();
    seeder
        .add_torrent(
            AddTorrent::TorrentFileBytes(bytes.clone()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(seed.path().to_str().unwrap().to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let file_url = {
        use axum::{routing::get, Router};
        let a = serve(Router::new().route(
            "/t.torrent",
            get(move || {
                let b = bytes.clone();
                async move { axum::body::Bytes::from(b.to_vec()) }
            }),
        ))
        .await;
        format!("http://{a}/t.torrent")
    };

    // Fetched, then kept shared for long enough: nobody fetches it from dm here.
    let dir = tempdir();
    let mut opts = common::options(dir.path());
    opts.torrent.share = Some(dm_core::Share {
        ratio: 1.0,
        minutes: 5,
        upload_kbps: 0,
    });
    let done = download_torrent(&client(), &file_url, &opts, None)
        .await
        .expect("the torrent did not finish");
    let mut port = None;
    for _ in 0..100 {
        port = dm_core::sharing(&done.path)
            .and_then(|s| s.listening)
            .map(|a| a.port());
        if port.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let port = port.expect("the finished torrent is not shared");
    assert!(
        more_than(&seen, port, 0, 10).await,
        "the shared torrent never announced itself"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Another address: found at the next look, and announced on.
    let before = count(&seen, port);
    adapters(Ipv4Addr::new(10, 2, 2, 2));
    assert!(
        more_than(&seen, port, before, 10).await,
        "no announce after the computer's address changed"
    );

    // The system saying so first (a phone's): announced on at once.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let before = count(&seen, port);
    dm_core::network_changed();
    assert!(
        more_than(&seen, port, before, 5).await,
        "no announce after the system said the network changed"
    );

    // Held with the downloads: paused, so a change of network announces nothing; the hold
    // lifted, it shares again and announces itself.
    let queue = DownloadList::new(client(), common::options(dir.path()), 1);
    queue.hold_for(Some(Hold::Metered));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let before = count(&seen, port);
    dm_core::network_changed();
    assert!(
        !more_than(&seen, port, before, 2).await,
        "a held torrent announced itself"
    );
    assert!(
        dm_core::sharing(&done.path).is_some(),
        "a hold ended the sharing rather than holding it"
    );
    queue.hold_for(None);
    assert!(
        more_than(&seen, port, before, 5).await,
        "the sharing did not go on once the hold ended"
    );

    dm_core::stop_sharing(&done.path);
    set_adapter_lister(None);
    seeder.stop().await;
}
