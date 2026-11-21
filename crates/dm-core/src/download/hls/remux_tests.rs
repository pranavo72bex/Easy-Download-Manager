// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An HLS stream put into an MP4 from its segments where they lie, by the room the disk has
//! (gaps row Y21): with room for the stream and its MP4, the segments go once the MP4 is whole;
//! with room for little more than the stream, each goes as it is read into the MP4; with less,
//! the stream is kept as its `.ts`, gathered as before.

use super::*;

/// A stream's four segments staged as dm names them, for `options`; and their bytes together.
fn staged(options: &Options) -> (Vec<PathBuf>, Vec<u8>) {
    std::fs::create_dir_all(&options.parts_dir).unwrap();
    let mut parts = Vec::new();
    let mut stream = Vec::new();
    for n in 0..4 {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/media/mp4mux/hls/segment{n}.ts"));
        let bytes = std::fs::read(from).unwrap();
        let to = part_path(&options.parts_dir, n);
        std::fs::write(&to, &bytes).unwrap();
        stream.extend(bytes);
        parts.push(to);
    }
    (parts, stream)
}

/// What is left in the parts folder.
fn left(options: &Options) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(&options.parts_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn a_stream_is_put_into_an_mp4_from_its_segments_by_the_room_there_is() {
    let dir = tempfile::tempdir().unwrap();
    let mp4 = || Some((PathBuf::from(REMUXED), "show.mp4".to_string()));
    let named = |made: Option<(PathBuf, String)>| {
        made.map(|(path, name)| (PathBuf::from(path.file_name().unwrap()), name))
    };

    // Room for the stream and its MP4: made from the segments, which go once it is whole.
    let roomy = super::coverage_tests::options(&dir.path().join("roomy"));
    let (parts, _) = staged(&roomy);
    let made = remux_ts("show.ts", &parts, &roomy).await.unwrap();
    assert_eq!(named(made), mp4());
    assert_eq!(left(&roomy), [REMUXED]);
    let whole = std::fs::read(roomy.parts_dir.join(REMUXED)).unwrap();

    // Room for little more than the stream: each segment freed as it is read in, and the
    // same MP4.
    let tight = super::coverage_tests::options(&dir.path().join("tight"));
    let (parts, stream) = staged(&tight);
    let size = stream.len() as u64;
    let tight = Options {
        room: crate::Room::with_free_space(move |_| Some(size - 1)),
        ..tight
    };
    let made = remux_ts("show.ts", &parts, &tight).await.unwrap();
    assert_eq!(named(made), mp4());
    assert_eq!(left(&tight), [REMUXED]);
    assert!(std::fs::read(tight.parts_dir.join(REMUXED)).unwrap() == whole);

    // Less: kept as the stream it came as, gathered into one `.ts`.
    let full = super::coverage_tests::options(&dir.path().join("full"));
    let (parts, stream) = staged(&full);
    let full = Options {
        room: crate::Room::with_free_space(|_| Some(0)),
        ..full
    };
    let made = remux_ts("show.ts", &parts, &full).await.unwrap();
    assert_eq!(
        named(made),
        Some((PathBuf::from(GATHERED), "show.ts".to_string()))
    );
    assert_eq!(left(&full), [GATHERED]);
    assert!(std::fs::read(full.parts_dir.join(GATHERED)).unwrap() == stream);
}
