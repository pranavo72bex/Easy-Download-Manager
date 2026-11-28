// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A made-up BitTorrent peer on the loopback (gaps row H32a): it takes one connection, reads
//! EDM's handshake, answers with the same info hash and the extended-messages bit, sends the
//! raw messages a test scripts, and keeps everything EDM sent, so a test can send EDM wrong peer
//! messages and see what it does.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The made-up peer: where it listens, and what EDM sent it so far (its handshake first).
pub struct Peer {
    pub addr: SocketAddr,
    pub got: Arc<Mutex<Vec<u8>>>,
    /// EDM closed the connection (or it broke).
    pub closed: Arc<std::sync::atomic::AtomicBool>,
}

/// The length of a BitTorrent handshake: 1 + 19 + 8 + 20 + 20 bytes.
pub const HANDSHAKE: usize = 68;

/// A peer that, once EDM has said its handshake, answers it and sends `script`'s messages, each
/// as given (length prefix included), then reads on until EDM lets go.
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

/// The id the made-up seeder gives `ut_metadata` in its extended handshake: EDM asks for the
/// metadata's pieces by it.
const OUR_METADATA_ID: u8 = 3;

/// A made-up seeder of one torrent's metadata (gaps row H33a): it takes every connection,
/// offers `ut_metadata` and `ut_pex` in its extended handshake, and hands `info` over to EDM
/// when asked. A swarm is counted only once a torrent is live, after its metadata; on a
/// connection that opens after the metadata was handed over (the live torrent's), it sends EDM
/// each PEX message's dictionary given to `pex`, under EDM's own id for `ut_pex`.
pub struct Seeder {
    pub addr: SocketAddr,
    pub pex: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

/// The seeder of `info`, a bencoded info dictionary.
pub async fn seeder(info: Vec<u8>) -> Seeder {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (pex, waiting) = tokio::sync::mpsc::unbounded_channel();
    let waiting = Arc::new(tokio::sync::Mutex::new(waiting));
    let info = Arc::new(info);
    let served = Arc::new(std::sync::atomic::AtomicBool::new(false));
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let live = served.load(std::sync::atomic::Ordering::SeqCst);
            let (info, served, waiting) =
                (Arc::clone(&info), Arc::clone(&served), Arc::clone(&waiting));
            tokio::spawn(async move { seed(socket, info, served, live.then_some(waiting)).await });
        }
    });
    Seeder { addr, pex }
}

/// One connection of the seeder: the handshakes, the metadata to whoever asks, and the PEX
/// messages waiting where `live`.
async fn seed(
    mut socket: tokio::net::TcpStream,
    info: Arc<Vec<u8>>,
    served: Arc<std::sync::atomic::AtomicBool>,
    live: Option<Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>>>,
) {
    let mut theirs = [0u8; HANDSHAKE];
    if socket.read_exact(&mut theirs).await.is_err() {
        return;
    }
    let mut ours = vec![19];
    ours.extend_from_slice(b"BitTorrent protocol");
    ours.extend_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0]);
    ours.extend_from_slice(&theirs[28..48]);
    ours.extend_from_slice(b"-DMTEST-000000000001");
    let said = format!(
        "d1:md11:ut_metadatai{OUR_METADATA_ID}e6:ut_pexi1ee13:metadata_sizei{}ee",
        info.len()
    );
    ours.extend_from_slice(&extended(0, said.as_bytes()));
    let (mut reader, writer) = socket.into_split();
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    if writer.lock().await.write_all(&ours).await.is_err() {
        return;
    }
    let pex_id = Arc::new(std::sync::atomic::AtomicU8::new(0));
    if let Some(waiting) = live {
        let (writer, pex_id) = (Arc::clone(&writer), Arc::clone(&pex_id));
        tokio::spawn(async move {
            let mut waiting = waiting.lock().await;
            while let Some(dictionary) = waiting.recv().await {
                let id = loop {
                    match pex_id.load(std::sync::atomic::Ordering::SeqCst) {
                        0 => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
                        id => break id,
                    }
                };
                if writer
                    .lock()
                    .await
                    .write_all(&extended(id, &dictionary))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });
    }
    let mut metadata_id = 0;
    while let Some(message) = read_message(&mut reader).await {
        match message.as_slice() {
            [20, 0, dictionary @ ..] => {
                metadata_id = id_in(dictionary, b"11:ut_metadatai").unwrap_or(0);
                let pex = id_in(dictionary, b"6:ut_pexi").unwrap_or(0);
                pex_id.store(pex, std::sync::atomic::Ordering::SeqCst);
            }
            [20, OUR_METADATA_ID, ..] if metadata_id != 0 => {
                let mut data =
                    format!("d8:msg_typei1e5:piecei0e10:total_sizei{}ee", info.len()).into_bytes();
                data.extend_from_slice(&info);
                if writer
                    .lock()
                    .await
                    .write_all(&extended(metadata_id, &data))
                    .await
                    .is_err()
                {
                    return;
                }
                served.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            _ => {}
        }
    }
}

/// An extended message (BEP 10), length prefix included: `id`, then `payload`.
fn extended(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut message = ((payload.len() + 2) as u32).to_be_bytes().to_vec();
    message.extend_from_slice(&[20, id]);
    message.extend_from_slice(payload);
    message
}

/// The next whole message EDM sent, without its length; `None` once it lets go.
async fn read_message(reader: &mut tokio::net::tcp::OwnedReadHalf) -> Option<Vec<u8>> {
    loop {
        let mut length = [0u8; 4];
        reader.read_exact(&mut length).await.ok()?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 {
            continue;
        }
        let mut message = vec![0u8; length];
        reader.read_exact(&mut message).await.ok()?;
        return Some(message);
    }
}

/// The number after `key` in a bencoded dictionary (`…11:ut_metadatai3e…` gives 3).
fn id_in(dictionary: &[u8], key: &[u8]) -> Option<u8> {
    let at = dictionary.windows(key.len()).position(|w| w == key)? + key.len();
    let digits: Vec<u8> = dictionary[at..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .copied()
        .collect();
    std::str::from_utf8(&digits).ok()?.parse().ok()
}
