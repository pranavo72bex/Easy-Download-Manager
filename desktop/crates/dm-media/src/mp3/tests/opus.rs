// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::rates::frame_rates;
use super::*;

/// An Opus packet of 20 ms of silence (RFC 6716): a table-of-contents byte for CELT alone,
/// full band, 20 ms, one frame (0xF8 mono, 0xFC stereo), and a frame whose first range-coded
/// symbol is the silence flag, set. Made here, so the fixture is the same on every run.
fn silent(channels: u8) -> Vec<u8> {
    vec![if channels == 2 { 0xFC } else { 0xF8 }, 0xFF, 0xFE]
}

/// Where the system decodes Opus (Windows), it says it does; a WebM is taken only for Opus
/// sound there, and an M4A, MP4 or AAC file always, by its name as before.
#[test]
fn a_webm_is_taken_for_its_opus_where_the_system_decodes_opus() {
    let dir = tempfile::tempdir().unwrap();
    let opus = dir.path().join("sound.webm");
    crate::mkvmux::testing::opus_webm(&opus, 2, 312, 20, &vec![silent(2); 10]);
    assert_eq!(takes(&opus), opus_possible());
    assert!(!takes(&dir.path().join("sound.weba")));
    let vorbis = dir.path().join("vorbis.webm");
    crate::mkvmux::testing::tiny_webm(&vorbis, "A_VORBIS", 2, 4);
    assert!(!takes(&vorbis));
    let picture = dir.path().join("picture.webm");
    crate::mkvmux::testing::tiny_webm(&picture, "V_VP9", 1, 4);
    assert!(!takes(&picture));
    for name in ["a.m4a", "a.MP4", "a.aac"] {
        assert!(takes(&dir.path().join(name)), "{name}");
    }
    for name in ["a.ogg", "a.opus", "a.mkv", "a"] {
        assert!(!takes(&dir.path().join(name)), "{name}");
    }
    #[cfg(windows)]
    assert!(
        crate::mf::opus_decoder(),
        "Windows 10 and 11 carry Media Foundation's Opus decoder"
    );
}

/// A WebM whose sound is not Opus, or that has no sound, is refused in words and never
/// panics.
#[test]
fn a_webm_with_other_sound_or_none_is_refused_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let vorbis = dir.path().join("vorbis.webm");
    crate::mkvmux::testing::tiny_webm(&vorbis, "A_VORBIS", 2, 4);
    let said = convert_bytes(&vorbis, None).unwrap_err();
    assert!(said.contains("VORBIS") && said.contains("Opus"), "{said}");
    let picture = dir.path().join("picture.webm");
    crate::mkvmux::testing::tiny_webm(&picture, "V_VP9", 1, 4);
    let said = convert_bytes(&picture, None).unwrap_err();
    assert!(said.contains("no sound"), "{said}");
    if !opus_possible() {
        let opus = dir.path().join("sound.webm");
        crate::mkvmux::testing::opus_webm(&opus, 2, 312, 20, &vec![silent(2); 10]);
        let said = convert_bytes(&opus, None).unwrap_err();
        assert!(said.contains("Opus decoder"), "{said}");
    }
}

/// Two seconds of Opus made here (100 silent stereo packets of 20 ms, the decoder's 312
/// samples of pre-skip named in its header) become an MP3 of whole frames at the rate asked
/// for, as long as the sound: 2 s less the pre-skip, at 48 kHz, which an MP3 keeps.
#[cfg(windows)]
#[test]
fn an_opus_sound_made_here_becomes_an_mp3_as_long_as_it_is() {
    if !opus_possible() {
        eprintln!("no Opus decoder on this Windows: nothing to check");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for (channels, kbps) in [(2u8, None), (1, Some(64))] {
        let webm = dir.path().join(format!("sound{channels}.webm"));
        crate::mkvmux::testing::opus_webm(&webm, channels, 312, 20, &vec![silent(channels); 100]);
        let pcm = decode::read(&webm).unwrap();
        assert_eq!((pcm.channels, pcm.rate), (usize::from(channels), 48000));
        assert_eq!(pcm.samples.len(), (96_000 - 312) * usize::from(channels));
        assert!(pcm.samples.iter().all(|s| s.unsigned_abs() < 2));
        // No rate asked for, as the app asks: written by `convert`, at the default rate.
        let mp3 = dir.path().join(format!("sound{channels}.mp3"));
        let length = convert(&webm, &mp3).unwrap();
        let mut bytes = std::fs::read(&mp3).unwrap();
        assert_eq!(bytes.len() as u64, length);
        let mut want = default_kbps(usize::from(channels));
        if let Some(kbps) = kbps {
            bytes = convert_bytes(&webm, Some(kbps)).unwrap();
            want = kbps;
        }
        let rates = frame_rates(&bytes, 48000);
        assert!(rates.iter().all(|r| *r == want), "{rates:?}");
        assert_eq!(rates.len(), (96_000 - 312usize).div_ceil(FRAME) + 1);
        let out = heard(&bytes);
        assert_eq!((out.channels, out.rate), (usize::from(channels), 48000));
        assert!(out.samples.iter().all(|s| s.unsigned_abs() < 2));
    }
}

/// A real YouTube sound in Opus (format 251) saved as MP3 is the same sound as the video's
/// AAC (format 140, the same two seconds): the MP3, decoded by another decoder, matches the
/// AAC read by the system at the shift that lines them up, so Opus is decoded right, not
/// merely read.
#[cfg(windows)]
#[test]
fn a_real_opus_sound_becomes_an_mp3_of_the_same_sound_as_its_aac() {
    if !opus_possible() || !possible() {
        eprintln!("no Opus or AAC decoder on this Windows: nothing to check");
        return;
    }
    let webm = crate::fixtures().join("media/mp4mux/av1-sound.f251.webm");
    let m4a = crate::fixtures().join("media/mp4mux/vp9-sound.f140.m4a");
    assert!(takes(&webm));
    let bytes = convert_bytes(&webm, Some(256)).unwrap();
    let rates = frame_rates(&bytes, 48000);
    assert!(rates.iter().all(|r| *r == 256));
    let opus = decode::read(&webm).unwrap();
    let seconds = opus.samples.len() as f64 / (opus.channels as f64 * 48000.0);
    let made = rates.len() as f64 * FRAME as f64 / 48000.0;
    assert!(
        (made - seconds).abs() < 0.1,
        "{made} s of MP3 for {seconds} s"
    );
    let mp3 = heard(&bytes);
    let aac = decode::read(&m4a).unwrap();
    assert_eq!((mp3.channels, aac.channels, aac.rate), (1, 1, 48000));
    // Normalised correlation of 0.2 s of the AAC with the MP3, at the best shift: the MP3
    // runs later, by Opus's delay against AAC's and the MP3 encoder's and decoder's.
    let (from, span) = (12_000usize, 9_600usize);
    let mut best = (0i64, f64::MIN);
    for shift in -6_000i64..2_000 {
        let (mut xy, mut xx, mut yy) = (0f64, 0f64, 0f64);
        for i in from..from + span {
            let Some(&y) = usize::try_from(i as i64 + shift)
                .ok()
                .and_then(|j| mp3.samples.get(j))
            else {
                continue;
            };
            let x = f64::from(aac.samples[i]);
            let y = f64::from(y);
            xy += x * y;
            xx += x * x;
            yy += y * y;
        }
        let c = xy / (xx * yy).sqrt().max(1e-9);
        if c > best.1 {
            best = (shift, c);
        }
    }
    eprintln!(
        "the MP3 of the Opus against the AAC: {:.4} at a shift of {}",
        best.1, best.0
    );
    assert!(best.1 > 0.99, "{best:?}");
}

/// A WebM's words, as dm writes them when it saves one (Info's title, Matroska's tags), are
/// read back for the MP3's ID3 header, as an M4A's are (gaps rows Y11, Y25).
#[test]
fn a_webms_words_are_read_for_the_mp3s_id3_header() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain.webm");
    crate::mkvmux::testing::opus_webm(&plain, 2, 312, 20, &vec![silent(2); 10]);
    assert_eq!(
        crate::mp4mux::tags_of(&plain),
        crate::mp4mux::Tags::default()
    );
    let said = crate::mp4mux::Tags {
        title: "Me at the zoo".into(),
        artist: "jawed".into(),
        date: "2005-04-23".into(),
        source: "https://www.youtube.com/watch?v=jNQXAC9IVRw".into(),
        ..Default::default()
    };
    let tagged = dir.path().join("tagged.webm");
    crate::mkvmux::join_tagged(&[plain], &[], &tagged, &said).unwrap();
    let read = crate::mp4mux::tags_of(&tagged);
    assert_eq!(
        (&read.title, &read.artist, &read.date, &read.source),
        (&said.title, &said.artist, &said.date, &said.source)
    );
    if opus_possible() {
        let bytes = convert_bytes(&tagged, None).unwrap();
        assert_eq!(&bytes[..4], b"ID3\x04");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("TIT2") && text.contains("Me at the zoo"));
        assert!(text.contains("TPE1") && text.contains("jawed"));
        assert!(text.contains("TDRC") && text.contains("2005-04-23"));
        assert!(text.contains("WOAS") && text.contains("jNQXAC9IVRw"));
        assert!(frame_rates(&bytes, 48000).iter().all(|r| *r == 192));
    }
}
