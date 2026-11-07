// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm against another implementation of encrypted peer connections (gaps
//! row 150): a libtorrent peer, in a container, set to make and take only
//! encrypted connections (MSE, RC4). libtorrent is the library most BitTorrent
//! clients are built on. Needs Docker and the image `scripts/mse-interop.sh`
//! builds, so it runs only when asked:
//!
//!     sh scripts/mse-interop.sh
//!
//! Everything stays on this computer: the trackers are the tests' own, a
//! container's port is published on loopback only, and the sessions have no
//! DHT, local discovery or port mapping.

mod common;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{options, serve};

const IMAGE: &str = "dm-libtorrent";

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn docker(args: &[&str]) -> String {
    let out = Command::new("docker")
        .args(args)
        .output()
        .expect("docker could not be run");
    assert!(
        out.status.success(),
        "docker {}: {}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Removes the container however the test ends.
struct Container(String);

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

/// A folder the containers are given, with the file the torrent is of.
fn shared_with(data: &[u8]) -> tempfile::TempDir {
    let shared = tempfile::Builder::new()
        .prefix("dm-mse-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(shared.path().join("file.bin"), data).unwrap();
    shared
}

/// The torrent of the shared file, made by libtorrent, announcing to `announce`.
fn make_torrent(shared: &std::path::Path, announce: &str) -> Vec<u8> {
    docker(&[
        "run",
        "--rm",
        "-v",
        &format!("{}:/data", shared.display()),
        IMAGE,
        "python3",
        "/peer.py",
        "make",
        announce,
    ]);
    std::fs::read(shared.join("t.torrent")).unwrap()
}

/// Where a bencoded value starting at `at` ends.
fn bencode_end(b: &[u8], at: usize) -> usize {
    match b[at] {
        b'i' => at + b[at..].iter().position(|c| *c == b'e').unwrap() + 1,
        b'l' | b'd' => {
            let mut i = at + 1;
            while b[i] != b'e' {
                i = bencode_end(b, i);
            }
            i + 1
        }
        _ => {
            let colon = at + b[at..].iter().position(|c| *c == b':').unwrap();
            let len: usize = std::str::from_utf8(&b[at..colon]).unwrap().parse().unwrap();
            colon + 1 + len
        }
    }
}

/// The torrent's info hash: SHA-1 of its bencoded `info` dictionary.
fn info_hash(torrent: &[u8]) -> [u8; 20] {
    use sha1::Digest;
    let mut i = 1;
    while torrent[i] != b'e' {
        let key_end = bencode_end(torrent, i);
        let value_end = bencode_end(torrent, key_end);
        if &torrent[i..key_end] == b"4:info" {
            let mut out = [0u8; 20];
            out.copy_from_slice(&sha1::Sha1::digest(&torrent[key_end..value_end]));
            return out;
        }
        i = value_end;
    }
    panic!("the torrent has no info dictionary");
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

/// dm downloads from a libtorrent seeder that takes only encrypted
/// connections: a plaintext handshake with the right info hash is refused by
/// it, and dm, refused in plaintext, asks again encrypted and gets the file.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker and the dm-libtorrent image: scripts/mse-interop.sh"]
async fn a_peer_that_takes_only_encrypted_connections_seeds_to_dm() {
    let data: Vec<u8> = (0..4 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 7) as u8)
        .collect();
    let shared = shared_with(&data);
    let port = free_port();
    let tracker = tracker_returning(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await;
    let torrent = make_torrent(shared.path(), &format!("http://{tracker}/announce"));

    let name = format!("dm-mse-seed-{}", std::process::id());
    docker(&[
        "run",
        "-d",
        "--name",
        &name,
        "-p",
        &format!("127.0.0.1:{port}:6881"),
        "-v",
        &format!("{}:/data", shared.path().display()),
        IMAGE,
        "python3",
        "/peer.py",
        "seed",
    ]);
    let _container = Container(name.clone());
    let deadline = Instant::now() + Duration::from_secs(60);
    while !docker(&["logs", &name]).contains("seeding") {
        assert!(Instant::now() < deadline, "the peer never seeded");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // Plaintext, with the right info hash: refused, as encryption is required.
    let plaintext_refused = tokio::task::spawn_blocking({
        let hash = info_hash(&torrent);
        move || {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut hello = b"\x13BitTorrent protocol".to_vec();
            hello.extend_from_slice(&[0u8; 8]);
            hello.extend_from_slice(&hash);
            hello.extend_from_slice(b"-DM0000-plaintextpeer");
            hello.truncate(68);
            s.write_all(&hello).unwrap();
            let mut answer = [0u8; 68];
            let mut got = 0;
            while got < answer.len() {
                match s.read(&mut answer[got..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
            got < 68
        }
    })
    .await
    .unwrap();
    assert!(plaintext_refused, "the peer answered a plaintext handshake");

    let url = {
        use axum::{routing::get, Router};
        let t = torrent.clone();
        let at = serve(Router::new().route(
            "/t.torrent",
            get(move || {
                let t = t.clone();
                async move { axum::body::Bytes::from(t) }
            }),
        ))
        .await;
        format!("http://{at}/t.torrent")
    };
    let dir = common::tempdir();
    let done = tokio::time::timeout(
        Duration::from_secs(120),
        dm_core::download_torrent(&dm_core::client(), &url, &options(dir.path()), None),
    )
    .await
    .expect("dm never got the file from the encrypted-only peer")
    .expect("the download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), data);
    println!("VERIFY MSE interop: 4 MB whole from libtorrent taking only encrypted peers");
}

/// The other way: a libtorrent peer that makes only encrypted connections
/// fetches the file from a seeder of dm's torrent engine that takes only
/// encrypted ones. libtorrent puts its BitTorrent handshake in the encrypted
/// exchange (the initial payload), which dm's side takes.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker and the dm-libtorrent image: scripts/mse-interop.sh"]
async fn a_peer_takes_a_file_from_dms_engine_over_an_encrypted_connection() {
    use librqbit::{
        AddTorrent, AddTorrentOptions, ConnectionOptions, ListenerMode, ListenerOptions,
        PeerConnectionOptions, Session, SessionOptions,
    };
    let data: Vec<u8> = (0..3 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(40503) >> 5) as u8)
        .collect();
    let shared = shared_with(&data);
    // No tracker is needed: the peer is told where the seeder is.
    let torrent = make_torrent(shared.path(), "http://127.0.0.1:9/announce");

    // This computer as a container reaches it (Docker's gateway to the
    // host's loopback).
    let probe = docker(&[
        "run",
        "--rm",
        "--add-host",
        "dmhost:host-gateway",
        IMAGE,
        "getent",
        "ahostsv4",
        "dmhost",
    ]);
    let gateway = probe
        .split_whitespace()
        .next()
        .expect("no address for the host")
        .to_string();

    let seeder_port = free_port();
    let session = Session::new_with_opts(
        shared.path().to_path_buf(),
        SessionOptions {
            dht: None,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, seeder_port)),
                ..Default::default()
            }),
            connect: Some(ConnectionOptions {
                peer_opts: Some(PeerConnectionOptions {
                    require_encryption: true,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    session
        .add_torrent(
            AddTorrent::TorrentFileBytes(torrent.clone().into()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(shared.path().to_str().unwrap().to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let fetched = tokio::task::spawn_blocking({
        let mount = format!("{}:/data", shared.path().display());
        let port = seeder_port.to_string();
        move || {
            Command::new("docker")
                .args([
                    "run",
                    "--rm",
                    "--add-host",
                    "dmhost:host-gateway",
                    "-v",
                    &mount,
                    IMAGE,
                    "python3",
                    "/peer.py",
                    "leech",
                    &gateway,
                    &port,
                ])
                .output()
                .unwrap()
        }
    })
    .await
    .unwrap();
    let said = String::from_utf8_lossy(&fetched.stdout);
    assert!(
        fetched.status.success() && said.contains("done"),
        "the peer did not get the file: {said}{}",
        String::from_utf8_lossy(&fetched.stderr)
    );
    assert_eq!(
        std::fs::read(shared.path().join("in/file.bin")).unwrap(),
        data
    );
    drop(session);
    println!("VERIFY MSE interop: 3 MB whole to libtorrent from dm's engine, encrypted both ways");
}
