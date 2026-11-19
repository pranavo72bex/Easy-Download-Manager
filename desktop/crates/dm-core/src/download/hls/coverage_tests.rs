// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What only this module reaches: a remux whose segments cannot be gathered, and a part's name for
//! a file with no extension.

use super::*;
use crate::{Categories, Limiter};

pub(super) fn options(dir: &Path) -> Options {
    Options {
        connections: 1,
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
        remux: true,
        server_dates: false,
        retry_wait: 0,
        record_limit: None,
        same_files: None,
        frame_rate: None,
        caption: None,
        format: None,
    }
}

#[tokio::test]
async fn segments_that_cannot_be_gathered_are_left_for_the_ordinary_join() {
    let dir = tempfile::tempdir().unwrap();
    let options = options(dir.path());
    fs::create_dir_all(&options.parts_dir).await.unwrap();
    // A segment that is not there: nothing is gathered, and the
    // half-gathered stream is not left behind.
    let gone = options.parts_dir.join("part-0000");
    assert_eq!(remux_ts("show.ts", &[gone], &options).await.unwrap(), None);
    assert!(!options.parts_dir.join("joined.ts.partial").exists());
    assert!(!options.parts_dir.join(GATHERED).exists());

    // Gathered, but its name is taken by a folder that cannot be
    // replaced: the parts stay as they are.
    let part = options.parts_dir.join("part-0001");
    fs::write(&part, b"segment").await.unwrap();
    let taken = options.parts_dir.join(GATHERED);
    fs::create_dir_all(taken.join("inside")).await.unwrap();
    assert_eq!(
        remux_ts("show.ts", std::slice::from_ref(&part), &options)
            .await
            .unwrap(),
        None
    );
    assert!(taken.is_dir());

    // Not a transport stream, or not asked: never touched.
    assert_eq!(
        remux_ts("show.mp4", std::slice::from_ref(&part), &options)
            .await
            .unwrap(),
        None
    );
    let off = Options {
        remux: false,
        ..options.clone()
    };
    assert_eq!(remux_ts("show.ts", &[part], &off).await.unwrap(), None);
}

#[test]
fn a_part_of_a_file_with_no_extension_is_named_after_the_whole_name() {
    let span = crate::clip::Span::new(90.0, 125.0).unwrap();
    let words = span.words();
    assert_eq!(named_for_span("stream", span), format!("stream ({words})"));
    // A leading dot is no extension, nor is a long tail after a dot.
    assert_eq!(
        named_for_span(".hidden", span),
        format!(".hidden ({words})")
    );
    assert_eq!(
        named_for_span("show.episode", span),
        format!("show.episode ({words})")
    );
    assert_eq!(
        named_for_span("show.mp4", span),
        format!("show ({words}).mp4")
    );
    assert_eq!(ts_stem("Show.TS"), "Show");
    assert_eq!(ts_stem("show.mp4"), "show.mp4");
}
