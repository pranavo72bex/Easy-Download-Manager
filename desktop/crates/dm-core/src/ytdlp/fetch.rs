//! A video page read by yt-dlp, its files fetched by dm.
//!
//! yt-dlp is what reads a video site's page: YouTube's player answers with
//! challenges that change week to week and are worked out by running its
//! own JavaScript, and yt-dlp is kept current against them by the many who
//! maintain it (`docs/decisions.md`, "No separate video tool"). What it does
//! not do well is the part dm is for: it fetches each stream on one
//! connection, so the picture of a long video came at the speed one
//! connection gets while dm's engine sat idle.
//!
//! So yt-dlp is asked only what to fetch (`--print`, nothing downloaded):
//! the address of each stream, the headers it must be asked with, and the
//! name it would have saved under. dm fetches each stream with its own
//! engine, on every connection set, with resume, splitting and the speed
//! ceiling as any download has them, and joins picture and sound itself as
//! it already did. The same page, the same streams, the same file name.
//!
//! Only plain files (`http`, `https`) are taken. A stream in pieces (HLS,
//! DASH fragments), a live one, or a site's own protocol is yt-dlp's to
//! download, as before. And where dm's own fetch is refused (a server that
//! will not answer it, a range it will not serve), yt-dlp downloads the
//! same reading (`--load-info-json`), so nothing that downloaded before
//! fails now: this route can only be faster or the same.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{
    binary, container_sort, cookies_by_file, download_ytdlp, download_ytdlp_from, finish_dm,
    format_for, fresh_reading, js_runtime, proxy_by_file, remove_staging, said_by,
    unpack_where_programs_run, write_private, RemoveOnDrop,
};
use crate::download::{Finished, Options, Update};
use crate::{category, Error};

/// A page handed to the video helper: read by yt-dlp, fetched by dm where
/// its streams are plain files, by yt-dlp otherwise.
pub async fn download_page(
    client: &reqwest::Client,
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let staging = &options.parts_dir;
    // An earlier run found dm's own fetch refused: yt-dlp's from the start,
    // rather than reading the page twice to be refused again. A YouTube
    // video is read by dm itself (`crate::youtube`), yt-dlp or none.
    let own = crate::youtube::video_id(url).is_some();
    // A part of the video is cut from streams dm fetches itself, never
    // fetched whole by the helper: with a span, dm reads the page whatever
    // an earlier run found.
    let part = options.span.is_some();
    if ((binary().is_none() && !own) || tokio::fs::metadata(staging.join(REFUSED)).await.is_ok())
        && !part
    {
        return download_ytdlp(url, options, report).await;
    }
    tokio::fs::create_dir_all(staging)
        .await
        .map_err(Error::Disk)?;

    /* The panel's list read this page moments ago, most likely: its
     * reading chooses the format without yt-dlp asking the site again
     * (a fifth of a second against seconds on YouTube). One that no longer
     * serves is passed over for a fresh reading. */
    super::wait_for_reading(url, &options.control).await?;
    let kept = match fresh_reading(url) {
        Some(saved) => match resolve(url, options, Some(&saved)).await {
            Ok(read) => Some(read),
            Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
            Err(_) => None,
        },
        None => None,
    };
    let from_saved = kept.is_some();
    let read = match kept {
        Some(read) => Ok(read),
        None => resolve(url, options, None).await,
    };
    let read = match read {
        Ok(read) => read,
        Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
        Err(why) if part => {
            return Err(Error::Internal(format!(
                "the page could not be read, so no part of its video can be cut ({why})"
            )))
        }
        // yt-dlp's own run says why in its own words.
        Err(_) => return download_ytdlp(url, options, report).await,
    };
    let Some(plan) = read.plan(staging) else {
        if part {
            return Err(Error::Internal(
                "this video's streams are not plain files with an index, so a part of it \
                 cannot be cut"
                    .into(),
            ));
        }
        // Streams dm does not fetch itself: yt-dlp downloads this reading.
        return from_reading(url, &read, options, report).await;
    };
    /* A reading kept from earlier whose addresses have lapsed is read
     * again now, rather than found out stream by stream. */
    let (plan, read) = if from_saved && plan.streams.iter().any(Stream::expired) {
        match resolve(url, options, None).await {
            Ok(fresh) => match fresh.plan(staging) {
                Some(again) => (again, fresh),
                None => (plan, read),
            },
            Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
            Err(_) => (plan, read),
        }
    } else {
        (plan, read)
    };

    let mut current = (plan, read);
    let mut outcome = fetch(client, &current.0, &current.1, options, report.clone()).await;
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
    let read = current.1;
    match outcome {
        Ok(done) => Ok(done),
        Err(Error::Paused) => Err(Error::Paused),
        Err(Error::Cancelled) => {
            remove_staging(staging).await;
            Err(Error::Cancelled)
        }
        // A part that could not be cut is said as it is: the helper would
        // fetch the whole video, which is not what was asked for.
        Err(why) if part => Err(why),
        Err(why) => {
            eprintln!("dm: fetching {url} itself failed ({why}); the video helper downloads it");
            forget_streams(staging).await;
            let _ = tokio::fs::write(staging.join(REFUSED), why.to_string()).await;
            from_reading(url, &read, options, report).await
        }
    }
}

/// The page read again, after a stream's address was refused (403 or 410),
/// where that is what an address that has run out looks like: the plan's
/// own `expire` has passed, or bytes were already fetched, so the address
/// served once. One refused at the first ask never did, and is yt-dlp's as
/// before. `None` where the fresh reading is not the same streams (another
/// format, name or size): what is staged belongs to the old ones.
async fn renewed(url: &str, plan: &Plan, options: &Options) -> Result<Option<(Plan, Read)>, Error> {
    let staging = &options.parts_dir;
    if !plan.streams.iter().any(Stream::expired) && !fetched_any(staging).await {
        return Ok(None);
    }
    let read = match resolve(url, options, None).await {
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

/// Left in the staging folder when dm's own fetch was refused.
const REFUSED: &str = ".dm-own-fetch-refused";

/// yt-dlp's reading of the page, saved so yt-dlp can download from it.
const READING: &str = ".dm-reading.json";

/// How long yt-dlp may take to read a page.
const READ_WITHIN: std::time::Duration = std::time::Duration::from_secs(120);

/// yt-dlp downloading from its reading, and from the page again should the
/// reading no longer serve (its addresses expire after some hours).
async fn from_reading(
    url: &str,
    read: &Read,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let path = options.parts_dir.join(READING);
    // It can carry the site's cookies: this account's alone, and gone after.
    if write_private(&path, &read.text).await.is_err() {
        return download_ytdlp(url, options, report).await;
    }
    let _gone = RemoveOnDrop(path.clone());
    match download_ytdlp_from(url, Some(&path), options, report.clone()).await {
        Ok(done) => Ok(done),
        Err(stop @ (Error::Paused | Error::Cancelled)) => Err(stop),
        Err(_) => download_ytdlp(url, options, report).await,
    }
}

/// What yt-dlp said about a page: the whole reading as it printed it, and
/// the parts of it dm reads.
struct Read {
    text: String,
    info: Info,
}

#[derive(Debug, Default, serde::Deserialize)]
struct Info {
    /// The path yt-dlp would have saved under (the picture and sound
    /// joined), from the same template its own download uses.
    #[serde(default)]
    filename: Option<String>,
    /// Which of yt-dlp's readers made this reading, its id for the video,
    /// and how many videos the page held (`unnumbered`).
    #[serde(default)]
    extractor_key: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    n_entries: Option<u64>,
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
}

#[derive(Debug, Default, Clone, serde::Deserialize)]
struct Format {
    #[serde(default)]
    url: Option<String>,
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
    (b >= a).then_some((a, b))
}

impl Format {
    /// The most bytes one request for this format may ask for, where yt-dlp
    /// says its server holds each to that (`http_chunk_size`).
    fn chunk(&self) -> Option<u64> {
        // `DM_YTDLP_NO_CHUNK` switches the cap off, for timing a long video
        // with and without it on a real connection (gaps.md row 22).
        if std::env::var_os("DM_YTDLP_NO_CHUNK").is_some() {
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
    /// The agent yt-dlp would ask with: a site can bind its addresses to the
    /// client that was given them.
    agent: Option<String>,
    /// Where the stream is put whole: beside `produced`, named as yt-dlp
    /// names the halves it leaves for dm to join.
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
            // `unknown_video` is yt-dlp's for a type it cannot name.
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

/// The name of a page's only `<video>` without the number yt-dlp gives it.
///
/// yt-dlp's reader of a plain page's media (`HTML5MediaEmbed`) numbers every
/// video it finds, one or several: a page titled "Talk" with one video was
/// saved as "Talk (1) [watch-1].mp4". Other download managers name it by the page alone,
/// and a number with nothing to tell apart says there is another.
fn unnumbered(produced: PathBuf, info: &Info) -> PathBuf {
    let only_one = info.extractor_key.as_deref() == Some("HTML5MediaEmbed")
        // One video: yt-dlp leaves the playlist fields out (several: `n_entries`).
        && info.n_entries.unwrap_or(1) == 1
        && info.id.as_deref().is_some_and(|id| id.ends_with("-1"));
    let (true, Some(stem), Some(ext)) = (
        only_one,
        produced.file_stem().and_then(|s| s.to_str()),
        produced.extension().and_then(|e| e.to_str()),
    ) else {
        return produced;
    };
    // Matched in the name as written, not against `id`: yt-dlp writes the
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

/// Asks yt-dlp what it would download, with the same choices of format,
/// name, sign-in and proxy its own download makes, and downloads nothing:
/// from the page, or from a reading of it already made (`saved`).
async fn resolve(url: &str, options: &Options, saved: Option<&Path>) -> Result<Read, Error> {
    let staging = &options.parts_dir;
    // A YouTube video is read by dm itself first (`crate::youtube`), with
    // yt-dlp behind it for what that cannot read; a reading the panel kept
    // is used as it is, which costs no request at all.
    if saved.is_none() && crate::youtube::video_id(url).is_some() {
        let own = crate::download::or_stop(
            &options.control,
            crate::youtube::read_for(url, options, staging),
        )
        .await;
        match own {
            Ok(text) => match serde_json::from_str::<Info>(&text) {
                Ok(info) => return Ok(Read { text, info }),
                Err(e) => eprintln!(
                    "dm: its own reading of the page could not be read ({e}); yt-dlp reads it"
                ),
            },
            Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
            Err(why) => {
                eprintln!("dm: could not read the YouTube page itself ({why}); yt-dlp reads it")
            }
        }
        // The helper, fetched behind dm's own reading, is waited for now.
        if binary().is_none() {
            let _ = crate::tools::for_video(&crate::client()).await;
        }
    }
    let binary = binary()
        .ok_or_else(|| Error::Internal("dm's video helper is not available here".into()))?;
    let mut command = super::writable_to(&binary, &[staging]);
    crate::no_window_async(&mut command);
    js_runtime(&mut command);
    unpack_where_programs_run(&mut command);
    command
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("-o")
        .arg(staging.join("%(title).200B [%(id)s].%(ext)s"))
        .arg("-f")
        .arg(format_for(options.quality, options.container.as_deref()))
        .args(container_sort(options.container.as_deref()))
        .arg("--socket-timeout")
        .arg("15")
        // What dm does not fetch from YouTube: its HLS manifests (the same
        // heights are in the DASH ones) and machine-translated subtitles.
        // About a third of the time a reading takes there; others ignore it.
        .arg("--extractor-args")
        .arg("youtube:skip=hls,translated_subs")
        // The whole reading, which yt-dlp can download from as it is.
        .arg("-O")
        .arg("%()j")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if options.subtitles {
        command.args(["--write-subs", "--sub-langs", "all", "--sub-format", "vtt"]);
    }
    let _proxied = match options.proxy_for(url) {
        Some(proxy) => Some(proxy_by_file(&mut command, staging, proxy).await?),
        None => None,
    };
    if let Some(referrer) = &options.referrer {
        command.arg("--referer").arg(referrer);
    }
    let _signed = cookies_by_file(
        &mut command,
        &staging.join("sign-in.read.cookies.txt"),
        options.sign_in.as_ref(),
        url,
    )
    .await?;
    match saved {
        Some(saved) => {
            command.arg("--load-info-json").arg(saved);
        }
        None => {
            command.arg("--").arg(url);
        }
    }

    let output = crate::download::or_stop(&options.control, async {
        tokio::time::timeout(READ_WITHIN, command.output())
            .await
            .map_err(|_| Error::Internal("yt-dlp took too long to read the page".into()))?
            .map_err(|e| Error::Internal(format!("yt-dlp would not start: {e}")))
    })
    .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(said_by(
            &stderr,
            "yt-dlp could not read that page",
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .unwrap_or("")
        .to_string();
    let info: Info = serde_json::from_str(&text)
        .map_err(|e| Error::Internal(format!("yt-dlp's reading could not be read: {e}")))?;
    Ok(Read { text, info })
}

/// Every stream fetched by dm's own engine at once, then joined and placed
/// as yt-dlp's own output would have been.
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

    // Left beside the video as yt-dlp leaves them, and read back as its are.
    subtitles(client, read, &plan.produced, options).await;
    let found = crate::subtitles::beside(&plan.produced);
    let (files, texts): (Vec<PathBuf>, Vec<crate::subtitles::Subtitle>) = found.into_iter().unzip();
    let outcome = finish_dm(&plan.produced, texts, options).await;
    for file in files {
        let _ = tokio::fs::remove_file(file).await;
    }
    outcome.map(|done| Finished {
        connections: options.connections,
        ..done
    })
}

/// One stream, by dm's engine, into its own parts folder, then moved to
/// where yt-dlp would have left it.
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

/// The parts of streams dm was fetching itself, before yt-dlp takes over.
async fn forget_streams(staging: &Path) {
    let Ok(mut listing) = tokio::fs::read_dir(staging).await else {
        return;
    };
    while let Ok(Some(entry)) = listing.next_entry().await {
        if entry.file_name().to_string_lossy().starts_with(".dm-f") {
            let _ = tokio::fs::remove_dir_all(entry.path()).await;
        }
    }
}

/// The subtitles yt-dlp's reading names, fetched and left beside the video
/// as yt-dlp leaves them (`name.<language>.vtt`). One that cannot be had is
/// passed over: subtitles are not worth the video.
async fn subtitles(client: &reqwest::Client, read: &Read, produced: &Path, options: &Options) {
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
                Update::Named { .. } | Update::Validator(_) => {}
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
        Read {
            text: json.to_string(),
            info: serde_json::from_str(json).unwrap(),
        }
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
    fn a_pages_only_video_is_named_without_the_number_ytdlp_gives_it() {
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
    fn a_numbered_name_is_matched_as_ytdlp_wrote_it() {
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
    fn a_type_ytdlp_cannot_name_is_still_fetched_by_dm() {
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
    fn streams_in_pieces_live_ones_and_names_elsewhere_stay_with_ytdlp() {
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
