// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A picture and its sound joined into the larger of the two where the disk has no room for the
//! joined file beside them (gaps row Y21): `mp4mux::join_in_place`, for a DASH stream's tracks
//! (and an HLS stream's or a player's pair, joined as they are) and a video page's two parts.
//! The disk then holds the video and little more where the usual join held it twice; the file's
//! index is at its end and its sound is not interleaved with its picture. Where there is room,
//! the usual join is made, as before.

use super::hls_text::Texts;
use super::*;
use crate::mp4mux::{InPlace, NotInPlace};
use crate::subtitles::Subtitle;

/// Where a join in place into or from `path` was cut short (EDM quit, the computer went off),
/// `path` is neither what was fetched nor the join: removed with its mark, to be fetched again.
/// Nothing where it has no mark.
pub(crate) async fn drop_spoiled(path: &Path) {
    let mark = crate::mp4mux::joining_mark(path);
    if fs::metadata(&mark).await.is_ok() {
        let _ = fs::remove_file(path).await;
        let _ = fs::remove_file(&mark).await;
    }
}

/// A DASH stream's tracks (or an HLS stream's or a player's pair) joined into the largest of
/// them, then moved to `joined`, with the subtitles timed as the usual join times them
/// (`dash::join_mp4`): `false`, nothing changed, where none can be joined into or the disk has
/// no room even for that.
pub(super) async fn tracks_in_place(
    files: &[PathBuf],
    texts: Texts,
    joined: &Path,
    options: &Options,
) -> Result<bool, Error> {
    let (cut, shape) = match options.span {
        Some(span) => (Some(span.millis()), options.shape),
        None => (None, None),
    };
    let job = |placed| InPlace {
        inputs: files.to_vec(),
        cut,
        shape,
        tags: crate::mp4mux::Tags::default(),
        placed,
        watch: None,
    };
    match texts {
        Texts::Timed(texts) => into_largest(job(false), move |_| texts, joined, options).await,
        Texts::Hls(texts) => {
            let place = move |zero| texts.timed(zero);
            into_largest(job(true), place, joined, options).await
        }
    }
}

/// A video page's picture and sound joined into the larger, then moved to `joined`, carrying
/// `texts` and `tags` and cut where Options says, as the usual join (`videopage`): `false`,
/// nothing changed, where neither can be joined into or the disk has no room even for that.
pub(crate) async fn parts_in_place(
    parts: &[PathBuf],
    texts: Vec<Subtitle>,
    tags: crate::mp4mux::Tags,
    joined: &Path,
    options: &Options,
) -> Result<bool, Error> {
    let job = InPlace {
        inputs: parts.to_vec(),
        cut: options.span.map(|span| span.millis()),
        shape: options.shape,
        tags,
        placed: false,
        watch: None,
    };
    into_largest(job, move |_| texts, joined, options).await
}

/// `job` joined in place, off the async runtime, if the disk where its inputs are has room for
/// what it adds; the input joined into is then moved to `joined`, and the rest go with every
/// mark. Run to its end like any join: cut loose from a pause it would go on writing.
async fn into_largest<F>(
    job: InPlace,
    place: F,
    joined: &Path,
    options: &Options,
) -> Result<bool, Error>
where
    F: FnOnce(i64) -> Vec<Subtitle> + Send + 'static,
{
    let inputs = job.inputs.clone();
    let room = options.room.clone();
    let made = tokio::task::spawn_blocking(move || {
        crate::mp4mux::join_in_place(job, place, |host, need| {
            let dir = host.parent().unwrap_or(host);
            room.free_space(dir).is_none_or(|free| need <= free)
        })
    })
    .await
    .map_err(|e| Error::Internal(format!("joining stopped: {e}")))?;
    let host = match made {
        Ok((host, _)) => host,
        Err(NotInPlace::Untouched(why)) => {
            crate::note!(info, "EDM: the tracks were not joined in place: {why}");
            return Ok(false);
        }
        Err(NotInPlace::Spoiled(why)) => {
            return Err(Error::Internal(format!(
                "the picture and sound could not be joined: {why}"
            )))
        }
    };
    if let Some(written) = inputs.get(host) {
        fs::rename(written, joined).await?;
    }
    // The marks first: an input left behind unmarked is passed over, the join being whole.
    for input in &inputs {
        let _ = fs::remove_file(crate::mp4mux::joining_mark(input)).await;
    }
    for (n, input) in inputs.iter().enumerate() {
        if n != host {
            let _ = fs::remove_file(input).await;
        }
    }
    Ok(true)
}
