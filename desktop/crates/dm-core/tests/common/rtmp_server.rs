//! A one-client RTMP server for the tests, playing an FLV file's tags.
//!
//! It does what a media server does for a player: the plain handshake,
//! then answers `connect` and `createStream`, and on `play` sends
//! StreamBegin, `NetStream.Play.Reset` and `NetStream.Play.Start`, every
//! tag of the file as an RTMP audio or video message at the tag's own time,
//! and `NetStream.Play.Stop`; then it hangs up. Window size, peer bandwidth
//! and chunk size are set as a real server sets them, so the client's
//! answers to those are exercised too.

#![allow(dead_code)] // each test binary uses a different part of this

use std::collections::HashMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// One FLV tag: 8 for sound, 9 for pictures, 18 for script data.
#[derive(Debug, Clone)]
pub struct Tag {
    pub kind: u8,
    pub ms: u32,
    pub body: Vec<u8>,
}

/// The tags of an FLV file, in order.
pub fn flv_tags(file: &[u8]) -> Vec<Tag> {
    assert_eq!(&file[..3], b"FLV", "not an FLV file");
    let header = u32::from_be_bytes(file[5..9].try_into().unwrap()) as usize;
    let u24 = |b: &[u8]| (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
    let mut tags = Vec::new();
    let mut at = header + 4; // the header, then PreviousTagSize0
    while at + 11 <= file.len() {
        let kind = file[at] & 0x1f;
        let size = u24(&file[at + 1..]) as usize;
        let ms = u24(&file[at + 4..]) | (u32::from(file[at + 7]) << 24);
        tags.push(Tag {
            kind,
            ms,
            body: file[at + 11..at + 11 + size].to_vec(),
        });
        at += 11 + size + 4;
    }
    assert_eq!(at, file.len(), "the FLV file ends inside a tag");
    tags
}

// ---- AMF0, as much as the conversation needs ---------------------------

fn amf_string(out: &mut Vec<u8>, s: &str) {
    out.push(0x02);
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn amf_number(out: &mut Vec<u8>, n: f64) {
    out.push(0x00);
    out.extend_from_slice(&n.to_be_bytes());
}

fn amf_object(out: &mut Vec<u8>, fields: &[(&str, &str)]) {
    out.push(0x03);
    for (key, value) in fields {
        out.extend_from_slice(&(key.len() as u16).to_be_bytes());
        out.extend_from_slice(key.as_bytes());
        amf_string(out, value);
    }
    out.extend_from_slice(&[0, 0, 9]);
}

/// A command's name and transaction number.
fn command_of(body: &[u8]) -> (String, f64) {
    if body.first() != Some(&0x02) || body.len() < 3 {
        return (String::new(), 0.0);
    }
    let len = usize::from(u16::from_be_bytes([body[1], body[2]]));
    let name = String::from_utf8_lossy(&body[3..3 + len]).into_owned();
    let rest = &body[3 + len..];
    let txn = if rest.first() == Some(&0x00) && rest.len() >= 9 {
        f64::from_be_bytes(rest[1..9].try_into().unwrap())
    } else {
        0.0
    };
    (name, txn)
}

fn status(code: &str, description: &str) -> Vec<u8> {
    let mut out = Vec::new();
    amf_string(&mut out, "onStatus");
    amf_number(&mut out, 0.0);
    out.push(0x05);
    amf_object(
        &mut out,
        &[
            ("level", "status"),
            ("code", code),
            ("description", description),
        ],
    );
    out
}

// ---- the chunk stream ---------------------------------------------------

/// The size the server's chunks are cut to, once it has said so.
const CHUNK: usize = 4096;

/// One message as chunks: a full header, then continuation chunks.
fn chunks(csid: u8, kind: u8, stream: u32, ms: u32, payload: &[u8], size: usize) -> Vec<u8> {
    let mut out = vec![csid];
    let extended = ms >= 0x00FF_FFFF;
    out.extend_from_slice(&(if extended { 0x00FF_FFFF } else { ms }).to_be_bytes()[1..]);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
    out.push(kind);
    out.extend_from_slice(&stream.to_le_bytes());
    if extended {
        out.extend_from_slice(&ms.to_be_bytes());
    }
    for (i, part) in payload.chunks(size).enumerate() {
        if i > 0 {
            out.push(0xC0 | csid);
            if extended {
                out.extend_from_slice(&ms.to_be_bytes());
            }
        }
        out.extend_from_slice(part);
    }
    out
}

#[derive(Default)]
struct Incoming {
    length: usize,
    kind: u8,
    body: Vec<u8>,
}

/// The client's messages off its chunk stream: (kind, body).
struct Reader<R> {
    io: R,
    chunk: usize,
    streams: HashMap<u32, Incoming>,
}

impl<R: tokio::io::AsyncRead + Unpin> Reader<R> {
    async fn message(&mut self) -> std::io::Result<(u8, Vec<u8>)> {
        loop {
            let first = self.io.read_u8().await?;
            let csid = match first & 0x3f {
                0 => 64 + u32::from(self.io.read_u8().await?),
                1 => 64 + u32::from(self.io.read_u16_le().await?),
                n => u32::from(n),
            };
            let mut header = vec![0u8; [11, 7, 3, 0][usize::from(first >> 6)]];
            self.io.read_exact(&mut header).await?;
            let state = self.streams.entry(csid).or_default();
            if header.len() >= 7 {
                state.length = (usize::from(header[3]) << 16)
                    | (usize::from(header[4]) << 8)
                    | usize::from(header[5]);
                state.kind = header[6];
            }
            if header.len() >= 3 && header[..3] == [0xff, 0xff, 0xff] {
                self.io.read_u32().await?;
            }
            let want = (state.length - state.body.len()).min(self.chunk);
            let mut part = vec![0u8; want];
            self.io.read_exact(&mut part).await?;
            let state = self.streams.entry(csid).or_default();
            state.body.extend_from_slice(&part);
            if state.body.len() >= state.length {
                let body = std::mem::take(&mut state.body);
                let kind = state.kind;
                if kind == 1 && body.len() >= 4 {
                    let size = u32::from_be_bytes(body[..4].try_into().unwrap());
                    self.chunk = (size & 0x7fff_ffff) as usize;
                }
                return Ok((kind, body));
            }
        }
    }
}

/// Serves `tags` to the one client that comes to `listener`, as the stream
/// it asks to play; returns the name it asked for.
pub async fn serve_rtmp(listener: tokio::net::TcpListener, tags: Vec<Tag>) -> String {
    let (socket, _) = listener.accept().await.unwrap();
    let (read, mut write) = socket.into_split();
    let mut read = tokio::io::BufReader::new(read);

    // The handshake: C0 and C1 in; S0, S1 and S2 (C1 echoed) out; C2 in.
    let mut c0c1 = vec![0u8; 1537];
    read.read_exact(&mut c0c1).await.unwrap();
    assert_eq!(c0c1[0], 3, "the client asked for a version other than 3");
    let mut hello = vec![3u8];
    hello.extend_from_slice(&[0u8; 8]);
    hello.extend((0..1528u32).map(|i| (i * 7 + 3) as u8));
    hello.extend_from_slice(&c0c1[1..]);
    write.write_all(&hello).await.unwrap();
    let mut c2 = vec![0u8; 1536];
    read.read_exact(&mut c2).await.unwrap();
    assert_eq!(c2, hello[1..1537], "the client did not echo S1");

    let mut reader = Reader {
        io: read,
        chunk: 128,
        streams: HashMap::new(),
    };
    let played = loop {
        let (kind, body) = reader.message().await.unwrap();
        if kind != 20 {
            continue;
        }
        let (name, txn) = command_of(&body);
        let mut out = Vec::new();
        match name.as_str() {
            "connect" => {
                out.extend(chunks(2, 5, 0, 0, &2_500_000u32.to_be_bytes(), 128));
                let mut bandwidth = 2_500_000u32.to_be_bytes().to_vec();
                bandwidth.push(2);
                out.extend(chunks(2, 6, 0, 0, &bandwidth, 128));
                out.extend(chunks(2, 1, 0, 0, &(CHUNK as u32).to_be_bytes(), 128));
                let mut result = Vec::new();
                amf_string(&mut result, "_result");
                amf_number(&mut result, txn);
                amf_object(&mut result, &[("fmsVer", "FMS/3,0,1,123")]);
                amf_object(
                    &mut result,
                    &[
                        ("level", "status"),
                        ("code", "NetConnection.Connect.Success"),
                        ("description", "Connection succeeded."),
                    ],
                );
                out.extend(chunks(3, 20, 0, 0, &result, CHUNK));
            }
            "createStream" => {
                let mut result = Vec::new();
                amf_string(&mut result, "_result");
                amf_number(&mut result, txn);
                result.push(0x05);
                amf_number(&mut result, 1.0);
                out.extend(chunks(3, 20, 0, 0, &result, CHUNK));
            }
            "play" => {
                // The stream's name follows the transaction and a null.
                let len = usize::from(u16::from_be_bytes([body[1], body[2]]));
                let after = &body[3 + len + 9 + 1..];
                assert_eq!(after[0], 0x02, "play names no stream");
                let n = usize::from(u16::from_be_bytes([after[1], after[2]]));
                break String::from_utf8_lossy(&after[3..3 + n]).into_owned();
            }
            _ => {}
        }
        write.write_all(&out).await.unwrap();
    };

    let mut out = chunks(2, 4, 0, 0, &[0, 0, 0, 0, 0, 1], CHUNK);
    out.extend(chunks(
        5,
        20,
        1,
        0,
        &status("NetStream.Play.Reset", "Playing and resetting."),
        CHUNK,
    ));
    out.extend(chunks(
        5,
        20,
        1,
        0,
        &status("NetStream.Play.Start", "Started playing."),
        CHUNK,
    ));
    write.write_all(&out).await.unwrap();
    for tag in tags.iter().filter(|t| t.kind == 8 || t.kind == 9) {
        let csid = if tag.kind == 8 { 4 } else { 6 };
        let message = chunks(csid, tag.kind, 1, tag.ms, &tag.body, CHUNK);
        if write.write_all(&message).await.is_err() {
            return played;
        }
    }
    let last = tags.last().map_or(0, |t| t.ms);
    let stop = chunks(
        5,
        20,
        1,
        last,
        &status("NetStream.Play.Stop", "Stopped playing."),
        CHUNK,
    );
    let _ = write.write_all(&stop).await;
    let _ = write.shutdown().await;
    // Whatever the client still says is read and dropped until it leaves.
    while reader.message().await.is_ok() {}
    played
}
