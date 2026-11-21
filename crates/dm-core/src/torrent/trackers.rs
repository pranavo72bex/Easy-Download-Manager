// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Trackers reached over TCP: those of a magnet link or a torrent, and none behind a proxy that
//! would leak where dm is.

use super::*;

/// Whether a tracker's address is one a proxy carries: HTTP or HTTPS.
/// A `udp://` tracker would be asked directly, around any proxy.
fn tcp_tracker(address: &str) -> bool {
    let lower = address.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// The refusal when a proxy leaves a torrent no way to find peers.
fn no_tracker_behind_proxy() -> Error {
    Error::Internal(
        "behind a proxy dm finds peers only through http or https trackers, and this torrent lists none"
            .into(),
    )
}

/// A magnet with only the trackers a proxy carries: behind one, the DHT is
/// off and a `udp://` tracker would be asked directly with the info-hash
/// and the real address.
pub(super) fn tcp_trackers_of_magnet(magnet: &str) -> Result<String, Error> {
    let (head, query) = magnet.split_once('?').unwrap_or((magnet, ""));
    let mut trackers = 0;
    let kept: Vec<&str> = query
        .split('&')
        .filter(|part| {
            let Some(value) = part.strip_prefix("tr=") else {
                return true;
            };
            let address = percent_encoding::percent_decode_str(value).decode_utf8_lossy();
            let keep = tcp_tracker(&address);
            trackers += usize::from(keep);
            keep
        })
        .collect();
    if trackers == 0 {
        return Err(no_tracker_behind_proxy());
    }
    Ok(format!("{head}?{}", kept.join("&")))
}

/// A `.torrent` with only the trackers a proxy carries, its info dictionary
/// kept byte for byte so it is still the same torrent.
pub(super) fn tcp_trackers_of(metainfo: &[u8]) -> Result<Vec<u8>, Error> {
    let parsed = dm_torrent::metainfo(metainfo)
        .map_err(|e| Error::Internal(format!("this file is not a torrent: {e}")))?;
    let trackers: Vec<String> = parsed
        .trackers
        .into_iter()
        .filter(|t| tcp_tracker(t))
        .collect();
    let Some(first) = trackers.first() else {
        return Err(no_tracker_behind_proxy());
    };
    let string = |out: &mut Vec<u8>, s: &[u8]| {
        out.extend_from_slice(s.len().to_string().as_bytes());
        out.push(b':');
        out.extend_from_slice(s);
    };
    // Its keys in order, as bencoding requires.
    let mut out = b"d".to_vec();
    string(&mut out, b"announce");
    string(&mut out, first.as_bytes());
    string(&mut out, b"announce-list");
    out.extend_from_slice(b"ll");
    for t in &trackers {
        string(&mut out, t.as_bytes());
    }
    out.extend_from_slice(b"ee");
    string(&mut out, b"info");
    out.extend_from_slice(&parsed.info);
    out.push(b'e');
    Ok(out)
}
