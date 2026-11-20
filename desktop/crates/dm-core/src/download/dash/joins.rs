// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The two halves of `join_tracks`: the tracks fetched, and the files they came to joined
//! into one.

use super::*;

/// The tracks fetched, gathered one file each, the row told how far they have got; checked
/// for a stop.
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
    Ok(files)
}

/// The fetched `files` joined into `joined`, written beside them under `.partial` and named once
/// whole; then they are removed. Run to its end like the remux of a transport stream: cut loose
/// from a pause it would go on writing a file a resume makes afresh.
pub(super) async fn join_beside(
    files: &[PathBuf],
    texts: Texts,
    joined: &Path,
    matroska: bool,
    options: &Options,
) -> Result<(), Error> {
    let extension = match joined.extension() {
        Some(named) => named.to_string_lossy().into_owned(),
        None if matroska => "mkv".into(),
        None => "mp4".into(),
    };
    let partial = options
        .parts_dir
        .join(format!("joined.partial.{extension}"));
    let _ = fs::remove_file(&partial).await;
    if let Err(why) = join_files(files, texts, &partial, matroska, options).await {
        let _ = fs::remove_file(&partial).await;
        return Err(Error::Internal(format!(
            "the picture and sound could not be joined: {why}"
        )));
    }
    fs::rename(&partial, joined).await?;
    for file in files {
        let _ = fs::remove_file(file).await;
    }
    Ok(())
}

/// A transport-stream picture scrambled by SAMPLE-AES beside packed AAC sound, which dm does not
/// put in the clear yet: refused before its pieces are fetched (`hls::resolve_pair`), and here.
const PACKED_BESIDE_SEALED: &str =
    "this stream's picture is scrambled (SAMPLE-AES) and its sound is packed audio";

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
        // A transport stream whose samples are scrambled (SAMPLE-AES) has its locks beside it.
        let sealed = if crate::tsmux::is_transport_stream(picture) {
            super::sealed::locks_beside(picture).await?
        } else {
            None
        };
        if crate::tsmux::is_transport_stream(picture) && crate::tsmux::is_packed_aac(sound) {
            if sealed.is_some() {
                return Err(PACKED_BESIDE_SEALED.into());
            }
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
            let heard = super::sealed::locks_beside(sound).await?;
            if sealed.is_some() || heard.is_some() {
                let locks = [sealed.unwrap_or_default(), heard.unwrap_or_default()];
                let place = move |clock| match texts {
                    Texts::Hls(texts) => texts.timed_on(clock),
                    Texts::Timed(_) => Vec::new(),
                };
                let (seen, said, out) = (picture.clone(), sound.clone(), partial.to_path_buf());
                return crate::tsmux::join_ts_clear_with_text_async(seen, said, out, locks, place)
                    .await;
            }
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
