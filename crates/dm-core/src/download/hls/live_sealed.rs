// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A live HLS recording whose samples are scrambled by SAMPLE-AES with a clear key, put in the
//! clear as it ends (gaps row Y16). Each segment was written as it came, its key and vector kept
//! by where it starts in the recording (`LiveTrack::locks`). A fragmented MP4's samples are
//! cleared where they lie (`mp4mux::clear_cbcs`) and the recording placed as any is; a transport
//! stream's as it is put into an MP4 (`tsmux::to_mp4_clear`), with its sound where that was
//! recorded as a transport stream of its own (`tsmux::join_ts_clear_with_text`). It must be: the
//! stream as it came does not play.

use super::*;

use dm_media::sealed::Locks;

/// A transport-stream recording, put into an MP4 in the clear.
const CLEARED: &str = "live.cleared.mp4";

/// What a recording whose samples could not be put in the clear says.
const UNCLEARED: &str =
    "this live stream's samples are scrambled (SAMPLE-AES) and could not be put in the clear";

/// A recording as it ended: its picture's file, its sound's where that was recorded apart and
/// some of it written whole, the name it is placed under, and whether its initialisation
/// segment changed part-way.
pub(super) struct Recorded {
    pub(super) picture: PathBuf,
    pub(super) sound: Option<PathBuf>,
    pub(super) filename: String,
    pub(super) remapped: bool,
}

/// The recording put in the clear by `locks` (the picture's, then the sound's) and placed as the
/// finished file. A sound that cannot be put in the clear with it is left out: the picture is
/// still the recording, as a recording cannot be asked for again.
pub(super) async fn place(
    recorded: Recorded,
    locks: [Locks; 2],
    options: &Options,
) -> Result<Finished, Error> {
    let Recorded {
        picture,
        sound,
        filename,
        remapped,
    } = recorded;
    let [seen, heard] = locks;
    if !crate::tsmux::is_transport_stream(&picture) {
        if seen.any() {
            clear_in_place(&picture, seen).await?;
        }
        let sound = match sound {
            Some(at) if heard.any() => match clear_in_place(&at, heard).await {
                Ok(()) => Some(at),
                Err(_) => {
                    let _ = fs::remove_file(&at).await;
                    None
                }
            },
            other => other,
        };
        return steps::place_live(picture, sound, filename, remapped, options).await;
    }
    let out = options.parts_dir.join(CLEARED);
    let _ = fs::remove_file(&out).await;
    // With its sound where that is a transport stream of its own; packed audio beside a scrambled
    // picture is not joined (`dash::joins`), and the picture is placed alone.
    let joined = match &sound {
        Some(at) if crate::tsmux::is_transport_stream(at) => {
            let locks = [seen.clone(), heard];
            let (from, said, into) = (picture.clone(), at.clone(), out.clone());
            crate::tsmux::join_ts_clear_with_text_async(from, said, into, locks, |_| Vec::new())
                .await
                .is_ok()
        }
        _ => false,
    };
    if !joined {
        let _ = fs::remove_file(&out).await;
        let (from, into) = (picture.clone(), out.clone());
        let made =
            tokio::task::spawn_blocking(move || crate::tsmux::to_mp4_clear(&from, &into, &seen))
                .await
                .map_err(|e| Error::Internal(format!("clearing stopped: {e}")))?;
        if let Err(why) = made {
            let _ = fs::remove_file(&out).await;
            return Err(Error::Internal(format!("{UNCLEARED}: {why}")));
        }
    }
    let _ = fs::remove_file(&picture).await;
    if let Some(at) = &sound {
        let _ = fs::remove_file(at).await;
    }
    // Stopped by the person: the recording is finished, so placing it does not wait on the stop.
    let placing = Options {
        control: crate::control::Control::new(),
        ..options.clone()
    };
    let name = format!("{}.mp4", media_stem(&filename));
    let (path, bytes, category) = finish(&name, None, &[out], &placing).await?;
    Ok(Finished {
        left_behind: None,
        disguised: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: false,
        mirrors: 1,
    })
}

/// A fragmented MP4 recording's samples (`cbcs`) put in the clear where they lie.
async fn clear_in_place(path: &Path, locks: Locks) -> Result<(), Error> {
    let at = path.to_path_buf();
    let made = tokio::task::spawn_blocking(move || crate::mp4mux::clear_cbcs(&at, &locks))
        .await
        .map_err(|e| Error::Internal(format!("clearing stopped: {e}")))?;
    made.map_err(|why| Error::Internal(format!("{UNCLEARED}: {why}")))
}
