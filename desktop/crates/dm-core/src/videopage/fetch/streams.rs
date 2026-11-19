// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A plan's streams fetched on dm's engine, each in its parts with their progress merged, and the
//! cover and subtitles saved beside them.

use super::*;

/// Reads a YouTube page by dm's own YouTube reader (`crate::youtube`); `without_numbered`, with
/// its formats served only as numbered pieces left out.
pub(super) async fn resolve(
    url: &str,
    options: &Options,
    without_numbered: bool,
) -> Result<Read, Error> {
    let staging = &options.parts_dir;
    let own = if without_numbered {
        crate::download::or_stop(
            &options.control,
            crate::youtube::read_for_without_numbered(url, options, staging),
        )
        .await
    } else {
        crate::download::or_stop(
            &options.control,
            crate::youtube::read_for(url, options, staging),
        )
        .await
    };
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
pub(super) async fn fetch(
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
        date: info.upload_date.as_deref().map(iso_day).unwrap_or_default(),
        source: info.webpage_url.clone().unwrap_or_default(),
        cover: cover(client, info.thumbnail.as_deref(), options).await,
        chapters: info
            .chapters
            .iter()
            .flatten()
            .filter(|c| c.start_time.is_finite() && c.start_time >= 0.0)
            .map(|c| ((c.start_time * 1000.0) as u64, c.title.trim().to_string()))
            .collect(),
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
    let done = if stream.numbered {
        crate::download::download_numbered_stream_with(client, &stream.url, &opts, report, hints)
            .await?
    } else {
        crate::download::download_video_stream_with(client, &stream.url, &opts, report, hints)
            .await?
    };
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
    // dm's reading of a part leaves numbered pieces out: they have no index to cut by.
    if stream.numbered {
        return Err(Error::Internal(
            "this video's streams are numbered pieces with no index, so a part of it cannot be cut"
                .into(),
        ));
    }
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

/// A day as a reading writes it (`20050423`) as tags write one (`2005-04-23`); anything else
/// as it is.
pub(super) fn iso_day(day: &str) -> String {
    match (day.get(..4), day.get(4..6), day.get(6..8)) {
        (Some(y), Some(m), Some(d))
            if day.len() == 8 && day.bytes().all(|b| b.is_ascii_digit()) =>
        {
            format!("{y}-{m}-{d}")
        }
        _ => day.to_string(),
    }
}

/// The video's picture, for its cover (gaps row Y11): from YouTube's picture server alone
/// (`https`, `*.ytimg.com`), 2 MiB at most, a JPEG or a PNG; else none, and the video is not
/// the worse for it.
async fn cover(client: &reqwest::Client, address: Option<&str>, options: &Options) -> Vec<u8> {
    let Some(address) = address else {
        return Vec::new();
    };
    let ours = url::Url::parse(address).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str()
                .is_some_and(|h| h == "i.ytimg.com" || h.ends_with(".ytimg.com"))
    });
    if !ours {
        return Vec::new();
    }
    // Ten seconds at most: a picture is not worth holding the finished video back.
    let fetching = crate::download::fetch_capped(client, address, 2 * 1024 * 1024, options);
    match tokio::time::timeout(COVER_WITHIN, fetching).await {
        Ok(Ok(got)) if crate::mp4mux::picture_kind(&got.body).is_some() => got.body.to_vec(),
        _ => Vec::new(),
    }
}

/// The subtitles the reading names, fetched and left beside the video
/// as they are left (`name.<language>.vtt`). One that cannot be had is
/// passed over: subtitles are not worth the video.
pub(super) async fn subtitles(
    client: &reqwest::Client,
    read: &Read,
    produced: &Path,
    options: &Options,
) {
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
pub(super) fn merge_progress(
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
                // A stream's note is the row's.
                note @ Update::Note(_) => {
                    let _ = report.send(note).await;
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
pub(super) fn sum(latest: &[Option<crate::Progress>]) -> crate::Progress {
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
pub(super) fn interned(agent: &str) -> &'static str {
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
