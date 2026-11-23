// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A part of a video saved as GIF: dm's own encoder in `dm_media::gif` (gaps row A6); here what
//! the engine adds, the file written and the work off the async runtime.

use std::path::{Path, PathBuf};

pub use dm_media::gif::*;

use crate::Error;

/// Whether a part of the video at `url` can be made a GIF here. On Linux it
/// is made from the video's AV1 stream, which only YouTube's pages are read
/// for by dm itself; a stream caught elsewhere is H.264.
pub fn possible_for(url: &str) -> bool {
    // Not on Android, which has no decoder dm can read a video's pictures with (gaps row N24).
    !cfg!(target_os = "android")
        && possible()
        && (!cfg!(target_os = "linux") || crate::youtube::video_id(url).is_some())
}

/// The part of a video at `mp4` (an H.264 MP4) saved as a GIF at `target`
/// ([`from_video_bytes`]). Returns the GIF's length.
pub fn from_video(mp4: &Path, target: &Path) -> Result<u64, Error> {
    // `to_string`, whichever error type `dm_media` gives.
    let bytes = from_video_bytes(mp4).map_err(|e| Error::Internal(e.to_string()))?;
    crate::write_atomic(target, &bytes)
        .map_err(|e| Error::Internal(format!("the GIF could not be written: {e}")))?;
    Ok(bytes.len() as u64)
}

/// [`from_video`], off the async runtime, with a turn at the computer's heavy work
/// (`heavy`, gaps row 185).
pub async fn from_video_async(mp4: PathBuf, target: PathBuf) -> Result<u64, Error> {
    crate::heavy::run(move || from_video(&mp4, &target))
        .await
        .map_err(|e| Error::Internal(format!("making the GIF stopped: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gif_is_offered_where_the_part_can_be_decoded() {
        let youtube = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
        let stream = "https://cdn.example/live/index.m3u8";
        if cfg!(target_os = "linux") {
            // From YouTube's AV1 stream only: a stream caught elsewhere is H.264.
            assert!(possible_for(youtube));
            assert!(!possible_for(stream));
        } else {
            assert_eq!(possible_for(youtube), possible());
            assert_eq!(possible_for(stream), possible());
        }
    }
}
