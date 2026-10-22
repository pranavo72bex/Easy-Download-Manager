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
    pub kind: u8,
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
    let described = request(&client, &address, 1, None)
        .send()
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
    let playing = request(&client, &address, 2, Some(&streams))
        .send()
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

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..800)) {
            let _ = header_facts(&data);
            let mut buffer = data.clone();
            let _ = take_chunks(&mut buffer);
        }
    }
}
