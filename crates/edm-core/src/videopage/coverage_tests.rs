// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::{Categories, Control, Limiter};

fn options(dir: &Path) -> Options {
    Options {
        connections: 4,
        control: Control::new(),
        dest_dir: dir.join("out"),
        parts_dir: dir.join("parts"),
        limiter: Limiter::unlimited(),
        room: Default::default(),
        categories: Categories::default(),
        checksum: None,
        quality: crate::stream::Quality::Best,
        sound: None,
        span: None,
        shape: None,
        mp3: false,
        gif: false,
        media: false,
        referrer: None,
        proxy: None,
        container: None,
        attempts: 1,
        host_limits: Vec::new(),
        proxy_bypass: Vec::new(),
        subtitles: false,
        sign_in: None,
        save_name: None,
        accept_page: false,
        torrent: Default::default(),
        system_proxy: false,
        title_hint: None,
        remux: false,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
        same_files: None,
        frame_rate: None,
        caption: None,
        format: None,
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn hello() -> Vec<crate::subtitles::Subtitle> {
    vec![crate::subtitles::Subtitle {
        language: "en".into(),
        cues: crate::subtitles::parse_vtt("WEBVTT\n\n00:00.000 --> 00:00.500\nhello there\n"),
    }]
}

fn has(data: &[u8], what: &[u8]) -> bool {
    data.windows(what.len()).any(|w| w == what)
}

/// A part is cut from MP4 streams only: WebM ones asked to be joined as
/// WebM with a span are refused in words.
#[tokio::test]
async fn a_part_of_webm_streams_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("v.f247.webm"),
        fixture("webm/picture.f247.webm"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("v.f251.webm"),
        fixture("webm/sound.f251.webm"),
    )
    .unwrap();
    let mut opts = options(dir.path());
    opts.container = Some("webm".into());
    opts.span = crate::clip::Span::new(0.0, 0.5);
    let why = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &opts)
        .await
        .unwrap_err()
        .to_string();
    assert!(why.contains("a part is cut from MP4 streams only"), "{why}");
}

/// A VP8 picture beside an MP4's sound, which no MP4 holds, is saved as
/// an MKV; a pair an MP4 holds is an MP4 still, unless MKV was asked.
#[tokio::test]
async fn a_pair_no_mp4_holds_is_saved_as_an_mkv() {
    let dir = tempfile::tempdir().unwrap();
    crate::mkvmux::testing::tiny_webm(&dir.path().join("v.f1.webm"), "V_VP8", 1, 5);
    std::fs::write(
        dir.path().join("v.f140.m4a"),
        fixture("media/mp4mux/sound.m4a"),
    )
    .unwrap();
    let done = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &options(dir.path()))
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mkv");
    let data = std::fs::read(&done.path).unwrap();
    assert!(has(&data, b"V_VP8") && has(&data, b"A_AAC"));

    let dir = tempfile::tempdir().unwrap();
    let both = |dir: &Path| {
        std::fs::write(dir.join("v.f137.mp4"), fixture("media/mp4mux/picture.mp4")).unwrap();
        std::fs::write(dir.join("v.f140.m4a"), fixture("media/mp4mux/sound.m4a")).unwrap();
    };
    both(dir.path());
    let done = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &options(dir.path()))
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mp4", "as before");

    let dir = tempfile::tempdir().unwrap();
    both(dir.path());
    let mut asked = options(dir.path());
    asked.container = Some("mkv".into());
    let done = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &asked)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mkv");
    assert!(has(&std::fs::read(&done.path).unwrap(), b"V_MPEG4/ISO/AVC"));
}

/// With no room for the join beside the picture and sound, they are joined into the larger
/// (gaps row Y21): the same samples and subtitles as the usual join, the parts gone.
#[tokio::test]
async fn with_no_room_beside_them_the_parts_are_joined_into_the_picture() {
    let staged = |dir: &Path| {
        std::fs::write(dir.join("v.f137.mp4"), fixture("media/mp4mux/picture.mp4")).unwrap();
        std::fs::write(dir.join("v.f140.m4a"), fixture("media/mp4mux/sound.m4a")).unwrap();
    };
    let dir = tempfile::tempdir().unwrap();
    staged(dir.path());
    let usual = finish_dm(&dir.path().join("v.mp4"), hello(), &options(dir.path()))
        .await
        .unwrap();
    let made = crate::mp4mux::testing::read_back(&usual.path);

    let dir = tempfile::tempdir().unwrap();
    staged(dir.path());
    let both = (fixture("media/mp4mux/picture.mp4").len() + fixture("media/mp4mux/sound.m4a").len())
        as u64;
    let tight = Options {
        room: crate::Room::with_free_space(move |_| Some(both - 1)),
        ..options(dir.path())
    };
    let done = finish_dm(&dir.path().join("v.mp4"), hello(), &tight)
        .await
        .unwrap();
    assert_eq!(done.path.extension().unwrap(), "mp4");
    let joined = crate::mp4mux::testing::read_back(&done.path);
    assert_eq!(
        joined.len(),
        made.len(),
        "a picture, a sound and the subtitles"
    );
    for (ours, theirs) in joined.iter().zip(&made) {
        assert_eq!(ours.samples, theirs.samples);
        assert_eq!(ours.durations, theirs.durations);
    }
    assert!(has(&std::fs::read(&done.path).unwrap(), b"tx3g"));
    let left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("v.f") || name.ends_with(".edm-joining"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// Streams that are not media, joined or cut, fail in words rather than
/// leaving a broken file placed as the download.
#[tokio::test]
async fn streams_that_cannot_be_joined_or_cut_fail_in_words() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("v.f137.mp4"), b"not a picture").unwrap();
    std::fs::write(dir.path().join("v.f140.m4a"), b"not a sound").unwrap();
    // Neither of these is a part of `v`.
    std::fs::write(dir.path().join("v.f1.txt"), b"x").unwrap();
    std::fs::write(dir.path().join("w.f1.mp4"), b"x").unwrap();
    assert_eq!(
        parts_of(&dir.path().join("v.mp4")).await,
        [dir.path().join("v.f137.mp4"), dir.path().join("v.f140.m4a")],
        "the picture first"
    );
    let opts = options(dir.path());
    let why = finish_dm(&dir.path().join("v.mp4"), Vec::new(), &opts)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        why.contains("the picture and sound could not be joined"),
        "{why}"
    );

    let single = dir.path().join("s.m4a");
    std::fs::write(&single, b"not a sound either").unwrap();
    let mut cut = options(dir.path());
    cut.span = crate::clip::Span::new(0.0, 0.5);
    let why = finish_dm(&single, Vec::new(), &cut)
        .await
        .unwrap_err()
        .to_string();
    assert!(why.contains("the part could not be cut"), "{why}");
}

/// A transport stream under an `.mp4` name is put into an MP4; one that
/// only starts like one is kept as it came.
#[tokio::test]
async fn a_transport_stream_is_put_into_an_mp4() {
    let dir = tempfile::tempdir().unwrap();
    let produced = dir.path().join("t.mp4");
    std::fs::write(&produced, fixture("media/mp4mux/stream.ts")).unwrap();
    let done = finish_dm(&produced, Vec::new(), &options(dir.path()))
        .await
        .unwrap();
    let data = std::fs::read(&done.path).unwrap();
    assert_eq!(&data[4..8], b"ftyp", "an MP4 now");
    assert_eq!(done.connections, 1);
    assert!(!done.verified);

    let odd = dir.path().join("odd.mp4");
    let mut bytes = vec![0u8; 400];
    bytes[0] = 0x47;
    bytes[188] = 0x47;
    std::fs::write(&odd, &bytes).unwrap();
    let done = finish_dm(&odd, Vec::new(), &options(dir.path()))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), bytes);
    assert!(!dir.path().join("odd.edm-remux.mp4").exists());
    assert!(!is_transport_stream(&dir.path().join("none.ts")).await);
}

/// Subtitles beside a video that already holds picture and sound are
/// written into it, MP4 or WebM; a video that will not take them is
/// kept as it is.
#[tokio::test]
async fn subtitles_go_inside_one_file_or_it_is_kept_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let mp4 = dir.path().join("m.mp4");
    std::fs::write(&mp4, fixture("mp4/picture.f137.mp4")).unwrap();
    let done = finish_dm(&mp4, hello(), &options(dir.path()))
        .await
        .unwrap();
    let data = std::fs::read(&done.path).unwrap();
    assert!(has(&data, b"tx3g") && has(&data, b"hello there"));

    let webm = dir.path().join("w.webm");
    std::fs::write(&webm, fixture("webm/picture.f247.webm")).unwrap();
    let done = finish_dm(&webm, hello(), &options(dir.path()))
        .await
        .unwrap();
    assert!(has(&std::fs::read(&done.path).unwrap(), b"hello there"));

    let broken = dir.path().join("b.mp4");
    std::fs::write(&broken, b"no boxes here").unwrap();
    let done = finish_dm(&broken, hello(), &options(dir.path()))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"no boxes here");
    assert!(!dir.path().join("b.edm-subtitles.mp4").exists());
}

/// A fetch that left nothing under the name it gave is said as that,
/// not as "No such file".
#[tokio::test]
async fn a_missing_file_is_said_in_words() {
    let dir = tempfile::tempdir().unwrap();
    for produced in [dir.path().join("gone.mp4"), PathBuf::from("/")] {
        let why = finish_dm(&produced, Vec::new(), &options(dir.path()))
            .await
            .unwrap_err()
            .to_string();
        assert!(why.contains("the file it named is missing"), "{why}");
    }
    assert!(parts_of(Path::new("/")).await.is_empty());
    assert!(parts_of(&dir.path().join("nowhere/v.mp4")).await.is_empty());
}

/// A page is told from a stream by its type.
#[test]
fn a_page_is_told_by_its_type() {
    assert!(is_page(Some("text/html; charset=utf-8")));
    assert!(!is_page(Some("video/mp4")));
    assert!(!is_page(None));
}

/// A staging folder that cannot be removed is tried for a few seconds,
/// then left: a cancel does not hang on it.
#[cfg(unix)]
#[tokio::test]
async fn a_staging_that_cannot_be_removed_is_given_up_on() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("parts/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(locked.join("a.part"), b"x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    // Root may remove it anyway; then there is nothing to wait for.
    let at = std::time::Instant::now();
    remove_staging(&dir.path().join("parts")).await;
    let held = locked.join("a.part").exists();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    if held {
        assert!(
            at.elapsed() >= std::time::Duration::from_secs(2),
            "it tried again"
        );
        assert!(
            at.elapsed() < std::time::Duration::from_secs(5),
            "then gave up"
        );
    }
}
