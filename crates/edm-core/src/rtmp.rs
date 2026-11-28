// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An RTMP client of EDM's own, for `rtmp://`, `rtmps://` and `rtmpt://`
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
pub fn target(url: &str) -> Result<Target, Error> {
    let (scheme, rest) = url
        .trim()
        .split_once("://")
        .ok_or_else(|| Error::Internal("not an rtmp:// address".into()))?;
    let scheme = scheme.to_ascii_lowercase();
    let (tls, tunnel) = match scheme.as_str() {
        "rtmp" => (false, false),
        "rtmps" => (true, false),
        "rtmpt" => (false, true),
        other => return Err(Error::Internal(format!("EDM does not speak {other}://"))),
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
        return Err(Error::Internal("the address names no server".into()));
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
        return Err(Error::Internal(
            "the address names no application and stream".into(),
        ));
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
mod tests;
