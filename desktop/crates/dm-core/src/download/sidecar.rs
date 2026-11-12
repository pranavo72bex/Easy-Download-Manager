// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A checksum file published beside a download (`dm.zip.sha256`, `SHA256SUMS`), found and read
//! when no digest was typed, so a file a release page vouches for is checked without anyone
//! pasting its hash (gaps row F5).

use super::*;
use crate::checksum::Algorithm;

/// The most of a checksum file that is read: one digest is under 200 bytes, and a release's
/// list of every file's digest a few lines more. A longer list is read this far.
const MOST: usize = 8 * 1024;

/// How long the look for a checksum file may take in all: a server that never answers holds a
/// finished download this long at most, and then it is not checked.
const WAIT: Duration = Duration::from_secs(10);

/// The digest a checksum file beside `asked` publishes for the file, where one does; `None`
/// where none answers, none names the file, or the address is not one a file sits beside.
///
/// Only an `http(s)` address whose last segment names something and which has no query is
/// looked beside: `get.php?id=4` has no folder a checksum file could share. Each candidate is
/// one GET to the same server (`dm.zip.sha256`, `.sha512`, `.sha1`, `.md5`, then `SHA256SUMS`
/// and `SHA512SUMS` in the same folder), one after another; what is not there is a quick 404 and
/// nothing is said of it. A checksum file that redirects is followed by dm's rules
/// (`redirects`, which never lets a public address send dm inside), and only to the server the
/// download itself was asked of or landed on.
pub(super) async fn published_beside(
    client: &reqwest::Client,
    asked: &str,
    landed: &str,
    saved_as: &str,
    options: &Options,
) -> Option<Checksum> {
    let base = url::Url::parse(asked).ok()?;
    if !matches!(base.scheme(), "http" | "https") || base.query().is_some() {
        return None;
    }
    let last = base.path_segments()?.next_back()?.to_string();
    if last.is_empty() {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(&last)
        .decode_utf8_lossy()
        .into_owned();
    let names = [decoded.as_str(), saved_as];
    let servers: Vec<(Option<String>, Option<u16>)> = [asked, landed]
        .iter()
        .filter_map(|address| url::Url::parse(address).ok())
        .map(|address| server_of(&address))
        .collect();

    let mut file = base.clone();
    file.set_fragment(None);
    let beside = checksum::BESIDE.iter().map(|&(ending, algorithm)| {
        let mut address = file.clone();
        address.set_path(&format!("{}.{ending}", file.path()));
        (address, algorithm, true)
    });
    let lists = checksum::LISTS
        .iter()
        .filter_map(|&(list, algorithm)| Some((file.join(list).ok()?, algorithm, false)));
    let candidates: Vec<(url::Url, Algorithm, bool)> = beside
        .chain(lists)
        // A checksum file is asked for as a first hop, which no redirect rule judged.
        .filter(|(address, _, _)| !crate::redirects::named_inside(asked, address.as_str()))
        .collect();

    /* One after another, stopping at the first that names the file: never more than one
     * request of the look open at once, so a server's connection limit, or one a person set
     * for it, is not overrun for a file already fetched. */
    let looked = tokio::time::timeout(WAIT, async {
        for (address, algorithm, alone) in &candidates {
            let Some(text) = fetch_small(client, address.as_str(), *alone, &servers, options).await
            else {
                continue;
            };
            if let Some(found) = checksum::listed_for(&text, *algorithm, &names, *alone) {
                return Some(found);
            }
        }
        None
    })
    .await;
    looked.ok().flatten()
}

/// A server as a redirect judges one: its host and its port.
fn server_of(address: &url::Url) -> (Option<String>, Option<u16>) {
    (
        address.host_str().map(str::to_ascii_lowercase),
        address.port_or_known_default(),
    )
}

/// The text of a small file at `address`, or `None` for anything else: an error status, a web
/// page, an answer from another server, a body sent compressed though it was asked for as it
/// is, or a file of one digest longer than [`MOST`]. A list longer than that is read that far.
async fn fetch_small(
    client: &reqwest::Client,
    address: &str,
    alone: bool,
    servers: &[(Option<String>, Option<u16>)],
    options: &Options,
) -> Option<String> {
    let request = get(
        client,
        address,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    );
    let res = or_stop(
        &options.control,
        crate::auth::send(client, request, address),
    )
    .await
    .ok()?;
    if !res.status().is_success() || !servers.contains(&server_of(res.url())) {
        return None;
    }
    let header = |name| {
        res.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_ascii_lowercase)
    };
    if header(reqwest::header::CONTENT_TYPE).is_some_and(|t| t.contains("html"))
        || header(reqwest::header::CONTENT_ENCODING).is_some_and(|c| c != IDENTITY)
    {
        return None;
    }
    if alone && res.content_length().is_some_and(|n| n > MOST as u64) {
        return None;
    }
    let mut body = Vec::new();
    let mut chunks = res.bytes_stream();
    while let Some(chunk) = next_or_stop(&mut chunks, &options.control).await.ok()? {
        body.extend_from_slice(&chunk.ok()?);
        if body.len() > MOST {
            if alone {
                return None;
            }
            // Only whole lines: one cut short could name `dm.zip` where it said `dm.zip.asc`.
            body.truncate(MOST);
            let whole = body.iter().rposition(|&b| b == b'\n').unwrap_or(0);
            body.truncate(whole);
            break;
        }
    }
    if super::http::looks_like_html(&body) {
        return None;
    }
    Some(String::from_utf8_lossy(&body).into_owned())
}
