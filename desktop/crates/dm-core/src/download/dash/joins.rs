// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The two halves of `join_tracks`: the tracks fetched, and the files they came to joined
//! into one.

use super::*;

/// The tracks fetched, gathered one file each, the row told how far they have got; checked
/// for a stop and for room to join them.
pub(super) async fn fetch_tracks_shown(
    client: &reqwest::Client,
    tracks: &[&Track],
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Vec<PathBuf>, Error> {
    let downloaded = Arc::new(AtomicU64::new(0));
    // The size where the manifest gives every piece's bytes; else about, from each track's
    // bitrate and playing time (gaps row Y22).
    let (exact, about) = size_of_tracks(tracks);
    let meter = report.map(|tx| {
        crate::download::ranged::watch_estimating(
            Arc::clone(&downloaded),
            exact,
            about.map(|bytes| Arc::new(AtomicU64::new(bytes))),
            tx,
        )
    });
    let fetched = fetch_tracks(client, tracks, options, &downloaded).await;
    if let Some((handle, finished)) = meter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    let files = fetched?;
    options.control.check()?;
    room_to_join(&files, &options.parts_dir, options)?;
    Ok(files)
}

/// The fetched `files` joined into one at `partial`: a Matroska file where one is to be
/// made (`matroska`), transport streams joined as such, else an MP4.
pub(super) async fn join_files(
    files: &[PathBuf],
    texts: Texts,
    partial: &Path,
    matroska: bool,
    options: &Options,
) -> Result<u64, String> {
    if matroska {
        join_mkv(files.to_vec(), texts, partial.to_path_buf()).await
    } else if let [picture, sound] = files.iter().collect::<Vec<_>>()[..]
        .iter()
        .map(|p| (*p).clone())
        .collect::<Vec<_>>()
        .as_slice()
    {
        if crate::tsmux::is_transport_stream(picture) && crate::tsmux::is_packed_aac(sound) {
            match texts {
                Texts::Hls(texts) => {
                    let place = move |clock| texts.timed_on(clock);
                    let (ts, aac, out) = (picture.clone(), sound.clone(), partial.to_path_buf());
                    crate::tsmux::join_packed_with_text_async(ts, aac, out, place).await
                }
                Texts::Timed(_) => {
                    crate::tsmux::join_packed_async(
                        picture.clone(),
                        sound.clone(),
                        partial.to_path_buf(),
                    )
                    .await
                }
            }
        } else if crate::tsmux::is_transport_stream(picture)
            && crate::tsmux::is_transport_stream(sound)
        {
            match texts {
                Texts::Hls(texts) => {
                    let place = move |clock| texts.timed_on(clock);
                    let (seen, heard, out) =
                        (picture.clone(), sound.clone(), partial.to_path_buf());
                    crate::tsmux::join_ts_with_text_async(seen, heard, out, place).await
                }
                Texts::Timed(_) => {
                    crate::tsmux::join_ts_async(
                        picture.clone(),
                        sound.clone(),
                        partial.to_path_buf(),
                    )
                    .await
                }
            }
        } else {
            join_mp4(files.to_vec(), texts, partial.to_path_buf(), options).await
        }
    } else {
        join_mp4(files.to_vec(), texts, partial.to_path_buf(), options).await
    }
}
