// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An HLS stream whose samples are scrambled by SAMPLE-AES with a clear key: its segments are
//! fetched as they come, joined, and the samples put in the clear (gaps row Y16).
//!
//! The key is the playlist's own, an address any player may fetch it from (a key a rights system
//! keeps is refused before this: `stream::protected_key`). A transport stream's H.264 and AAC are
//! cleared as they are put into an MP4, which they must be: the stream as it came does not play
//! (`tsmux::to_mp4_clear`); a fragmented MP4's samples, scrambled by Common Encryption's `cbcs`,
//! are cleared where they lie (`mp4mux::clear_cbcs`). Each segment keeps its own key and vector,
//! by where it starts in the joined file.

use super::*;

use dm_media::sealed::Locks;

/// The joined stream, while its samples are put in the clear.
const JOINED: &str = "sealed.joined";
/// A transport stream's, put into an MP4 in the clear.
const CLEARED: &str = "sealed.mp4";

/// The stream's fetched segments (`parts`, one for each of `listed`'s parts) joined and put in
/// the clear: the one file to place, and its name.
pub(super) async fn clear(
    listed: &stream::Stream,
    keys: &Keys,
    parts: &[PathBuf],
    options: &Options,
) -> Result<(Vec<PathBuf>, String), Error> {
    if parts.len() != listed.parts.len() {
        return Err(Error::Internal(
            "a scrambled stream's segments went missing before they were joined".into(),
        ));
    }
    let mut locks = Vec::with_capacity(parts.len());
    let mut at = 0u64;
    for (part, path) in listed.parts.iter().zip(parts) {
        let lock = match part.key.as_ref().filter(|k| k.sample && !part.init) {
            Some(declared) => {
                let key: [u8; 16] = keys
                    .get(&declared.uri)
                    .and_then(|k| k.as_slice().try_into().ok())
                    .ok_or_else(|| Error::Internal("a stream key must be 16 bytes".into()))?;
                Some((key, stream::iv_for(declared, part.sequence)))
            }
            None => None,
        };
        locks.push((at, lock));
        at += fs::metadata(path).await?.len();
    }
    let locks = Locks(locks);
    options.control.check()?;
    let joined = options.parts_dir.join(JOINED);
    let _ = fs::remove_file(&joined).await;
    join_parts_copy(parts, &joined).await?;
    let stopped = |e: tokio::task::JoinError| Error::Internal(format!("clearing stopped: {e}"));
    match listed.container.as_str() {
        "ts" => {
            let out = options.parts_dir.join(CLEARED);
            let (from, into) = (joined.clone(), out.clone());
            let made = tokio::task::spawn_blocking(move || {
                crate::tsmux::to_mp4_clear(&from, &into, &locks)
            })
            .await
            .map_err(stopped)?;
            let _ = fs::remove_file(&joined).await;
            if let Err(why) = made {
                let _ = fs::remove_file(&out).await;
                return Err(Error::Internal(format!(
                    "this stream's samples are scrambled (SAMPLE-AES) and could not be put in \
                     the clear: {why}"
                )));
            }
            Ok((vec![out], format!("{}.mp4", ts_stem(&listed.filename))))
        }
        "mp4" | "m4a" => {
            let at = joined.clone();
            let made = tokio::task::spawn_blocking(move || crate::mp4mux::clear_cbcs(&at, &locks))
                .await
                .map_err(stopped)?;
            if let Err(why) = made {
                let _ = fs::remove_file(&joined).await;
                return Err(Error::Internal(format!(
                    "this stream's samples are scrambled (SAMPLE-AES) and could not be put in \
                     the clear: {why}"
                )));
            }
            Ok((vec![joined], listed.filename.clone()))
        }
        _ => {
            let _ = fs::remove_file(&joined).await;
            Err(Error::Internal(
                "this stream's sound is scrambled (SAMPLE-AES) in packed audio, which dm does \
                 not put in the clear yet"
                    .into(),
            ))
        }
    }
}
