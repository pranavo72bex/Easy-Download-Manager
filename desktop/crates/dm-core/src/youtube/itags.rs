// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What YouTube's format numbers (`itag`) stand for: the picture's height and frame rate, the
//! codec family, and a sound's usual bitrate. The player's answer carries these as fields; this
//! table is for the answer that leaves them out (a format listed by `itag` alone), so that such
//! a stream still sorts and is chosen by its quality and not dropped to the bottom as size 0.

/// What a format carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// A picture with no sound.
    Video,
    /// A sound with no picture.
    Audio,
    /// Picture and sound in one file (the old direct formats).
    Muxed,
}

/// One format number's meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Itag {
    pub kind: Kind,
    /// The picture's height in pixels; 0 for a sound.
    pub height: u32,
    /// Frames a second; 0 for a sound.
    pub fps: u32,
    /// `avc1`, `vp9`, `av01`, `mp4a` or `opus`.
    pub codec: &'static str,
    /// A sound's usual bitrate in kilobits a second; 0 for a picture.
    pub kbps: u32,
}

const fn video(height: u32, fps: u32, codec: &'static str) -> Itag {
    Itag {
        kind: Kind::Video,
        height,
        fps,
        codec,
        kbps: 0,
    }
}

const fn audio(kbps: u32, codec: &'static str) -> Itag {
    Itag {
        kind: Kind::Audio,
        height: 0,
        fps: 0,
        codec,
        kbps,
    }
}

/// The meaning of a format number, where it is one of the well-known ones.
pub(super) fn describe(itag: u64) -> Option<Itag> {
    Some(match itag {
        // Direct files with picture and sound.
        18 => Itag {
            kind: Kind::Muxed,
            ..video(360, 30, "avc1")
        },
        22 => Itag {
            kind: Kind::Muxed,
            ..video(720, 30, "avc1")
        },
        // H.264 pictures.
        160 => video(144, 30, "avc1"),
        133 => video(240, 30, "avc1"),
        134 => video(360, 30, "avc1"),
        135 => video(480, 30, "avc1"),
        136 => video(720, 30, "avc1"),
        298 => video(720, 60, "avc1"),
        137 => video(1080, 30, "avc1"),
        299 => video(1080, 60, "avc1"),
        264 => video(1440, 30, "avc1"),
        266 => video(2160, 30, "avc1"),
        // VP9 pictures.
        278 => video(144, 30, "vp9"),
        242 => video(240, 30, "vp9"),
        243 => video(360, 30, "vp9"),
        244 => video(480, 30, "vp9"),
        247 => video(720, 30, "vp9"),
        302 => video(720, 60, "vp9"),
        248 => video(1080, 30, "vp9"),
        303 => video(1080, 60, "vp9"),
        271 => video(1440, 30, "vp9"),
        308 => video(1440, 60, "vp9"),
        313 => video(2160, 30, "vp9"),
        315 => video(2160, 60, "vp9"),
        // AV1 pictures.
        394 => video(144, 30, "av01"),
        395 => video(240, 30, "av01"),
        396 => video(360, 30, "av01"),
        397 => video(480, 30, "av01"),
        398 => video(720, 30, "av01"),
        399 => video(1080, 30, "av01"),
        400 => video(1440, 30, "av01"),
        401 => video(2160, 30, "av01"),
        // Sounds.
        139 => audio(48, "mp4a"),
        140 => audio(128, "mp4a"),
        141 => audio(256, "mp4a"),
        249 => audio(50, "opus"),
        250 => audio(70, "opus"),
        251 => audio(160, "opus"),
        _ => return None,
    })
}

/// How good a codec family is for the same picture size, for ordering formats of one height and
/// frame rate: AV1 over VP9 over H.264. Anything else ranks lowest.
pub(super) fn codec_rank(codec: &str) -> u8 {
    let codec = codec.to_ascii_lowercase();
    if codec.starts_with("av01") || codec.starts_with("av1") {
        3
    } else if codec.starts_with("vp9") || codec.starts_with("vp09") {
        2
    } else if codec.starts_with("avc1") || codec.starts_with("h264") {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_well_known_pictures_are_what_youtube_documents() {
        let video = |n| describe(n).map(|i| (i.height, i.fps, i.codec));
        assert_eq!(video(137), Some((1080, 30, "avc1")));
        assert_eq!(video(248), Some((1080, 30, "vp9")));
        assert_eq!(video(399), Some((1080, 30, "av01")));
        assert_eq!(video(299), Some((1080, 60, "avc1")));
        assert_eq!(video(303), Some((1080, 60, "vp9")));
        assert_eq!(video(313), Some((2160, 30, "vp9")));
        assert_eq!(video(401), Some((2160, 30, "av01")));
        assert_eq!(video(315), Some((2160, 60, "vp9")));
    }

    #[test]
    fn sounds_and_direct_files_are_told_apart() {
        let kbps = |n| describe(n).map(|i| (i.kind, i.kbps, i.codec));
        assert_eq!(kbps(140), Some((Kind::Audio, 128, "mp4a")));
        assert_eq!(kbps(251), Some((Kind::Audio, 160, "opus")));
        assert_eq!(kbps(250), Some((Kind::Audio, 70, "opus")));
        assert_eq!(
            describe(18).map(|i| (i.kind, i.height)),
            Some((Kind::Muxed, 360))
        );
        assert_eq!(
            describe(22).map(|i| (i.kind, i.height)),
            Some((Kind::Muxed, 720))
        );
    }

    #[test]
    fn an_unknown_number_is_none_and_no_picture_has_a_bitrate() {
        assert_eq!(describe(0), None);
        assert_eq!(describe(9999), None);
        for n in 0..500 {
            if let Some(i) = describe(n) {
                match i.kind {
                    Kind::Audio => assert!(i.kbps > 0 && i.height == 0, "{n}"),
                    _ => assert!(i.height > 0 && i.fps > 0 && i.kbps == 0, "{n}"),
                }
            }
        }
    }

    #[test]
    fn at_one_size_av1_ranks_over_vp9_over_h264() {
        assert!(codec_rank("av01.0.08M.08") > codec_rank("vp9"));
        assert!(codec_rank("vp9") > codec_rank("avc1.640028"));
        assert!(codec_rank("avc1.640028") > codec_rank("mp4a.40.2"));
    }
}
