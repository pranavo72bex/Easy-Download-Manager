// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The tracks fetched: every piece of each track, on as many connections as are set, into a file of
//! its own.

use super::*;

/// Every piece of each track, then each track gathered into a file of its
/// own. The tracks are fetched at the same time (a picture and a sound do not
/// wait for one another), and the pieces of a track on as many connections as
/// are set. A piece is written under a temporary name and moved when whole,
/// so a part file that is present is a finished one.
pub(super) async fn fetch_tracks(
    client: &reqwest::Client,
    tracks: &[&Track],
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<Vec<PathBuf>, Error> {
    // A track that is one whole file (no range, no key) is cut into ranged pieces first, as a
    // picture and sound fetched apart are: held whole in memory, it was refused past 512 MiB and
    // fetched again from its start after a stop (gaps row Y17). Where the server will not say
    // its length by a range, it is fetched whole, as before.
    let mut cut: Vec<Track> = Vec::with_capacity(tracks.len());
    for track in tracks {
        cut.push(whole_file_in_pieces(client, track, options).await);
    }
    let mut first = 0usize;
    let mut work = Vec::new();
    for (n, track) in cut.iter().enumerate() {
        work.push(fetch_track(client, track, n, first, options, downloaded));
        first += track.pieces.len();
    }
    futures_util::future::try_join_all(work).await
}

/// `track` as ranged pieces of [`ONE_FILE_PIECE`] where it is one whole file a server serves
/// by ranges; as it was otherwise. A part of one (`options.span`) is the index's to cut.
async fn whole_file_in_pieces(client: &reqwest::Client, track: &Track, options: &Options) -> Track {
    let whole = match track.pieces.as_slice() {
        [only] if only.range.is_none() && only.lock.is_none() && options.span.is_none() => only,
        _ => return track.clone(),
    };
    match total_of(client, &whole.url, options).await {
        Ok(total) => Track {
            pieces: pieces_of(&whole.url, total, ONE_FILE_PIECE),
            ..track.clone()
        },
        Err(_) => track.clone(),
    }
}

/// The size of one request of a one-file track: as a picture and sound fetched apart.
pub(super) const ONE_FILE_PIECE: u64 = 8 * 1024 * 1024;

/// One track: its pieces (numbered from `first`), gathered into `track{n}`.
async fn fetch_track(
    client: &reqwest::Client,
    track: &Track,
    n: usize,
    first: usize,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<PathBuf, Error> {
    let kind = match (track.medium, track.webm) {
        (_, true) => "webm",
        (Medium::Video, false) => "mp4",
        (Medium::Audio, false) => "m4a",
    };
    let gathered = options.parts_dir.join(format!("track{n}.{kind}"));
    // Gathered before a stop: its pieces were freed as they were
    // consumed, and the file is named only when whole.
    if fs::metadata(&gathered).await.is_ok_and(|m| m.len() > 0) {
        return Ok(gathered);
    }
    // One file with an index of its own, for a part of it: the index says
    // which bytes, and only those are asked for.
    if let (Some(indexed), Some(span), Some(first_piece)) =
        (track.indexed, options.span, track.pieces.first())
    {
        let url = first_piece.url.clone();
        let mut pairs = Vec::new();
        let cookies = crate::cookies::header_with(options.sign_in.as_ref(), &url);
        if let Some(cookies) = &cookies {
            pairs.push(("cookie", cookies.as_str()));
        }
        if let Some(referrer) = options.referrer.as_deref() {
            pairs.push(("referer", referrer));
        }
        let source = crate::clip::Source {
            agent: Some(crate::agent_for(&url).to_string()),
            url,
            init: indexed.init,
            index: indexed.index,
            headers: Headers::of(pairs),
            chunk: None,
            target: options.parts_dir.join(format!("track{n}.partial")),
        };
        let part = crate::clip::fetch_part(client, &source, span, options, None).await?;
        downloaded.fetch_add(part.bytes, Ordering::Relaxed);
        fs::rename(&source.target, &gathered).await?;
        return Ok(gathered);
    }
    // As many at once as there are connections, each taking the next piece as
    // it finishes its own: a slow piece holds only the connection it is on.
    let at_once = options.connections.max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let worker = || async {
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            let Some(piece) = track.pieces.get(i) else {
                return Ok::<(), Error>(());
            };
            fetch_piece(client, piece, first + i, options, downloaded).await?;
        }
    };
    futures_util::future::try_join_all((0..at_once).map(|_| worker())).await?;
    let parts: Vec<PathBuf> = (first..first + track.pieces.len())
        .map(|i| part_path(&options.parts_dir, i))
        .collect();
    let partial = options.parts_dir.join(format!("track{n}.partial"));
    join_parts(&parts, None, &partial, &options.control).await?;
    fs::rename(&partial, &gathered).await?;
    Ok(gathered)
}

/// One piece, written to its part file when it is whole.
async fn fetch_piece(
    client: &reqwest::Client,
    piece: &dash::Piece,
    index: usize,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<(), Error> {
    let done = part_path(&options.parts_dir, index);
    if let Ok(meta) = fs::metadata(&done).await {
        downloaded.fetch_add(meta.len(), Ordering::Relaxed);
        return Ok(());
    }
    options.control.check()?;
    let staging = done.with_extension("partial");
    let client = &crate::second_path::client_for_piece(client, &options.parts_dir, index);
    // One in the clear is written as it comes, not held whole first (gaps row Y17).
    if piece.lock.is_none() {
        let written = retry::run_up_to(&options.control, options.tries(), || {
            hls::fetch_segment_to(client, &piece.url, piece.range, options, &staging)
        })
        .await?;
        downloaded.fetch_add(written, Ordering::Relaxed);
        fs::rename(&staging, &done).await?;
        return Ok(());
    }
    let body = retry::run_up_to(&options.control, options.tries(), || {
        hls::fetch_segment_range(client, &piece.url, piece.range, options)
    })
    .await?;
    let body = match &piece.lock {
        Some(lock) => stream::decrypt(&lock.key, &lock.iv, &body)?,
        None => body,
    };
    downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
    fs::write(&staging, &body).await?;
    fs::rename(&staging, &done).await?;
    Ok(())
}
