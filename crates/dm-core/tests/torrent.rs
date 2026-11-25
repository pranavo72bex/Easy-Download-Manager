// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A torrent, downloaded end to end, without touching the public network.
//!
//! `torrent::is_torrent` and the queue's acceptance of a magnet are unit
//! tested; this is the part that matters and cannot be faked: that
//! `download_torrent` actually joins a swarm, fetches every piece, and writes
//! the file. It stands up the smallest real swarm there is — one seeder and
//! one tracker, both on loopback — so the bytes genuinely cross a BitTorrent
//! connection rather than being copied.
//!
//! Hermetic on purpose: the seeder's DHT is off and the leecher finds it
//! through the tracker named in the torrent, so nothing here reaches past
//! `127.0.0.1` and the test is deterministic on a machine with no internet.

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::time::Duration;

use dm_core::download_torrent;
use dm_core::Options;
use dm_core::{client, Categories, Control, Limiter};

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerMode, ListenerOptions, Session,
    SessionOptions,
};

mod common;
use common::{serve, tempdir};

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            out.push(e.path());
            if e.path().is_dir() {
                out.extend(walkdir(&e.path()));
            }
        }
    }
    out
}

/// A one-endpoint BitTorrent tracker that always answers with one peer.
///
/// A real tracker records who announced and hands the list back; this test
/// knows the seeder's address up front, so it can skip the bookkeeping and
/// return that one peer to every announce — which is all the leecher needs to
/// find it. The reply is bencoded with a compact peer list, the format
/// librqbit asks for.
async fn tracker_returning(peer: SocketAddr) -> SocketAddr {
    use axum::{extract::State, routing::get, Router};

    async fn announce(State(peer): State<SocketAddr>) -> axum::body::Bytes {
        let ip = match peer.ip() {
            std::net::IpAddr::V4(v4) => v4.octets(),
            _ => unreachable!("the test binds v4"),
        };
        let mut compact = ip.to_vec();
        compact.extend_from_slice(&peer.port().to_be_bytes());

        // d ... 5:peers 6:<ip><port> e
        let mut body = b"d8:completei1e10:incompletei0e8:intervali60e5:peers6:".to_vec();
        body.extend_from_slice(&compact);
        body.push(b'e');
        body.into()
    }

    serve(
        Router::new()
            .route("/announce", get(announce))
            .with_state(peer),
    )
    .await
}

/// A free loopback port, released before it is handed back. A seeder that
/// binds a fixed port cannot collide with another test that picked its own.
fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_is_fetched_from_a_real_swarm() {
    // Small on purpose: the point is that the transfer happens, not that it
    // is large, and the disk this runs on is not always generous.
    let payload: Vec<u8> = (0..256 * 1024).map(|i| (i * 7 % 251) as u8).collect();

    // What the seeder serves, laid out as the torrent will describe it.
    let seed_dir = tempdir();
    let content = seed_dir.path().join("payload.bin");
    std::fs::write(&content, &payload).unwrap();

    // The seeder listens here, and the tracker will name exactly this.
    let seed_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, free_port()));
    let tracker = tracker_returning(seed_addr).await;
    let announce = format!("http://{tracker}/announce");

    // The torrent, announcing to the local tracker so the leecher can find
    // the seeder without a public DHT.
    let torrent = librqbit::create_torrent(
        seed_dir.path(),
        CreateTorrentOptions {
            name: Some("payload"),
            trackers: vec![announce.clone()],
            piece_length: Some(32 * 1024),
        },
        &BlockingSpawner::new(1),
    )
    .await
    .expect("create torrent");
    let torrent_bytes = torrent.as_bytes().expect("torrent bytes");

    // The seeder: DHT off (hermetic), a fixed loopback port, seeding the file
    // already on disk.
    let seeder = Session::new_with_opts(
        seed_dir.path().to_path_buf(),
        SessionOptions {
            dht: None,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: seed_addr,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .expect("seeder session");
    seeder
        .add_torrent(
            AddTorrent::TorrentFileBytes(torrent_bytes.clone()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(seed_dir.path().to_str().unwrap().to_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("seed the torrent");

    // The leecher: dm's own `download_torrent`, fed the .torrent over HTTP the
    // way a real one arrives, saving into its own folder.
    let out = tempdir();
    let torrent_url = {
        use axum::{routing::get, Router};
        let bytes = torrent_bytes.clone();
        let addr = serve(Router::new().route(
            "/payload.torrent",
            get(move || {
                let bytes = bytes.clone();
                async move { axum::body::Bytes::from(bytes.to_vec()) }
            }),
        ))
        .await;
        format!("http://{addr}/payload.torrent")
    };

    let options = Options {
        connections: 8,
        control: Control::new(),
        dest_dir: out.path().to_path_buf(),
        parts_dir: out.path().join("parts"),
        limiter: Limiter::unlimited(),
        categories: Categories::default(),
        checksum: None,
        quality: dm_core::Quality::Best,
        sound: None,
        span: None,
        shape: None,
        mp3: false,
        gif: false,
        media: false,
        referrer: None,
        proxy: None,
        container: None,
        attempts: 5,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        room: Default::default(),
        remux: false,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
        same_files: None,
        frame_rate: None,
        caption: None,
        format: None,
        sign_in: None,
        save_name: None,
        accept_page: false,
        // On loopback only: no DHT or local discovery, which would announce
        // this computer to the public network from a test.
        torrent: dm_core::TorrentOptions {
            no_public_discovery: true,
            dht_nodes: None,
            ..Default::default()
        },
        system_proxy: false,
        title_hint: None,
    };

    let finished = tokio::time::timeout(
        Duration::from_secs(60),
        download_torrent(&client(), &torrent_url, &options, None),
    )
    .await
    .expect("the torrent download timed out")
    .unwrap_or_else(|e| {
        let tree: Vec<_> = walkdir(out.path());
        panic!("the torrent download failed: {e:?}; on disk: {tree:?}")
    });

    // One file, so it is placed as any single download is: in the download
    // folder under its own name, and the row reports exactly where.
    assert_eq!(finished.path, out.path().join("payload.bin"));
    let saved = std::fs::read(&finished.path).expect("the finished file is where it was reported");
    assert_eq!(saved, payload, "the downloaded file is not what was seeded");
    // Staged under the parts directory while it ran, like every other
    // download, and nothing of it left there once placed.
    let leftover: Vec<_> = std::fs::read_dir(out.path().join("parts"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(
        leftover.is_empty(),
        "left in the parts directory: {leftover:?}"
    );

    // The same .torrent opened from this computer, as a file manager hands
    // it over: a file:// address, read rather than fetched.
    let here = tempdir();
    let file = here.path().join("payload copy.torrent");
    std::fs::write(&file, &torrent_bytes).unwrap();
    let local = dm_core::file_url(&file).unwrap();
    assert!(dm_core::is_torrent(&local) && dm_core::accepted(&local));
    let out2 = tempdir();
    let options = Options {
        control: Control::new(),
        dest_dir: out2.path().to_path_buf(),
        parts_dir: out2.path().join("parts"),
        ..options
    };
    let finished = tokio::time::timeout(
        Duration::from_secs(60),
        download_torrent(&client(), &local, &options, None),
    )
    .await
    .expect("the local torrent timed out")
    .expect("the local torrent failed");
    assert_eq!(std::fs::read(&finished.path).unwrap(), payload);
}

#[tokio::test]
async fn a_torrent_behind_an_http_proxy_is_refused_rather_than_sent_around_it() {
    let out = tempdir();
    let options = Options {
        proxy: Some("http://proxy.example:8080".into()),
        ..common::options(out.path())
    };
    let err = download_torrent(
        &client(),
        "magnet:?xt=urn:btih:0123456789abcdef",
        &options,
        None,
    )
    .await
    .expect_err("a torrent went out around the proxy");
    assert!(err.to_string().contains("SOCKS5"), "{err}");
}

/// A magnet whose metadata never comes — its only tracker is a closed port —
/// still stops when paused. The add waits on peers with no end of its own,
/// and awaited unraced it held the row until the process ended.
#[tokio::test(flavor = "multi_thread")]
async fn a_magnet_with_no_peers_stops_when_paused() {
    let dir = tempdir();
    let options = common::options(dir.path());
    let control = options.control.clone();
    let run = tokio::spawn(async move {
        download_torrent(
            &client(),
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567\
             &tr=http://127.0.0.1:9/announce",
            &options,
            None,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    control.pause();
    let ended = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("the paused magnet was still running five seconds later")
        .unwrap();
    assert!(
        matches!(ended, Err(dm_core::Error::Paused)),
        "{:?}",
        ended.err()
    );
}

/// Runs `bytes` as a `.torrent` through a queue, giving the words its row failed with.
async fn failed_with(bytes: &[u8]) -> String {
    let dir = tempdir();
    let file = dir.path().join("crafted.torrent");
    std::fs::write(&file, bytes).unwrap();
    let options = Options {
        connections: 1,
        ..common::options(dir.path())
    };
    let queue = dm_core::DownloadList::new(client(), options, 1);
    let url = url::Url::from_file_path(&file).unwrap().to_string();
    let id = queue.add(&url).await.expect("the row");
    let running = queue.clone();
    tokio::spawn(async move { running.run().await });
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        match queue.get(id).await.unwrap().state {
            dm_core::State::Failed(why) => return why,
            other => assert!(
                std::time::Instant::now() < deadline,
                "still {other:?} after twenty seconds"
            ),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// What a torrent's info says is refused before the torrent library works from it (gaps rows
/// H10, H32): a piece length of 0 divided by zero (the row then stayed "running" for good),
/// and a 200-byte file claiming 4 GiB of one-byte pieces asked for some 34 GB of locks.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_whose_info_cannot_be_true_fails_its_row_with_why() {
    let mut zero = b"d4:infod6:lengthi10e4:name4:zero12:piece lengthi0e6:pieces20:".to_vec();
    zero.extend_from_slice(&[0u8; 20]);
    zero.extend_from_slice(b"ee");
    let why = failed_with(&zero).await;
    assert!(why.contains("no length"), "{why}");

    let mut huge =
        b"d4:infod6:lengthi4294967295e4:name4:huge12:piece lengthi1e6:pieces20:".to_vec();
    huge.extend_from_slice(&[0u8; 20]);
    huge.extend_from_slice(b"ee");
    let why = failed_with(&huge).await;
    assert!(why.contains("pieces"), "{why}");
}

/// A SOCKS5 server (RFC 1928, no sign-in) on the loopback that relays each CONNECT and keeps
/// where each went.
async fn socks5_relay() -> (
    SocketAddr,
    std::sync::Arc<std::sync::Mutex<Vec<SocketAddr>>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let went = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = std::sync::Arc::clone(&went);
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            let kept = std::sync::Arc::clone(&kept);
            tokio::spawn(async move {
                let mut head = [0u8; 2];
                client.read_exact(&mut head).await?;
                let mut methods = vec![0u8; usize::from(head[1])];
                client.read_exact(&mut methods).await?;
                client.write_all(&[5, 0]).await?;
                let mut asked = [0u8; 4];
                client.read_exact(&mut asked).await?;
                let target = match asked[3] {
                    1 => {
                        let mut to = [0u8; 6];
                        client.read_exact(&mut to).await?;
                        let ip = Ipv4Addr::new(to[0], to[1], to[2], to[3]);
                        SocketAddr::from((ip, u16::from_be_bytes([to[4], to[5]])))
                    }
                    3 => {
                        let mut length = [0u8; 1];
                        client.read_exact(&mut length).await?;
                        let mut name = vec![0u8; usize::from(length[0])];
                        client.read_exact(&mut name).await?;
                        let mut port = [0u8; 2];
                        client.read_exact(&mut port).await?;
                        let name = String::from_utf8_lossy(&name).into_owned();
                        tokio::net::lookup_host((name, u16::from_be_bytes(port)))
                            .await?
                            .find(SocketAddr::is_ipv4)
                            .ok_or_else(|| std::io::Error::other("no IPv4 address"))?
                    }
                    _ => return Err(std::io::Error::other("an address kind not relayed")),
                };
                kept.lock().unwrap().push(target);
                let mut server = tokio::net::TcpStream::connect(target).await?;
                client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
                tokio::io::copy_bidirectional(&mut client, &mut server).await?;
                Ok::<_, std::io::Error>(())
            });
        }
    });
    (addr, went)
}

/// A listener on the loopback that counts the connections it takes: each is answered with
/// `answer` (an HTTP response) where there is one, and otherwise held until it closes.
async fn counting(
    answer: Option<Vec<u8>>,
) -> (SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let came = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&came);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let answer = answer.clone();
            tokio::spawn(async move {
                let mut read = Vec::new();
                let mut buf = [0u8; 1024];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        return;
                    }
                    read.extend_from_slice(&buf[..n]);
                    if let Some(answer) = &answer {
                        if read.windows(4).any(|w| w == b"\r\n\r\n") {
                            let _ = socket.write_all(answer).await;
                            return;
                        }
                    }
                }
            });
        }
    });
    (addr, came)
}

/// A torrent behind a SOCKS5 proxy reaches its tracker and its peer only through it: every
/// connection either saw is one the proxy relayed, so a torrent cannot slip around the proxy
/// a person set (gaps row H24b).
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_behind_a_socks5_proxy_reaches_its_tracker_and_peer_only_through_it() {
    use std::sync::atomic::Ordering;
    let (peer, peer_saw) = counting(None).await;
    let std::net::IpAddr::V4(ip) = peer.ip() else {
        unreachable!("the test binds v4");
    };
    let mut body = b"d8:completei1e10:incompletei0e8:intervali60e5:peers6:".to_vec();
    body.extend_from_slice(&ip.octets());
    body.extend_from_slice(&peer.port().to_be_bytes());
    body.push(b'e');
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend_from_slice(&body);
    let (tracker, tracker_saw) = counting(Some(answer)).await;
    let (proxy, went) = socks5_relay().await;
    let out = tempdir();
    let options = Options {
        proxy: Some(format!("socks5://{proxy}")),
        ..common::options(out.path())
    };
    let control = options.control.clone();
    let magnet = format!(
        "magnet:?xt=urn:btih:2123456789abcdef0123456789abcdef01234567&tr=http://{tracker}/announce"
    );
    let run =
        tokio::spawn(async move { download_torrent(&client(), &magnet, &options, None).await });
    let began = std::time::Instant::now();
    while peer_saw.load(Ordering::SeqCst) == 0 {
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "the peer was never reached (the tracker saw {}, the proxy relayed to {:?})",
            tracker_saw.load(Ordering::SeqCst),
            went.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    control.pause();
    let _ = tokio::time::timeout(Duration::from_secs(10), run).await;
    let went = went.lock().unwrap().clone();
    let relayed = |to: SocketAddr| went.iter().filter(|w| **w == to).count();
    let (to_tracker, to_peer) = (
        tracker_saw.load(Ordering::SeqCst),
        peer_saw.load(Ordering::SeqCst),
    );
    assert!(to_tracker > 0, "the tracker was never asked");
    assert!(
        to_tracker <= relayed(tracker),
        "the tracker saw {to_tracker} connections, the proxy relayed {}",
        relayed(tracker)
    );
    assert!(
        to_peer <= relayed(peer),
        "the peer saw {to_peer} connections, the proxy relayed {}",
        relayed(peer)
    );
}
