// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An HLS picture and sound kept in playlists of their own, each segment with its own key
//! (`Part::key`, which may change part-way) and its own initialisation segment (which may change
//! part-way too), and samples scrambled by SAMPLE-AES with a clear key put in the clear track by
//! track before the two are joined (gaps row Y16).
//!
//! A piece scrambled whole (AES-128) is decrypted as it is fetched, as before. One whose samples
//! are scrambled is kept as it comes; once its track is gathered, a fragmented MP4's samples
//! (Common Encryption's `cbcs`) are put in the clear where they lie, before the track is named
//! (`mp4mux::clear_cbcs`), and a transport stream's as the two tracks are joined into the MP4
//! (`tsmux::join_ts_clear_with_text`), by the locks written beside its gathered track before it
//! was named: a gathered track found after a stop is either in the clear or has its locks.

use std::fmt::Write as _;

use super::*;
use edm_media::sealed::Locks;

/// What a track whose samples could not be put in the clear says.
const UNCLEARED: &str =
    "this stream's samples are scrambled (SAMPLE-AES) and could not be put in the clear";

/// An HLS stream as a track's pieces, each segment with the lock of its own key, where it has
/// one, and the vector its number makes (an initialisation segment never has one).
pub(super) fn hls_track(medium: Medium, s: &stream::Stream, keys: &hls::Keys) -> Track {
    Track {
        medium,
        id: String::new(),
        bandwidth: 0,
        height: 0,
        webm: false,
        indexed: None,
        seams: Vec::new(),
        pieces: s
            .parts
            .iter()
            .map(|p| dash::Piece {
                url: p.url.clone(),
                range: p.range,
                lock: lock_of(p, keys),
                millis: (p.duration * 1000.0).round() as u64,
            })
            .collect(),
    }
}

/// The lock of one segment: its own key (fetched once, by its address) and vector.
fn lock_of(part: &stream::Part, keys: &hls::Keys) -> Option<dash::Lock> {
    let declared = part.key.as_ref().filter(|_| !part.init)?;
    Some(dash::Lock {
        // One missing is refused by the decrypting or the clearing, as a key of the wrong length.
        key: keys.get(&declared.uri).cloned().unwrap_or_default(),
        iv: stream::iv_for(declared, part.sequence),
        sample: declared.sample,
    })
}

/// Where each of `track`'s pieces (its part files numbered from `first`, every one whole)
/// starts in the file they are gathered into, one after another, with its SAMPLE-AES key and
/// vector; `None` where no piece's samples are scrambled.
pub(super) async fn sealed_locks(
    track: &Track,
    first: usize,
    parts_dir: &Path,
) -> Result<Option<Locks>, Error> {
    let sampled = |p: &dash::Piece| p.lock.as_ref().is_some_and(|l| l.sample);
    if !track.pieces.iter().any(sampled) {
        return Ok(None);
    }
    let mut locks = Vec::with_capacity(track.pieces.len());
    let mut at = 0u64;
    for (i, piece) in track.pieces.iter().enumerate() {
        let lock = match piece.lock.as_ref().filter(|l| l.sample) {
            Some(lock) => {
                let key: [u8; 16] = lock
                    .key
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::Internal("a stream key must be 16 bytes".into()))?;
                Some((key, lock.iv))
            }
            None => None,
        };
        locks.push((at, lock));
        at += fs::metadata(part_path(parts_dir, first + i)).await?.len();
    }
    Ok(Some(Locks(locks)))
}

/// The track gathered at `partial` (to be named `gathered`) put in the clear by `locks`: a
/// fragmented MP4's samples where they lie; a transport stream's locks written beside
/// `gathered`, for the join that puts it into the MP4 to clear it.
pub(super) async fn clear_gathered(
    partial: &Path,
    gathered: &Path,
    locks: Locks,
) -> Result<(), Error> {
    if crate::tsmux::is_transport_stream(partial) {
        fs::write(locks_path(gathered), encode(&locks)).await?;
        return Ok(());
    }
    let at = partial.to_path_buf();
    let made = tokio::task::spawn_blocking(move || crate::mp4mux::clear_cbcs(&at, &locks))
        .await
        .map_err(|e| Error::Internal(format!("clearing stopped: {e}")))?;
    made.map_err(|why| Error::Internal(format!("{UNCLEARED}: {why}")))
}

/// The locks written beside a gathered transport-stream track whose samples are scrambled;
/// `None` for one in the clear.
pub(super) async fn locks_beside(track: &Path) -> Result<Option<Locks>, String> {
    match fs::read_to_string(locks_path(track)).await {
        Ok(text) => decode(&text)
            .map(Some)
            .ok_or_else(|| format!("{UNCLEARED}: its keys could not be read back")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{UNCLEARED}: {e}")),
    }
}

/// Where a gathered track's locks are written.
fn locks_path(track: &Path) -> PathBuf {
    track.with_extension("locks")
}

/// Locks as lines of text: where a segment starts, then its key and vector in hex, or `-`.
fn encode(locks: &Locks) -> String {
    let mut out = String::new();
    for (at, lock) in &locks.0 {
        let _ = write!(out, "{at}");
        match lock {
            Some((key, iv)) => {
                for bytes in [key, iv] {
                    out.push(' ');
                    for b in bytes {
                        let _ = write!(out, "{b:02x}");
                    }
                }
            }
            None => out.push_str(" -"),
        }
        out.push('\n');
    }
    out
}

/// [`encode`]'s lines read back; `None` where one is not as it writes them.
fn decode(text: &str) -> Option<Locks> {
    let bytes = |hex: &str| -> Option<[u8; 16]> {
        let mut out = [0u8; 16];
        if hex.len() != 32 {
            return None;
        }
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(out)
    };
    let mut locks = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let lock = match words.as_slice() {
            [_, "-"] => None,
            [_, key, iv] => Some((bytes(key)?, bytes(iv)?)),
            _ => return None,
        };
        locks.push((words.first()?.parse().ok()?, lock));
    }
    Some(Locks(locks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_read_back_as_they_were_written() {
        let locks = Locks(vec![
            (0, None),
            (188, Some(([0x3c; 16], [7; 16]))),
            (5_000_000_000, Some(([0xa5; 16], [0; 16]))),
        ]);
        assert_eq!(decode(&encode(&locks)), Some(locks));
        assert_eq!(decode(""), Some(Locks::default()));
        assert_eq!(decode("12 zz"), None);
        assert_eq!(decode("x -"), None);
    }
}
