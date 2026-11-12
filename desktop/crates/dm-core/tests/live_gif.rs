// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A part of a real YouTube video saved as a GIF, end to end, on a computer
//! with no video decoder of its own (Linux: dm takes the video's AV1 stream
//! and decodes it itself, gaps row 54). It reaches youtube.com, so it runs
//! only when asked:
//!
//!     cargo test -p dm-core --test live_gif -- --ignored --nocapture

mod common;

use common::{options, tempdir};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "reaches youtube.com: run by hand"]
async fn a_part_of_a_youtube_video_becomes_a_gif() {
    let dir = tempdir();
    let mut o = options(dir.path());
    o.gif = true;
    o.media = true;
    o.span = dm_core::Span::parse("0:02", "0:05");
    let done = dm_core::download_reporting(
        &dm_core::client(),
        "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        &o,
        None,
    )
    .await
    .expect("the part was not made a GIF");
    let bytes = std::fs::read(&done.path).unwrap();
    assert_eq!(
        done.path.extension().and_then(|e| e.to_str()),
        Some("gif"),
        "{:?}",
        done.path
    );
    assert_eq!(&bytes[..6], b"GIF89a");
    let (w, h) = (
        u16::from_le_bytes([bytes[6], bytes[7]]),
        u16::from_le_bytes([bytes[8], bytes[9]]),
    );
    // Images in the file, walked block by block: 12 a second for 3 seconds.
    let pictures = images_in(&bytes);
    println!("GIF {w} x {h}, {pictures} pictures, {} bytes", bytes.len());
    assert!(w > 0 && w <= 480 && h > 0, "{w} x {h}");
    assert!((30..=40).contains(&pictures), "{pictures} pictures");
}

/// How many images a GIF holds, by walking its blocks.
fn images_in(gif: &[u8]) -> usize {
    let table = |flags: u8| {
        if flags & 0x80 != 0 {
            3 << ((flags & 7) + 1)
        } else {
            0
        }
    };
    let mut at = 13 + table(gif[10]);
    let skip_blocks = |mut at: usize| {
        while at < gif.len() && gif[at] != 0 {
            at += usize::from(gif[at]) + 1;
        }
        at + 1
    };
    let mut images = 0;
    while at < gif.len() {
        match gif[at] {
            0x21 => at = skip_blocks(at + 2),
            0x2C => {
                images += 1;
                at = skip_blocks(at + 10 + table(gif[at + 9]) + 1);
            }
            _ => break,
        }
    }
    images
}
