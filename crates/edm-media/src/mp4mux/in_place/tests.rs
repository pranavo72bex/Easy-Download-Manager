// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A join written into its picture holds the disk under 1.3 times the file, where the usual join
//! holds it twice over; the same samples, timing and set-up (gaps row Y21).

use std::sync::{Arc, Mutex};

use super::super::testing::{fragmented, plain, read_back};
use super::*;

/// The bytes of every file in `dir`, together: what the disk holds there.
fn held(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.metadata().unwrap().len())
        .sum()
}

fn job(inputs: Vec<PathBuf>, watch: Option<Box<dyn FnMut() + Send>>) -> InPlace {
    InPlace {
        inputs,
        cut: None,
        shape: None,
        tags: Tags::default(),
        placed: false,
        watch,
    }
}

#[test]
fn a_join_written_into_its_picture_holds_the_disk_under_1_3_times_the_file() {
    let dir = tempfile::tempdir().unwrap();
    // A picture of 240 samples and a sound of 120, about a twentieth of it, as a video's are.
    let (video, pictures) = fragmented(dir.path(), "v.mp4", 80);
    let (audio, sounds) = plain(dir.path(), "a.m4a", 120, 7);
    let usual_dir = tempfile::tempdir().unwrap();
    let usual = usual_dir.path().join("joined.mp4");
    join(&[video.clone(), audio.clone()], &usual).unwrap();
    let inputs = held(dir.path());
    let usual_length = std::fs::metadata(&usual).unwrap().len();
    let usual_peak = inputs + usual_length;

    let peak = Arc::new(Mutex::new(0u64));
    let (seen, at) = (Arc::clone(&peak), dir.path().to_path_buf());
    let watch: Box<dyn FnMut() + Send> = Box::new(move || {
        let mut most = seen.lock().unwrap();
        *most = (*most).max(held(&at));
    });
    let mut asked = 0;
    let (host, length) = join_in_place(
        job(vec![video.clone(), audio.clone()], Some(watch)),
        |_| Vec::new(),
        |_, need| {
            asked = need;
            true
        },
    )
    .unwrap();
    assert_eq!(host, 0, "the picture, the larger");
    assert_eq!(length, std::fs::metadata(&video).unwrap().len());
    assert!(
        asked > 0 && asked < length / 4,
        "a chunk and the index: {asked}"
    );
    let peak = *peak.lock().unwrap();
    assert!(peak >= length, "the file and what is left of the sound");
    assert!(
        peak * 10 < length * 13,
        "peak {peak} for a file of {length}"
    );
    assert!(
        usual_peak * 10 > usual_length * 19,
        "the usual join: {usual_peak}"
    );
    assert!(
        std::fs::metadata(&audio).unwrap().len() < 1024,
        "the sound let go of"
    );
    assert!(joining_mark(&video).exists() && joining_mark(&audio).exists());

    let joined = read_back(&video);
    let made = read_back(&usual);
    assert_eq!(joined.len(), 2);
    assert_eq!(joined[0].samples, pictures);
    assert_eq!(joined[1].samples, sounds);
    for (ours, theirs) in joined.iter().zip(&made) {
        assert_eq!(ours.durations, theirs.durations);
        assert_eq!(ours.entries, theirs.entries);
        assert_eq!(ours.timescale, theirs.timescale);
    }
    let data = std::fs::read(&video).unwrap();
    let kinds: Vec<[u8; 4]> = boxes(&data).map(|(kind, _, _)| kind).collect();
    assert_eq!(kinds.first(), Some(b"ftyp"));
    assert_eq!(kinds.last(), Some(b"moov"), "the index last");
    assert_eq!(kinds.iter().filter(|k| *k == b"moov").count(), 1);
    assert!(!kinds.contains(b"moof"), "its fragments made free");
    assert_eq!(&data[8..12], b"isom");
}

#[test]
fn with_no_room_or_nothing_to_join_into_nothing_is_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (video, _) = fragmented(dir.path(), "v.mp4", 4);
    let (audio, _) = plain(dir.path(), "a.m4a", 7, 200);
    let before = (
        std::fs::read(&video).unwrap(),
        std::fs::read(&audio).unwrap(),
    );
    let refused = join_in_place(
        job(vec![video.clone(), audio.clone()], None),
        |_| Vec::new(),
        |_, _| false,
    );
    assert!(
        matches!(refused, Err(NotInPlace::Untouched(_))),
        "{refused:?}"
    );
    let after = (
        std::fs::read(&video).unwrap(),
        std::fs::read(&audio).unwrap(),
    );
    assert!(after == before);
    assert!(!joining_mark(&video).exists() && !joining_mark(&audio).exists());

    let webm = dir.path().join("v.webm");
    crate::mkvmux::testing::tiny_webm(&webm, "V_VP9", 1, 5);
    let refused = join_in_place(job(vec![webm], None), |_| Vec::new(), |_, _| true);
    assert!(
        matches!(refused, Err(NotInPlace::Untouched(_))),
        "{refused:?}"
    );
}
