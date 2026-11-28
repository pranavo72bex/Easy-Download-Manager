// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::testing::*;
use super::*;

fn read_back(path: &Path) -> Vec<Track> {
    let mut file = File::open(path).unwrap();
    let length = file.metadata().unwrap().len();
    read_input(&mut file, length, 0).unwrap()
}

fn bytes_of(path: &Path, track: &Track) -> Vec<Vec<u8>> {
    let data = std::fs::read(path).unwrap();
    track
        .samples
        .iter()
        .map(|s| data[s.offset as usize..(s.offset + u64::from(s.size)) as usize].to_vec())
        .collect()
}

#[test]
fn a_fragmented_picture_and_a_plain_sound_join_sample_for_sample() {
    let dir = tempfile::tempdir().unwrap();
    let (video, pictures) = fragmented(dir.path(), "v.mp4", 4);
    let (audio, sounds) = plain(dir.path(), "a.m4a", 7, 200);
    let out = dir.path().join("joined.mp4");
    let length = join(&[video, audio], &out).unwrap();
    assert_eq!(length, std::fs::metadata(&out).unwrap().len());

    let data = std::fs::read(&out).unwrap();
    let kinds: Vec<[u8; 4]> = boxes(&data).map(|(k, _, _)| k).collect();
    assert_eq!(
        kinds,
        [*b"ftyp", *b"moov", *b"mdat"],
        "the index comes first"
    );

    let tracks = read_back(&out);
    assert_eq!(tracks.len(), 2);
    assert_eq!((tracks[0].id, tracks[1].id), (1, 2));
    assert_eq!(tracks[0].timescale, 90000);
    assert_eq!(tracks[1].timescale, 48000);
    assert_eq!(bytes_of(&out, &tracks[0]), pictures);
    assert_eq!(bytes_of(&out, &tracks[1]), sounds);
    // Timing and the sync samples survive: every second picture of a
    // fragment of three is not one.
    assert!(tracks[0].samples.iter().all(|s| s.duration == 3000));
    let syncs: Vec<bool> = tracks[0].samples.iter().map(|s| s.sync).collect();
    assert_eq!(syncs, [true, false, true].repeat(4));
    assert!(tracks[1]
        .samples
        .iter()
        .all(|s| s.duration == 1024 && s.sync));
}

/// A joined file carries its title, artist, date, page and picture in `udta/meta/ilst`, and
/// its chapters in `udta/chpl`, as players read them; with none to say, no `udta` (gaps row
/// Y11).
#[test]
fn a_joined_file_carries_its_title_and_artist() {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 2);
    let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
    let tags = Tags {
        title: "Me at the zoo".into(),
        artist: "jawed".into(),
        date: String::new(),
        source: "https://www.youtube.com/watch?v=jNQXAC9IVRw".into(),
        cover: b"\xff\xd8\xff\xe0 a picture".to_vec(),
        chapters: vec![(0, "Intro".into()), (61_500, "Élephants".into())],
    };
    let out = dir.path().join("tagged.mp4");
    join_cut_tagged(
        &[video.clone(), audio.clone()],
        &[],
        &out,
        None,
        None,
        &tags,
    )
    .unwrap();
    let data = std::fs::read(&out).unwrap();
    let moov = read::child(&data, b"moov").unwrap();
    let meta = read::child(read::child(moov, b"udta").unwrap(), b"meta").unwrap();
    // `meta` is a full box: four bytes of version and flags before its children.
    let ilst = read::child(meta.get(4..).unwrap(), b"ilst").unwrap();
    let said = |key: &[u8; 4]| {
        let data = read::child(read::child(ilst, key).unwrap(), b"data").unwrap();
        String::from_utf8(data.get(8..).unwrap().to_vec()).unwrap()
    };
    assert_eq!(said(b"\xa9nam"), "Me at the zoo");
    assert_eq!(said(b"\xa9ART"), "jawed");
    let cover = read::child(read::child(ilst, b"covr").unwrap(), b"data").unwrap();
    assert_eq!(&cover[..4], &13u32.to_be_bytes(), "named a JPEG");
    assert!(read::child(read::child(moov, b"udta").unwrap(), b"chpl").is_some());
    assert!(
        read::child(ilst, b"\xa9day").is_none(),
        "an empty word is left out"
    );
    assert_eq!(read_back(&out).len(), 2, "the tracks are as they were");
    // And read back as they were written (gaps row Y25, for an MP3 made from it).
    assert_eq!(
        tags_of(&out),
        Tags {
            date: String::new(),
            ..tags.clone()
        }
    );
    let plain_out = dir.path().join("plain.mp4");
    join_cut(&[video, audio], &[], &plain_out, None, None).unwrap();
    let data = std::fs::read(&plain_out).unwrap();
    assert!(read::child(read::child(&data, b"moov").unwrap(), b"udta").is_none());
}

#[test]
fn a_cut_plays_the_part_alone_by_each_tracks_edit_list() {
    // Four fragments of three samples, each sample 3000 of 90000 a
    // second: 0.4 s in all. Cut to 0.15-0.35 s, the output holds every
    // sample and its edit list starts 150 ms in for 200 ms. The plain
    // sound (7 samples of 1024 at 48000: 0.149 s) ends before the cut
    // does, so its edit is what it has left of it.
    let dir = tempfile::tempdir().unwrap();
    let (video, pictures) = fragmented(dir.path(), "v.mp4", 4);
    let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
    let out = dir.path().join("part.mp4");
    join_cut(&[video, audio], &[], &out, Some((150, 350)), None).unwrap();
    let data = std::fs::read(&out).unwrap();
    let moov = read::child(&data, b"moov").unwrap();
    let edits: Vec<Vec<Edit>> = read::boxes(moov)
        .filter(|(k, _, _)| k == b"trak")
        .map(|(_, trak, _)| {
            let elst = read::child(read::child(trak, b"edts").unwrap(), b"elst").unwrap();
            read::read_edits(elst)
        })
        .collect();
    assert_eq!(edits.len(), 2);
    // The picture: 150 ms at 90000 is 13500; 200 ms at the movie's
    // 1000 is 200.
    assert_eq!(edits[0][0].media_time, 13500);
    assert_eq!(edits[0][0].duration, 200);
    // The sound: 150 ms at 48000 is 7200, and what it has after that
    // is nothing, which is written as the least an edit can be.
    assert_eq!(edits[1][0].media_time, 7200);
    assert!(edits[1][0].duration <= 1);
    let back = read_back(&out);
    assert_eq!(
        back[0].samples.len(),
        pictures.len(),
        "every sample is kept"
    );
    // A cut starting before a track that begins late waits for it:
    // the picture's fragments say they start at 0.2 s (tfdt).
    let (late, _) = fragmented_from(dir.path(), "late.mp4", 4, 18000);
    let out = dir.path().join("late.mp4.part");
    join_cut(&[late], &[], &out, Some((100, 400)), None).unwrap();
    let data = std::fs::read(&out).unwrap();
    let moov = read::child(&data, b"moov").unwrap();
    let trak = read::child(moov, b"trak").unwrap();
    let elst = read::child(read::child(trak, b"edts").unwrap(), b"elst").unwrap();
    let edits = read::read_edits(elst);
    assert_eq!(
        edits[0],
        Edit {
            duration: 100,
            media_time: -1
        },
        "waits 100 ms"
    );
    assert_eq!(edits[1].media_time, 0);
    assert_eq!(edits[1].duration, 200, "300 ms asked, less the wait");
}

#[test]
fn an_index_pointing_past_the_end_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (audio, _) = plain(dir.path(), "a.m4a", 4, 1);
    let data = std::fs::read(&audio).unwrap();
    std::fs::write(&audio, &data[..data.len() - 5]).unwrap();
    let err = join(&[audio], &dir.path().join("out.mp4")).unwrap_err();
    assert!(err.contains("cut short"), "{err}");
}

#[test]
fn what_is_not_an_mp4_is_refused_not_panicked_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.html");
    std::fs::write(&path, b"<!doctype html><title>no</title>").unwrap();
    assert!(join(&[path], &dir.path().join("out.mp4")).is_err());
}

proptest::proptest! {
    #[test]
    fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(proptest::num::u8::ANY, 0..600)) {
        let _ = boxes(&data).count();
        let _ = tracks_of(&data, 0);
        let _ = plain_samples(&data);
        let mut tracks = vec![Track { id: 1, timescale: 1, ..Track::default() }];
        let _ = read_fragment(0, &data, &[], &mut tracks);
    }
}
