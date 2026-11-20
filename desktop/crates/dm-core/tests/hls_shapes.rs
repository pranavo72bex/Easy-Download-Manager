// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! HLS shapes that were refused (gaps row Y16), each fetched from a stream made here and served
//! on loopback, and its finished MP4 read back whole: an initialisation segment that changes
//! part-way (an advert break and back, its clock started afresh), and SAMPLE-AES with a clear
//! key, in a transport stream (Apple's sample encryption of H.264 and AAC) and in fragmented MP4
//! (Common Encryption's `cbcs`).

use std::collections::HashMap;

use dm_core::{client, download};
use dm_media::fmp4::{self, Spec, Writer};
use dm_media::mp4mux::testing::read_back;

mod common;
mod sealing;
use common::tempdir;
use sealing::*;

/// One run of a stream: its picture and sound, the samples each holds.
struct Run {
    init: Vec<u8>,
    segments: Vec<Vec<u8>>,
    picture: Vec<Vec<u8>>,
    sound: Vec<Vec<u8>>,
}

/// A run of `seconds` of 30-a-second pictures (one key frame a second) at `video_scale`, and
/// AAC frames of 1024 at `rate`, each sample's bytes marked with `mark`.
fn run(mark: u8, seconds: u32, video: Spec, rate: u32) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.mp4");
    let video_scale = video.timescale;
    let audio = Spec {
        entry: fmp4::mp4a(0x40, Some(&[0x11, 0x90]), rate, 2),
        timescale: rate,
        video: false,
        width: 0,
        height: 0,
    };
    let audio = if mark == 2 {
        Spec {
            entry: fmp4::mp4a(0x40, Some(&[0x12, 0x10]), rate, 2),
            ..audio
        }
    } else {
        audio
    };
    let mut writer = Writer::create(&path, vec![video, audio]).unwrap();
    let frames = seconds * 30;
    let sounds = u64::from(seconds) * u64::from(rate) / 1024;
    let (mut picture, mut sound) = (Vec::new(), Vec::new());
    let (mut f, mut a) = (0u32, 0u64);
    while f < frames || a < sounds {
        let picture_at = f64::from(f) / 30.0;
        let sound_at = a as f64 * 1024.0 / f64::from(rate);
        if f < frames && (picture_at <= sound_at || a >= sounds) {
            let data: Vec<u8> = (0..200 + f as usize % 50)
                .map(|i| mark ^ (i as u8).wrapping_mul(7) ^ f as u8)
                .collect();
            let dts = i64::from(f) * i64::from(video_scale) / 30;
            writer
                .push(0, data.clone(), dts, 0, f % 30 == 0, None)
                .unwrap();
            picture.push(data);
            f += 1;
        } else {
            let data: Vec<u8> = (0..60 + a as usize % 9)
                .map(|i| mark.wrapping_add(i as u8) ^ a as u8)
                .collect();
            writer
                .push(1, data.clone(), (a * 1024) as i64, 0, true, Some(1024))
                .unwrap();
            sound.push(data);
            a += 1;
        }
    }
    writer.finish().unwrap();
    let (init, segments) = cut(&std::fs::read(&path).unwrap());
    Run {
        init,
        segments,
        picture,
        sound,
    }
}

/// An advert break and back: the stream's initialisation segment changes to the advert's (another
/// picture size, another timescale, another sound rate) and back, and each run's clock starts
/// again at nought. The MP4 holds every sample once, in order, each decoded with its own run's
/// set-up, picture and sound starting each run together (gaps row Y16): refused before.
#[tokio::test]
async fn an_advert_break_with_its_own_initialisation_segment_is_one_mp4_in_step() {
    let main = || {
        picture_spec(
            &[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 0x67, 1, 2, 3],
            640,
            360,
            90_000,
        )
    };
    let advert = picture_spec(
        &[1, 0x4d, 0, 0x28, 0xff, 0xe1, 0, 4, 0x67, 9, 9, 9],
        1280,
        720,
        30_000,
    );
    let runs = [
        run(1, 3, main(), 48_000),
        run(2, 2, advert, 44_100),
        run(3, 2, main(), 48_000),
    ];
    assert_eq!(
        runs[0].init, runs[2].init,
        "the main stream's set-up is the same both times"
    );
    let mut files = vec![
        ("/v/main-init.mp4".to_string(), runs[0].init.clone()),
        ("/v/ad-init.mp4".to_string(), runs[1].init.clone()),
    ];
    let mut playlist = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:2\n");
    for (r, (run, map)) in runs.iter().zip(["main", "ad", "main"]).enumerate() {
        if r > 0 {
            playlist.push_str("#EXT-X-DISCONTINUITY\n");
        }
        playlist.push_str(&format!("#EXT-X-MAP:URI=\"{map}-init.mp4\"\n"));
        for (s, segment) in run.segments.iter().enumerate() {
            let name = format!("r{r}s{s}.m4s");
            playlist.push_str(&format!("#EXTINF:1.0,\n{name}\n"));
            files.push((format!("/v/{name}"), segment.clone()));
        }
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    files.push(("/v/show.m3u8".into(), playlist.into_bytes()));
    let url = serve(files).await;
    let dir = tempdir();
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a stream with an advert break is fetched");
    assert!(
        done.path.to_string_lossy().ends_with(".mp4"),
        "{:?}",
        done.path
    );
    let tracks = read_back(&done.path);
    assert_eq!(tracks.len(), 2, "a picture and a sound");
    let (picture, sound) = (&tracks[0], &tracks[1]);
    assert_eq!((&picture.handler, &sound.handler), (b"vide", b"soun"));

    // Every sample once, in order, with its own run's description.
    let all = |of: fn(&Run) -> &Vec<Vec<u8>>| -> Vec<Vec<u8>> {
        runs.iter().flat_map(|r| of(r).iter().cloned()).collect()
    };
    assert!(
        picture.samples == all(|r| &r.picture),
        "the pictures, whole and in order"
    );
    assert!(
        sound.samples == all(|r| &r.sound),
        "the sound, whole and in order"
    );
    assert_eq!(
        picture.entries.len(),
        2,
        "the main picture's set-up and the advert's"
    );
    assert_eq!(sound.entries.len(), 2);
    let counts: Vec<usize> = runs.iter().map(|r| r.picture.len()).collect();
    let expected: Vec<u32> = [1, 2, 1]
        .iter()
        .zip(&counts)
        .flat_map(|(d, n)| std::iter::repeat_n(*d, *n))
        .collect();
    assert_eq!(picture.described, expected);
    assert!(
        picture
            .durations
            .iter()
            .chain(&sound.durations)
            .all(|d| *d > 0),
        "time only goes on"
    );

    // Seven seconds, picture and sound alike, and each run starts on both together.
    assert!(
        (length_of(picture) - 7.0).abs() < 0.1,
        "{}",
        length_of(picture)
    );
    assert!((length_of(sound) - 7.0).abs() < 0.1, "{}", length_of(sound));
    let (mut p, mut s) = (0, 0);
    for run in &runs {
        let apart = starts_at(picture, p) - starts_at(sound, s);
        assert!(
            apart.abs() < 0.03,
            "a run's picture and sound {apart} s apart"
        );
        p += run.picture.len();
        s += run.sound.len();
    }
}

/* ---- SAMPLE-AES in a transport stream ------------------------------------------------ */

/// A transport stream whose H.264 slices and AAC frames are scrambled by SAMPLE-AES, its key
/// changing between its two segments, is put into an MP4 whose pictures and sound are, byte for
/// byte and stamp for stamp, those of the stream before it was scrambled (gaps row Y16): refused
/// before.
#[tokio::test]
async fn a_sample_aes_transport_stream_is_cleared_into_the_mp4_of_the_stream_in_the_clear() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/media/mp4mux/stream.ts"
    );
    let clear = std::fs::read(fixture).unwrap();
    let (pes, picture, sound) = pes_of(&clear);
    assert!(
        picture != 0 && sound != 0 && pes.len() > 4,
        "the fixture has a picture and sound"
    );
    let half = pes.len() / 2;
    let mut counters = HashMap::new();
    let mut segments = Vec::new();
    for (n, (part, key)) in [(&pes[..half], KEY_ONE), (&pes[half..], KEY_TWO)]
        .into_iter()
        .enumerate()
    {
        let sealed: Vec<(u16, Vec<u8>)> = part
            .iter()
            .map(|(pid, bytes)| {
                let header = 9 + usize::from(bytes[8]);
                let mut out = bytes[..header].to_vec();
                out.extend(seal_payload(
                    &bytes[header..],
                    *pid == picture,
                    &key,
                    &iv_of(n as u64),
                ));
                if *pid == picture {
                    // A picture's PES may say no length; emulation prevention changed it.
                    out[4] = 0;
                    out[5] = 0;
                }
                (*pid, out)
            })
            .collect();
        // Both kinds are scrambled: there is something to clear.
        for kind in [picture, sound] {
            let changed = part
                .iter()
                .zip(&sealed)
                .filter(|((p, a), (_, b))| *p == kind && a != b);
            assert!(changed.count() > 0, "PID {kind} was scrambled");
        }
        segments.push(segment_of(
            &sealed,
            (picture, sound),
            (0xdb, 0xcf),
            &mut counters,
        ));
    }
    let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:0\n\
                    #EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"one.key\"\n#EXTINF:5.0,\ns0.ts\n\
                    #EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"two.key\",KEYFORMAT=\"identity\"\n\
                    #EXTINF:5.0,\ns1.ts\n#EXT-X-ENDLIST\n";
    let url = serve(vec![
        ("/v/show.m3u8".into(), playlist.as_bytes().to_vec()),
        ("/v/one.key".into(), KEY_ONE.to_vec()),
        ("/v/two.key".into(), KEY_TWO.to_vec()),
        ("/v/s0.ts".into(), segments[0].clone()),
        ("/v/s1.ts".into(), segments[1].clone()),
    ])
    .await;
    let dir = tempdir();
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a SAMPLE-AES stream with a clear key is fetched");
    assert!(
        done.path.to_string_lossy().ends_with(".mp4"),
        "{:?}",
        done.path
    );

    // What the stream in the clear makes, of the same PES packets.
    let plain = dir.path().join("plain.ts");
    let unsealed = segment_of(&pes, (picture, sound), (0x1b, 0x0f), &mut HashMap::new());
    std::fs::write(&plain, unsealed).unwrap();
    let expected = dir.path().join("expected.mp4");
    dm_core::tsmux::to_mp4(&plain, &expected).unwrap();
    let (got, want) = (read_back(&done.path), read_back(&expected));
    assert_eq!(got.len(), 2);
    assert_eq!(got.len(), want.len());
    for (got, want) in got.iter().zip(&want) {
        assert_eq!(got.handler, want.handler);
        assert_eq!(got.entries, want.entries, "the same set-up");
        assert_eq!(got.samples.len(), want.samples.len());
        assert!(got.samples == want.samples, "every sample in the clear");
        assert_eq!(got.durations, want.durations, "stamped alike");
    }
}

/* ---- SAMPLE-AES in fragmented MP4: cbcs ------------------------------------------------ */

/// One segment: a fragment of `pictures` and `sounds` (each a sample and its length), the
/// pictures with a subsample map (`clear` bytes, the rest protected), the sound whole.
fn cbcs_fragment(
    sequence: u32,
    start: (u64, u64),
    pictures: &[Vec<u8>],
    sounds: &[Vec<u8>],
    clear: u16,
) -> Vec<u8> {
    let traf =
        |id: u32, decode: u64, samples: &[Vec<u8>], duration: u32, offset: u32, mapped: bool| {
            let tfhd = full(b"tfhd", 0, 0x2_0000, &id.to_be_bytes());
            let tfdt = full(b"tfdt", 1, 0, &decode.to_be_bytes());
            let mut trun = (samples.len() as u32).to_be_bytes().to_vec();
            trun.extend_from_slice(&offset.to_be_bytes());
            for (i, s) in samples.iter().enumerate() {
                trun.extend_from_slice(&duration.to_be_bytes());
                trun.extend_from_slice(&(s.len() as u32).to_be_bytes());
                let flags: u32 = if i == 0 || !mapped {
                    0x0200_0000
                } else {
                    0x0101_0000
                };
                trun.extend_from_slice(&flags.to_be_bytes());
            }
            let mut senc = (samples.len() as u32).to_be_bytes().to_vec();
            for s in samples {
                if mapped {
                    senc.extend_from_slice(&1u16.to_be_bytes());
                    senc.extend_from_slice(&clear.to_be_bytes());
                    senc.extend_from_slice(&(s.len() as u32 - u32::from(clear)).to_be_bytes());
                }
            }
            let senc = full(b"senc", 0, if mapped { 2 } else { 0 }, &senc);
            boxed(
                b"traf",
                &[&tfhd, &tfdt, &full(b"trun", 0, 0x701, &trun), &senc],
            )
        };
    let build = |offsets: (u32, u32)| {
        boxed(
            b"moof",
            &[
                &full(b"mfhd", 0, 0, &sequence.to_be_bytes()),
                &traf(1, start.0, pictures, 3000, offsets.0, true),
                &traf(2, start.1, sounds, 1024, offsets.1, false),
            ],
        )
    };
    let moof_len = build((0, 0)).len() as u32;
    let pictures_len: u32 = pictures.iter().map(|s| s.len() as u32).sum();
    let moof = build((moof_len + 8, moof_len + 8 + pictures_len));
    let data: Vec<u8> = pictures.iter().chain(sounds).flatten().copied().collect();
    [moof, boxed(b"mdat", &[&data])].concat()
}

/// A fragmented MP4 whose samples are scrambled by `cbcs` (SAMPLE-AES for fragmented MP4): the
/// picture one block in ten past its subsample's clear bytes, the sound every whole block, a
/// constant vector, the key changing between the two segments. Its samples come out byte for
/// byte as they were before, and its sample entries named for what they hold (gaps row Y16).
#[tokio::test]
async fn a_cbcs_scrambled_fragmented_stream_is_put_in_the_clear() {
    let picture = picture_spec(
        &[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 0x67, 1, 2, 3],
        640,
        360,
        90_000,
    );
    let avc1 = picture.entry.clone();
    let mp4a = fmp4::mp4a(0x40, Some(&[0x11, 0x90]), 48_000, 2);
    let specs = vec![
        Spec {
            entry: sealed_entry(&avc1, b"encv", 0x19),
            ..picture
        },
        Spec {
            entry: sealed_entry(&mp4a, b"enca", 0),
            timescale: 48_000,
            video: false,
            width: 0,
            height: 0,
        },
    ];
    let dir = tempdir();
    let init_path = dir.path().join("init.mp4");
    Writer::create(&init_path, specs).unwrap().finish().unwrap();
    let init = std::fs::read(&init_path).unwrap();

    let picture_of = |n: usize| -> Vec<u8> {
        (0..300 + n * 37 % 200)
            .map(|i| (i * 13 + n) as u8)
            .collect()
    };
    let sound_of =
        |n: usize| -> Vec<u8> { (0..90 + n % 40).map(|i| (i * 5 + n * 3) as u8).collect() };
    let mut files = vec![("/v/init.mp4".to_string(), init)];
    let (mut pictures, mut sounds) = (Vec::new(), Vec::new());
    for (s, key) in [KEY_ONE, KEY_TWO].iter().enumerate() {
        let clear_pictures: Vec<Vec<u8>> = (0..30).map(|i| picture_of(s * 30 + i)).collect();
        let clear_sounds: Vec<Vec<u8>> = (0..47).map(|i| sound_of(s * 47 + i)).collect();
        let sealed_pictures: Vec<Vec<u8>> = clear_pictures
            .iter()
            .map(|p| {
                let mut p = p.clone();
                seal_cbcs(key, &mut p, 37, 1, 9);
                p
            })
            .collect();
        let sealed_sounds: Vec<Vec<u8>> = clear_sounds
            .iter()
            .map(|a| {
                let mut a = a.clone();
                seal_cbcs(key, &mut a, 0, 0, 0);
                a
            })
            .collect();
        assert!(sealed_pictures != clear_pictures && sealed_sounds != clear_sounds);
        let start = (s as u64 * 90_000, s as u64 * 47 * 1024);
        let segment = cbcs_fragment(s as u32 + 1, start, &sealed_pictures, &sealed_sounds, 37);
        files.push((format!("/v/s{s}.m4s"), segment));
        pictures.extend(clear_pictures);
        sounds.extend(clear_sounds);
    }
    let playlist = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:1\n\
                    #EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"one.key\",KEYFORMAT=\"identity\"\n\
                    #EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1.0,\ns0.m4s\n\
                    #EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"two.key\",KEYFORMAT=\"identity\"\n\
                    #EXTINF:1.0,\ns1.m4s\n#EXT-X-ENDLIST\n";
    files.push(("/v/show.m3u8".into(), playlist.as_bytes().to_vec()));
    files.push(("/v/one.key".into(), KEY_ONE.to_vec()));
    files.push(("/v/two.key".into(), KEY_TWO.to_vec()));
    let url = serve(files).await;
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a cbcs stream with a clear key is fetched");
    let tracks = read_back(&done.path);
    assert_eq!(tracks.len(), 2);
    assert!(tracks[0].samples == pictures, "every picture in the clear");
    assert!(
        tracks[1].samples == sounds,
        "every sound frame in the clear"
    );
    assert_eq!(
        &tracks[0].entries[0][4..8],
        b"avc1",
        "named for what it holds"
    );
    assert_eq!(&tracks[1].entries[0][4..8], b"mp4a");
    assert!((length_of(&tracks[0]) - 2.0).abs() < 0.05);
    assert!((length_of(&tracks[1]) - 94.0 * 1024.0 / 48_000.0).abs() < 0.05);
}
