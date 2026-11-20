// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! HLS pairs that were refused (gaps row Y16): a picture and its sound kept in playlists of
//! their own (a master's audio rendition), each scrambled by SAMPLE-AES with a clear key of its
//! own, the picture's key changing part-way; transport streams on both sides scrambled so; and
//! a picture whose initialisation segment changes part-way (an advert break and back); and a
//! live recording scrambled so. Each is fetched from a stream made here and served on loopback,
//! and its MP4 read back whole, every sample in the clear, byte for byte, as the single
//! playlist's are (`hls_shapes.rs`).

use std::collections::HashMap;

use dm_core::{client, download};
use dm_media::fmp4::{self, Spec, Writer};
use dm_media::mp4mux::testing::{read_back, ReadBack};

mod common;
mod sealing;
use common::tempdir;
use sealing::*;

/// The sound's key, unlike either of the picture's.
const KEY_THREE: [u8; 16] = [
    0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0xa5, 0x96, 0x87, 0x78, 0x69, 0x5a, 0x4b, 0x3c, 0x2d, 0x1e, 0x0f,
];

/// The master, at `/v/show.m3u8`: one rendition, its sound the default of its audio group, a
/// playlist of its own.
const MASTER: &str = "#EXTM3U\n#EXT-X-VERSION:7\n\
    #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"main\",DEFAULT=YES,URI=\"sound.m3u8\"\n\
    #EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO=\"aud\"\npicture.m3u8\n";

/// The track of `handler` (`vide`, `soun`) among those read back.
fn track<'a>(tracks: &'a [ReadBack], handler: &[u8; 4]) -> &'a ReadBack {
    tracks
        .iter()
        .find(|t| &t.handler == handler)
        .expect("the track is in the file")
}

/// A media playlist of `segments`, each a second long, with its initialisation segment
/// `map` where it has one; `keys[n]`, where there is one, is the SAMPLE-AES key named before
/// segment `n`.
fn playlist(map: Option<&str>, segments: &[String], keys: &[Option<String>]) -> String {
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:1\n");
    for (n, segment) in segments.iter().enumerate() {
        if let Some(Some(key)) = keys.get(n) {
            text.push_str(&format!(
                "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"{key}\",KEYFORMAT=\"identity\"\n"
            ));
        }
        if let (0, Some(map)) = (n, map) {
            text.push_str(&format!("#EXT-X-MAP:URI=\"{map}\"\n"));
        }
        text.push_str(&format!("#EXTINF:1.0,\n{segment}\n"));
    }
    text.push_str("#EXT-X-ENDLIST\n");
    text
}

/// The names of two segments of one `side` of a pair: `p0.m4s` and `p1.m4s`, say.
fn names(side: &str, extension: &str) -> Vec<String> {
    (0..2).map(|n| format!("{side}{n}.{extension}")).collect()
}

/// The initialisation segment of one track of `spec`, alone.
fn init_of(spec: Spec) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("init.mp4");
    Writer::create(&path, vec![spec]).unwrap().finish().unwrap();
    std::fs::read(&path).unwrap()
}

/// One segment of a track of its own (track 1): a fragment of `samples` from `start`, each
/// `duration` long, the samples scrambled by `cbcs`; with a subsample map where `clear` names
/// the bytes of each in the clear (a picture's), none where all of it is protected (sound).
fn fragment(
    sequence: u32,
    start: u64,
    samples: &[Vec<u8>],
    duration: u32,
    clear: Option<u16>,
) -> Vec<u8> {
    let build = |offset: u32| {
        let tfhd = full(b"tfhd", 0, 0x2_0000, &1u32.to_be_bytes());
        let tfdt = full(b"tfdt", 1, 0, &start.to_be_bytes());
        let mut trun = (samples.len() as u32).to_be_bytes().to_vec();
        trun.extend_from_slice(&offset.to_be_bytes());
        let mut senc = (samples.len() as u32).to_be_bytes().to_vec();
        for (i, s) in samples.iter().enumerate() {
            trun.extend_from_slice(&duration.to_be_bytes());
            trun.extend_from_slice(&(s.len() as u32).to_be_bytes());
            let flags: u32 = if i == 0 || clear.is_none() {
                0x0200_0000
            } else {
                0x0101_0000
            };
            trun.extend_from_slice(&flags.to_be_bytes());
            if let Some(clear) = clear {
                senc.extend_from_slice(&1u16.to_be_bytes());
                senc.extend_from_slice(&clear.to_be_bytes());
                senc.extend_from_slice(&(s.len() as u32 - u32::from(clear)).to_be_bytes());
            }
        }
        let senc = full(b"senc", 0, if clear.is_some() { 2 } else { 0 }, &senc);
        let trun = full(b"trun", 0, 0x701, &trun);
        let traf = boxed(b"traf", &[&tfhd, &tfdt, &trun, &senc]);
        boxed(
            b"moof",
            &[&full(b"mfhd", 0, 0, &sequence.to_be_bytes()), &traf],
        )
    };
    let moof = build(build(0).len() as u32 + 8);
    let data: Vec<u8> = samples.iter().flatten().copied().collect();
    [moof, boxed(b"mdat", &[&data])].concat()
}

/// A pair of fragmented MP4, two segments a side, scrambled by `cbcs`: the picture one block
/// in ten past 37 bytes in the clear, by `picture_keys` (one for the whole, or one a segment),
/// the sound every whole block by `KEY_THREE`. Fetched, its MP4 holds every picture and every
/// sound frame in the clear, each named for what it holds.
async fn a_cbcs_pair_is_put_in_the_clear(picture_keys: &[[u8; 16]]) {
    let picture = picture_spec(
        &[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 0x67, 1, 2, 3],
        640,
        360,
        90_000,
    );
    let avc1 = picture.entry.clone();
    let mp4a = fmp4::mp4a(0x40, Some(&[0x11, 0x90]), 48_000, 2);
    let mut files = vec![
        (
            "/v/picture-init.mp4".to_string(),
            init_of(Spec {
                entry: sealed_entry(&avc1, b"encv", 0x19),
                ..picture
            }),
        ),
        (
            "/v/sound-init.mp4".to_string(),
            init_of(Spec {
                entry: sealed_entry(&mp4a, b"enca", 0),
                timescale: 48_000,
                video: false,
                width: 0,
                height: 0,
            }),
        ),
    ];
    let (mut pictures, mut sounds) = (Vec::new(), Vec::new());
    for s in 0..2usize {
        let key = picture_keys.get(s).unwrap_or(&picture_keys[0]);
        let clear: Vec<Vec<u8>> = (0..30)
            .map(|i| {
                let n = s * 30 + i;
                (0..300 + n * 37 % 200)
                    .map(|b| (b * 13 + n) as u8)
                    .collect()
            })
            .collect();
        let sealed: Vec<Vec<u8>> = clear
            .iter()
            .map(|p| {
                let mut p = p.clone();
                seal_cbcs(key, &mut p, 37, 1, 9);
                p
            })
            .collect();
        let heard: Vec<Vec<u8>> = (0..47)
            .map(|i| {
                let n = s * 47 + i;
                (0..90 + n % 40).map(|b| (b * 5 + n * 3) as u8).collect()
            })
            .collect();
        let muffled: Vec<Vec<u8>> = heard
            .iter()
            .map(|a| {
                let mut a = a.clone();
                seal_cbcs(&KEY_THREE, &mut a, 0, 0, 0);
                a
            })
            .collect();
        assert!(sealed != clear && muffled != heard, "both are scrambled");
        let at = s as u64;
        files.push((
            format!("/v/p{s}.m4s"),
            fragment(s as u32 + 1, at * 90_000, &sealed, 3000, Some(37)),
        ));
        files.push((
            format!("/v/a{s}.m4s"),
            fragment(s as u32 + 1, at * 47 * 1024, &muffled, 1024, None),
        ));
        pictures.extend(clear);
        sounds.extend(heard);
    }
    let picture_named: Vec<Option<String>> = (0..picture_keys.len())
        .map(|n| Some(format!("p{n}.key")))
        .collect();
    for (n, key) in picture_keys.iter().enumerate() {
        files.push((format!("/v/p{n}.key"), key.to_vec()));
    }
    files.push(("/v/s.key".into(), KEY_THREE.to_vec()));
    let seen = playlist(Some("picture-init.mp4"), &names("p", "m4s"), &picture_named);
    let heard = playlist(
        Some("sound-init.mp4"),
        &names("a", "m4s"),
        &[Some("s.key".into())],
    );
    files.push(("/v/picture.m3u8".into(), seen.into_bytes()));
    files.push(("/v/sound.m3u8".into(), heard.into_bytes()));
    files.push(("/v/show.m3u8".into(), MASTER.as_bytes().to_vec()));
    let url = serve(files).await;
    let dir = tempdir();
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a SAMPLE-AES pair with clear keys is fetched");
    assert!(done.path.to_string_lossy().ends_with(".mp4"));
    let tracks = read_back(&done.path);
    assert_eq!(tracks.len(), 2, "a picture and a sound");
    let (seen, heard) = (track(&tracks, b"vide"), track(&tracks, b"soun"));
    assert!(seen.samples == pictures, "every picture in the clear");
    assert!(heard.samples == sounds, "every sound frame in the clear");
    assert_eq!(&seen.entries[0][4..8], b"avc1", "named for what it holds");
    assert_eq!(&heard.entries[0][4..8], b"mp4a");
    assert!((length_of(seen) - 2.0).abs() < 0.05, "{}", length_of(seen));
    assert!((length_of(heard) - 94.0 * 1024.0 / 48_000.0).abs() < 0.05);
}

/// A picture and a sound in playlists of their own, each scrambled by `cbcs` with a key of its
/// own (gaps row Y16): refused before.
#[tokio::test]
async fn a_cbcs_picture_and_sound_each_with_its_own_key_are_put_in_the_clear() {
    a_cbcs_pair_is_put_in_the_clear(&[KEY_ONE]).await;
}

/// The same, the picture's key changing between its two segments (gaps row Y16).
#[tokio::test]
async fn a_cbcs_pair_whose_picture_key_changes_part_way_is_put_in_the_clear() {
    a_cbcs_pair_is_put_in_the_clear(&[KEY_ONE, KEY_TWO]).await;
}

/// A transport stream's PES packets scrambled by SAMPLE-AES, segment `n` of them by `keys[n]`
/// (or the last there is), each segment a transport stream of its own (0xdb and 0xcf).
fn sealed_segments(
    halves: [&[(u16, Vec<u8>)]; 2],
    (picture, sound): (u16, u16),
    keys: &[[u8; 16]],
) -> Vec<Vec<u8>> {
    let mut counters = HashMap::new();
    let mut segments = Vec::new();
    for (n, part) in halves.into_iter().enumerate() {
        let key = keys.get(n).or(keys.last()).unwrap();
        let sealed: Vec<(u16, Vec<u8>)> = part
            .iter()
            .map(|(pid, bytes)| {
                let header = 9 + usize::from(bytes[8]);
                let mut out = bytes[..header].to_vec();
                out.extend(seal_payload(
                    &bytes[header..],
                    *pid == picture,
                    key,
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
        segments.push(segment_of(
            &sealed,
            (picture, sound),
            (0xdb, 0xcf),
            &mut counters,
        ));
    }
    segments
}

/// A picture and its sound as transport streams in playlists of their own, each scrambled by
/// SAMPLE-AES with its own keys, the picture's changing between its two segments: the MP4 holds
/// the picture of the one and the sound of the other, every sample as it was before it was
/// scrambled, as the stream in the clear makes them (gaps row Y16): refused before.
#[tokio::test]
async fn a_sample_aes_transport_stream_pair_is_cleared_into_one_mp4() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/media/mp4mux/stream.ts"
    );
    let clear = std::fs::read(fixture).unwrap();
    let (pes, picture, sound) = pes_of(&clear);
    assert!(picture != 0 && sound != 0 && pes.len() > 4);
    let half = pes.len() / 2;
    let halves = [&pes[..half], &pes[half..]];
    let seen = sealed_segments(halves, (picture, sound), &[KEY_ONE, KEY_TWO]);
    let heard = sealed_segments(halves, (picture, sound), &[KEY_THREE]);
    let mut files = vec![
        ("/v/show.m3u8".to_string(), MASTER.as_bytes().to_vec()),
        ("/v/p0.key".into(), KEY_ONE.to_vec()),
        ("/v/p1.key".into(), KEY_TWO.to_vec()),
        ("/v/s.key".into(), KEY_THREE.to_vec()),
    ];
    let picture_keys = [Some("p0.key".to_string()), Some("p1.key".to_string())];
    let seen_list = playlist(None, &names("p", "ts"), &picture_keys);
    let heard_list = playlist(None, &names("a", "ts"), &[Some("s.key".into())]);
    files.push(("/v/picture.m3u8".into(), seen_list.into_bytes()));
    files.push(("/v/sound.m3u8".into(), heard_list.into_bytes()));
    for (n, (seen, heard)) in seen.iter().zip(&heard).enumerate() {
        files.push((format!("/v/p{n}.ts"), seen.clone()));
        files.push((format!("/v/a{n}.ts"), heard.clone()));
    }
    let url = serve(files).await;
    let dir = tempdir();
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a SAMPLE-AES transport-stream pair with clear keys is fetched");
    assert!(done.path.to_string_lossy().ends_with(".mp4"));

    // What the stream in the clear makes, of the same PES packets.
    let plain = dir.path().join("plain.ts");
    let unsealed = segment_of(&pes, (picture, sound), (0x1b, 0x0f), &mut HashMap::new());
    std::fs::write(&plain, unsealed).unwrap();
    let expected = dir.path().join("expected.mp4");
    dm_core::tsmux::to_mp4(&plain, &expected).unwrap();
    let (got, want) = (read_back(&done.path), read_back(&expected));
    assert_eq!(got.len(), 2, "a picture and a sound");
    for handler in [b"vide", b"soun"] {
        let (got, want) = (track(&got, handler), track(&want, handler));
        assert_eq!(got.entries, want.entries, "the same set-up");
        assert_eq!(got.samples.len(), want.samples.len());
        assert!(got.samples == want.samples, "every sample in the clear");
    }
}

/// `seconds` of 30-a-second pictures (a key frame a second) of `spec` on a track of their own,
/// each sample's bytes marked with `mark`, cut as a packager cuts them: the initialisation
/// segment, a segment a second, and the samples.
fn picture_run(mark: u8, seconds: u32, spec: Spec) -> (Vec<u8>, Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.mp4");
    let scale = i64::from(spec.timescale);
    let mut writer = Writer::create(&path, vec![spec]).unwrap();
    let mut samples = Vec::new();
    for f in 0..seconds * 30 {
        let data: Vec<u8> = (0..200 + f as usize % 50)
            .map(|i| mark ^ (i as u8).wrapping_mul(7) ^ f as u8)
            .collect();
        let dts = i64::from(f) * scale / 30;
        writer
            .push(0, data.clone(), dts, 0, f % 30 == 0, None)
            .unwrap();
        samples.push(data);
    }
    writer.finish().unwrap();
    let (init, segments) = cut(&std::fs::read(&path).unwrap());
    (init, segments, samples)
}

/// `seconds` of AAC frames of 1024 at 48 kHz on a track of their own, cut as a packager cuts
/// them: the initialisation segment, a segment about a second, and the samples.
fn sound_run(seconds: u64) -> (Vec<u8>, Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sound.mp4");
    let spec = Spec {
        entry: fmp4::mp4a(0x40, Some(&[0x11, 0x90]), 48_000, 2),
        timescale: 48_000,
        video: false,
        width: 0,
        height: 0,
    };
    let mut writer = Writer::create(&path, vec![spec]).unwrap();
    let mut samples = Vec::new();
    for a in 0..seconds * 48_000 / 1024 {
        let data: Vec<u8> = (0..60 + a as usize % 9)
            .map(|i| (i as u8) ^ a as u8)
            .collect();
        writer
            .push(0, data.clone(), (a * 1024) as i64, 0, true, Some(1024))
            .unwrap();
        samples.push(data);
    }
    writer.finish().unwrap();
    let (init, segments) = cut(&std::fs::read(&path).unwrap());
    (init, segments, samples)
}

/// A picture whose initialisation segment changes to an advert's (another size, another
/// timescale, its clock started afresh) and back, beside a sound of one: the MP4 holds every
/// picture once, in order, each decoded with its own run's set-up, and the sound whole, both
/// as long as each other (gaps row Y16): refused before.
#[tokio::test]
async fn a_pair_whose_picture_changes_its_initialisation_segment_is_one_mp4() {
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
        picture_run(1, 2, main()),
        picture_run(2, 1, advert),
        picture_run(3, 1, main()),
    ];
    let mut files = vec![
        ("/v/main-init.mp4".to_string(), runs[0].0.clone()),
        ("/v/ad-init.mp4".to_string(), runs[1].0.clone()),
    ];
    let mut seen = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:1\n");
    for (r, ((_, segments, _), map)) in runs.iter().zip(["main", "ad", "main"]).enumerate() {
        if r > 0 {
            seen.push_str("#EXT-X-DISCONTINUITY\n");
        }
        seen.push_str(&format!("#EXT-X-MAP:URI=\"{map}-init.mp4\"\n"));
        for (s, segment) in segments.iter().enumerate() {
            let name = format!("r{r}s{s}.m4s");
            seen.push_str(&format!("#EXTINF:1.0,\n{name}\n"));
            files.push((format!("/v/{name}"), segment.clone()));
        }
    }
    seen.push_str("#EXT-X-ENDLIST\n");

    let dir = tempdir();
    let (sound_init, sound_segments, sounds) = sound_run(4);
    files.push(("/v/sound-init.mp4".into(), sound_init));
    let mut names = Vec::new();
    for (s, segment) in sound_segments.into_iter().enumerate() {
        names.push(format!("a{s}.m4s"));
        files.push((format!("/v/a{s}.m4s"), segment));
    }
    let heard = playlist(Some("sound-init.mp4"), &names, &[]);
    files.push(("/v/picture.m3u8".into(), seen.into_bytes()));
    files.push(("/v/sound.m3u8".into(), heard.into_bytes()));
    files.push(("/v/show.m3u8".into(), MASTER.as_bytes().to_vec()));
    let url = serve(files).await;
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a pair with an advert break in its picture is fetched");
    let tracks = read_back(&done.path);
    assert_eq!(tracks.len(), 2, "a picture and a sound");
    let (picture, sound) = (track(&tracks, b"vide"), track(&tracks, b"soun"));
    let pictures: Vec<Vec<u8>> = runs.iter().flat_map(|r| r.2.iter().cloned()).collect();
    assert!(
        picture.samples == pictures,
        "the pictures, whole and in order"
    );
    assert_eq!(
        picture.entries.len(),
        2,
        "the main picture's set-up and the advert's"
    );
    let counts: Vec<usize> = runs.iter().map(|r| r.2.len()).collect();
    let expected: Vec<u32> = [1, 2, 1]
        .iter()
        .zip(&counts)
        .flat_map(|(d, n)| std::iter::repeat_n(*d, *n))
        .collect();
    assert_eq!(picture.described, expected);
    assert!(sound.samples == sounds, "the sound, whole and in order");
    assert!(
        (length_of(picture) - 4.0).abs() < 0.1,
        "{}",
        length_of(picture)
    );
    assert!((length_of(sound) - length_of(picture)).abs() < 0.1);
}

/// A picture scrambled whole by AES-128, its key changing between its two segments, beside a
/// sound in the clear, in playlists of their own: each segment is decrypted with its own key
/// (gaps row Y16), where a pair took its first key for every segment before.
#[tokio::test]
async fn a_pair_whose_aes_key_changes_part_way_decrypts_each_segment_with_its_own() {
    use cbc::cipher::{block_padding::Pkcs7, BlockModeEncrypt, InnerIvInit, KeyInit};

    let spec = picture_spec(
        &[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 0x67, 1, 2, 3],
        640,
        360,
        90_000,
    );
    let (init, segments, pictures) = picture_run(4, 2, spec);
    assert_eq!(segments.len(), 2, "a segment a second");
    let seal = |data: &[u8], key: &[u8; 16], sequence: u64| {
        cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(
            aes::Aes128::new_from_slice(key).unwrap(),
            &iv_of(sequence),
        )
        .unwrap()
        .encrypt_padded_vec::<Pkcs7>(data)
    };
    let mut files = vec![
        ("/v/picture-init.mp4".to_string(), init),
        ("/v/p0.key".into(), KEY_ONE.to_vec()),
        ("/v/p1.key".into(), KEY_TWO.to_vec()),
    ];
    // The initialisation segment before any key: it is in the clear.
    let mut seen = String::from(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:1\n#EXT-X-MAP:URI=\"picture-init.mp4\"\n",
    );
    for (n, (segment, key)) in segments.iter().zip([KEY_ONE, KEY_TWO]).enumerate() {
        seen.push_str(&format!(
            "#EXT-X-KEY:METHOD=AES-128,URI=\"p{n}.key\"\n#EXTINF:1.0,\np{n}.m4s\n"
        ));
        files.push((
            format!("/v/p{n}.m4s"),
            seal(segment.as_slice(), &key, n as u64),
        ));
    }
    seen.push_str("#EXT-X-ENDLIST\n");
    let (sound_init, sound_segments, sounds) = sound_run(2);
    files.push(("/v/sound-init.mp4".into(), sound_init));
    let mut names = Vec::new();
    for (s, segment) in sound_segments.into_iter().enumerate() {
        names.push(format!("a{s}.m4s"));
        files.push((format!("/v/a{s}.m4s"), segment));
    }
    let heard = playlist(Some("sound-init.mp4"), &names, &[]);
    files.push(("/v/picture.m3u8".into(), seen.into_bytes()));
    files.push(("/v/sound.m3u8".into(), heard.into_bytes()));
    files.push(("/v/show.m3u8".into(), MASTER.as_bytes().to_vec()));
    let url = serve(files).await;
    let dir = tempdir();
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a pair whose key changes part-way is fetched");
    let tracks = read_back(&done.path);
    assert_eq!(tracks.len(), 2, "a picture and a sound");
    let (picture, sound) = (track(&tracks, b"vide"), track(&tracks, b"soun"));
    assert!(picture.samples == pictures, "every picture decrypted");
    assert!(sound.samples == sounds, "the sound, whole");
}

/// A live transport stream scrambled by SAMPLE-AES, its key changing between its two segments,
/// its playlist listing the first alone for its first asks and then both, ended: the recording
/// is put into an MP4 whose samples are those of the stream in the clear (gaps row Y16):
/// refused before, as a recording was never joined whole to clear it.
#[tokio::test]
async fn a_live_sample_aes_transport_stream_is_recorded_in_the_clear() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::routing::get;

    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/media/mp4mux/stream.ts"
    );
    let clear = std::fs::read(fixture).unwrap();
    let (pes, picture, sound) = pes_of(&clear);
    let half = pes.len() / 2;
    let halves = [&pes[..half], &pes[half..]];
    let segments = sealed_segments(halves, (picture, sound), &[KEY_ONE, KEY_TWO]);
    // Live for its first five asks (the address probed, the playlist read, a round recorded),
    // then ended.
    let asks = Arc::new(AtomicUsize::new(0));
    let listed = get(move || async move {
        let out = if asks.fetch_add(1, Ordering::SeqCst) < 5 {
            1
        } else {
            2
        };
        let mut text = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n");
        for n in 0..out {
            text.push_str(&format!(
                "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k{n}.key\"\n#EXTINF:1.0,\ns{n}.ts\n"
            ));
        }
        if out == 2 {
            text.push_str("#EXT-X-ENDLIST\n");
        }
        text
    });
    let bytes = |body: Vec<u8>| get(move || async move { body });
    let app = axum::Router::new()
        .route("/v/show.m3u8", listed)
        .route("/v/k0.key", bytes(KEY_ONE.to_vec()))
        .route("/v/k1.key", bytes(KEY_TWO.to_vec()))
        .route("/v/s0.ts", bytes(segments[0].clone()))
        .route("/v/s1.ts", bytes(segments[1].clone()));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let url = format!("http://{addr}/v/show.m3u8");
    let done = download(&client(), &url, &common::options(dir.path()))
        .await
        .expect("a live SAMPLE-AES stream with clear keys is recorded");
    assert!(
        done.path.to_string_lossy().ends_with(".mp4"),
        "{:?}",
        done.path
    );

    let plain = dir.path().join("plain.ts");
    let unsealed = segment_of(&pes, (picture, sound), (0x1b, 0x0f), &mut HashMap::new());
    std::fs::write(&plain, unsealed).unwrap();
    let expected = dir.path().join("expected.mp4");
    dm_core::tsmux::to_mp4(&plain, &expected).unwrap();
    let (got, want) = (read_back(&done.path), read_back(&expected));
    assert_eq!(got.len(), 2, "a picture and a sound");
    for handler in [b"vide", b"soun"] {
        let (got, want) = (track(&got, handler), track(&want, handler));
        assert_eq!(got.entries, want.entries, "the same set-up");
        assert!(got.samples == want.samples, "every sample in the clear");
    }
}
