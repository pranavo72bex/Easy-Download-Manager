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

    let read = resolve(url, options).await?;
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

    let mut current = (plan, read);
    wait_until_available(&current.1, options).await?;
    let mut outcome = fetch(client, &current.0, &current.1, options, report.clone()).await;
    let young = current.1.made.elapsed() < Duration::from_secs(10);
    if young && matches!(&outcome, Err(Error::Refused { status: 403, .. })) {
        let left = Duration::from_secs(6).saturating_sub(current.1.made.elapsed());
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
        match renewed(url, &current.0, options).await {
            Ok(Some(next)) => {
                current = next;
                outcome = fetch(client, &current.0, &current.1, options, report.clone()).await;
            }
            Ok(None) => {}
            Err(stop) => outcome = Err(stop),
        }
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

/// The page read again, after a stream's address was refused (403 or 410),
/// where that is what an address that has run out looks like: the plan's
/// own `expire` has passed, or bytes were already fetched, so the address
/// served once. One refused at the first ask never did, and is not read as
/// before. `None` where the fresh reading is not the same streams (another
/// format, name or size): what is staged belongs to the old ones.
async fn renewed(url: &str, plan: &Plan, options: &Options) -> Result<Option<(Plan, Read)>, Error> {
    let staging = &options.parts_dir;
    if !plan.streams.iter().any(Stream::expired) && !fetched_any(staging).await {
        return Ok(None);
    }
    let read = match resolve(url, options).await {
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
    let wait = Duration::from_secs((at - now).min(10));
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
    #[allow(dead_code)]
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
        if std::env::var_os("DM_NO_CHUNK").is_some() {
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
            let url = format.url.as_deref()?;
            if !plain || !(url.starts_with("https://") || url.starts_with("http://")) {
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

/// Reads a YouTube page by dm's own YouTube reader (`crate::youtube`).
async fn resolve(url: &str, options: &Options) -> Result<Read, Error> {
    let staging = &options.parts_dir;
    let own = crate::download::or_stop(
        &options.control,
        crate::youtube::read_for(url, options, staging),
    )
    .await;
    match own {
        Ok(text) => {
            let info: Info = serde_json::from_str(&text).map_err(|e| {
                Error::Internal(format!(
                    "dm's own reading of the page could not be read ({e})"
                ))
            })?;
            Ok(Read::new(text, info))
        }
        Err(stop @ (Error::Paused | Error::Cancelled)) => Err(stop),
        Err(why) => Err(why),
    }
}

/// Every stream fetched by dm's own engine at once, then joined and placed
/// as a reader's own output would have been.
async fn fetch(
    client: &reqwest::Client,
    plan: &Plan,
    read: &Read,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    // A part of the video: each stream's part, cut by its index, under
    // names of their own.
    let part = options.span.map(|span| plan.for_part(span));
    let plan = part.as_ref().unwrap_or(plan);
    if let Some(tx) = &report {
        let name = plan
            .produced
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = tx
            .send(Update::Named {
                category: category::classify(&name, None),
                name,
            })
            .await;
    }
    let (senders, merging) = merge_progress(report, plan.streams.len());
    let fetching = plan
        .streams
        .iter()
        .zip(senders)
        .map(|(stream, tx)| async move {
            match options.span {
                Some(span) => fetch_part(client, stream, span, options, tx).await,
                None => fetch_stream(client, stream, options, tx).await,
            }
        });
    let fetched = futures_util::future::try_join_all(fetching).await;
    if let Some(merging) = merging {
        let _ = merging.await;
    }
    fetched?;

    // Left beside the video as they are left, and read back as its are.
    subtitles(client, read, &plan.produced, options).await;
    let found = crate::subtitles::beside(&plan.produced);
    let (files, texts): (Vec<PathBuf>, Vec<crate::subtitles::Subtitle>) = found.into_iter().unzip();
    let info = &read.info;
    let tags = crate::mp4mux::Tags {
        title: info.title.clone().unwrap_or_default(),
        artist: info.uploader.clone().unwrap_or_default(),
        date: info.upload_date.clone().unwrap_or_default(),
        source: info.webpage_url.clone().unwrap_or_default(),
    };
    let outcome = finish_dm_tagged(&plan.produced, texts, tags, options).await;
    for file in files {
        let _ = tokio::fs::remove_file(file).await;
    }
    outcome.map(|done| Finished {
        connections: options.connections,
        ..done
    })
}

/// One stream, by dm's engine, into its own parts folder, then moved to
/// where it would have been left.
async fn fetch_stream(
    client: &reqwest::Client,
    stream: &Stream,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<(), Error> {
    if tokio::fs::metadata(&stream.target)
        .await
        .is_ok_and(|m| m.len() > 0)
    {
        return Ok(());
    }
    if let Some(agent) = stream.agent.as_deref() {
        crate::prefer_agent(&stream.url, interned(agent));
    }
    let own = options.parts_dir.join(format!(".dm-f{}", stream.id));
    let name = stream
        .target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    let opts = Options {
        parts_dir: own.join("parts"),
        dest_dir: own.join("out"),
        save_name: name,
        sound: None,
        media: false,
        checksum: None,
        remux: false,
        server_dates: false,
        ..options.clone()
    };
    let hints = crate::download::StreamHints {
        chunk: stream.chunk,
        headers: stream.headers.clone(),
    };
    let done =
        crate::download::download_video_stream_with(client, &stream.url, &opts, report, hints)
            .await?;
    tokio::fs::rename(&done.path, &stream.target).await?;
    let _ = tokio::fs::remove_dir_all(&own).await;
    Ok(())
}

/// The part of one stream within the span, cut by the stream's own index
/// (`crate::clip`), to where the whole stream would have gone.
async fn fetch_part(
    client: &reqwest::Client,
    stream: &Stream,
    span: crate::clip::Span,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<(), Error> {
    let source = crate::clip::Source {
        url: stream.url.clone(),
        init: stream.init,
        index: stream.index,
        agent: stream.agent.clone(),
        headers: stream.headers.clone(),
        chunk: stream.chunk,
        target: stream.target.clone(),
    };
    crate::clip::fetch_part(client, &source, span, options, report)
        .await
        .map(|_| ())
}

/// The subtitles the reading names, fetched and left beside the video
/// as they are left (`name.<language>.vtt`). One that cannot be had is
/// passed over: subtitles are not worth the video.
async fn subtitles(client: &reqwest::Client, read: &Read, produced: &Path, options: &Options) {
    // Only where the video is to carry them: what is fetched is put inside it, then removed.
    if !options.subtitles {
        return;
    }
    let Some(asked) = read.info.requested_subtitles.as_ref() else {
        return;
    };
    let (Some(dir), Some(stem)) = (produced.parent(), produced.file_stem()) else {
        return;
    };
    for (language, subtitle) in asked {
        let plain_name = !language.is_empty()
            && language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        let (Some(url), true, true) = (
            subtitle.url.as_deref(),
            subtitle.ext.as_deref() == Some("vtt"),
            plain_name,
        ) else {
            continue;
        };
        if let Ok(got) = crate::download::fetch_capped(client, url, 16 * 1024 * 1024, options).await
        {
            let path = dir.join(format!("{}.{language}.vtt", stem.to_string_lossy()));
            let _ = tokio::fs::write(path, got.body).await;
        }
    }
}

/// A sender for each stream's progress, and the task that adds them up into
/// one for the row: the bytes and sizes of all of them, the speeds summed.
/// What a stream says of its name or version is not the row's.
fn merge_progress(
    report: Option<tokio::sync::mpsc::Sender<Update>>,
    streams: usize,
) -> (
    Vec<Option<tokio::sync::mpsc::Sender<Update>>>,
    Option<tokio::task::JoinHandle<()>>,
) {
    let Some(report) = report else {
        return ((0..streams).map(|_| None).collect(), None);
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, Update)>(64);
    let mut senders = Vec::new();
    for n in 0..streams {
        let (own, mut heard) = tokio::sync::mpsc::channel::<Update>(16);
        let tx = tx.clone();
        tokio::spawn(async move {
            while let Some(update) = heard.recv().await {
                if tx.send((n, update)).await.is_err() {
                    return;
                }
            }
        });
        senders.push(Some(own));
    }
    drop(tx);
    let merging = tokio::spawn(async move {
        let mut latest: Vec<Option<crate::Progress>> = vec![None; streams];
        while let Some((n, update)) = rx.recv().await {
            match update {
                Update::Progress(progress) => {
                    if let Some(slot) = latest.get_mut(n) {
                        *slot = Some(progress);
                    }
                    let _ = report.send(Update::Progress(sum(&latest))).await;
                }
                retrying @ Update::Retrying { .. } => {
                    let _ = report.send(retrying).await;
                }
                Update::Named { .. }
                | Update::Validator(_)
                | Update::Swarm(_)
                | Update::Files(_) => {}
            }
        }
    });
    (senders, Some(merging))
}

/// The streams' progress as one: a total only once every stream has said
/// its own.
fn sum(latest: &[Option<crate::Progress>]) -> crate::Progress {
    let downloaded = latest.iter().flatten().map(|p| p.downloaded).sum::<u64>();
    let speed = latest.iter().flatten().map(|p| p.speed).sum::<f64>();
    let total = latest
        .iter()
        .map(|p| p.as_ref().and_then(|p| p.total))
        .sum::<Option<u64>>();
    let eta = total.and_then(|total| {
        (speed > 0.0).then(|| {
            std::time::Duration::from_secs_f64(total.saturating_sub(downloaded) as f64 / speed)
        })
    });
    crate::Progress {
        downloaded,
        total,
        speed,
        eta,
        estimated: false,
    }
}

/// An agent as dm's table of agents keeps them: for the life of the
/// program. A handful of distinct ones in a run, each kept once; past a
/// bound, a browser's.
fn interned(agent: &str) -> &'static str {
    static KEPT: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
        std::sync::OnceLock::new();
    let mut kept = crate::lock(KEPT.get_or_init(Default::default));
    if let Some(found) = kept.get(agent) {
        return found;
    }
    if kept.len() >= 32 {
        return crate::BROWSER_AGENT;
    }
    let leaked: &'static str = Box::leak(agent.to_string().into_boxed_str());
    kept.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Read {
        Read::new(json.to_string(), serde_json::from_str(json).unwrap())
    }

    #[test]
    fn a_reading_says_when_its_streams_are_served() {
        let none = read(r#"{"url": "https://e.com/v"}"#);
        assert_eq!(none.available_at(), None);
        let one = read(r#"{"url": "https://e.com/v", "available_at": 1700000004}"#);
        assert_eq!(one.available_at(), Some(1_700_000_004));
        let pair = read(
            r#"{"requested_formats": [
                {"url": "https://e.com/a", "available_at": 1700000002},
                {"url": "https://e.com/b", "available_at": 1700000005}
            ]}"#,
        );
        assert_eq!(pair.available_at(), Some(1_700_000_005));
    }

    #[test]
    fn a_picture_and_sound_over_https_are_fetched_by_dm_beside_the_joined_name() {
        let r = read(
            r#"{"filename": "/s/Talk [abc].mp4", "ext": "mp4",
                "requested_formats": [
                  {"format_id": "137", "ext": "mp4", "protocol": "https",
                   "url": "https://cdn.example/v?itag=137",
                   "http_headers": {"User-Agent": "Agent/1", "Accept": "*/*"}},
                  {"format_id": "251-drc", "ext": "webm", "protocol": "https",
                   "url": "https://cdn.example/v?itag=251", "http_headers": {}}
                ]}"#,
        );
        let plan = r.plan(Path::new("/s")).expect("a plan");
        assert_eq!(plan.produced, PathBuf::from("/s/Talk [abc].mp4"));
        assert_eq!(plan.streams.len(), 2);
        assert_eq!(
            plan.streams[0].target,
            PathBuf::from("/s/Talk [abc].f137.mp4")
        );
        assert_eq!(plan.streams[0].agent.as_deref(), Some("Agent/1"));
        assert_eq!(
            plan.streams[1].target,
            PathBuf::from("/s/Talk [abc].f251-drc.webm")
        );
        assert_eq!(plan.streams[1].agent, None);
    }

    #[test]
    fn a_pages_only_video_is_named_without_a_number() {
        let one = read(
            r#"{"filename": "/s/QA clip (1) [watch-1].mp4", "ext": "mp4",
                "extractor_key": "HTML5MediaEmbed", "id": "watch-1",
                "format_id": "0", "protocol": "http", "url": "http://site/clip.mp4"}"#,
        );
        let plan = one.plan(Path::new("/s")).expect("a plan");
        assert_eq!(plan.produced, PathBuf::from("/s/QA clip [watch].mp4"));
        assert_eq!(plan.streams[0].target, plan.produced);
        // Two videos on the page: the numbers tell them apart and stay.
        let two = read(
            r#"{"filename": "/s/QA clip (1) [watch-1].mp4", "ext": "mp4",
                "extractor_key": "HTML5MediaEmbed", "id": "watch-1", "n_entries": 2,
                "format_id": "0", "protocol": "http", "url": "http://site/clip.mp4"}"#,
        );
        assert_eq!(
            two.plan(Path::new("/s")).unwrap().produced,
            PathBuf::from("/s/QA clip (1) [watch-1].mp4")
        );
        // Another site's title that ends in "(1)" is its own.
        let other = read(
            r#"{"filename": "/s/Part (1) [abc-1].mp4", "ext": "mp4",
                "extractor_key": "Vimeo", "id": "abc-1", "n_entries": 1,
                "format_id": "0", "protocol": "https", "url": "https://v/1.mp4"}"#,
        );
        assert_eq!(
            other.plan(Path::new("/s")).unwrap().produced,
            PathBuf::from("/s/Part (1) [abc-1].mp4")
        );
    }

    #[test]
    fn a_numbered_name_is_matched_as_it_is_written() {
        let r = read(
            r#"{"filename": "/s/QA clip (1) [watch\uff1fv=ok-1].mp4", "ext": "mp4",
                "extractor_key": "HTML5MediaEmbed", "id": "watch?v=ok-1",
                "format_id": "0", "protocol": "http", "url": "http://site/clip.mp4"}"#,
        );
        assert_eq!(
            r.plan(Path::new("/s")).unwrap().produced,
            PathBuf::from("/s/QA clip [watch\u{ff1f}v=ok].mp4")
        );
    }

    #[test]
    fn a_chunk_size_is_read_where_given_and_ignored_where_odd() {
        let r = read(
            r#"{"filename": "/s/a.mp4", "requested_formats": [
                {"format_id": "1", "protocol": "https", "url": "https://x/1",
                 "downloader_options": {"http_chunk_size": 10485760}},
                {"format_id": "2", "protocol": "https", "url": "https://x/2",
                 "downloader_options": {"http_chunk_size": "ten"}},
                {"format_id": "3", "protocol": "https", "url": "https://x/3",
                 "downloader_options": {"http_chunk_size": 0}},
                {"format_id": "4", "protocol": "https", "url": "https://x/4"}]}"#,
        );
        let plan = r.plan(Path::new("/s")).expect("a plan");
        let chunks: Vec<_> = plan.streams.iter().map(|s| s.chunk).collect();
        assert_eq!(chunks, [Some(10_485_760), None, None, None]);
    }

    #[test]
    fn a_type_dm_cannot_name_is_still_fetched_by_dm() {
        let r = read(
            r#"{"filename": "/s/file [file].unknown_video", "format_id": "0",
                "ext": "unknown_video", "protocol": "http", "url": "http://h/file.bin"}"#,
        );
        assert!(r.plan(Path::new("/s")).is_some());
    }

    #[test]
    fn one_plain_format_is_fetched_under_the_name_itself() {
        let r = read(
            r#"{"filename": "/s/clip [clip].mp4", "format_id": "mp4", "ext": "mp4",
                "protocol": "http", "url": "http://127.0.0.1:9/clip.mp4"}"#,
        );
        let plan = r.plan(Path::new("/s")).expect("a plan");
        assert_eq!(plan.streams.len(), 1);
        assert_eq!(plan.streams[0].target, plan.produced);
    }

    #[test]
    fn streams_in_pieces_live_ones_and_names_elsewhere_are_not_planned() {
        let pieces = read(
            r#"{"filename": "/s/a.mp4", "requested_formats": [
                {"format_id": "1", "protocol": "https", "url": "https://x/1"},
                {"format_id": "2", "protocol": "m3u8_native", "url": "https://x/2.m3u8"}]}"#,
        );
        assert_eq!(pieces.plan(Path::new("/s")), None);
        let live = read(
            r#"{"filename": "/s/a.mp4", "is_live": true, "protocol": "https", "url": "https://x/1"}"#,
        );
        assert_eq!(live.plan(Path::new("/s")), None);
        let elsewhere =
            read(r#"{"filename": "/etc/a.mp4", "protocol": "https", "url": "https://x/1"}"#);
        assert_eq!(elsewhere.plan(Path::new("/s")), None);
        let unnamed = read(r#"{"protocol": "https", "url": "https://x/1"}"#);
        assert_eq!(unnamed.plan(Path::new("/s")), None);
        let sabr = read(r#"{"filename": "/s/a.mp4", "protocol": "sabr", "url": "https://x/1"}"#);
        assert_eq!(sabr.plan(Path::new("/s")), None);
    }

    #[test]
    fn progress_adds_up_and_has_a_total_once_every_stream_has_one() {
        let p = |downloaded, total, speed| crate::Progress {
            downloaded,
            total,
            speed,
            eta: None,
            estimated: false,
        };
        let one = sum(&[Some(p(100, Some(1000), 10.0)), None]);
        assert_eq!((one.downloaded, one.total), (100, None));
        let both = sum(&[Some(p(100, Some(1000), 10.0)), Some(p(50, Some(200), 40.0))]);
        assert_eq!(
            (both.downloaded, both.total, both.speed),
            (150, Some(1200), 50.0)
        );
        assert_eq!(both.eta, Some(std::time::Duration::from_secs(21)));
    }

    #[test]
    fn an_agent_is_kept_once() {
        let a = interned("Agent/7 (test)");
        let b = interned(&String::from("Agent/7 (test)"));
        assert!(std::ptr::eq(a, b));
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use crate::{Categories, Control, Limiter};

    fn options(dir: &Path) -> Options {
        Options {
            connections: 4,
            control: Control::new(),
            dest_dir: dir.join("out"),
            parts_dir: dir.join("parts"),
            limiter: Limiter::unlimited(),
            room: Default::default(),
            categories: Categories::default(),
            checksum: None,
            quality: crate::stream::Quality::Best,
            sound: None,
            span: None,
            shape: None,
            mp3: false,
            gif: false,
            media: false,
            referrer: None,
            proxy: None,
            container: None,
            attempts: 1,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            sign_in: None,
            save_name: None,
            accept_page: false,
            torrent: Default::default(),
            system_proxy: false,
            title_hint: None,
            remux: false,
            server_dates: false,
            retry_wait: 0,
            record_limit: None,
            frame_rate: None,
            caption: None,
        }
    }

    fn read(json: &str) -> Read {
        Read::new(json.to_string(), serde_json::from_str(json).unwrap())
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// A reading served from a moment past is fetched at once; one served a
    /// second from now is waited for; a cancel ends the wait.
    #[tokio::test]
    async fn the_wait_for_a_stream_to_be_served_ends_on_time_or_on_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let opts = options(dir.path());
        let past = read(&format!(r#"{{"available_at": {}}}"#, now() - 5));
        let at = std::time::Instant::now();
        wait_until_available(&past, &opts).await.unwrap();
        assert!(at.elapsed() < Duration::from_millis(100));

        let soon = read(&format!(r#"{{"available_at": {}}}"#, now() + 1));
        wait_until_available(&soon, &opts).await.unwrap();
        assert!(at.elapsed() >= Duration::from_millis(400), "it waited");

        let later = read(&format!(r#"{{"available_at": {}}}"#, now() + 60));
        let cancelled = options(dir.path());
        cancelled.control.cancel();
        let at = std::time::Instant::now();
        assert!(matches!(
            wait_until_available(&later, &cancelled).await,
            Err(Error::Cancelled)
        ));
        assert!(at.elapsed() < Duration::from_secs(2), "the wait was cut");
    }

    /// Bytes on disk are a ledger of the engine's with more than nothing in
    /// it, in a stream's own parts folder; anything else is not.
    #[tokio::test]
    async fn bytes_fetched_are_told_by_the_engines_ledgers() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("parts");
        assert!(!fetched_any(&staging).await, "no staging at all");
        std::fs::create_dir_all(staging.join("elsewhere/parts")).unwrap();
        std::fs::write(staging.join("elsewhere/parts/done-0"), "99").unwrap();
        std::fs::create_dir_all(staging.join(".dm-f140")).unwrap();
        std::fs::create_dir_all(staging.join(".dm-f137/parts")).unwrap();
        std::fs::write(staging.join(".dm-f137/parts/piece-0"), "99").unwrap();
        std::fs::write(staging.join(".dm-f137/parts/done-0"), "0").unwrap();
        std::fs::write(staging.join(".dm-f137/parts/done-1"), "many").unwrap();
        assert!(!fetched_any(&staging).await);
        std::fs::write(staging.join(".dm-f137/parts/done-2"), " 4096\n").unwrap();
        assert!(fetched_any(&staging).await);
    }

    /// A refused address that neither ran out nor served bytes is not read
    /// again: the page is not asked at all.
    #[tokio::test]
    async fn a_refusal_at_the_first_ask_is_not_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let r = read(
            r#"{"filename": "/s/a.mp4", "protocol": "https",
                "url": "https://x.example/v?expire=99999999999"}"#,
        );
        let plan = r.plan(Path::new("/s")).unwrap();
        assert!(!plan.streams[0].expired());
        let renewed = renewed(
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            &plan,
            &options(dir.path()),
        )
        .await;
        assert!(matches!(renewed, Ok(None)));
        let odd = Stream {
            url: "not an address".into(),
            ..plan.streams[0].clone()
        };
        assert!(!odd.expired());
    }

    /// Streams are the same read again where their ids, files and stated
    /// sizes agree; a size one reading did not state does not count.
    #[test]
    fn the_same_streams_are_told_by_id_file_and_size() {
        let r = read(
            r#"{"filename": "/s/a.mp4", "requested_formats": [
                {"format_id": "1", "protocol": "https", "url": "https://x/1", "filesize": 10},
                {"format_id": "2", "protocol": "https", "url": "https://x/2"}]}"#,
        );
        let a = r.plan(Path::new("/s")).unwrap();
        let b = r.plan(Path::new("/s")).unwrap();
        assert!(a.same_streams(&b));
        let mut no_size = r.plan(Path::new("/s")).unwrap();
        no_size.streams[0].size = None;
        assert!(a.same_streams(&no_size));
        let mut bigger = r.plan(Path::new("/s")).unwrap();
        bigger.streams[0].size = Some(11);
        assert!(!a.same_streams(&bigger));
        let mut other = r.plan(Path::new("/s")).unwrap();
        other.streams[1].id = "3".into();
        assert!(!a.same_streams(&other));
        let mut fewer = r.plan(Path::new("/s")).unwrap();
        fewer.streams.pop();
        assert!(!a.same_streams(&fewer));
        // A range that is not one, or too large to be a stream's first boxes.
        assert_eq!(range_of(Some("10-5")), None);
        assert_eq!(range_of(Some("0-99999999")), None);
        assert_eq!(range_of(Some("a-b")), None);
        assert_eq!(range_of(Some(" 0-740 ")), Some((0, 740)));
    }

    /// The subtitles a reading names are fetched beside the video as
    /// `name.<language>.vtt`; one that is not WebVTT, has an odd language,
    /// no address or cannot be had is passed over.
    #[tokio::test]
    async fn the_subtitles_named_are_left_beside_the_video() {
        let app = axum::Router::new().route(
            "/en.vtt",
            axum::routing::get(|| async { "WEBVTT\n\n00:00.000 --> 00:01.000\nhello\n" }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let at = |path: &str| format!("http://{addr}{path}");
        let r = read(
            &serde_json::json!({"requested_subtitles": {
                "en": {"url": at("/en.vtt"), "ext": "vtt"},
                "fr": {"url": at("/en.vtt"), "ext": "srv3"},
                "../x": {"url": at("/en.vtt"), "ext": "vtt"},
                "": {"url": at("/en.vtt"), "ext": "vtt"},
                "de": {"url": at("/missing.vtt"), "ext": "vtt"},
                "es": {"ext": "vtt"},
            }})
            .to_string(),
        );
        let dir = tempfile::tempdir().unwrap();
        let produced = dir.path().join("Talk [x].mp4");
        // With subtitles off, none is fetched: what is fetched is only to be put inside.
        subtitles(&crate::client(), &r, &produced, &options(dir.path())).await;
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        let carrying = Options {
            subtitles: true,
            ..options(dir.path())
        };
        subtitles(&crate::client(), &r, &produced, &carrying).await;
        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["Talk [x].en.vtt"]);
        // None asked, or a video with no name: nothing.
        subtitles(
            &crate::client(),
            &read("{}"),
            &produced,
            &options(dir.path()),
        )
        .await;
        subtitles(&crate::client(), &r, Path::new("/"), &options(dir.path())).await;
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// A retry a stream says goes to the row as it is; its name and
    /// validator do not, and its progress is summed with the others'.
    #[tokio::test]
    async fn a_streams_retry_is_passed_on_and_its_name_is_not() {
        let (report, mut heard) = tokio::sync::mpsc::channel(64);
        let (senders, merging) = merge_progress(Some(report), 1);
        let tx = senders[0].clone().unwrap();
        drop(senders);
        let progress = crate::Progress {
            downloaded: 5,
            total: Some(10),
            speed: 1.0,
            eta: None,
            estimated: false,
        };
        tx.send(Update::Named {
            name: "x.f137.mp4".into(),
            category: category::classify("x.mp4", None),
        })
        .await
        .unwrap();
        tx.send(Update::Validator("\"v\"".into())).await.unwrap();
        tx.send(Update::Progress(progress)).await.unwrap();
        tx.send(Update::Retrying {
            attempt: 2,
            of: 5,
            why: "reset".into(),
            wait: Duration::from_secs(1),
        })
        .await
        .unwrap();
        drop(tx);
        merging.unwrap().await.unwrap();
        let mut got = Vec::new();
        while let Ok(update) = heard.try_recv() {
            got.push(update);
        }
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(matches!(&got[0], Update::Progress(p) if p.downloaded == 5 && p.total == Some(10)));
        assert!(matches!(&got[1], Update::Retrying { attempt: 2, .. }));
        // Without a row to tell, each stream gets no sender and nothing merges.
        let (none, merging) = merge_progress(None, 2);
        assert!(none.iter().all(Option::is_none) && none.len() == 2);
        assert!(merging.is_none());
    }

    /// A type that is no file name's, and names a reader numbers but that
    /// are not that reader's shape, are left as they are.
    #[test]
    fn odd_types_and_odd_numbered_names_are_left_alone() {
        let odd = read(
            r#"{"filename": "/s/a.mp4", "ext": "m p4", "protocol": "https", "url": "https://x/1"}"#,
        );
        assert_eq!(odd.plan(Path::new("/s")), None);
        let named = |filename: &str| {
            read(
                &serde_json::json!({"filename": filename, "extractor_key": "HTML5MediaEmbed",
                "id": "watch-1", "protocol": "http", "url": "http://site/clip.mp4"})
                .to_string(),
            )
            .plan(Path::new("/s"))
            .unwrap()
            .produced
        };
        assert_eq!(
            named("/s/QA clip [watch-1].mp4"),
            PathBuf::from("/s/QA clip [watch-1].mp4")
        );
        assert_eq!(
            named("/s/QA (1) [x [watch-1].mp4"),
            PathBuf::from("/s/QA (1) [x [watch-1].mp4")
        );
        assert_eq!(
            named("/s/QA (1) [watch-2].mp4"),
            PathBuf::from("/s/QA (1) [watch-2].mp4")
        );
    }

    /// Past its bound the table of agents gives a browser's, rather than
    /// keep every agent a reading names.
    #[test]
    fn the_table_of_agents_is_bounded() {
        let given: Vec<&'static str> = (0..40)
            .map(|n| interned(&format!("Bounded/{n} (coverage)")))
            .collect();
        assert!(given.contains(&crate::BROWSER_AGENT));
        assert_eq!(interned("Bounded/0 (coverage)"), "Bounded/0 (coverage)");
    }
}
