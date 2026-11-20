// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Downloading from sites that serve video through their own player.
//!
//! YouTube is read by dm itself (`crate::youtube`): the same requests a
//! browser makes, and the public-domain solver scripts for the player's
//! `n` values (run by QuickJS, fetched as a tool when needed). Every
//! other site's video comes from the browser extension's caught player
//! requests — dm never reads those pages itself.
//!
//! This module holds the data types (`Read`, `Plan`, `Stream`), the
//! fetch/merge pipeline that turns a reading into a finished file, the
//! quality choices panel, playlist reading, and the readings cache.

use std::path::{Path, PathBuf};

use crate::download::{Finished, Options};
use crate::Error;

mod choices;
mod fetch;
mod page_data;
mod playlist;

pub use choices::{choices, class_name, list_choices, list_choices_fast, with_mp3, Choice};
pub use fetch::{download_page, waits_for_premiere};
pub use playlist::{playlist_of, read_playlist};
/// Whether `url` is a YouTube page whose own data lists its qualities
/// (`list_choices_fast`).
pub fn is_watch_page(url: &str) -> bool {
    page_data::is_watch_page(url)
}

/// Whether a response is a web page rather than a file dm fetches itself.
///
/// A file has a body dm can range over; a web page does not, and saving the
/// page's HTML as though it were the download is the wrong answer whether or
/// not a reader is there to do better. So this is decided on the content type
/// alone, and the caller decides between reading it as a video page and refusing.
pub fn is_page(content_type: Option<&str>) -> bool {
    crate::probe::mime_essence(content_type) == "text/html"
}

/// The picture and the sound left apart, beside the name they are joined under,
/// `name.f137.mp4` and `name.f140.m4a`, picture first.
async fn parts_of(joined: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (joined.parent(), joined.file_stem()) else {
        return Vec::new();
    };
    let prefix = format!("{}.f", stem.to_string_lossy());
    let Ok(mut listing) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    while let Ok(Some(entry)) = listing.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let media = [".mp4", ".m4a", ".m4v", ".mov", ".webm"]
            .iter()
            .any(|ext| name.to_ascii_lowercase().ends_with(ext));
        if name.starts_with(&prefix) && media {
            parts.push(entry.path());
        }
    }
    // Picture before sound, as players expect the first track to be.
    parts.sort_by_key(|p| {
        let audio = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("m4a"));
        (audio, p.clone())
    });
    parts
}

/// What dm does with a page's fetched streams: joins the picture and sound left
/// apart, puts a transport stream into an MP4, carries the subtitles
/// inside, and places the result.
#[cfg(test)]
async fn finish_dm(
    produced: &Path,
    texts: Vec<crate::subtitles::Subtitle>,
    options: &Options,
) -> Result<Finished, Error> {
    finish_dm_tagged(produced, texts, crate::mp4mux::Tags::default(), options).await
}

/// [`finish_dm`], the joined MP4 carrying the video's title, channel, date and page (gaps row
/// Y11).
async fn finish_dm_tagged(
    produced: &Path,
    texts: Vec<crate::subtitles::Subtitle>,
    tags: crate::mp4mux::Tags,
    options: &Options,
) -> Result<Finished, Error> {
    let produced = produced.to_path_buf();
    // A part of the video: the pieces fetched hold a little around it, and
    // the joined file's edit lists play the part alone (`clip`).
    let cut = options.span.map(|span| span.millis());
    if !produced.is_file() {
        let parts = parts_of(&produced).await;
        if !parts.is_empty() {
            // A WebM where one was asked for; else an MP4, which takes a
            // WebM picture or sound too.
            let webm = options.container.as_deref() == Some("webm")
                && parts.iter().all(|p| {
                    p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("webm"))
                });
            if webm && cut.is_some() {
                return Err(Error::Internal(
                    "a part is cut from MP4 streams only, not WebM".into(),
                ));
            }
            // Matroska where it was asked for, or where the picture and
            // sound are a pair no MP4 holds (VP8 or Vorbis beside an MP4's
            // samples), rather than refused. A part or a reshaped picture
            // is made by the MP4 writer alone, as before.
            let mkv = !webm
                && cut.is_none()
                && options.shape.is_none()
                && (options.container.as_deref() == Some("mkv")
                    || crate::mkvmux::needs_matroska(&parts));
            let joined = produced.with_extension(if webm {
                "webm"
            } else if mkv {
                "mkv"
            } else {
                "mp4"
            });
            // Said before the join where the disk cannot hold it; the parts are kept (gaps row Y21).
            let into = joined.parent().unwrap_or(options.parts_dir.as_path());
            crate::download::room_to_join(&parts, into, options)?;
            let joining = if webm || mkv {
                crate::mkvmux::join_tagged_async(parts.clone(), texts, joined.clone(), tags).await
            } else {
                crate::mp4mux::join_cut_tagged_async(
                    parts.clone(),
                    texts,
                    joined.clone(),
                    cut,
                    options.shape,
                    tags,
                )
                .await
            };
            joining.map_err(|why| {
                Error::Internal(format!("the picture and sound could not be joined: {why}"))
            })?;
            for part in parts {
                let _ = tokio::fs::remove_file(part).await;
            }
            return place(&joined, options).await;
        }
    } else if let Some(cut) = cut {
        // One stream's part alone (the sound): rewritten with the cut.
        let ext = produced
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mp4".into());
        let cut_file = produced.with_extension(format!("dm-cut.{ext}"));
        crate::mp4mux::join_cut_tagged_async(
            vec![produced.clone()],
            texts,
            cut_file.clone(),
            Some(cut),
            options.shape,
            tags,
        )
        .await
        .map_err(|why| Error::Internal(format!("the part could not be cut: {why}")))?;
        tokio::fs::rename(&cut_file, &produced)
            .await
            .map_err(Error::Disk)?;
        return place(&produced, options).await;
    }
    /* An HLS site's video, fetched by a stream downloader, is the
     * stream's transport stream under an `.mp4` name: dm puts it right. */
    if is_transport_stream(&produced).await {
        let fixed = produced.with_extension("dm-remux.mp4");
        if crate::tsmux::to_mp4_async(produced.clone(), fixed.clone())
            .await
            .is_ok()
        {
            let _ = tokio::fs::rename(&fixed, &produced).await;
        } else {
            let _ = tokio::fs::remove_file(&fixed).await;
        }
    }
    /* One file that already held picture and sound, with subtitles beside
     * it: written again with them inside. A video that will not take them
     * is kept as it is; the subtitles are not worth the video. */
    let kind = produced
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    // An MP4, M4A or WebM of one stream is written again with the video's words in it too (its
    // title and channel, gaps rows Y11 and Y25), as a joined one is.
    let tagged = tags != crate::mp4mux::Tags::default()
        && ["mp4", "m4v", "m4a", "webm", "mkv"].contains(&kind.as_str());
    if (!texts.is_empty() && ["mp4", "m4v", "webm", "mkv"].contains(&kind.as_str())) || tagged {
        let with = produced.with_extension(format!("dm-subtitles.{kind}"));
        let written = if kind == "webm" || kind == "mkv" {
            crate::mkvmux::join_tagged_async(vec![produced.clone()], texts, with.clone(), tags)
                .await
        } else {
            crate::mp4mux::join_cut_tagged_async(
                vec![produced.clone()],
                texts,
                with.clone(),
                None,
                None,
                tags,
            )
            .await
        };
        if written.is_ok() {
            let _ = tokio::fs::rename(&with, &produced).await;
        } else {
            let _ = tokio::fs::remove_file(&with).await;
        }
    }
    place(&produced, options).await
}

/// Whether a file is an MPEG transport stream, whatever its name: a sync
/// byte at the start of its first two packets.
async fn is_transport_stream(path: &Path) -> bool {
    use tokio::io::AsyncReadExt;
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut head = [0u8; 189];
    file.read_exact(&mut head).await.is_ok() && head[0] == 0x47 && head[188] == 0x47
}

/// Moves a finished video file into its category folder under the download
/// directory, the same placement every other download gets.
async fn place(produced: &Path, options: &Options) -> Result<Finished, Error> {
    // The fetch names the file it meant to leave; if it is not there, the usual
    // cause is a join that never happened, and "No such file" says nothing.
    if !produced.is_file() {
        return Err(Error::Internal(
            "the video was fetched but the file it named is missing".into(),
        ));
    }
    let name = produced
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());

    /* Placed by the same code as every other download — the category
     * folder, a name nothing else has, a rename that falls back to a copy
     * across filesystems, the digest checked and a failing file removed. */
    let (dest, bytes, category) =
        crate::download::finish(&name, None, &[produced.to_path_buf()], options).await?;
    let _ = tokio::fs::remove_dir_all(&options.parts_dir).await;

    Ok(Finished {
        left_behind: None,
        path: dest,
        bytes,
        // The streams manage their own connections; the column would be guessing.
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 0,
    })
}

/// The wait between two tries at removing a cancelled run's staging folder ([`remove_staging`]).
const REMOVE_AGAIN_AFTER: std::time::Duration = std::time::Duration::from_millis(200);

/// A cancelled run's staging folder, removed. Tried again for a few
/// seconds: on Windows a file still open by a process that is only now
/// ending cannot be deleted, and the first try left the folder behind.
async fn remove_staging(dir: &Path) {
    for attempt in 0..15 {
        match tokio::fs::remove_dir_all(dir).await {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) if attempt < 14 => {
                tokio::time::sleep(REMOVE_AGAIN_AFTER).await;
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod coverage_tests;
