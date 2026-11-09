// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! MMS streams (`mms://`, `mmsh://`), fetched by dm itself over HTTP.
//!
//! Windows Media servers that speak MMS speak it over HTTP too (MMSH), and
//! that is what a player tries for an `mms://` address it cannot reach
//! otherwise. Two requests: the first is answered with the stream's ASF
//! header, from which every stream's number is read; the second asks to
//! play them all and is answered with that header again and the data
//! packets, each framed as `$H`, `$D` or `$E` (the end). The file written
//! is what the server sent: the ASF header and its Data Object's start,
//! then each packet padded to the packet size the header names — an ASF
//! file any player of Windows Media opens.
//!
//! Everything read comes from a server: nothing here indexes past what it
//! checked.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::control::Control;
use crate::Error;

const FILE_PROPERTIES: [u8; 16] = guid(
    0x8CAB_DCA1,
    0xA947,
    0x11CF,
    [0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65],
);
const STREAM_PROPERTIES: [u8; 16] = guid(
    0xB7DC_0791,
    0xA9B7,
    0x11CF,
    [0x8E, 0xE6, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65],
);

/// A GUID as ASF stores it: the first three fields little-endian.
const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6],
        d[7],
    ]
}

/// The HTTP address an `mms://` one is asked at.
pub fn http_address(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    ["mms", "mmsh", "mmst"]
        .contains(&scheme.to_ascii_lowercase().as_str())
        .then(|| format!("http://{rest}"))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The packet size and the stream numbers an ASF header names.
pub fn header_facts(header: &[u8]) -> (Option<u32>, Vec<u16>) {
    let count = le32(header, 24).unwrap_or(0);
    let mut at = 30usize;
    let mut packet = None;
    let mut streams = Vec::new();
    for _ in 0..count.min(1024) {
        let (Some(id), Some(size)) = (header.get(at..at + 16), le64(header, at + 16)) else {
            break;
        };
        if id == FILE_PROPERTIES {
            packet = le32(header, at + 92).filter(|p| *p > 0 && *p <= 1 << 20);
        } else if id == STREAM_PROPERTIES {
            if let Some(flags) = header.get(at + 72..at + 74) {
                let number = u16::from_le_bytes([flags.first().copied().unwrap_or(0), 0]) & 0x7f;
                if number > 0 && !streams.contains(&number) {
                    streams.push(number);
                }
            }
        }
        let Some(next) = usize::try_from(size).ok().and_then(|s| at.checked_add(s)) else {
            break;
        };
        if size < 24 {
            break;
        }
        at = next;
    }
    (packet, streams)
}

/// One MMSH chunk: its kind (`b'H'`, `b'D'`, `b'E'` …) and body.
pub struct Chunk {
    /// The chunk's kind letter.
    pub kind: u8,
    /// The chunk's bytes.
    pub body: Vec<u8>,
}

/// Chunks read off the bytes so far; what is left is the start of the next.
pub fn take_chunks(buffer: &mut Vec<u8>) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut at = 0;
    while let Some(head) = buffer.get(at..at + 4) {
        let kind = head.get(1).copied().unwrap_or(0);
        let length = usize::from(u16::from_le_bytes([
            head.get(2).copied().unwrap_or(0),
            head.get(3).copied().unwrap_or(0),
        ]));
        if head.first() != Some(&b'$') {
            // Out of step: drop what cannot be read.
            at = buffer.len();
            break;
        }
        // Header and data chunks carry eight more bytes of their own.
        let skip = if matches!(kind, b'H' | b'D') { 8 } else { 0 };
        let Some(chunk) = buffer.get(at + 4..at + 4 + length) else {
            break;
        };
        chunks.push(Chunk {
            kind,
            body: chunk
                .get(skip.min(chunk.len())..)
                .unwrap_or_default()
                .to_vec(),
        });
        at += 4 + length;
    }
    buffer.drain(..at.min(buffer.len()));
    chunks
}

fn request(
    client: &reqwest::Client,
    url: &str,
    context: u32,
    play: Option<&[u16]>,
) -> reqwest::RequestBuilder {
    let offset = if play.is_some() {
        "4294967295:4294967295"
    } else {
        "0:0"
    };
    let mut builder = client
        .get(url)
        .header("User-Agent", "NSPlayer/7.10.0.3059")
        .header("Accept", "*/*")
        .header(
            "Pragma",
            format!(
                "no-cache,rate=1.000000,stream-time=0,stream-offset={offset},request-context={context},max-duration=0"
            ),
        )
        .header("Pragma", "xClientGUID={3300AD50-2C39-46c0-AE0A-D1F2E3A4B5C6}");
    if let Some(streams) = play {
        let entries: Vec<String> = streams.iter().map(|n| format!("ffff:{n}:0 ")).collect();
        builder = builder
            .header("Pragma", "xPlayStrm=1")
            .header("Pragma", format!("stream-switch-count={}", streams.len()))
            .header(
                "Pragma",
                format!("stream-switch-entry={}", entries.concat().trim_end()),
            );
    }
    builder
}

/// Records the stream at `url` into `part` until the server ends it or
/// `control` stops it. Returns the bytes written; a stop is returned after
/// the file is closed.
pub async fn record(
    url: &str,
    part: &Path,
    control: &Control,
    downloaded: &Arc<AtomicU64>,
) -> Result<u64, Error> {
    let address =
        http_address(url).ok_or_else(|| Error::Internal("not an mms:// address".into()))?;
    let client = crate::builder()
        .timeout(Duration::from_secs(60 * 60 * 24))
        .build()
        .map_err(|e| Error::Internal(e.to_string()))?;
    // First: the header alone, for the streams in it.
    let described = crate::redirects::send(request(&client, &address, 1, None))
        .await
        .map_err(|e| Error::Internal(format!("the MMS server: {e}")))?;
    if !described.status().is_success() {
        return Err(Error::Internal(format!(
            "the MMS server answered {}",
            described.status()
        )));
    }
    let mut buffer = described
        .bytes()
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
        .to_vec();
    let header = take_chunks(&mut buffer)
        .into_iter()
        .find(|c| c.kind == b'H')
        .ok_or_else(|| Error::Internal("the MMS server sent no stream header".into()))?;
    let (_, streams) = header_facts(&header.body);
    if streams.is_empty() {
        return Err(Error::Internal("the MMS stream names no streams".into()));
    }

    // Then: every stream played.
    let playing = crate::redirects::send(request(&client, &address, 2, Some(&streams)))
        .await
        .map_err(|e| Error::Internal(format!("the MMS server: {e}")))?;
    if !playing.status().is_success() {
        return Err(Error::Internal(format!(
            "the MMS server answered {}",
            playing.status()
        )));
    }
    let mut body = playing.bytes_stream();
    let mut file = tokio::fs::File::create(part).await?;
    let mut written = 0u64;
    let mut packet_size: Option<usize> = None;
    let mut buffer = Vec::new();
    let mut stop = None;
    'stream: loop {
        let next = tokio::select! {
            next = tokio::time::timeout(Duration::from_secs(30), body.next()) => next,
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if let Err(e) = control.check() {
                    stop = Some(e);
                    break;
                }
                continue;
            }
        };
        let bytes = match next {
            Ok(Some(Ok(bytes))) => bytes,
            Ok(Some(Err(e))) if written == 0 => {
                stop = Some(Error::Internal(format!("the MMS server: {e}")));
                break;
            }
            Ok(None) | Ok(Some(Err(_))) => break,
            Err(_) if written == 0 => {
                stop = Some(Error::Internal(
                    "the MMS server sent nothing for 30 seconds".into(),
                ));
                break;
            }
            Err(_) => break,
        };
        buffer.extend_from_slice(&bytes);
        for chunk in take_chunks(&mut buffer) {
            match chunk.kind {
                b'H' if packet_size.is_none() => {
                    packet_size = header_facts(&chunk.body).0.map(|p| p as usize);
                    file.write_all(&chunk.body).await?;
                    written += chunk.body.len() as u64;
                }
                b'D' if packet_size.is_some() => {
                    let mut packet = chunk.body;
                    // Padded to the header's packet size, as a file has them.
                    if let Some(size) = packet_size {
                        if packet.len() < size {
                            packet.resize(size, 0);
                        }
                    }
                    file.write_all(&packet).await?;
                    written += packet.len() as u64;
                }
                b'E' => break 'stream,
                _ => {}
            }
        }
        downloaded.store(written, Ordering::Relaxed);
    }
    file.flush().await?;
    file.sync_all().await?;
    match stop {
        Some(stop) => Err(stop),
        None if packet_size.is_none() => Err(Error::Internal(
            "the MMS server ended the stream before sending any of it".into(),
        )),
        None => Ok(written),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_read_across_reads() {
        let mut data = vec![b'$', b'H', 10, 0];
        data.extend_from_slice(&[0u8; 8]);
        data.extend_from_slice(b"ab");
        data.extend_from_slice(&[b'$', b'D', 11, 0]);
        data.extend_from_slice(&[0u8; 8]);
        data.extend_from_slice(b"xyz");
        let mut buffer = data[..20].to_vec();
        let first = take_chunks(&mut buffer);
        assert_eq!(first.len(), 1);
        assert_eq!(
            (first[0].kind, first[0].body.as_slice()),
            (b'H', &b"ab"[..])
        );
        buffer.extend_from_slice(&data[20..]);
        let second = take_chunks(&mut buffer);
        assert_eq!(
            (second[0].kind, second[0].body.as_slice()),
            (b'D', &b"xyz"[..])
        );
        assert!(buffer.is_empty());
        assert_eq!(
            http_address("mms://old.example/clip.asf").as_deref(),
            Some("http://old.example/clip.asf")
        );
        assert_eq!(http_address("rtsp://x/y"), None);
    }

    /// An ASF header naming a packet size of 32 and, where `streams` is set,
    /// stream 1.
    fn asf_header(streams: bool) -> Vec<u8> {
        let mut file = FILE_PROPERTIES.to_vec();
        file.extend_from_slice(&104u64.to_le_bytes());
        file.resize(92, 0);
        file.extend_from_slice(&32u32.to_le_bytes());
        file.resize(104, 0);
        let mut stream = STREAM_PROPERTIES.to_vec();
        stream.extend_from_slice(&78u64.to_le_bytes());
        stream.resize(72, 0);
        stream.extend_from_slice(&[0x01, 0x00]);
        stream.resize(78, 0);
        let objects: Vec<&[u8]> = if streams {
            vec![&file, &stream]
        } else {
            vec![&file]
        };
        let mut header = vec![0u8; 24];
        header.extend_from_slice(&(objects.len() as u32).to_le_bytes());
        header.extend_from_slice(&[1, 2]);
        for o in objects {
            header.extend_from_slice(o);
        }
        header
    }

    /// An MMSH chunk of `kind` holding `body`.
    fn chunk(kind: u8, body: &[u8]) -> Vec<u8> {
        let extra = if matches!(kind, b'H' | b'D') { 8 } else { 0 };
        let mut out = vec![b'$', kind];
        out.extend_from_slice(&((body.len() + extra) as u16).to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, extra));
        out.extend_from_slice(body);
        out
    }

    /// A server answering the description request with (`status`, `described`)
    /// and the playing one with (`playing_status`, `played`).
    async fn serve(
        status: u16,
        described: Vec<u8>,
        playing_status: u16,
        played: Vec<u8>,
    ) -> String {
        let app = axum::Router::new().fallback(move |headers: axum::http::HeaderMap| {
            let (described, played) = (described.clone(), played.clone());
            async move {
                let playing = headers
                    .get_all("pragma")
                    .iter()
                    .any(|p| p.to_str().is_ok_and(|p| p.contains("xPlayStrm=1")));
                let (status, body) = if playing {
                    (playing_status, played)
                } else {
                    (status, described)
                };
                (axum::http::StatusCode::from_u16(status).unwrap(), body)
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("mms://{addr}/live/clip.asf")
    }

    async fn record_from(url: &str) -> (Result<u64, Error>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let got = record(
            url,
            &dir.path().join("clip.part"),
            &Control::new(),
            &Arc::new(AtomicU64::new(0)),
        )
        .await;
        (got, dir)
    }

    #[test]
    fn a_header_says_its_packet_size_and_its_streams() {
        assert_eq!(header_facts(&asf_header(true)), (Some(32), vec![1]));
        assert_eq!(header_facts(&asf_header(false)), (Some(32), vec![]));
    }

    #[tokio::test]
    async fn what_an_mms_server_refuses_or_leaves_out_is_said() {
        let header = chunk(b'H', &asf_header(true));
        let cases: Vec<(String, &str)> = vec![
            (
                serve(404, Vec::new(), 200, Vec::new()).await,
                "answered 404",
            ),
            (
                serve(200, chunk(b'E', b""), 200, Vec::new()).await,
                "sent no stream header",
            ),
            (
                serve(200, chunk(b'H', &asf_header(false)), 200, Vec::new()).await,
                "names no streams",
            ),
            (
                serve(200, header.clone(), 503, Vec::new()).await,
                "answered 503",
            ),
            (
                serve(200, header.clone(), 200, chunk(b'E', b"")).await,
                "before sending any of it",
            ),
        ];
        for (url, said) in cases {
            let (got, _dir) = record_from(&url).await;
            let why = got.unwrap_err().to_string();
            assert!(why.contains(said), "{why} is not {said:?}");
        }
        let (got, _dir) = record_from("http://127.0.0.1:1/clip.asf").await;
        assert!(got.unwrap_err().to_string().contains("not an mms://"));
    }

    #[tokio::test]
    async fn packets_are_padded_to_the_headers_size_and_others_left_out() {
        let header = asf_header(true);
        let mut played = chunk(b'D', b"early");
        played.extend(chunk(b'H', &header));
        played.extend(chunk(b'C', b"change"));
        played.extend(chunk(b'D', b"12345"));
        played.extend(chunk(b'E', b""));
        played.extend(chunk(b'D', b"after the end"));
        let url = serve(200, chunk(b'H', &header), 200, played).await;
        let (got, dir) = record_from(&url).await;
        let written = got.unwrap();
        let data = std::fs::read(dir.path().join("clip.part")).unwrap();
        let mut packet = b"12345".to_vec();
        packet.resize(32, 0);
        assert_eq!(data, [header, packet].concat());
        assert_eq!(written, data.len() as u64);
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..800)) {
            let _ = header_facts(&data);
            let mut buffer = data.clone();
            let _ = take_chunks(&mut buffer);
        }
    }
}
