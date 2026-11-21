// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The codec filter (gaps row Y20): a picture and a sound are taken only as a pair dm can join
//! into the file it makes of them, where the manifest lists one.
//!
//! dm writes an MP4 (`mp4mux`, which copies an MP4's samples whatever they hold, and takes a
//! WebM's VP9, AV1 and Opus) or a Matroska file (`mkvmux`, which copies a WebM's blocks whatever
//! they hold, and takes an MP4's H.264, HEVC, AV1, VP9, VP8, AAC, MPEG audio, AC-3, E-AC-3 and
//! Opus). Which it writes is chosen as `download::dash` chooses: Matroska where it was asked
//! for (`mkv`); else an MP4, or Matroska where a WebM's codec has no place in one; a sound alone
//! is an `.m4a`. A codec the manifest does not name is taken to fit, as before.

use super::Candidate;

/// Which of the files dm writes a track can go into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fits {
    pub(super) mp4: bool,
    pub(super) matroska: bool,
}

/// What a track whose `codecs` the manifest names fits, in a WebM or an MP4: every codec it
/// lists must.
pub(super) fn fits(codecs: &str, webm: bool) -> Fits {
    codecs
        .split(',')
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
        .map(|codec| fits_one(codec, webm))
        .fold(
            Fits {
                mp4: true,
                matroska: true,
            },
            |all, one| Fits {
                mp4: all.mp4 && one.mp4,
                matroska: all.matroska && one.matroska,
            },
        )
}

/// One codec as RFC 6381 names it (`avc1.64001f`, `mp4a.40.2`, `vp09.00.40.08`, `opus`).
fn fits_one(codec: &str, webm: bool) -> Fits {
    let named = codec.to_ascii_lowercase();
    let (kind, rest) = named.split_once('.').unwrap_or((named.as_str(), ""));
    if webm {
        // A WebM's blocks go into Matroska as they are; an MP4 takes these of them.
        let mp4 = matches!(kind, "vp9" | "vp09" | "av1" | "av01" | "opus");
        return Fits {
            mp4,
            matroska: true,
        };
    }
    let matroska = match kind {
        "avc1" | "avc3" | "hvc1" | "hev1" | "av01" | "vp09" | "vp08" | "mp3" | "ac-3" | "ec-3"
        | "opus" => true,
        // AAC (MPEG-4's 40, MPEG-2's 66 to 68) and MPEG audio (69, 6B), by the object type after
        // the dot; none named is taken for AAC.
        "mp4a" => {
            let object = rest.split('.').next().unwrap_or_default();
            matches!(object, "" | "40" | "66" | "67" | "68" | "69" | "6b")
        }
        _ => false,
    };
    Fits {
        mp4: true,
        matroska,
    }
}

/// Whether dm joins a picture and a sound (either may be missing) into the file `container`
/// asks for.
fn joins(video: Option<Fits>, audio: Option<Fits>, container: Option<&str>) -> bool {
    let tracks = [video, audio];
    let mp4 = tracks.iter().flatten().all(|f| f.mp4);
    let matroska = tracks.iter().flatten().all(|f| f.matroska);
    match (video, container) {
        // A sound alone is an `.m4a`, an MP4 whatever was asked.
        (None, _) => mp4,
        (Some(_), Some("mkv")) => matroska,
        (Some(_), _) => mp4 || matroska,
    }
}

/// The pictures dm can join with one of `audios` (alone, where there is none) into what
/// `container` asks for; every one where none can, for the join to say why.
pub(super) fn with_a_partner(
    videos: Vec<Candidate>,
    audios: &[Candidate],
    container: Option<&str>,
) -> Vec<Candidate> {
    kept_or_all(videos, |v| {
        if audios.is_empty() {
            joins(Some(v.fits), None, container)
        } else {
            audios
                .iter()
                .any(|a| joins(Some(v.fits), Some(a.fits), container))
        }
    })
}

/// The sounds dm can join with `video` (alone, where there is none) into what `container` asks
/// for; every one where none can.
pub(super) fn beside(
    audios: Vec<Candidate>,
    video: Option<&Candidate>,
    container: Option<&str>,
) -> Vec<Candidate> {
    kept_or_all(audios, |a| {
        joins(video.map(|v| v.fits), Some(a.fits), container)
    })
}

/// Those of `all` that `keep` keeps; all of them where it keeps none.
fn kept_or_all(all: Vec<Candidate>, keep: impl Fn(&Candidate) -> bool) -> Vec<Candidate> {
    if all.iter().any(&keep) {
        all.into_iter().filter(|c| keep(c)).collect()
    } else {
        all
    }
}

#[cfg(test)]
mod tests;
