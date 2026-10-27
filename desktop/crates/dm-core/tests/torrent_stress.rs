//! Torrents under load, on loopback only: parallel swarms, many files, a
//! torrent that names files outside its folder, a swarm with a piece
//! missing, a pause and a resume, a magnet with a peer and no tracker.
//!
//! Ignored by default: run by `scripts/stress-protocols.py torrent`, which
//! runs this binary inside a macOS sandbox that allows the network on
//! loopback only, so that nothing here, DHT included, can reach a peer or a
//! tracker on the public network. Each test prints `STRESS <name>: ...`.
//!
//!     cargo test -p dm-core --release --test torrent_stress -- --ignored --nocapture --test-threads=1

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dm_core::download::Options;
use dm_core::torrent::download_torrent;
use dm_core::{client, Categories, Control, Limiter};

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerMode, ListenerOptions, Session,
    SessionOptions,
};
use sha2::{Digest, Sha256};

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
        let ip = match peer.ip() {
            std::net::IpAddr::V4(v4) => v4.octets(),
            _ => unreachable!(),
        };
        let mut compact = ip.to_vec();
        compact.extend_from_slice(&peer.port().to_be_bytes());
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

fn pattern(seed: u64, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i as u64).wrapping_mul(2654435761).wrapping_add(seed * 97) >> 7) as u8)
        .collect()
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            out.push(e.path());
            if e.path().is_dir() {
                out.extend(walk(&e.path()));
            }
        }
    }
    out
}

/// A loopback seeder for `content` (a folder holding the torrent's files),
/// and the .torrent that announces to a loopback tracker for it.
struct Swarm {
    torrent: Vec<u8>,
    _seeder: std::sync::Arc<Session>,
    _keep: tempfile::TempDir,
}

async fn seed(
    content: &Path,
    name: &str,
    piece: u32,
    upload_bps: Option<std::num::NonZeroU32>,
) -> Swarm {
    let seed_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, free_port()));
    let tracker = tracker_returning(seed_addr).await;
    let torrent = librqbit::create_torrent(
        content,
        CreateTorrentOptions {
            name: Some(name),
            trackers: vec![format!("http://{tracker}/announce")],
            piece_length: Some(piece),
        },
        &BlockingSpawner::new(1),
    )
    .await
    .expect("create torrent");
    let bytes = torrent.as_bytes().expect("torrent bytes");
    let keep = tempdir();
    let seeder = Session::new_with_opts(
        content.to_path_buf(),
        SessionOptions {
            dht: None,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: seed_addr,
                ..Default::default()
            }),
            ratelimits: librqbit::limits::LimitsConfig {
                download_bps: None,
                upload_bps,
            },
            ..Default::default()
        },
    )
    .await
    .expect("seeder");
    seeder
        .add_torrent(
            AddTorrent::TorrentFileBytes(bytes.clone()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(content.to_str().unwrap().to_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("seed");
    Swarm {
        torrent: bytes.to_vec(),
        _seeder: seeder,
        _keep: keep,
    }
}

fn options(out: &Path) -> Options {
    Options {
        connections: 8,
        control: Control::new(),
        dest_dir: out.to_path_buf(),
        parts_dir: out.join("parts"),
        limiter: Limiter::unlimited(),
        categories: Categories::default(),
        checksum: None,
        quality: dm_core::stream::Quality::Best,
        sound: None,
        media: false,
        referrer: None,
        proxy: None,
        container: None,
        attempts: 3,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        room: Default::default(),
        remux: false,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
        sign_in: None,
        save_name: None,
        system_proxy: false,
    }
}

fn file_url(bytes: &[u8]) -> (tempfile::TempDir, String) {
    let dir = tempdir();
    let file = dir.path().join("t.torrent");
    std::fs::write(&file, bytes).unwrap();
    let url = dm_core::torrent::file_url(&file).unwrap();
    (dir, url)
}

fn rss_mb() -> f64 {
    memory_stats::memory_stats().map_or(0.0, |m| m.physical_mem as f64 / 1048576.0)
}

/// Six torrents of 16 MB at once, each from a seeder of its own.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a stress test: run by scripts/stress-protocols.py"]
async fn parallel_swarms() {
    let started = Instant::now();
    let mut keep = Vec::new();
    let mut jobs = Vec::new();
    for n in 0..6u64 {
        let dir = tempdir();
        let data = pattern(n, 16 * 1024 * 1024);
        std::fs::write(dir.path().join(format!("par{n}.bin")), &data).unwrap();
        let swarm = seed(dir.path(), &format!("par{n}"), 256 * 1024, None).await;
        let (tdir, url) = file_url(&swarm.torrent);
        let out = tempdir();
        let opts = options(out.path());
        let want = sha(&data);
        let name = format!("par{n}.bin");
        jobs.push(tokio::spawn(async move {
            let done = tokio::time::timeout(
                Duration::from_secs(120),
                download_torrent(&client(), &url, &opts, None),
            )
            .await;
            let verdict = match done {
                Ok(Ok(f)) => {
                    let got = std::fs::read(&f.path).unwrap_or_default();
                    if sha(&got) == want {
                        "exact".to_string()
                    } else {
                        format!("WRONG ({} bytes)", got.len())
                    }
                }
                Ok(Err(e)) => format!("failed: {e}"),
                Err(_) => "timed out at 120 s".to_string(),
            };
            let _ = name;
            (verdict, out, tdir)
        }));
        keep.push((dir, swarm));
    }
    let mut ok = 0;
    for j in jobs {
        let (v, _o, _t) = j.await.unwrap();
        if v == "exact" {
            ok += 1;
        } else {
            println!("STRESS parallel_swarms: one torrent: {v}");
        }
    }
    println!(
        "STRESS parallel_swarms: {ok} of 6 torrents (16 MB each) byte-exact in {:.1}s, rss {:.0} MB",
        started.elapsed().as_secs_f64(),
        rss_mb()
    );
}

/// One torrent of 400 small files in nested folders.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a stress test: run by scripts/stress-protocols.py"]
async fn many_files() {
    let dir = tempdir();
    let root = dir.path().join("tree");
    let mut expect = Vec::new();
    for i in 0..400u64 {
        let sub = root.join(format!("d{}", i % 20));
        std::fs::create_dir_all(&sub).unwrap();
        let data = pattern(i, 500 + (i as usize * 37) % 9000);
        std::fs::write(sub.join(format!("f{i}.bin")), &data).unwrap();
        expect.push((format!("d{}/f{i}.bin", i % 20), sha(&data)));
    }
    let swarm = seed(&root, "tree", 16 * 1024, None).await;
    let (_t, url) = file_url(&swarm.torrent);
    let out = tempdir();
    let opts = options(out.path());
    let started = Instant::now();
    let done = tokio::time::timeout(
        Duration::from_secs(180),
        download_torrent(&client(), &url, &opts, None),
    )
    .await;
    match done {
        Ok(Ok(f)) => {
            let mut wrong = 0;
            let mut missing = 0;
            for (rel, want) in &expect {
                match std::fs::read(f.path.join(rel)) {
                    Ok(b) if sha(&b) == *want => {}
                    Ok(_) => wrong += 1,
                    Err(_) => missing += 1,
                }
            }
            println!(
                "STRESS many_files: 400 files in {:.1}s: {} wrong, {} missing (placed at {})",
                started.elapsed().as_secs_f64(),
                wrong,
                missing,
                f.path.display()
            );
        }
        Ok(Err(e)) => println!("STRESS many_files: failed: {e}"),
        Err(_) => println!("STRESS many_files: timed out at 180 s"),
    }
}

/// Torrents that name files outside their folder, made by the script (the
/// piece hashes need SHA-1) into `DM_STRESS_TORRENT_DIR`: with no peers, to
/// see only where dm and the engine put each file.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a stress test: run by scripts/stress-protocols.py"]
async fn paths_outside_the_folder() {
    let Ok(dir) = std::env::var("DM_STRESS_TORRENT_DIR") else {
        println!("STRESS paths_outside_the_folder: no DM_STRESS_TORRENT_DIR");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for torrent_file in files {
        let label = torrent_file
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let url = dm_core::torrent::file_url(&torrent_file).unwrap();
        let base = tempdir();
        let out = base.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let opts = options(&out);
        let r = tokio::time::timeout(
            Duration::from_secs(12),
            download_torrent(&client(), &url, &opts, None),
        )
        .await;
        let state = match r {
            Ok(Ok(_)) => "completed".to_string(),
            Ok(Err(e)) => format!("refused: {e}"),
            Err(_) => "waiting for peers (stopped by the test)".to_string(),
        };
        let escaped: Vec<PathBuf> = walk(base.path())
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().contains("escape_torrent"))
                    && !p.starts_with(&out)
            })
            .collect();
        let abs = Path::new("/tmp/escape_torrent_abs.txt").exists();
        let inside = walk(&out)
            .iter()
            .map(|p| p.strip_prefix(&out).unwrap().display().to_string())
            .collect::<Vec<_>>();
        println!("STRESS paths_outside_the_folder: {label}: {state}; outside the folder: {escaped:?}; /tmp file: {abs}; inside: {inside:?}");
        let _ = std::fs::remove_file("/tmp/escape_torrent_abs.txt");
    }
}

/// A seeder missing a piece: the leecher must not call it finished.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a stress test: run by scripts/stress-protocols.py"]
async fn swarm_with_a_piece_missing() {
    let dir = tempdir();
    let data = pattern(99, 4 * 1024 * 1024);
    let file = dir.path().join("gap.bin");
    std::fs::write(&file, &data).unwrap();
    // The torrent is made from the whole file; then one piece of what the
    // seeder holds is damaged, so the seeder cannot offer it.
    let swarm = {
        let tmp = tempdir();
        std::fs::write(tmp.path().join("gap.bin"), &data).unwrap();
        let torrent = librqbit::create_torrent(
            tmp.path(),
            CreateTorrentOptions {
                name: Some("gap"),
                trackers: vec![],
                piece_length: Some(256 * 1024),
            },
            &BlockingSpawner::new(1),
        )
        .await
        .unwrap();
        torrent.as_bytes().unwrap().to_vec()
    };
    let mut damaged = data.clone();
    for b in &mut damaged[1024 * 1024..1024 * 1024 + 1000] {
        *b ^= 0xFF;
    }
    std::fs::write(&file, &damaged).unwrap();
    let seed_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, free_port()));
    let tracker = tracker_returning(seed_addr).await;
    // The torrent again, announcing to the tracker (same content hash).
    let torrent = {
        let tmp = tempdir();
        std::fs::write(tmp.path().join("gap.bin"), &data).unwrap();
        librqbit::create_torrent(
            tmp.path(),
            CreateTorrentOptions {
                name: Some("gap"),
                trackers: vec![format!("http://{tracker}/announce")],
                piece_length: Some(256 * 1024),
            },
            &BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap()
        .to_vec()
    };
    let _ = swarm;
    let seeder = Session::new_with_opts(
        dir.path().to_path_buf(),
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
    .unwrap();
    seeder
        .add_torrent(
            AddTorrent::TorrentFileBytes(torrent.clone().into()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(dir.path().to_str().unwrap().to_owned()),
                ..Default::default()
            }),
        )
        .await
        .expect("the seeder with a damaged piece");
    let (_t, url) = file_url(&torrent);
    let out = tempdir();
    let opts = options(out.path());
    let r = tokio::time::timeout(
        Duration::from_secs(25),
        download_torrent(&client(), &url, &opts, None),
    )
    .await;
    match r {
        Ok(Ok(f)) => {
            let got = std::fs::read(&f.path).unwrap_or_default();
            println!(
                "STRESS swarm_with_a_piece_missing: COMPLETED with {} bytes, {}",
                got.len(),
                if sha(&got) == sha(&data) {
                    "the true content"
                } else if got == damaged {
                    "THE DAMAGED CONTENT"
                } else {
                    "other content"
                }
            );
        }
        Ok(Err(e)) => println!("STRESS swarm_with_a_piece_missing: failed: {e}"),
        Err(_) => println!(
            "STRESS swarm_with_a_piece_missing: not finished after 25 s, as it should not be"
        ),
    }
}

/// Paused after three seconds against a slow seeder, then resumed.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a stress test: run by scripts/stress-protocols.py"]
async fn pause_and_resume() {
    let dir = tempdir();
    let data = pattern(5, 6 * 1024 * 1024);
    std::fs::write(dir.path().join("slow.bin"), &data).unwrap();
    let swarm = seed(
        dir.path(),
        "slow",
        128 * 1024,
        std::num::NonZeroU32::new(1024 * 1024),
    )
    .await;
    let (_t, url) = file_url(&swarm.torrent);
    let out = tempdir();
    let first = Control::new();
    let opts = Options {
        control: first.clone(),
        ..options(out.path())
    };
    let pauser = {
        let first = first.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            first.pause();
        })
    };
    let r = tokio::time::timeout(
        Duration::from_secs(60),
        download_torrent(&client(), &url, &opts, None),
    )
    .await;
    pauser.await.unwrap();
    let paused = matches!(r, Ok(Err(dm_core::Error::Paused)));
    let staged: u64 = walk(&out.path().join("parts"))
        .iter()
        .filter(|p| p.is_file())
        .filter_map(|p| p.metadata().ok())
        .map(|m| m.len())
        .sum();
    let t = Instant::now();
    let opts2 = Options {
        control: Control::new(),
        ..options(out.path())
    };
    let r2 = tokio::time::timeout(
        Duration::from_secs(90),
        download_torrent(&client(), &url, &opts2, None),
    )
    .await;
    match r2 {
        Ok(Ok(f)) => println!(
            "STRESS pause_and_resume: paused={paused} with {staged} bytes staged; resumed and finished in {:.1}s, {}",
            t.elapsed().as_secs_f64(),
            if sha(&std::fs::read(&f.path).unwrap_or_default()) == sha(&data) { "byte-exact" } else { "WRONG CONTENT" }
        ),
        Ok(Err(e)) => println!("STRESS pause_and_resume: paused={paused}; resume failed: {e}"),
        Err(_) => println!("STRESS pause_and_resume: paused={paused}; resume timed out at 90 s"),
    }
}

/// A magnet naming only a tracker: the torrent's description comes from the peer.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a stress test: run by scripts/stress-protocols.py"]
async fn magnet_through_a_tracker() {
    let dir = tempdir();
    let data = pattern(11, 2 * 1024 * 1024);
    std::fs::write(dir.path().join("mag.bin"), &data).unwrap();
    let seed_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, free_port()));
    let tracker = tracker_returning(seed_addr).await;
    let torrent = librqbit::create_torrent(
        dir.path(),
        CreateTorrentOptions {
            name: Some("mag"),
            trackers: vec![format!("http://{tracker}/announce")],
            piece_length: Some(64 * 1024),
        },
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let info_hash = torrent.info_hash().as_string();
    let bytes = torrent.as_bytes().unwrap();
    let seeder = Session::new_with_opts(
        dir.path().to_path_buf(),
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
    .unwrap();
    seeder
        .add_torrent(
            AddTorrent::TorrentFileBytes(bytes),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(dir.path().to_str().unwrap().to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    // The usual shape of a magnet: a tracker to ask, no torrent file; the
    // engine gets the torrent's description from the peer itself.
    let magnet =
        format!("magnet:?xt=urn:btih:{info_hash}&dn=mag&tr=http%3A%2F%2F{tracker}%2Fannounce");
    let out = tempdir();
    let opts = options(out.path());
    let r = tokio::time::timeout(
        Duration::from_secs(40),
        download_torrent(&client(), &magnet, &opts, None),
    )
    .await;
    match r {
        Ok(Ok(f)) => println!(
            "STRESS magnet_through_a_tracker: finished, {}",
            if sha(&std::fs::read(&f.path).unwrap_or_default()) == sha(&data) {
                "byte-exact"
            } else {
                "WRONG CONTENT"
            }
        ),
        Ok(Err(e)) => println!("STRESS magnet_through_a_tracker: failed: {e}"),
        Err(_) => println!("STRESS magnet_through_a_tracker: nothing after 40 s"),
    }
}
