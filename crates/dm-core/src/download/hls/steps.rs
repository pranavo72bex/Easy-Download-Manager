// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Steps of fetching a stream as one file (`download_stream`) and of ending a live
//! recording (`record_live`).

use super::*;
use crate::download::hls_text::{self, HlsTexts};

/// Each key once, not once per segment: a playlist of a thousand segments would otherwise
/// be a thousand requests for the same sixteen bytes. A key that changes part-way is
/// several (gaps row Y16); a pair of playlists has each its own (`dash::download_pair`).
pub(in crate::download) async fn fetch_keys(
    client: &reqwest::Client,
    listed: &stream::Stream,
    options: &Options,
) -> Result<Keys, Error> {
    let mut keys = Keys::new();
    for declared in listed.parts.iter().filter_map(|p| p.key.as_ref()) {
        if !keys.contains_key(&declared.uri) {
            let body = fetch_capped(client, &declared.uri, 1024, options)
                .await?
                .body;
            keys.insert(declared.uri.clone(), body);
        }
    }
    Ok(keys)
}

/// The MP4 a remux stopped before finished made the file, where it is whole and the remux
/// would run now (`remuxing`); one from before that would not is removed.
///
/// Stopped after every segment had arrived — paused during the remux, or
/// dm quit or killed then — the segments are still there, or in the gathered
/// stream (kept as `.ts`), and once remuxed, in the MP4: none is fetched
/// again. The gathered stream and the MP4 are each named only once whole
/// (each is written under `.partial`), and the segments are removed only
/// once the MP4 is, so whichever is here is complete.
pub(super) async fn remuxed_before(
    listed: &stream::Stream,
    remuxing: bool,
    gathered: &Path,
    options: &Options,
) -> Result<Option<Finished>, Error> {
    let remuxed = options.parts_dir.join(REMUXED);
    if !remuxing {
        let _ = fs::remove_file(&remuxed).await;
    }
    if remuxing
        && fs::metadata(gathered).await.is_err()
        && fs::metadata(&remuxed).await.is_ok_and(|m| m.len() > 0)
    {
        let stem = ts_stem(&listed.filename);
        let (path, bytes, category) =
            finish(&format!("{stem}.mp4"), None, &[remuxed], options).await?;
        // Segments the remux had not yet removed when it was stopped (gaps row Y21).
        for i in 0..listed.parts.len() {
            let _ = fs::remove_file(part_path(&options.parts_dir, i)).await;
        }
        return Ok(Some(Finished {
            left_behind: None,
            path,
            bytes,
            connections: 1,
            category,
            verified: options.checksum.is_some(),
            mirrors: 1,
        }));
    }
    Ok(None)
}

/// The subtitle renditions the master lists for this rendition, carried inside the file
/// where Options says so and there is a picture to show them over (gaps row Y20): fetched
/// before the segments, so a stop while they come is a stop, not a file without them. A
/// transport stream carries them only where it is put into an MP4 (not one kept as `.ts`,
/// nor one scrambled by SAMPLE-AES, whose remux takes none).
pub(super) async fn carried_texts(
    client: &reqwest::Client,
    listed: &stream::Stream,
    subtitles: &[stream::SubtitleRendition],
    remuxing: bool,
    sealed: bool,
    skipped: f64,
    options: &Options,
) -> Result<HlsTexts, Error> {
    let carried = match listed.container.as_str() {
        "m4a" | "aac" => false,
        "ts" => remuxing && !sealed,
        _ => true,
    };
    let pictured = options.quality != Quality::Audio && carried;
    let texts = if options.subtitles && pictured && !subtitles.is_empty() {
        hls_text::fetch(client, subtitles, options)
            .await?
            .skipping(skipped)
    } else {
        HlsTexts::default()
    };
    Ok(texts)
}

/// The stream's pieces: the gathered stream where it is already here, else its segments,
/// fetched.
pub(super) async fn gathered_or_fetched(
    client: &reqwest::Client,
    listed: &stream::Stream,
    keys: &Keys,
    gathered: PathBuf,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Vec<PathBuf>, Error> {
    let parts: Vec<PathBuf> = if fs::metadata(&gathered).await.is_ok() {
        vec![gathered]
    } else {
        // A playlist states no size: the row shows one about, from the pieces fetched so far
        // and how many there are, marked as such (gaps row Y22), rather than none at all.
        let downloaded = Arc::new(AtomicU64::new(0));
        let estimate = Arc::new(AtomicU64::new(0));
        let meter = report.map(|tx| {
            crate::download::ranged::watch_estimating(
                Arc::clone(&downloaded),
                None,
                Some(Arc::clone(&estimate)),
                tx,
            )
        });
        let outcome =
            fetch_segments(client, listed, keys, options, &downloaded, Some(&estimate)).await;
        if let Some((handle, finished)) = meter {
            let _ = finished.send(());
            let _ = handle.await;
        }
        outcome?;
        (0..listed.parts.len())
            .map(|i| part_path(&options.parts_dir, i))
            .collect()
    };
    Ok(parts)
}

/// The pieces made one MP4 where they must be: a part of the stream trimmed (`cut`), runs
/// with their own set-ups, or subtitles to carry in fragments.
pub(super) async fn cut_or_joined(
    parts: Vec<PathBuf>,
    cut: Option<(u64, u64)>,
    runs: bool,
    fragmented: bool,
    texts: &HlsTexts,
    options: &Options,
) -> Result<Vec<PathBuf>, Error> {
    let parts = match cut {
        Some(window) => vec![cut_fragments_with(&parts, Some(window), texts, options).await?],
        // Runs, each with its set-up, as one MP4 (gaps row Y16).
        None if runs && fragmented => {
            vec![cut_fragments_with(&parts, None, texts, options).await?]
        }
        // Subtitles to carry: the fragments made one MP4 with them; kept as they came where dm
        // cannot make one, which is still the stream (gaps row Y20).
        None if fragmented && !texts.is_empty() => {
            let made = cut_fragments_with(&parts, None, texts, options).await;
            match made {
                Ok(joined) => vec![joined],
                Err(stop @ (Error::Paused | Error::Cancelled)) => return Err(stop),
                Err(_) => parts,
            }
        }
        None => parts,
    };
    Ok(parts)
}

/// How a live recording ended, judged: cancelled, its files go; stopped before anything was
/// recorded, the error (or that nothing came) is returned. Otherwise what was recorded is
/// kept, whatever stopped it.
pub(super) async fn kept_or_cleared(
    ended: Result<(), Error>,
    recorded: bool,
    picture_at: &Path,
    sound_file: &Path,
) -> Result<(), Error> {
    match ended {
        Ok(()) | Err(Error::Paused) => {}
        Err(Error::Cancelled) => {
            let _ = fs::remove_file(picture_at).await;
            let _ = fs::remove_file(sound_file).await;
            return Err(Error::Cancelled);
        }
        // Whatever stopped it, what was recorded is kept where there is any.
        Err(e) if !recorded => {
            let _ = fs::remove_file(sound_file).await;
            return Err(e);
        }
        Err(_) => {}
    }
    if !recorded {
        let _ = fs::remove_file(sound_file).await;
        return Err(Error::Internal(
            "the live stream sent nothing to record".into(),
        ));
    }
    Ok(())
}

/// A live recording placed as the finished file: joined with its sound where that was
/// recorded, its runs made one MP4, or a transport stream remuxed.
pub(super) async fn place_live(
    picture_at: PathBuf,
    sound_at: Option<PathBuf>,
    filename: String,
    remapped: bool,
    options: &Options,
) -> Result<Finished, Error> {
    // Stopped by the person: the recording is finished, not paused, so
    // placing it must not wait on the stop that ended it.
    let placing = Options {
        control: crate::control::Control::new(),
        ..options.clone()
    };
    let joined = match &sound_at {
        Some(sound) => join_live(&picture_at, sound, &filename, &placing).await,
        None => None,
    };
    let (parts, filename) = match joined {
        Some((mp4, name)) => (vec![mp4], name),
        // Runs of a fragmented recording, each with its own set-up, as one MP4 (gaps row Y16).
        None if remapped && !crate::tsmux::is_transport_stream(&picture_at) => {
            let flat = cut_fragments(std::slice::from_ref(&picture_at), None, &placing).await?;
            (vec![flat], format!("{}.mp4", media_stem(&filename)))
        }
        None => {
            let parts = vec![picture_at];
            match remux_ts(&filename, &parts, &placing).await? {
                Some((mp4, name)) => (vec![mp4], name),
                None => (parts, filename),
            }
        }
    };
    let (path, bytes, category) = finish(&filename, None, &parts, &placing).await?;
    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: false,
        mirrors: 1,
    })
}
