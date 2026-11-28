// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The peers' blocklist: fetched, kept fresh on disk, and its size held.

use super::*;

/// How long a blocklist fetched from the web is used before it is fetched again.
const BLOCKLIST_FRESH: Duration = Duration::from_secs(24 * 3600);

/// The largest blocklist read: the lists in use are a few megabytes, gzipped.
const BLOCKLIST_MOST: u64 = 64 * 1024 * 1024;

/// The blocklist as librqbit reads it, a `file:` address: one on this
/// computer as it is; one on the web fetched with EDM's own client (so
/// through the proxy, which librqbit's own fetch would go around) and kept
/// a day beside the parts. One that cannot be read is an error: a torrent
/// does not go on as if the person had asked for no list.
pub(super) async fn blocklist_file(
    client: &reqwest::Client,
    address: &str,
    parts_dir: &Path,
) -> Result<String, Error> {
    let address = address.trim();
    let refused = |why: String| {
        Error::Internal(format!(
            "the peer blocklist at {address} could not be read: {why}"
        ))
    };
    if address.starts_with("file:") {
        return Ok(address.to_string());
    }
    if !(address.starts_with("http://") || address.starts_with("https://")) {
        let path = Path::new(address);
        if !path.is_file() {
            return Err(refused("there is no such file".into()));
        }
        return file_url(path).ok_or_else(|| refused("its path cannot be made an address".into()));
    }
    let folder = parts_dir.parent().unwrap_or(parts_dir);
    let kept = folder.join(format!("peer-blocklist-{:016x}", fnv(address)));
    let fresh = tokio::fs::metadata(&kept)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age < BLOCKLIST_FRESH);
    if !fresh {
        let mut response = client
            .get(address)
            .send()
            .await
            .map_err(|e| refused(e.to_string()))?;
        if !response.status().is_success() {
            return Err(refused(format!(
                "the server answered {}",
                response.status()
            )));
        }
        let too_big = || refused("it is larger than 64 MB".into());
        if response
            .content_length()
            .is_some_and(|n| n > BLOCKLIST_MOST)
        {
            return Err(too_big());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| refused(e.to_string()))? {
            body.extend_from_slice(&chunk);
            if body.len() as u64 > BLOCKLIST_MOST {
                return Err(too_big());
            }
        }
        tokio::fs::create_dir_all(folder)
            .await
            .map_err(Error::Disk)?;
        // Whole or not at all: a torrent started meanwhile reads the last one.
        let partial = kept.with_extension("part");
        tokio::fs::write(&partial, &body)
            .await
            .map_err(Error::Disk)?;
        tokio::fs::rename(&partial, &kept)
            .await
            .map_err(Error::Disk)?;
    }
    file_url(&kept).ok_or_else(|| refused("its copy's path cannot be made an address".into()))
}

/// A short name for an address, for the file its list is kept in.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
