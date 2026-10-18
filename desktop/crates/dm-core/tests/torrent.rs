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

use dm_core::download::Options;
use dm_core::torrent::download_torrent;
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
        quality: dm_core::stream::Quality::Best,
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
        sign_in: None,
        system_proxy: false,
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
