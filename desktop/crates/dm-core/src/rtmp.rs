// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An RTMP client of dm's own, for `rtmp://`, `rtmps://` and `rtmpt://`
//! streams.
//!
//! What a player does to watch one: the handshake, then `connect` to the
//! application, `createStream` and `play` the stream's name, in AMF0
//! commands over RTMP's chunk stream; the server's chunk size, window and
//! pings answered as it asks. Pictures (H.264, and HEVC as either RTMP
//! extension carries it) and sound (AAC, MP3) arrive as FLV tags and are
//! written as they come into a fragmented MP4 (`fmp4`), which plays however
//! far it got. RTMPT carries the same conversation in HTTP requests
//! (`tunnel`). RTMPE (Flash's encryption) is not spoken here.
//!
//! Everything read comes from a server: nothing here indexes past what it
//! checked.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::control::Control;
use crate::fmp4;
use crate::Error;

/// The parts of an `rtmp://` address a player uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The server's host.
    pub host: String,
    /// The server's port.
    pub port: u16,
    /// Whether the connection is encrypted (`rtmps`).
    pub tls: bool,
    /// RTMPT: the same conversation carried in HTTP requests, for networks
    /// that let only the web through.
    pub tunnel: bool,
    /// The application: every part of the path but the last.
    pub app: String,
    /// The stream's name: the last part, with its query.
    pub play: String,
    /// The `tcUrl` the server is told: the address up to the application.
    pub tc_url: String,
}

/// Reads an `rtmp://`, `rtmps://` or `rtmpt://` address.
pub fn target(url: &str) -> Result<Target, String> {
    let (scheme, rest) = url
        .trim()
        .split_once("://")
        .ok_or("not an rtmp:// address")?;
    let scheme = scheme.to_ascii_lowercase();
    let (tls, tunnel) = match scheme.as_str() {
        "rtmp" => (false, false),
        "rtmps" => (true, false),
        "rtmpt" => (false, true),
        other => return Err(format!("dm does not speak {other}://")),
    };
    let usual = match (tls, tunnel) {
        (true, _) => 443,
        (_, true) => 80,
        _ => 1935,
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || authority.starts_with('[') => {
            match port.parse() {
                Ok(port) => (host, port),
                Err(_) => (authority, usual),
            }
        }
        _ => (authority, usual),
    };
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host.is_empty() {
        return Err("the address names no server".into());
    }
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let path = path.trim_end_matches('/');
    let (app, play) = match path.rsplit_once('/') {
        Some((app, play)) => (app.to_string(), play.to_string()),
        None => (path.to_string(), String::new()),
    };
    let play = match query {
        Some(q) if !play.is_empty() => format!("{play}?{q}"),
        _ => play,
    };
    if app.is_empty() || play.is_empty() {
        return Err("the address names no application and stream".into());
    }
    // An IPv6 address in brackets; any other host borrowed as it is.
    let tc_url = if host.contains(':') {
        format!("{scheme}://[{host}]:{port}/{app}")
    } else {
        format!("{scheme}://{host}:{port}/{app}")
    };
    Ok(Target {
        host,
        port,
        tls,
        tunnel,
        app,
        play,
        tc_url,
    })
}

mod amf;
mod connect;
mod recorder;
mod session;
mod wire;

pub use amf::*;
pub(crate) use connect::*;
use recorder::*;
pub use session::*;
use wire::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_read_as_a_player_reads_them() {
        let t = target("rtmp://media.example/live/show").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.tls),
            ("media.example", 1935, false)
        );
        assert_eq!((t.app.as_str(), t.play.as_str()), ("live", "show"));
        assert_eq!(t.tc_url, "rtmp://media.example:1935/live");
        let t = target("rtmps://media.example:8443/app/inst/name?key=1").unwrap();
        assert_eq!((t.port, t.tls), (8443, true));
        assert_eq!(
            (t.app.as_str(), t.play.as_str()),
            ("app/inst", "name?key=1")
        );
        assert!(target("rtmp://media.example/").is_err());
        assert!(target("rtmpe://media.example/live/x").is_err());
    }

    #[test]
    fn amf_round_trips() {
        let mut out = Vec::new();
        amf_string(&mut out, "onStatus");
        amf_number(&mut out, 0.0);
        out.push(0x05);
        amf_object(
            &mut out,
            &[
                ("level", Amf::String("error".into())),
                ("code", Amf::String("NetStream.Play.StreamNotFound".into())),
            ],
        );
        let values = amf_values(&out);
        assert_eq!(values.len(), 4);
        assert_eq!(values[0].text(), Some("onStatus"));
        assert_eq!(
            values[3].field("code").and_then(Amf::text),
            Some("NetStream.Play.StreamNotFound")
        );
    }

    #[test]
    fn a_message_goes_out_in_chunks() {
        let payload = vec![7u8; 300];
        let out = chunks(3, 20, 1, 0, &payload, 128);
        // A full header, then two continuation bytes between the parts.
        assert_eq!(out.len(), 12 + 300 + 2);
        assert_eq!(out[0], 3);
        assert_eq!(out[12 + 128], 0xC3);
    }

    #[tokio::test]
    async fn chunks_of_every_header_size_and_stream_id_form_are_read_back() {
        let mut wire = Vec::new();
        // A full header with an extended time, on chunk stream 3.
        wire.push(0x03);
        wire.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0, 0, 5, 20, 1, 0, 0, 0]);
        wire.extend_from_slice(&0x0100_0000u32.to_be_bytes());
        wire.extend_from_slice(b"hello");
        // A header without the stream: a time delta, a new length and kind.
        wire.push(0x43);
        wire.extend_from_slice(&[0, 0, 10, 0, 0, 3, 8]);
        wire.extend_from_slice(b"abc");
        // The delta alone, then nothing: each adds the last delta again.
        wire.push(0x83);
        wire.extend_from_slice(&[0, 0, 5]);
        wire.extend_from_slice(b"def");
        wire.push(0xC3);
        wire.extend_from_slice(b"ghi");
        // A stream id past 63 in one byte (69), its message in two chunks of 128.
        wire.extend_from_slice(&[0x00, 5]);
        wire.extend_from_slice(&[0, 0, 0, 0, 0, 200, 9, 0, 0, 0, 0]);
        wire.extend_from_slice(&[1u8; 128]);
        wire.extend_from_slice(&[0xC0, 5]);
        wire.extend_from_slice(&[2u8; 72]);
        // One past 319 in two bytes.
        wire.extend_from_slice(&[0x01, 0x10, 0x01]);
        wire.extend_from_slice(&[0, 0, 7, 0, 0, 1, 18, 0, 0, 0, 0]);
        wire.push(b'z');
        let mut reader = Reader {
            io: &wire[..],
            chunk_size: 128,
            streams: HashMap::new(),
            read: 0,
        };
        let mut got = Vec::new();
        for _ in 0..6 {
            let m = reader.message().await.unwrap();
            got.push((m.kind, m.timestamp, m.body));
        }
        assert_eq!(got[0], (20, 0x0100_0000, b"hello".to_vec()));
        assert_eq!(got[1], (8, 0x0100_000A, b"abc".to_vec()));
        assert_eq!(got[2], (8, 0x0100_000F, b"def".to_vec()));
        assert_eq!(got[3], (8, 0x0100_0014, b"ghi".to_vec()));
        let mut whole = vec![1u8; 128];
        whole.extend_from_slice(&[2u8; 72]);
        assert_eq!(got[4], (9, 0, whole));
        assert_eq!(got[5], (18, 7, b"z".to_vec()));
        assert!(reader.streams.contains_key(&(64 + 16 + 256)));
        assert_eq!(reader.read, wire.len() as u64);
        // And then the end of the connection.
        assert!(reader.message().await.is_err());
    }

    #[test]
    fn amf_arrays_dates_and_long_strings_are_stepped_over_or_read() {
        let mut data = Vec::new();
        // An ECMA array: a count, then fields as an object's.
        data.push(0x08);
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(b"ok");
        data.extend_from_slice(&[0x01, 1]);
        data.extend_from_slice(&[0, 0, 0x09]);
        // A strict array of two numbers, a date, a long string, an undefined.
        data.push(0x0A);
        data.extend_from_slice(&2u32.to_be_bytes());
        amf_number(&mut data, 1.0);
        amf_number(&mut data, 2.0);
        data.push(0x0B);
        data.extend_from_slice(&[0u8; 10]);
        data.push(0x0C);
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(b"long");
        data.push(0x06);
        let values = amf_values(&data);
        assert_eq!(
            values,
            vec![
                Amf::Object(vec![("ok".into(), Amf::Bool(true))]),
                Amf::Other,
                Amf::Other,
                Amf::String("long".into()),
                Amf::Null,
            ]
        );
        // An object written with a bool and a null reads back as written.
        let mut out = Vec::new();
        amf_object(&mut out, &[("b", Amf::Bool(false)), ("n", Amf::Null)]);
        assert_eq!(
            amf_values(&out),
            vec![Amf::Object(vec![
                ("b".into(), Amf::Bool(false)),
                ("n".into(), Amf::Null)
            ])]
        );
        // A marker dm does not know ends the reading; nesting past sixteen deep too.
        assert_eq!(amf_values(&[0x0D]), Vec::<Amf>::new());
        let mut deep = Vec::new();
        for _ in 0..20 {
            deep.push(0x03);
            deep.extend_from_slice(&1u16.to_be_bytes());
            deep.push(b'k');
        }
        assert!(amf_values(&deep).is_empty());
        assert_eq!(Amf::Null.field("x"), None);
        assert_eq!(Amf::Number(1.0).text(), None);
    }

    /// An FLV audio tag of AAC: its configuration, or a frame.
    fn aac(config: bool) -> Vec<u8> {
        if config {
            vec![0xAF, 0, 0x12, 0x10]
        } else {
            vec![0xAF, 1, 0x21, 0x00, 0x49, 0x90]
        }
    }

    #[test]
    fn sound_alone_is_recorded_once_its_shape_is_known() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.mp4");
        let mut r = Recorder::new(&path);
        // A frame before the configuration is held, not lost.
        r.audio(0, &aac(false)).unwrap();
        assert_eq!(r.written(), 0);
        r.audio(0, &aac(true)).unwrap();
        for ms in [23, 46, 69] {
            r.audio(ms, &aac(false)).unwrap();
        }
        // Kinds dm does not record, and empty tags, are passed over.
        r.audio(90, &[0x00, 1, 2]).unwrap();
        r.audio(90, &[]).unwrap();
        r.audio(90, &[0xAF, 1]).unwrap();
        let written = r.finish().unwrap().expect("a file");
        let data = std::fs::read(&path).unwrap();
        assert_eq!(written, data.len() as u64);
        assert!(data.windows(4).any(|w| w == b"mp4a"));
        assert_eq!(data.windows(4).filter(|w| *w == b"trak").count(), 1);
    }

    #[test]
    fn mp3_sound_is_recorded_with_its_own_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.mp4");
        let mut r = Recorder::new(&path);
        // MPEG-1 layer III, 128 kbit/s at 44.1 kHz: 417 bytes a frame.
        let mut frame = vec![0x2F, 0xFF, 0xFB, 0x90, 0x00];
        frame.resize(1 + 417, 0);
        for ms in [0, 26, 52] {
            r.audio(ms, &frame).unwrap();
        }
        r.audio(60, &[0x2F]).unwrap();
        let written = r.finish().unwrap().expect("a file");
        assert!(written > 3 * 417);
        let data = std::fs::read(&path).unwrap();
        assert!(data.windows(4).any(|w| w == b"mp4a"));
    }

    #[test]
    fn enhanced_and_hevc_pictures_are_recorded_and_other_codecs_left_out() {
        let dir = tempfile::tempdir().unwrap();
        // Enhanced RTMP, HEVC: a sequence start, a coded frame with its time
        // offset, a coded frame without one, and a packet kind dm leaves.
        let path = dir.path().join("h.mp4");
        let mut r = Recorder::new(&path);
        let mut head = vec![0x90, b'h', b'v', b'c', b'1'];
        head.extend_from_slice(&[1u8; 23]);
        r.video(0, &head).unwrap();
        r.video(0, &[0x91, b'h', b'v', b'c', b'1', 0, 0, 40, 9, 9, 9])
            .unwrap();
        r.video(40, &[0xA3, b'h', b'v', b'c', b'1', 8, 8]).unwrap();
        r.video(80, &[0x92, b'h', b'v', b'c', b'1', 7]).unwrap();
        // A FourCC dm does not record.
        r.video(80, &[0x91, b'v', b'p', b'0', b'9', 0, 0, 0, 1])
            .unwrap();
        r.audio(3500, &[0x00]).unwrap();
        r.video(3500, &[0xA3, b'h', b'v', b'c', b'1', 6]).unwrap();
        let written = r.finish().unwrap().expect("a file");
        let data = std::fs::read(&path).unwrap();
        assert_eq!(written, data.len() as u64);
        assert!(data.windows(4).any(|w| w == b"hvc1"));

        // The older spelling of HEVC (codec 12), and a codec dm does not record.
        let path = dir.path().join("h12.mp4");
        let mut r = Recorder::new(&path);
        r.video(0, &[0x02, 0, 0, 0, 0]).unwrap();
        r.video(0, &[0x1C, 0, 0, 0, 0, 1, 2, 3]).unwrap();
        r.video(0, &[0x1C, 1, 0, 0, 0, 5, 5]).unwrap();
        r.video(0, &[0x1C, 2, 0, 0, 0]).unwrap();
        r.audio(0, &aac(true)).unwrap();
        r.video(40, &[0x2C, 1, 0, 0, 0, 6]).unwrap();
        r.finish().unwrap().expect("a file");
        let data = std::fs::read(&path).unwrap();
        assert!(data.windows(4).any(|w| w == b"hvc1"));
        assert!(data.windows(4).any(|w| w == b"mp4a"));
        assert_eq!(hevc_size(&[0u8; 23]), (0, 0));
    }

    #[test]
    fn a_picture_that_never_says_its_shape_is_not_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("none.mp4");
        let mut r = Recorder::new(&path);
        r.video(0, &[0x17, 1, 0, 0, 0, 1, 2]).unwrap();
        r.video(4000, &[0x27, 1, 0, 0, 0, 3]).unwrap();
        assert_eq!(r.finish().unwrap(), None);
        assert!(!path.exists());
    }

    /// One client of a scripted RTMP server on 127.0.0.1: the handshake (S0 is
    /// `version`), then on `connect` it sends `replies`, then reads what the
    /// client sends until it lets go, or keeps quiet while `hold` is set. What
    /// the client sent comes back as (kind, body).
    async fn scripted(
        version: u8,
        replies: Vec<u8>,
        hold: bool,
    ) -> (String, tokio::task::JoinHandle<Vec<(u8, Vec<u8>)>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut c0c1 = vec![0u8; 1537];
            socket.read_exact(&mut c0c1).await.unwrap();
            let mut hello = vec![version];
            hello.extend_from_slice(&[0u8; 1536]);
            hello.extend_from_slice(&c0c1[1..]);
            socket.write_all(&hello).await.unwrap();
            let mut seen = Vec::new();
            if version != 3 {
                return seen;
            }
            let mut c2 = vec![0u8; 1536];
            socket.read_exact(&mut c2).await.unwrap();
            let (read, mut write) = tokio::io::split(socket);
            let mut reader = Reader {
                io: read,
                chunk_size: 128,
                streams: HashMap::new(),
                read: 0,
            };
            while let Ok(m) = reader.message().await {
                if m.kind == 1 {
                    reader.chunk_size = be(&m.body, 4) as usize;
                }
                let connect = m.kind == 20
                    && amf_values(&m.body).first() == Some(&Amf::String("connect".into()));
                seen.push((m.kind, m.body));
                if connect {
                    write.write_all(&replies).await.unwrap();
                    if hold {
                        // Quiet, until the client hangs up.
                        while reader.message().await.is_ok() {}
                        break;
                    }
                }
            }
            seen
        });
        (format!("rtmp://127.0.0.1:{port}/live/show"), task)
    }

    fn said(name: &str, transaction: f64, info: &[(&str, Amf)]) -> Vec<u8> {
        let mut body = Vec::new();
        amf_string(&mut body, name);
        amf_number(&mut body, transaction);
        body.push(0x05);
        if !info.is_empty() {
            amf_object(&mut body, info);
        }
        body
    }

    async fn record_from(url: &str, control: &Control) -> (Result<u64, Error>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let got = record(
            url,
            &dir.path().join("show.part"),
            control,
            &Arc::new(AtomicU64::new(0)),
        )
        .await;
        (got, dir)
    }

    #[tokio::test]
    async fn a_servers_pings_and_windows_are_answered_and_its_refusal_is_said() {
        let mut replies = Vec::new();
        replies.extend(chunks(2, 5, 0, 0, &100u32.to_be_bytes(), 128));
        let mut bandwidth = 2_500_000u32.to_be_bytes().to_vec();
        bandwidth.push(2);
        replies.extend(chunks(2, 6, 0, 0, &bandwidth, 128));
        replies.extend(chunks(2, 4, 0, 0, &[0, 6, 0, 0, 1, 2], 128));
        replies.extend(chunks(2, 1, 0, 0, &256u32.to_be_bytes(), 128));
        // The refusal in AMF3's message kind: one byte, then AMF0.
        let mut refusal = vec![0];
        refusal.extend(said(
            "_error",
            1.0,
            &[
                ("level", Amf::String("error".into())),
                ("description", Amf::String("no such application".into())),
            ],
        ));
        replies.extend(chunks(3, 17, 0, 0, &refusal, 256));
        let (url, server) = scripted(3, replies, false).await;
        let (got, _dir) = record_from(&url, &Control::new()).await;
        let why = got.unwrap_err().to_string();
        assert!(why.contains("no such application"), "{why}");
        let seen = server.await.unwrap();
        assert!(
            seen.iter()
                .any(|(k, b)| *k == 4 && b[..] == [0, 7, 0, 0, 1, 2]),
            "the ping was not answered with its time"
        );
        assert!(
            seen.iter()
                .any(|(k, b)| *k == 5 && b[..] == 2_500_000u32.to_be_bytes()),
            "the peer bandwidth was not answered"
        );
        assert!(
            seen.iter().any(|(k, _)| *k == 3),
            "nothing was acknowledged"
        );
    }

    #[tokio::test]
    async fn a_stream_the_server_will_not_play_is_said_in_its_words() {
        let mut replies = chunks(3, 20, 0, 0, &said("_result", 1.0, &[]), 128);
        let mut created = Vec::new();
        amf_string(&mut created, "_result");
        amf_number(&mut created, 2.0);
        created.push(0x05);
        amf_number(&mut created, 7.0);
        replies.extend(chunks(3, 20, 0, 0, &created, 128));
        replies.extend(chunks(
            5,
            20,
            7,
            0,
            &said(
                "onStatus",
                0.0,
                &[
                    ("level", Amf::String("status".into())),
                    ("code", Amf::String("NetStream.Play.Reset".into())),
                ],
            ),
            128,
        ));
        replies.extend(chunks(
            5,
            20,
            7,
            0,
            &said(
                "onStatus",
                0.0,
                &[
                    ("level", Amf::String("error".into())),
                    ("code", Amf::String("NetStream.Play.StreamNotFound".into())),
                    ("description", Amf::String("show is not here".into())),
                ],
            ),
            128,
        ));
        let (url, server) = scripted(3, replies, false).await;
        let (got, _dir) = record_from(&url, &Control::new()).await;
        let why = got.unwrap_err().to_string();
        assert_eq!(why, "the server would not play it: show is not here");
        let seen = server.await.unwrap();
        let commands: Vec<String> = seen
            .iter()
            .filter(|(k, _)| *k == 20)
            .filter_map(|(_, b)| amf_values(b).first().and_then(Amf::text).map(String::from))
            .collect();
        assert_eq!(commands, ["connect", "createStream", "play"]);
    }

    #[tokio::test]
    async fn sound_in_an_aggregate_message_is_recorded_until_the_stream_stops() {
        let mut aggregate = Vec::new();
        for (n, tag) in [aac(true), aac(false), aac(false)].iter().enumerate() {
            aggregate.push(8);
            aggregate.extend_from_slice(&(tag.len() as u32).to_be_bytes()[1..]);
            aggregate.extend_from_slice(&(1000 + n as u32 * 23).to_be_bytes()[1..]);
            aggregate.extend_from_slice(&[0, 0, 0, 0]);
            aggregate.extend_from_slice(tag);
            aggregate.extend_from_slice(&((tag.len() + 11) as u32).to_be_bytes());
        }
        // A tag of a kind dm does not record, and one cut short.
        aggregate.extend_from_slice(&[18, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12]);
        aggregate.extend_from_slice(&[8, 0, 0, 50, 0, 0, 0, 0, 0, 0, 0, 1]);
        let mut replies = chunks(6, 22, 1, 0, &aggregate, 128);
        replies.extend(chunks(6, 8, 1, 80, &aac(false), 128));
        replies.extend(chunks(
            5,
            20,
            1,
            0,
            &said(
                "onStatus",
                0.0,
                &[("code", Amf::String("NetStream.Play.Stop".into()))],
            ),
            128,
        ));
        let (url, _server) = scripted(3, replies, false).await;
        let downloaded = Arc::new(AtomicU64::new(0));
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("show.part");
        let bytes = record(&url, &part, &Control::new(), &downloaded)
            .await
            .unwrap();
        assert_eq!(bytes, std::fs::metadata(&part).unwrap().len());
        assert!(std::fs::read(&part)
            .unwrap()
            .windows(4)
            .any(|w| w == b"mp4a"));
    }

    #[tokio::test]
    async fn a_closed_stream_with_nothing_in_it_is_an_error() {
        let replies = chunks(3, 20, 0, 0, &said("close", 0.0, &[]), 128);
        let (url, _server) = scripted(3, replies, false).await;
        let (got, dir) = record_from(&url, &Control::new()).await;
        let why = got.unwrap_err().to_string();
        assert!(why.contains("before sending any of it"), "{why}");
        assert!(!dir.path().join("show.part").exists());
    }

    #[tokio::test]
    async fn a_quiet_server_is_left_when_the_row_is_stopped() {
        let (url, _server) = scripted(3, Vec::new(), true).await;
        let control = Control::new();
        let stopper = control.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            stopper.cancel();
        });
        let (got, _dir) = record_from(&url, &control).await;
        assert!(matches!(got, Err(Error::Cancelled)), "{got:?}");
    }

    #[tokio::test]
    async fn a_server_that_does_not_speak_plain_rtmp_is_refused() {
        let (url, _server) = scripted(6, Vec::new(), false).await;
        let (got, _dir) = record_from(&url, &Control::new()).await;
        let why = got.unwrap_err().to_string();
        assert!(why.contains("does not speak plain RTMP"), "{why}");
        let (got, _dir) = record_from("rtmpe://127.0.0.1/live/show", &Control::new()).await;
        assert!(got.unwrap_err().to_string().contains("rtmpe"));
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..600)) {
            let _ = amf_values(&data);
            let _ = avc_size(&data);
            let _ = hevc_size(&data);
            let dir = tempfile::tempdir().unwrap();
            let mut r = Recorder::new(&dir.path().join("x.mp4"));
            let _ = r.video(0, &data);
            let _ = r.audio(0, &data);
            let _ = r.finish();
        }
    }
}
