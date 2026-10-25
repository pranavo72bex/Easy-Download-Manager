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
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// RTMPT: the same conversation carried in HTTP requests, for networks
    /// that let only the web through.
    pub tunnel: bool,
    /// The application: every part of the path but the last.
    pub app: String,
    /// The stream's name: the last part, with its query.
    pub play: String,
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
    let tc_url = format!(
        "{scheme}://{}:{port}/{app}",
        if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        }
    );
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
