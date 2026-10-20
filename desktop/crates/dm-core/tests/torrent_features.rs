//! Every torrent feature against a swarm on loopback: magnets, a torrent of
//! several files, checksums, progress and the speed limit, pause and resume,
//! category folders, the queue, and two torrents at once.

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dm_core::download::{Options, Update};
use dm_core::torrent::download_torrent;
use dm_core::{client, Categories, Control, Limiter, Queue, State};
use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerMode, ListenerOptions, Session,
    SessionOptions,
};

mod common;
use common::{serve, tempdir};

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn tracker_returning(peer: SocketAddr) -> SocketAddr {
    use axum::{extract::State, routing::get, Router};
    async fn announce(State(peer): State<SocketAddr>) -> axum::body::Bytes {
        let std::net::IpAddr::V4(v4) = peer.ip() else {
            unreachable!()
        };
        let mut body = b"d8:completei1e10:incompletei0e8:intervali60e5:peers6:".to_vec();
        body.extend_from_slice(&v4.octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
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

struct Swarm {
    _seed: tempfile::TempDir,
    _session: std::sync::Arc<Session>,
    torrent_url: String,
    magnet: String,
}

/// A seeder and tracker on loopback serving `files` under `name`.
async fn swarm(name: &str, files: &[(&str, Vec<u8>)]) -> Swarm {
    let seed = tempdir();
    let root = if files.len() == 1 {
        seed.path().to_path_buf()
    } else {
        seed.path().join(name)
    };
    std::fs::create_dir_all(&root).unwrap();
    for (f, bytes) in files {
        std::fs::write(root.join(f), bytes).unwrap();
    }
    let source = if files.len() == 1 {
        root.join(files[0].0)
    } else {
        root.clone()
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, free_port()));
    let tracker = tracker_returning(addr).await;
    let announce = format!("http://{tracker}/announce");
    let torrent = librqbit::create_torrent(
        &source,
        CreateTorrentOptions {
            name: Some(name),
            trackers: vec![announce.clone()],
            piece_length: Some(32 * 1024),
        },
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let bytes = torrent.as_bytes().unwrap();
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&dn={name}&tr={}",
        torrent.info_hash().as_string(),
        urlencode(&announce)
    );
    let session = Session::new_with_opts(
        seed.path().to_path_buf(),
        SessionOptions {
            dht: None,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: addr,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let out_folder = if files.len() == 1 {
        seed.path().to_path_buf()
    } else {
        root.clone()
    };
    session
        .add_torrent(
            AddTorrent::TorrentFileBytes(bytes.clone()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(out_folder.to_str().unwrap().to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let url = {
        use axum::{routing::get, Router};
        let b = bytes.clone();
        let a = serve(Router::new().route(
            "/t.torrent",
            get(move || {
                let b = b.clone();
                async move { axum::body::Bytes::from(b.to_vec()) }
            }),
        ))
        .await;
        format!("http://{a}/t.torrent")
    };
    Swarm {
        _seed: seed,
        _session: session,
        torrent_url: url,
        magnet,
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn options(dir: &Path) -> Options {
    Options {
        connections: 8,
        control: Control::new(),
        dest_dir: dir.join("out"),
        parts_dir: dir.join("parts"),
        limiter: Limiter::unlimited(),
        categories: Categories::default(),
        checksum: None,
        quality: dm_core::stream::Quality::Best,
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
        sign_in: None,
        save_name: None,
        system_proxy: false,
    }
}

fn bytes(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| {
            (i as u32)
                .wrapping_mul(2654435761)
                .wrapping_add(seed as u32) as u8
        })
        .collect()
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.extend(files_under(&e.path()))
            } else {
                out.push(e.path())
            }
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn magnet_link_downloads_from_the_swarm() {
    let data = bytes(512 * 1024, 1);
    let s = swarm("magnet.bin", &[("magnet.bin", data.clone())]).await;
    let dir = tempdir();
    let done = tokio::time::timeout(
        Duration::from_secs(60),
        download_torrent(&client(), &s.magnet, &options(dir.path()), None),
    )
    .await
    .expect("timeout")
    .expect("magnet failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    println!("VERIFY magnet: {} byte-exact", done.path.display());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_of_several_files_lands_as_a_folder() {
    let a = bytes(300 * 1024, 2);
    let b = bytes(70 * 1024, 3);
    let s = swarm("album", &[("one.bin", a.clone()), ("two.bin", b.clone())]).await;
    let dir = tempdir();
    let done = tokio::time::timeout(
        Duration::from_secs(60),
        download_torrent(&client(), &s.torrent_url, &options(dir.path()), None),
    )
    .await
    .expect("timeout")
    .expect("multi-file failed");
    assert!(done.path.is_dir(), "{:?}", done.path);
    assert_eq!(std::fs::read(done.path.join("one.bin")).unwrap(), a);
    assert_eq!(std::fs::read(done.path.join("two.bin")).unwrap(), b);
    assert!(
        files_under(&dir.path().join("parts")).is_empty(),
        "staging left behind"
    );
    println!(
        "VERIFY multi-file: folder {} with both files byte-exact",
        done.path.display()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_checksum_is_checked_and_a_wrong_one_fails_and_removes() {
    use sha2::Digest;
    let data = bytes(200 * 1024, 4);
    let s = swarm("sum.bin", &[("sum.bin", data.clone())]).await;
    let good: String = sha2::Sha256::digest(&data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let dir = tempdir();
    let mut o = options(dir.path());
    o.checksum = dm_core::Checksum::parse(&good);
    let done = download_torrent(&client(), &s.torrent_url, &o, None)
        .await
        .expect("good sum failed");
    assert!(done.verified);
    let dir2 = tempdir();
    let mut o = options(dir2.path());
    o.checksum = dm_core::Checksum::parse(&"0".repeat(64));
    let err = download_torrent(&client(), &s.torrent_url, &o, None)
        .await
        .expect_err("a wrong sum passed");
    assert!(
        files_under(&dir2.path().join("out")).is_empty(),
        "the wrong file was kept"
    );
    println!(
        "VERIFY checksum: right one verified; wrong one failed ({err}) and the file was removed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_checksum_on_several_files_fails_rather_than_going_unchecked() {
    let s = swarm(
        "tree",
        &[("a.bin", bytes(40_000, 5)), ("b.bin", bytes(40_000, 6))],
    )
    .await;
    let dir = tempdir();
    let mut o = options(dir.path());
    o.checksum = dm_core::Checksum::parse(&"a".repeat(64));
    let err = download_torrent(&client(), &s.torrent_url, &o, None)
        .await
        .expect_err("unchecked");
    // Refused before the tree was fetched, and nothing of it left staged
    // for a Retry that could only fail again (torrent check, 2026-10-19).
    fn fetched(dir: &std::path::Path) -> u64 {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| {
                        let path = e.path();
                        if path.is_dir() {
                            fetched(&path)
                        } else {
                            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    assert_eq!(
        fetched(dir.path()),
        0,
        "the tree was fetched before refusing"
    );
    println!("VERIFY checksum on a tree: refused ({err})");
}

#[tokio::test(flavor = "multi_thread")]
async fn progress_is_reported_and_the_speed_limit_holds() {
    let data = bytes(4 * 1024 * 1024, 7);
    let s = swarm("slow.bin", &[("slow.bin", data.clone())]).await;
    let dir = tempdir();
    let mut o = options(dir.path());
    o.limiter = Limiter::new(512 * 1024);
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let started = Instant::now();
    let run = tokio::spawn({
        let url = s.torrent_url.clone();
        async move { download_torrent(&client(), &url, &o, Some(tx)).await }
    });
    let (mut named, mut samples, mut last) = (None, 0, 0u64);
    while let Some(u) = rx.recv().await {
        match u {
            Update::Named { name, .. } => named = Some(name),
            Update::Progress(p) => {
                samples += 1;
                assert!(p.downloaded >= last, "progress went back");
                last = p.downloaded;
            }
            _ => {}
        }
    }
    let done = run.await.unwrap().expect("limited torrent failed");
    let took = started.elapsed();
    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    assert_eq!(named.as_deref(), Some("slow.bin"));
    assert!(samples >= 3, "only {samples} progress samples");
    // About seven seconds, a first second's allowance aside. Set afresh
    // twice a second, the engine's limit ran at three times its share and
    // this took under three (torrent check, 2026-10-19); held to half the
    // ceiling with nothing else downloading, it took about fifteen.
    assert!(
        took >= Duration::from_secs(6),
        "4 MB at 512 KB/s took only {took:?}"
    );
    assert!(
        took <= Duration::from_secs(12),
        "4 MB at 512 KB/s took {took:?}: held below the ceiling"
    );
    println!("VERIFY progress+limit: named, {samples} samples, 4 MB at 512 KB/s took {took:.1?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_torrent_resumes_from_its_files_on_disk() {
    let data = bytes(3 * 1024 * 1024, 8);
    let s = swarm("pause.bin", &[("pause.bin", data.clone())]).await;
    let dir = tempdir();
    let mut o = options(dir.path());
    o.limiter = Limiter::new(1024 * 1024);
    let control = o.control.clone();
    let first = tokio::spawn({
        let url = s.torrent_url.clone();
        let o = o.clone();
        async move { download_torrent(&client(), &url, &o, None).await }
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    control.pause();
    let paused = first.await.unwrap();
    assert!(matches!(paused, Err(dm_core::Error::Paused)), "{paused:?}");
    let staged: u64 = files_under(&dir.path().join("parts"))
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();
    assert!(
        files_under(&dir.path().join("out")).is_empty(),
        "a half file sits in the download folder"
    );
    let mut o2 = options(dir.path());
    o2.limiter = Limiter::unlimited();
    let started = Instant::now();
    let done = download_torrent(&client(), &s.torrent_url, &o2, None)
        .await
        .expect("resume failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    println!("VERIFY pause/resume: paused with {staged} bytes staged (nothing in the download folder), resumed and finished byte-exact in {:.1?}", started.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_single_file_torrent_is_sorted_into_its_category_folder() {
    let data = bytes(100 * 1024, 9);
    let s = swarm("clip.mp4", &[("clip.mp4", data.clone())]).await;
    let dir = tempdir();
    let mut o = options(dir.path());
    o.categories = Categories {
        enabled: true,
        ..Categories::default()
    };
    let done = download_torrent(&client(), &s.torrent_url, &o, None)
        .await
        .expect("sorted failed");
    assert_eq!(
        done.path,
        dir.path().join("out").join("Videos").join("clip.mp4"),
        "{:?}",
        done.path
    );
    println!("VERIFY category: {}", done.path.display());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_magnet_through_the_queue_completes_and_removing_a_paused_one_clears_its_files() {
    let data = bytes(256 * 1024, 10);
    let s = swarm("queued.bin", &[("queued.bin", data.clone())]).await;
    let dir = tempdir();
    let q = Queue::new(client(), options(dir.path()), 2);
    let id = q.add(&s.magnet).await.expect("the queue refused a magnet");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        q.run().await;
        let row = q.get(id).await.unwrap();
        if row.state == State::Completed {
            assert_eq!(std::fs::read(&row.path).unwrap(), data);
            break;
        }
        assert!(Instant::now() < deadline, "row stuck at {:?}", row.state);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!(
        "VERIFY queue: magnet row completed with progress {:?}",
        q.get(id).await.unwrap().progress.downloaded
    );

    // A big one, paused part-way, then removed: its staged files go.
    let big = bytes(4 * 1024 * 1024, 11);
    let s2 = swarm("big.bin", &[("big.bin", big)]).await;
    q.set_speed_limit(512 * 1024).await;
    let id2 = q.add(&s2.torrent_url).await.unwrap();
    let runner = tokio::spawn({
        let q = q.clone();
        async move { q.run().await }
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    q.pause(id2).await;
    let _ = runner.await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let staged = dir.path().join("parts").join(id2.to_string());
    let before = files_under(&staged).len();
    q.remove(id2).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        files_under(&staged).is_empty(),
        "staged torrent files left after remove"
    );
    println!(
        "VERIFY remove: paused torrent had {before} staged file(s); removing the row cleared them"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_torrents_download_at_the_same_time() {
    let a = bytes(400 * 1024, 20);
    let b = bytes(400 * 1024, 21);
    let sa = swarm("a.bin", &[("a.bin", a.clone())]).await;
    let sb = swarm("b.bin", &[("b.bin", b.clone())]).await;
    let (da, db) = (tempdir(), tempdir());
    let (oa, ob) = (options(da.path()), options(db.path()));
    let (ca, cb) = (client(), client());
    let (ra, rb) = tokio::join!(
        download_torrent(&ca, &sa.magnet, &oa, None),
        download_torrent(&cb, &sb.torrent_url, &ob, None)
    );
    assert_eq!(std::fs::read(ra.expect("first failed").path).unwrap(), a);
    assert_eq!(std::fs::read(rb.expect("second failed").path).unwrap(), b);
    println!("VERIFY two at once: both finished byte-exact");
}
