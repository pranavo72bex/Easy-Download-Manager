// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A made-up BitTorrent peer on the loopback (gaps row H32a): it takes one connection, reads
//! dm's handshake, answers with the same info hash and the extended-messages bit, sends the
//! raw messages a test scripts, and keeps everything dm sent, so a test can send dm wrong peer
//! messages and see what it does.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The made-up peer: where it listens, and what dm sent it so far (its handshake first).
pub struct Peer {
    pub addr: SocketAddr,
    pub got: Arc<Mutex<Vec<u8>>>,
    /// dm closed the connection (or it broke).
    pub closed: Arc<std::sync::atomic::AtomicBool>,
}

/// The length of a BitTorrent handshake: 1 + 19 + 8 + 20 + 20 bytes.
pub const HANDSHAKE: usize = 68;

/// A peer that, once dm has said its handshake, answers it and sends `script`'s messages, each
/// as given (length prefix included), then reads on until dm lets go.
pub async fn peer(script: Vec<Vec<u8>>) -> Peer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let got = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&got);
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shut = Arc::clone(&closed);
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        let mut theirs = [0u8; HANDSHAKE];
        if socket.read_exact(&mut theirs).await.is_err() {
            return;
        }
        kept.lock().unwrap().extend_from_slice(&theirs);
        let mut ours = Vec::with_capacity(HANDSHAKE);
        ours.push(19);
        ours.extend_from_slice(b"BitTorrent protocol");
        // The reserved bytes, with the extension protocol's bit (BEP 10).
        ours.extend_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
        ours.extend_from_slice(&theirs[28..48]);
        ours.extend_from_slice(b"-DMTEST-000000000000");
        if socket.write_all(&ours).await.is_err() {
            return;
        }
        for message in script {
            if socket.write_all(&message).await.is_err() {
                return;
            }
        }
        let mut buf = [0u8; 4096];
        while let Ok(n) = socket.read(&mut buf).await {
            if n == 0 {
                break;
            }
            kept.lock().unwrap().extend_from_slice(&buf[..n]);
        }
        shut.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    Peer { addr, got, closed }
}
