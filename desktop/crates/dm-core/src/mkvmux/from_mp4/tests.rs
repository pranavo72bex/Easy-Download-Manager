// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! MKV files written from MP4 inputs, read back by dm's own Matroska reader
//! (`read_input`): tracks, codec IDs, CodecPrivate, every frame's bytes and
//! when it shows.

use super::super::{
    elements, find, join, needs_matroska, read_input,
    tests::{tiny_webm, tiny_webm_at},
};
use super::*;
use crate::mp4mux::{Edit, Sample};
use std::path::PathBuf;

fn media(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media/mp4mux")
        .join(name)
}

/// The EBML header's DocType.
fn doc_type(path: &Path) -> String {
    let data = std::fs::read(path).unwrap();
    let (_, header, _) = elements(&data).next().unwrap();
    String::from_utf8_lossy(find(header, 0x4282).unwrap()).into_owned()
}

/// One track of an MKV, as dm's reader reads it back.
struct Back {
    codec: String,
    rest: Vec<u8>,
    frames: Vec<Vec<u8>>,
    keys: usize,
    /// When each block shows, in milliseconds.
    ms: Vec<i64>,
}

impl Back {
    fn private(&self) -> Option<&[u8]> {
        find(&self.rest, CODEC_PRIVATE)
    }
}

fn read_back(path: &Path) -> Vec<Back> {
    let input = read_input(path, 0).unwrap();
    let data = std::fs::read(path).unwrap();
    input
        .tracks
        .iter()
        .map(|t| {
            let blocks: Vec<&Block> = input
                .blocks
                .iter()
                .filter(|b| b.track == t.number)
                .collect();
            Back {
                codec: t.codec.clone(),
                rest: t.rest.clone(),
                frames: blocks
                    .iter()
                    .map(|b| data[b.data_at as usize..(b.data_at + b.data_len) as usize].to_vec())
                    .collect(),
                keys: blocks.iter().filter(|b| b.key).count(),
                ms: blocks
                    .iter()
                    .map(|b| b.ticks * input.scale as i64 / 1_000_000)
                    .collect(),
            }
        })
        .collect()
}

/// The MP4 track with handler `handler` at `path`, with its samples' bytes.
fn mp4_track(path: &Path, handler: &[u8; 4]) -> (Track, Vec<Vec<u8>>) {
    let track = crate::mp4mux::tracks_of_input(path, 0)
        .unwrap()
        .into_iter()
        .find(|t| t.hdlr.get(16..20) == Some(handler))
        .unwrap();
    let data = std::fs::read(path).unwrap();
    let frames = track
        .samples
        .iter()
        .map(|s| data[s.offset as usize..(s.offset + u64::from(s.size)) as usize].to_vec())
        .collect();
    (track, frames)
}

/// The record a picture's sample entry keeps in its box `kind`.
fn picture_config(track: &Track, kind: &[u8; 4]) -> Vec<u8> {
    let (_, entry) = first_entry(&track.stsd).unwrap();
    child(&entry[78..], kind).unwrap().to_vec()
}

/// What the sample tables say each sample's show time is, in milliseconds.
fn shows_ms(track: &Track) -> Vec<i64> {
    block_times(track, false)
        .0
        .iter()
        .map(|(shows, _)| shows / 1_000_000)
        .collect()
}

/// Every track of the MKV against the MP4 track it came from: the same
/// frames, key frames and show times.
fn same_as(back: &Back, track: &Track, frames: &[Vec<u8>]) {
    assert_eq!(back.frames.len(), track.samples.len(), "{}", back.codec);
    assert!(back.frames == frames, "{}: the frames differ", back.codec);
    let syncs = track.samples.iter().filter(|s| s.sync).count();
    assert_eq!(back.keys, syncs, "{}", back.codec);
    assert_eq!(back.ms, shows_ms(track), "{}", back.codec);
}

#[test]
fn an_h264_picture_and_aac_sound_join_into_an_mkv_dm_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let (picture, sound) = (media("picture.mp4"), media("sound.m4a"));
    let out = dir.path().join("joined.mkv");
    let length = join(&[picture.clone(), sound.clone()], &out).unwrap();
    assert_eq!(length, std::fs::metadata(&out).unwrap().len());
    assert_eq!(doc_type(&out), "matroska", "H.264 and AAC are not WebM's");

    let back = read_back(&out);
    let codecs: Vec<&str> = back.iter().map(|b| b.codec.as_str()).collect();
    assert_eq!(codecs, ["V_MPEG4/ISO/AVC", "A_AAC"]);

    let (video, video_frames) = mp4_track(&picture, b"vide");
    assert_eq!(
        back[0].private(),
        Some(&picture_config(&video, b"avcC")[..])
    );
    same_as(&back[0], &video, &video_frames);
    assert!(find(&back[0].rest, VIDEO).is_some_and(|v| find(v, PIXEL_WIDTH).is_some()));

    let (audio, audio_frames) = mp4_track(&sound, b"soun");
    let (_, entry) = first_entry(&audio.stsd).unwrap();
    let (_, _, boxes_of) = sound_entry(entry, audio.timescale);
    let (object, asc) = esds_config(child(boxes_of, b"esds").unwrap()).unwrap();
    assert_eq!(object, 0x40);
    assert_eq!(back[1].private(), asc, "the AudioSpecificConfig");
    same_as(&back[1], &audio, &audio_frames);
    // The encoder's priming is before zero, as the edit list put it.
    if audio.edits.first().is_some_and(|e| e.media_time > 0) {
        assert!(back[1].ms[0] < 0, "{:?}", &back[1].ms[..2]);
    }
    assert!(back[0].ms.first().is_some_and(|ms| (0..=1).contains(ms)));
}

#[test]
fn an_hevc_picture_and_ac3_sound_go_into_an_mkv() {
    let dir = tempfile::tempdir().unwrap();
    let hevc = dir.path().join("hevc.mp4");
    crate::tsmux::to_mp4(&media("hevc.ts"), &hevc).unwrap();
    let ac3 = dir.path().join("ac3.mp4");
    crate::tsmux::to_mp4(&media("ac3.ts"), &ac3).unwrap();

    let out = dir.path().join("hevc.mkv");
    join(std::slice::from_ref(&hevc), &out).unwrap();
    let back = read_back(&out);
    assert_eq!(back[0].codec, "V_MPEGH/ISO/HEVC");
    let (video, frames) = mp4_track(&hevc, b"vide");
    assert_eq!(
        back[0].private(),
        Some(&picture_config(&video, b"hvcC")[..])
    );
    same_as(&back[0], &video, &frames);

    let out = dir.path().join("ac3.mkv");
    join(std::slice::from_ref(&ac3), &out).unwrap();
    let back = read_back(&out);
    let codecs: Vec<&str> = back.iter().map(|b| b.codec.as_str()).collect();
    assert_eq!(codecs, ["V_MPEG4/ISO/AVC", "A_AC3"]);
    assert_eq!(back[1].private(), None, "AC-3 has no CodecPrivate");
    let (audio, frames) = mp4_track(&ac3, b"soun");
    same_as(&back[1], &audio, &frames);
    let sound = find(&back[1].rest, AUDIO).unwrap();
    assert!(find(sound, CHANNELS).is_some_and(|c| super::super::uint(c) > 0));
}

#[test]
fn mpeg_audio_is_named_by_its_layer() {
    let dir = tempfile::tempdir().unwrap();
    for (name, codec) in [("mp3.ts", "A_MPEG/L3"), ("mp2.ts", "A_MPEG/L2")] {
        let mp4 = dir.path().join(format!("{name}.mp4"));
        crate::tsmux::to_mp4(&media(name), &mp4).unwrap();
        let out = dir.path().join(format!("{name}.mkv"));
        join(std::slice::from_ref(&mp4), &out).unwrap();
        let back = read_back(&out);
        assert_eq!(back[1].codec, codec);
        let (audio, frames) = mp4_track(&mp4, b"soun");
        same_as(&back[1], &audio, &frames);
    }
}

/// AV1 and Opus from an MP4 dm wrote out of WebM come back as the WebM
/// had them: the `av1C` record and the OpusHead are what it kept.
#[test]
fn av1_and_opus_from_an_mp4_come_back_as_the_webm_had_them() {
    let dir = tempfile::tempdir().unwrap();
    let (picture, sound) = (media("av1.f399.webm"), media("av1-sound.f251.webm"));
    let mp4 = dir.path().join("av1.mp4");
    crate::mp4mux::join(&[picture.clone(), sound.clone()], &mp4).unwrap();
    let out = dir.path().join("av1.mkv");
    join(std::slice::from_ref(&mp4), &out).unwrap();
    assert_eq!(doc_type(&out), "webm", "AV1 and Opus are WebM's");
    let back = read_back(&out);
    let codecs: Vec<&str> = back.iter().map(|b| b.codec.as_str()).collect();
    assert_eq!(codecs, ["V_AV1", "A_OPUS"]);
    let original = |path: &Path| {
        let input = read_input(path, 0).unwrap();
        find(&input.tracks[0].rest, CODEC_PRIVATE).unwrap().to_vec()
    };
    assert_eq!(back[0].private(), Some(&original(&picture)[..]));
    assert_eq!(back[1].private(), Some(&original(&sound)[..]));
    assert!(find(&back[1].rest, CODEC_DELAY).is_some());
    let (audio, frames) = mp4_track(&mp4, b"soun");
    assert_eq!(back[1].frames, frames);
    // Opus keeps its delay in its times: the first block at zero.
    assert_eq!(back[1].ms.first(), Some(&0));
    assert_eq!(audio.samples.len(), back[1].frames.len());
}

/// A pair no MP4 holds (VP8 or Vorbis beside an MP4) is joined into an
/// MKV; the MP4 writer refuses it as before.
#[test]
fn vp8_or_vorbis_beside_an_mp4_is_joined_into_an_mkv() {
    let dir = tempfile::tempdir().unwrap();
    let vp8 = dir.path().join("vp8.webm");
    tiny_webm(&vp8, "V_VP8", 1, 10);
    let vorbis = dir.path().join("vorbis.webm");
    tiny_webm(&vorbis, "A_VORBIS", 2, 10);
    let (picture, sound) = (media("picture.mp4"), media("sound.m4a"));

    for (pair, codecs) in [
        ([vp8.clone(), sound.clone()], ["V_VP8", "A_AAC"]),
        (
            [picture.clone(), vorbis.clone()],
            ["V_MPEG4/ISO/AVC", "A_VORBIS"],
        ),
    ] {
        assert!(needs_matroska(&pair));
        let refused = crate::mp4mux::join(&pair, &dir.path().join("no.mp4")).unwrap_err();
        assert!(refused.contains("into an MP4"), "{refused}");
        let out = dir.path().join("pair.mkv");
        join(&pair, &out).unwrap();
        assert_eq!(doc_type(&out), "matroska");
        let back = read_back(&out);
        let got: Vec<&str> = back.iter().map(|b| b.codec.as_str()).collect();
        assert_eq!(got, codecs);
        let tiny = back
            .iter()
            .find(|b| b.codec == "V_VP8" || b.codec == "A_VORBIS")
            .unwrap();
        assert_eq!(tiny.frames.len(), 10);
        assert_eq!(tiny.ms, (0..10).map(|i| i * 33).collect::<Vec<i64>>());
    }
    let vorbis_back = read_back(&dir.path().join("pair.mkv"));
    assert_eq!(
        vorbis_back[1].private(),
        Some(&[2, 1, 1, 0xAA, 0xBB, 0xCC][..]),
        "Vorbis's headers are carried"
    );

    // What an MP4 holds is not sent to Matroska.
    assert!(!needs_matroska(&[
        media("picture.f248.webm"),
        sound.clone()
    ]));
    assert!(!needs_matroska(&[picture, sound]));
    assert!(!needs_matroska(&[dir.path().join("missing.webm")]));
}

#[test]
fn show_times_come_from_the_tables_and_the_edit_list() {
    // Three pictures decoded I P B, shown I B P: 3000 ticks of 90000 each
    // (33.3 ms), composition offsets of two frames, one and none, and an
    // edit that starts the track at the first shown, two frames in.
    let sample = |cts: i32, sync: bool| Sample {
        offset: 0,
        size: 1,
        duration: 3000,
        cts,
        sync,
    };
    let mut track = Track {
        timescale: 90_000,
        edits: vec![Edit {
            duration: 100,
            media_time: 6000,
        }],
        edits_timescale: 1000,
        samples: vec![sample(6000, true), sample(9000, false), sample(3000, false)],
        ..Track::default()
    };
    let ms = |times: &[(i64, i64)]| -> Vec<(i64, i64)> {
        times
            .iter()
            .map(|(s, d)| (s / 1_000_000, d / 1_000_000))
            .collect()
    };
    let (times, start) = block_times(&track, false);
    assert_eq!(start, None);
    assert_eq!(ms(&times), [(0, -66), (66, -33), (33, 0)]);
    // Opus keeps the edit's start in its times: CodecDelay says it.
    assert_eq!(ms(&block_times(&track, true).0)[0], (66, 0));
    // An empty edit first waits: 500 ms of the movie's 1000.
    track.edits.insert(
        0,
        Edit {
            duration: 500,
            media_time: -1,
        },
    );
    assert_eq!(ms(&block_times(&track, false).0)[0], (500, 433));
    // A fragment's start (tfdt) counts from where it says.
    track.edits.clear();
    track.start = Some(90_000);
    let (times, start) = block_times(&track, false);
    assert_eq!(start, Some(1_000_000_000));
    assert_eq!(ms(&times)[0], (1066, 1000));
}

/// Fragmented MP4 inputs that say when they start are moved together, so
/// the earliest starts at zero and the later one keeps its lead.
#[test]
fn inputs_that_say_when_they_start_line_up_at_zero() {
    let input = |origin: Option<i64>, ticks: i64| Input {
        scale: 1,
        tracks: Vec::new(),
        blocks: vec![Block {
            input: 0,
            track: 1,
            ticks,
            decode: ticks,
            key: true,
            flags: 0x80,
            data_at: 0,
            data_len: 0,
            group: None,
        }],
        origin,
    };
    let mut read = vec![
        input(Some(10_000_000_000), 10_000_000_000),
        input(Some(10_500_000_000), 10_500_000_000),
        input(None, 0),
    ];
    super::super::line_up(&mut read, &[false; 3]);
    let ticks: Vec<i64> = read.iter().map(|i| i.blocks[0].ticks).collect();
    assert_eq!(ticks, [0, 500_000_000, 0]);

    // A WebM beside them is placed by its first block, on the same clock.
    let mut read = vec![
        input(Some(10_200_000_000), 10_200_000_000),
        input(None, 10_000_000_000),
    ];
    super::super::line_up(&mut read, &[false, true]);
    let ticks: Vec<i64> = read.iter().map(|i| i.blocks[0].ticks).collect();
    assert_eq!(ticks, [200_000_000, 0]);
    // WebM inputs alone keep their own times, as before.
    let mut read = vec![input(None, 10_000_000_000), input(None, 10_000_000_000)];
    super::super::line_up(&mut read, &[true, true]);
    assert_eq!(read[0].blocks[0].ticks, 10_000_000_000);
}

/// A live stream's VP8 picture, its clusters ten seconds in, beside its
/// MP4 sound whose fragment says it starts there too: both start at zero
/// in the MKV, as they played together.
#[test]
fn a_live_webm_picture_and_mp4_sound_stay_together() {
    let dir = tempfile::tempdir().unwrap();
    let vp8 = dir.path().join("live.webm");
    tiny_webm_at(&vp8, "V_VP8", 1, 10, 10_000);
    // The fragmented sound's tfdt rewritten to ten seconds.
    let sound = dir.path().join("live.m4a");
    let mut data = std::fs::read(media("sound.f140.m4a")).unwrap();
    let scale = crate::mp4mux::tracks_of_input(&media("sound.f140.m4a"), 0).unwrap()[0].timescale;
    let at = data.windows(4).position(|w| w == b"tfdt").unwrap() + 4;
    if data[at] == 1 {
        data[at + 4..at + 12].copy_from_slice(&(u64::from(scale) * 10).to_be_bytes());
    } else {
        data[at + 4..at + 8].copy_from_slice(&(scale * 10).to_be_bytes());
    }
    std::fs::write(&sound, &data).unwrap();

    let out = dir.path().join("live.mkv");
    join(&[vp8, sound], &out).unwrap();
    let back = read_back(&out);
    assert_eq!(back[0].codec, "V_VP8");
    assert_eq!(back[0].ms.first(), Some(&0));
    let first_sound = back[1].ms.first().copied().unwrap();
    assert!((-100..100).contains(&first_sound), "{first_sound}");
}

#[test]
fn an_esds_is_read_past_its_flags_and_long_lengths() {
    // ES descriptor with a dependence and an OCR stream (flags 0xA0), its
    // lengths written in four bytes, as some writers do.
    let info = [0x12, 0x10];
    let mut specific = vec![5, 0x80, 0x80, 0x80, info.len() as u8];
    specific.extend_from_slice(&info);
    let mut decoder = vec![4, 0x80, 0x80, 0x80, (13 + specific.len()) as u8, 0x40, 0x15];
    decoder.extend_from_slice(&[0u8; 11]);
    decoder.extend_from_slice(&specific);
    let mut es = vec![0, 1, 0xA0, 0, 2, 0, 3];
    es.extend_from_slice(&decoder);
    let mut esds = vec![0, 0, 0, 0, 3, es.len() as u8];
    esds.extend_from_slice(&es);
    assert_eq!(esds_config(&esds), Some((0x40, Some(&info[..]))));
    // A decoder config without its specific info, and bytes that are not
    // an esds at all.
    let mut bare = vec![0, 0, 0, 0, 3, 18, 0, 1, 0, 4, 13, 0x6B];
    bare.extend_from_slice(&[0u8; 12]);
    assert_eq!(esds_config(&bare), Some((0x6B, None)));
    assert_eq!(esds_config(&[0, 0, 0, 0, 9, 1, 1]), None);
    assert_eq!(esds_config(&[]), None);
}

#[test]
fn what_an_mkv_cannot_name_is_refused_in_words() {
    let entry = vec![0u8; 100];
    let said = describe(b"mp4v", &entry, 1, 90_000, &[]).unwrap_err();
    assert!(said.contains("mp4v"), "{said}");
    let said = describe(b"avc1", &entry, 1, 90_000, &[]).unwrap_err();
    assert!(said.contains("no configuration"), "{said}");
    let said = describe(b"alac", &entry, 2, 44_100, &[]).unwrap_err();
    assert!(said.contains("alac"), "{said}");
    // AC-3's rate and channels from its entry; its timescale where the
    // entry gives no rate.
    let described = describe(b"ac-3", &entry, 2, 44_100, &[]).unwrap();
    assert_eq!(described.sound, Some((44_100.0, 0)));
    assert_eq!(mpeg_audio(&[0xFF, 0xFB]), "A_MPEG/L3");
    assert_eq!(mpeg_audio(&[0xFF, 0xFD]), "A_MPEG/L2");
    assert_eq!(mpeg_audio(&[]), "A_MPEG/L3");
    assert_eq!(language(0x55c4), Some(*b"und"));
    assert_eq!(language(0x15c7), Some(*b"eng"));
    assert_eq!(language(0), None);
}

#[test]
fn what_is_not_an_mp4_is_refused_not_panicked_on() {
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("page.mp4");
    std::fs::write(&page, b"<!doctype html>").unwrap();
    let said = join(std::slice::from_ref(&page), &dir.path().join("out.mkv")).unwrap_err();
    assert!(said.contains("not an MP4 dm can join"), "{said}");
}

proptest::proptest! {
    #[test]
    fn any_sample_entry_is_described_without_a_panic(
        entry in proptest::collection::vec(proptest::num::u8::ANY, 0..200),
        kind in 1u64..3,
        fourcc in proptest::sample::select(vec![*b"avc1", *b"hvc1", *b"av01", *b"mp4a", *b"Opus", *b"ac-3", *b"zzzz"]),
    ) {
        let _ = describe(&fourcc, &entry, kind, 48_000, &entry);
        let _ = esds_config(&entry);
        let _ = first_entry(&entry);
    }
}
