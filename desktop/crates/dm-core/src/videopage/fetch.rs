// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A YouTube page read by dm itself, its streams fetched by dm's engine.
//!
//! YouTube is read by dm's own reader (`crate::youtube`): the same few
//! requests the reference managers make, and the public-domain solver
//! scripts for the player's `n` values. The result is a list of plain
//! `https` addresses for each stream, which dm fetches on every connection
//! set, with resume, splitting and the speed ceiling as any download has
//! them, and joins picture and sound itself.
//!
//! Every other site's video comes from the player's own requests, caught
//! by the browser extension and handed to dm as a regular download.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{finish_dm_tagged, remove_staging};
use crate::download::{Finished, Options, Update};
use crate::{category, Error};

mod premiere;
mod streams;

pub use premiere::waits_for_premiere;
use streams::{fetch, resolve};
#[cfg(test)]
use streams::{interned, iso_day, merge_progress, subtitles, sum};

/// A reading younger than this whose stream is refused is fetched once more after a short
/// wait, rather than read again.
const YOUNG_FOR: Duration = Duration::from_secs(10);

/// How old a reading is let grow before that one more try: a refusal of a reading a few
/// seconds old passes.
const REFUSED_AGAIN_AT: Duration = Duration::from_secs(6);

/// The longest wait for the site's "available at" before the streams are fetched.
const AVAILABLE_WAIT_SECS: u64 = 10;

/// The longest the video's picture is waited for: not worth holding the finished video back.
const COVER_WITHIN: Duration = Duration::from_secs(10);

/// A YouTube page read by dm itself, its streams fetched by dm's engine.
pub async fn download_page(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    if crate::youtube::video_id(url).is_none() {
        return Err(Error::Internal(
            "that is a web page; its video, if any, comes from the browser's \
             Download video button when the page plays"
                .into(),
        ));
    }
    let staging = &options.parts_dir;
    let part = options.span.is_some();
    tokio::fs::create_dir_all(staging)
        .await
        .map_err(Error::Disk)?;

    let read = resolve(url, options, false).await?;
    // A premiere, or a live stream not begun: waited for, its row saying so, then read again
    // (gaps row Y5).
    let read = premiere::waited(url, options, report.as_ref(), read).await?;
    // A live stream (YouTube's own HLS playlist of it): recorded by the engine's live HLS
    // recording, as a stream's address is, from now until it ends or is stopped (gaps row Y5).
    if read.info.is_live == Some(true) && !part {
        if let Some(live) = read
            .info
            .single
            .url
            .as_deref()
            .filter(|u| crate::youtube::playlist_address(u))
            .filter(|_| read.info.single.protocol.as_deref() == Some("m3u8_native"))
        {
            return crate::download::once_more(client, live, options, report).await;
        }
    }
    let Some(plan) = read.plan(staging) else {
        if part {
            return Err(Error::Internal(
                "this video's streams are not plain files with an index, so a part of it cannot be cut"
                    .into(),
            ));
        }
        return Err(Error::Internal(
            "this video's streams cannot be fetched by dm's own engine".into(),
        ));
    };

    let numbered: Vec<String> = plan
        .streams
        .iter()
        .filter(|s| s.numbered)
        .map(|s| s.id.clone())
        .collect();
    let mut outcome = fetch_renewing(client, url, plan, read, options, report.clone(), false).await;
    // Numbered pieces whose first does not say how many follow, or none of which could be
    // fetched: the video from its other formats, as before dm read numbered pieces (gaps row
    // Y14). Nothing new is said on the row; the log says why.
    if matches!(&outcome, Err(Error::Internal(why)) if why == crate::download::NUMBERED_UNREAD) {
        crate::note!(
            info,
            "dm: {url}: fetched from its formats that are not numbered pieces"
        );
        for id in &numbered {
            let _ = tokio::fs::remove_dir_all(staging.join(format!(".dm-f{id}"))).await;
        }
        outcome = match resolve(url, options, true).await {
            Ok(read) => match read.plan(staging) {
                Some(plan) => {
                    fetch_renewing(client, url, plan, read, options, report.clone(), true).await
                }
                None => Err(Error::Internal(
                    "this video's streams cannot be fetched by dm's own engine".into(),
                )),
            },
            Err(why) => Err(why),
        };
    }
    match outcome {
        Ok(done) => Ok(done),
        Err(Error::Paused) => Err(Error::Paused),
        Err(Error::Cancelled) => {
            remove_staging(staging).await;
            Err(Error::Cancelled)
        }
        Err(why) => Err(why),
    }
}

/// `plan` fetched, waiting first until its streams are served; where a stream is refused,
/// once more after a short wait (a reading a few seconds old), and from the page read again
/// where its addresses have run out. `without_numbered` reads it again as it was read.
async fn fetch_renewing(
    client: &reqwest::Client,
    url: &str,
    plan: Plan,
    read: Read,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    without_numbered: bool,
) -> Result<Finished, Error> {
    let mut current = (plan, read);
    wait_until_available(&current.1, options).await?;
    let mut outcome = fetch(client, &current.0, &current.1, options, report.clone()).await;
    let young = current.1.made.elapsed() < YOUNG_FOR;
    if young && matches!(&outcome, Err(Error::Refused { status: 403, .. })) {
        let left = REFUSED_AGAIN_AT.saturating_sub(current.1.made.elapsed());
        crate::download::or_stop(&options.control, async {
            tokio::time::sleep(left).await;
            Ok(())
        })
        .await?;
        outcome = fetch(client, &current.0, &current.1, options, report.clone()).await;
    }
    if matches!(
        &outcome,
        Err(Error::Refused {
            status: 403 | 410,
            ..
        })
    ) {
        match renewed(url, &current.0, options, without_numbered).await {
            Ok(Some(next)) => {
                current = next;
                outcome = fetch(client, &current.0, &current.1, options, report).await;
            }
            Ok(None) => {}
            Err(stop) => outcome = Err(stop),
        }
    }
    outcome
}

/// The page read again, after a stream's address was refused (403 or 410),
/// where that is what an address that has run out looks like: the plan's
/// own `expire` has passed, or bytes were already fetched, so the address
/// served once. One refused at the first ask never did, and is not read as
/// before. `None` where the fresh reading is not the same streams (another
/// format, name or size): what is staged belongs to the old ones.
async fn renewed(
    url: &str,
    plan: &Plan,
    options: &Options,
    without_numbered: bool,
) -> Result<Option<(Plan, Read)>, Error> {
    let staging = &options.parts_dir;
    if !plan.streams.iter().any(Stream::expired) && !fetched_any(staging).await {
        return Ok(None);
    }
    let read = match resolve(url, options, without_numbered).await {
        Ok(read) => read,
        Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
        Err(_) => return Ok(None),
    };
    Ok(read
        .plan(staging)
        .filter(|fresh| fresh.same_streams(plan))
        .map(|fresh| (fresh, read)))
}

/// Whether any stream has bytes on disk in its parts folder, by the ledgers
/// the engine keeps there.
async fn fetched_any(staging: &Path) -> bool {
    let Ok(mut folders) = tokio::fs::read_dir(staging).await else {
        return false;
    };
    while let Ok(Some(folder)) = folders.next_entry().await {
        if !folder.file_name().to_string_lossy().starts_with(".dm-f") {
            continue;
        }
        let Ok(mut parts) = tokio::fs::read_dir(folder.path().join("parts")).await else {
            continue;
        };
        while let Ok(Some(part)) = parts.next_entry().await {
            if !part.file_name().to_string_lossy().starts_with("done-") {
                continue;
            }
            let held = tokio::fs::read_to_string(part.path()).await;
            if held.is_ok_and(|text| text.trim().parse::<u64>().is_ok_and(|n| n > 0)) {
                return true;
            }
        }
    }
    false
}

/// What dm's reader said about a page: the reading as JSON, and the parts
/// dm reads from it.
struct Read {
    #[allow(dead_code)]
    text: String,
    info: Info,
    /// When it was made: YouTube serves a TV client's streams a few seconds
    /// after it hands out their addresses.
    made: std::time::Instant,
}

impl Read {
    fn new(text: String, info: Info) -> Read {
        Read {
            text,
            info,
            made: std::time::Instant::now(),
        }
    }

    /// The latest `available_at` of what it would fetch (Unix seconds):
    /// the moment the site serves it, which a player waits for.
    fn available_at(&self) -> Option<u64> {
        let formats = self.info.requested_formats.iter().flatten();
        formats
            .filter_map(|f| f.available_at)
            .chain(self.info.available_at)
            .max()
    }
}

/// Waits until `read`'s streams are served, as the site asks (at most ten
/// seconds; a pause or cancel ends the wait).
async fn wait_until_available(read: &Read, options: &Options) -> Result<(), Error> {
    let Some(at) = read.available_at() else {
        return Ok(());
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if at <= now {
        return Ok(());
    }
    let wait = Duration::from_secs((at - now).min(AVAILABLE_WAIT_SECS));
    crate::download::or_stop(&options.control, async {
        tokio::time::sleep(wait).await;
        Ok(())
    })
    .await
}

#[derive(Debug, Default, serde::Deserialize)]
struct Info {
    /// The path the video would be saved under (the picture and sound
    /// joined), from the same template its own download uses.
    #[serde(default)]
    filename: Option<String>,
    /// Which reader made this reading, its id for the video,
    /// and how many videos the page held (`unnumbered`).
    #[serde(default)]
    extractor_key: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    n_entries: Option<u64>,
    #[serde(default)]
    read_by: Option<String>,
    /// When the site serves the one format chosen (Unix seconds).
    #[serde(default)]
    available_at: Option<u64>,
    /// A picture and a sound to be joined, where it chose two.
    #[serde(default)]
    requested_formats: Option<Vec<Format>>,
    /// The one format, where it chose one.
    #[serde(flatten)]
    single: Format,
    #[serde(default)]
    is_live: Option<bool>,
    /// When a premiere, or a live stream not begun, is set to start (Unix seconds): it names
    /// no stream until then (gaps row Y5).
    #[serde(default)]
    premiere_at: Option<u64>,
    /// Subtitles asked for, by language.
    #[serde(default)]
    requested_subtitles: Option<HashMap<String, Subtitle>>,
    /// The video's title, its channel, when it was published and its page: written into the
    /// joined file (gaps row Y11).
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    upload_date: Option<String>,
    #[serde(default)]
    webpage_url: Option<String>,
    /// Its picture's address, written into the file as its cover (gaps row Y11).
    #[serde(default)]
    thumbnail: Option<String>,
    /// Its chapters, written into the file.
    #[serde(default)]
    chapters: Option<Vec<Chapter>>,
}

/// A chapter of a video: where it starts, in seconds, and its name.
#[derive(Debug, Clone, serde::Deserialize)]
struct Chapter {
    start_time: f64,
    title: String,
}

#[derive(Debug, Default, Clone, serde::Deserialize)]
struct Format {
    #[serde(default)]
    url: Option<String>,
    /// When the site serves it (Unix seconds), where it says.
    #[serde(default)]
    available_at: Option<u64>,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    format_id: Option<String>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    http_headers: HashMap<String, serde_json::Value>,
    /// Left as it came: a figure of an unexpected kind must not make the
    /// whole reading unreadable (`chunk`).
    #[serde(default)]
    downloader_options: Option<serde_json::Value>,
    /// The exact size, where the site stated it; the estimate is not one.
    #[serde(default)]
    filesize: Option<serde_json::Value>,
    /// dm's own reading's word on where the stream's first boxes and its
    /// index of pieces are ("start-end"), for cutting a part (`clip`).
    #[serde(default)]
    dm_init_range: Option<String>,
    #[serde(default)]
    dm_index_range: Option<String>,
}

/// A "start-end" byte range, both ends inclusive.
fn range_of(text: Option<&str>) -> Option<(u64, u64)> {
    let (a, b) = text?.trim().split_once('-')?;
    let (a, b) = (a.parse::<u64>().ok()?, b.parse::<u64>().ok()?);
    // A stream's first boxes and its index are kilobytes: a range of more
    // than a few megabytes is not one.
    (b >= a && b - a < 8 << 20).then_some((a, b))
}

impl Format {
    /// The most bytes one request for this format may ask for, where the reading
    /// says its server holds each to that.
    fn chunk(&self) -> Option<u64> {
        // `DM_NO_CHUNK` switches the cap off, for timing a long video
        // with and without it on a real connection (gaps.md row 22).
        if crate::env::get_os("DM_NO_CHUNK").is_some() {
            return None;
        }
        self.downloader_options
            .as_ref()?
            .get("http_chunk_size")?
            .as_u64()
            .filter(|&n| n > 0)
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct Subtitle {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    ext: Option<String>,
}

/// What dm fetches: each stream and the file it goes to, and the file they
/// make once joined (or the one stream, where there is one).
#[derive(Debug, PartialEq)]
struct Plan {
    streams: Vec<Stream>,
    produced: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
struct Stream {
    url: String,
    /// The agent the reading says to ask with: a site can bind its addresses
    /// to the client that was given them.
    agent: Option<String>,
    /// Where the stream is put whole: beside `produced`, named by format id.
    target: PathBuf,
    /// The format's name, which keeps one stream's parts apart from
    /// another's across a pause.
    id: String,
    /// The most one request may ask for; see `Format::chunk`.
    chunk: Option<u64>,
    /// The size the site stated, which a renewed reading must agree with.
    size: Option<u64>,
    /// The rest of what the reading says the server is asked with; kept to
    /// this stream's requests (`crate::download::Headers`).
    headers: crate::download::Headers,
    /// Where its first boxes and its index of pieces are, where the
    /// reading said, for cutting a part of it.
    init: Option<(u64, u64)>,
    index: Option<(u64, u64)>,
    /// Served only as numbered pieces (`http_dash_segments`, `&sq=N`), fetched by the DASH
    /// piece fetcher rather than as one file (gaps row Y14).
    numbered: bool,
}

impl Stream {
    /// Whether the address says it has run out: a generic `expire` query
    /// value (seconds since 1970) that has passed. Without one, no.
    fn expired(&self) -> bool {
        let Ok(address) = reqwest::Url::parse(&self.url) else {
            return false;
        };
        let at = address
            .query_pairs()
            .find(|(name, _)| name == "expire")
            .and_then(|(_, value)| value.parse::<u64>().ok());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        at.is_some_and(|at| at <= now)
    }
}

impl Plan {
    /// Whether `other` is the same streams read again: what was fetched so
    /// far is only good for these. The addresses are what differs.
    fn same_streams(&self, other: &Plan) -> bool {
        self.produced == other.produced
            && self.streams.len() == other.streams.len()
            && self.streams.iter().zip(&other.streams).all(|(a, b)| {
                a.id == b.id
                    && a.target == b.target
                    && (a.size.is_none() || b.size.is_none() || a.size == b.size)
            })
    }
}

impl Read {
    /// The streams dm fetches itself, or `None` where any of them is not a
    /// plain file (or the reading says too little to be sure).
    fn plan(&self, staging: &Path) -> Option<Plan> {
        let info = &self.info;
        if info.is_live == Some(true) {
            return None;
        }
        let produced = unnumbered(PathBuf::from(info.filename.as_deref()?), info);
        // Only under the staging folder it was told to use.
        if !produced.starts_with(staging) || produced.file_stem().is_none() {
            return None;
        }
        let formats: Vec<&Format> = match &info.requested_formats {
            Some(pair) if !pair.is_empty() => pair.iter().collect(),
            _ => vec![&info.single],
        };
        let joined = formats.len() > 1;
        let stem = produced.file_stem()?.to_string_lossy().into_owned();
        let dir = produced.parent()?;
        let mut streams = Vec::new();
        for format in formats {
            let plain = matches!(format.protocol.as_deref(), Some("http" | "https"));
            // dm's own reading of a stream served as numbered pieces (gaps row Y14).
            let numbered = format.protocol.as_deref() == Some("http_dash_segments")
                && info.read_by.as_deref() == Some("dm");
            let url = format.url.as_deref()?;
            if !(plain || numbered) || !(url.starts_with("https://") || url.starts_with("http://"))
            {
                return None;
            }
            let id: String = format
                .format_id
                .as_deref()
                .unwrap_or("0")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .take(40)
                .collect();
            let ext = format.ext.as_deref().unwrap_or("mp4");
            // `unknown_video` is the reader's for a type it cannot name.
            if ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            let target = if joined {
                dir.join(format!("{stem}.f{id}.{ext}"))
            } else {
                produced.clone()
            };
            let agent = format
                .http_headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
                .and_then(|(_, value)| value.as_str())
                .map(str::to_string);
            streams.push(Stream {
                url: url.to_string(),
                agent,
                target,
                id,
                chunk: format.chunk(),
                size: format.filesize.as_ref().and_then(|v| v.as_u64()),
                headers: crate::download::Headers::of(
                    format
                        .http_headers
                        .iter()
                        .filter_map(|(name, value)| Some((name.as_str(), value.as_str()?))),
                ),
                init: range_of(format.dm_init_range.as_deref()),
                index: range_of(format.dm_index_range.as_deref()),
                numbered,
            });
        }
        Some(Plan { streams, produced })
    }
}

impl Plan {
    /// The plan for a part of the video: the file and each stream's part
    /// named for the span ("Talk [id] (4m10s-4m45s).mp4"), so a part's
    /// bytes never pass for the whole's, and the whole's parts are left
    /// alone.
    fn for_part(&self, span: crate::clip::Span) -> Plan {
        let stem = self
            .produced
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = self
            .produced
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mp4".into());
        let stem = format!("{stem} ({})", span.words());
        let produced = self.produced.with_file_name(format!("{stem}.{ext}"));
        let joined = self.streams.len() > 1;
        let streams = self
            .streams
            .iter()
            .map(|stream| {
                let target = if joined {
                    let ext = stream
                        .target
                        .extension()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "mp4".into());
                    produced.with_file_name(format!("{stem}.f{}.{ext}", stream.id))
                } else {
                    produced.clone()
                };
                Stream {
                    target,
                    ..stream.clone()
                }
            })
            .collect();
        Plan { streams, produced }
    }
}

/// The name of a page's only `<video>` without the number a reader gives it.
///
/// the reader of a plain page's media (`HTML5MediaEmbed`) numbers every
/// video it finds, one or several: a page titled "Talk" with one video was
/// saved as "Talk (1) [watch-1].mp4". Other download managers name it by the page alone,
/// and a number with nothing to tell apart says there is another.
fn unnumbered(produced: PathBuf, info: &Info) -> PathBuf {
    let only_one = info.extractor_key.as_deref() == Some("HTML5MediaEmbed")
        // One video: the reader leaves the playlist fields out (several: `n_entries`).
        && info.n_entries.unwrap_or(1) == 1
        && info.id.as_deref().is_some_and(|id| id.ends_with("-1"));
    let (true, Some(stem), Some(ext)) = (
        only_one,
        produced.file_stem().and_then(|s| s.to_str()),
        produced.extension().and_then(|e| e.to_str()),
    ) else {
        return produced;
    };
    // Matched in the name as written, not against `id`: the reader writes the
    // characters a name cannot hold as others ("watch?v=ok" became
    // "watch？v=ok").
    let Some((title, rest)) = stem.rsplit_once(" (1) [") else {
        return produced;
    };
    match rest.strip_suffix("-1]") {
        Some(id) if !id.contains(" [") => produced.with_file_name(format!("{title} [{id}].{ext}")),
        _ => produced,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod coverage_tests;
