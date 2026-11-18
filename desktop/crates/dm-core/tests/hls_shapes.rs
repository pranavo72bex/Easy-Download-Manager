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
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Response;
use cbc::cipher::{block_padding::NoPadding, BlockModeEncrypt, InnerIvInit, KeyInit};
use dm_core::{client, download};
use dm_media::fmp4::{self, Spec, Writer};
use dm_media::mp4mux::testing::{read_back, ReadBack};

mod common;
use common::tempdir;

type Files = Arc<HashMap<String, Vec<u8>>>;

async fn answer(State(files): State<Files>, uri: Uri) -> Response {
    match files.get(uri.path()) {
        Some(body) => Response::new(Body::from(body.clone())),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

/// Serves `files` by path; the address of `/v/show.m3u8` on it.
async fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    let files: Files = Arc::new(files.into_iter().collect());
    let addr = common::serve(axum::Router::new().fallback(answer).with_state(files)).await;
    format!("http://{addr}/v/show.m3u8")
}

fn boxed(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
    let len: usize = parts.iter().map(|p| p.len()).sum::<usize>() + 8;
    let mut out = (len as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let head = ((u32::from(version) << 24) | flags).to_be_bytes();
    boxed(kind, &[&head, body])
}

/// A fragmented MP4 cut where an HLS packager cuts it: its initialisation segment, and one
/// segment for each fragment.
fn cut(file: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut at = 0;
    let mut init = Vec::new();
    let mut segments: Vec<Vec<u8>> = Vec::new();
    while at + 8 <= file.len() {
        let size = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        let kind = &file[at + 4..at + 8];
        let whole = &file[at..at + size];
        match kind {
            b"moof" => segments.push(whole.to_vec()),
            b"mdat" => segments.last_mut().unwrap().extend_from_slice(whole),
            _ => init.extend_from_slice(whole),
        }
        at += size;
    }
    (init, segments)
}

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

fn picture_spec(avcc: &[u8], width: u32, height: u32, timescale: u32) -> Spec {
    Spec {
        entry: fmp4::avc1(avcc, width, height),
        timescale,
        video: true,
        width,
        height,
    }
}

/// When sample `index` of a track starts, in seconds.
fn starts_at(track: &ReadBack, index: usize) -> f64 {
    let ticks: u64 = track.durations[..index].iter().map(|d| u64::from(*d)).sum();
    (track.start.unwrap_or(0) + ticks) as f64 / f64::from(track.timescale)
}

fn length_of(track: &ReadBack) -> f64 {
    starts_at(track, track.samples.len())
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

const KEY_ONE: [u8; 16] = [0x3c; 16];
const KEY_TWO: [u8; 16] = [
    0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f,
];

/// `blocks` (whole 16-byte blocks) scrambled in one chain from `iv`.
fn seal_blocks(key: &[u8; 16], iv: &[u8; 16], blocks: &mut [u8]) {
    let len = blocks.len();
    let cipher = aes::Aes128::new_from_slice(key).unwrap();
    cbc::Encryptor::<aes::Aes128>::inner_iv_slice_init(cipher, iv)
        .unwrap()
        .encrypt_padded::<NoPadding>(blocks, len)
        .unwrap();
}

/// Scrambles the blocks of `data` at `starts` (each 16 bytes) as one chain from `iv`.
fn seal_at(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8], starts: &[usize]) {
    let mut gathered: Vec<u8> = starts
        .iter()
        .flat_map(|&at| data[at..at + 16].to_vec())
        .collect();
    if gathered.is_empty() {
        return;
    }
    seal_blocks(key, iv, &mut gathered);
    for (n, &at) in starts.iter().enumerate() {
        data[at..at + 16].copy_from_slice(&gathered[n * 16..n * 16 + 16]);
    }
}

/// Emulation prevention put on: a `03` after two zeros before any byte up to 3.
fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 8);
    let mut zeros = 0;
    for &byte in data {
        if zeros >= 2 && byte <= 3 {
            out.push(3);
            zeros = 0;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        out.push(byte);
    }
    out
}

/// One NAL unit (its emulation prevention in place) scrambled as Apple's sample encryption has
/// it: a coded slice over 48 bytes, 32 in the clear, then one block in ten, emulation prevention
/// put on again over the whole.
fn seal_nal(unit: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    let kind = unit[0] & 0x1f;
    if !(kind == 1 || kind == 5) || unit.len() <= 48 {
        return unit.to_vec();
    }
    let mut data = unit.to_vec();
    let mut starts = Vec::new();
    let mut at = 32;
    while at < data.len() {
        if data.len() - at > 16 {
            starts.push(at);
            at += 16;
        }
        at += (data.len() - at).min(144);
    }
    seal_at(key, iv, &mut data, &starts);
    escape(&data)
}

/// The NAL units of an Annex B stream, each with the start code before it.
fn units(data: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| {
            let mut end = starts.get(n + 1).map_or(data.len(), |next| next - 3);
            while end > start && data[end - 1] == 0 {
                end -= 1;
            }
            (start, end)
        })
        .collect()
}

/// A PES's payload scrambled: each picture's slices, or each ADTS frame after its header and
/// 16 bytes, every whole block (each a chain of its own from `iv`).
fn seal_payload(payload: &[u8], picture: bool, key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    if picture {
        let mut out = Vec::new();
        for (start, end) in units(payload) {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend(seal_nal(&payload[start..end], key, iv));
        }
        return out;
    }
    let mut out = payload.to_vec();
    let mut at = 0;
    while at + 7 <= out.len() && out[at] == 0xff {
        let header = if out[at + 1] & 1 != 0 { 7 } else { 9 };
        let length = (usize::from(out[at + 3] & 3) << 11)
            | (usize::from(out[at + 4]) << 3)
            | (usize::from(out[at + 5]) >> 5);
        let body = at + header + 16;
        let whole = (at + length).saturating_sub(body) / 16;
        let starts: Vec<usize> = (0..whole).map(|b| body + b * 16).collect();
        seal_at(key, iv, &mut out[..at + length], &starts);
        at += length;
    }
    out
}

/// A transport stream's PES packets, in the order they start: each one's PID and bytes; and
/// the picture's and the sound's PIDs.
fn pes_of(ts: &[u8]) -> (Vec<(u16, Vec<u8>)>, u16, u16) {
    let (mut pmt, mut picture, mut sound) = (0u16, 0u16, 0u16);
    let mut gathering: HashMap<u16, usize> = HashMap::new();
    let mut out: Vec<(u16, Vec<u8>)> = Vec::new();
    for packet in ts.as_chunks::<188>().0 {
        let start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let control = (packet[3] >> 4) & 3;
        let mut at = 4;
        if control & 2 != 0 {
            at += 1 + usize::from(packet[4]);
        }
        if control & 1 == 0 || at >= 188 {
            continue;
        }
        let payload = &packet[at..];
        if pid == 0 && start {
            let section = &payload[1 + usize::from(payload[0])..];
            pmt = (u16::from(section[10] & 0x1f) << 8) | u16::from(section[11]);
        } else if pid == pmt && start && pmt != 0 {
            let section = &payload[1 + usize::from(payload[0])..];
            let length = usize::from(u16::from_be_bytes([section[1], section[2]]) & 0x0fff);
            let info = usize::from(u16::from_be_bytes([section[10], section[11]]) & 0x0fff);
            let mut e = 12 + info;
            while e + 5 <= 3 + length - 4 {
                let es = (u16::from(section[e + 1] & 0x1f) << 8) | u16::from(section[e + 2]);
                match section[e] {
                    0x1b if picture == 0 => picture = es,
                    0x0f if sound == 0 => sound = es,
                    _ => {}
                }
                e += 5 + usize::from(u16::from_be_bytes([section[e + 3], section[e + 4]]) & 0x0fff);
            }
        } else if pid != 0 && (pid == picture || pid == sound) {
            if start {
                gathering.insert(pid, out.len());
                out.push((pid, payload.to_vec()));
            } else if let Some(&n) = gathering.get(&pid) {
                out[n].1.extend_from_slice(payload);
            }
        }
    }
    (out, picture, sound)
}

/// A PSI section in one packet of `pid`.
fn psi(pid: u16, section: &[u8]) -> Vec<u8> {
    let mut packet = vec![0x47, 0x40 | (pid >> 8) as u8, pid as u8, 0x10, 0];
    packet.extend_from_slice(section);
    packet.resize(188, 0xff);
    packet
}

/// A section: its table, its body after the five bytes every one has, and a CRC dm does not
/// read (left nought).
fn section(table: u8, id: u16, body: &[u8]) -> Vec<u8> {
    let length = 5 + body.len() + 4;
    let mut out = vec![table, 0xb0 | (length >> 8) as u8, length as u8];
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0xc1, 0, 0]);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0; 4]);
    out
}

/// A segment of a transport stream: its tables naming the picture and the sound as `kinds` (0xdb
/// and 0xcf for SAMPLE-AES), then each PES in packets.
fn segment_of(
    pes: &[(u16, Vec<u8>)],
    (picture, sound): (u16, u16),
    kinds: (u8, u8),
    counters: &mut HashMap<u16, u8>,
) -> Vec<u8> {
    const PMT: u16 = 0x1000;
    let mut out = psi(
        0,
        &section(0, 1, &[0, 1, 0xe0 | (PMT >> 8) as u8, PMT as u8]),
    );
    let mut pmt = vec![0xe0 | (picture >> 8) as u8, picture as u8, 0xf0, 0];
    for (kind, pid) in [(kinds.0, picture), (kinds.1, sound)] {
        pmt.extend_from_slice(&[kind, 0xe0 | (pid >> 8) as u8, pid as u8, 0xf0, 0]);
    }
    out.extend(psi(PMT, &section(2, 1, &pmt)));
    for (pid, bytes) in pes {
        let mut rest: &[u8] = bytes;
        let mut first = true;
        while !rest.is_empty() {
            let counter = counters.entry(*pid).or_insert(0);
            let take = rest.len().min(184);
            let mut packet = vec![
                0x47,
                (if first { 0x40 } else { 0 }) | (pid >> 8) as u8,
                *pid as u8,
            ];
            if take == 184 {
                packet.push(0x10 | *counter);
            } else {
                // Stuffed in the adaptation field to a whole packet.
                packet.push(0x30 | *counter);
                let field = 184 - take - 1;
                packet.push(field as u8);
                if field > 0 {
                    packet.push(0);
                    packet.extend(std::iter::repeat_n(0xff, field - 1));
                }
            }
            *counter = (*counter + 1) & 0x0f;
            packet.extend_from_slice(&rest[..take]);
            assert_eq!(packet.len(), 188);
            out.extend(packet);
            rest = &rest[take..];
            first = false;
        }
    }
    out
}

/// The vector of segment `n` of a playlist that names none: its sequence number.
fn iv_of(n: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&n.to_be_bytes());
    iv
}

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

const CONSTANT_IV: [u8; 16] = [0x5a; 16];

/// A sample entry made `encv` or `enca`: scrambled by `cbcs` with `pattern` (crypt and skip, in
/// four bits each) and a constant vector.
fn sealed_entry(entry: &[u8], kind: &[u8; 4], pattern: u8) -> Vec<u8> {
    let original: [u8; 4] = entry[4..8].try_into().unwrap();
    let mut tenc = vec![0, pattern, 1, 0];
    tenc.extend_from_slice(&[0x77; 16]);
    tenc.push(16);
    tenc.extend_from_slice(&CONSTANT_IV);
    let sinf = boxed(
        b"sinf",
        &[
            &boxed(b"frma", &[&original]),
            &full(
                b"schm",
                0,
                0,
                &[b"cbcs".as_slice(), &0x0001_0000u32.to_be_bytes()].concat(),
            ),
            &boxed(b"schi", &[&full(b"tenc", 1, 0, &tenc)]),
        ],
    );
    boxed(kind, &[&entry[8..], &sinf])
}

/// A sample's protected bytes scrambled as `cbcs` has them, from `from`: `crypt` blocks then
/// `skip` in the clear, over and over (every block where both are nought), one chain from the
/// constant vector.
fn seal_cbcs(key: &[u8; 16], sample: &mut [u8], from: usize, crypt: usize, skip: usize) {
    let mut starts = Vec::new();
    let mut at = from;
    while at + 16 <= sample.len() {
        if crypt == 0 {
            starts.push(at);
            at += 16;
            continue;
        }
        for _ in 0..crypt {
            if at + 16 <= sample.len() {
                starts.push(at);
            }
            at += 16;
        }
        at += skip * 16;
    }
    seal_at(key, &CONSTANT_IV, sample, &starts);
}

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
