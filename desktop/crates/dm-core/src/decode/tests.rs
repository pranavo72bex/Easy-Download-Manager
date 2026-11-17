// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn headers(encoding: &str) -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(reqwest::header::CONTENT_ENCODING, encoding.parse().unwrap());
    h
}

/// `body` through the decoder for `coding`, in pieces of `piece`: what came
/// out, and why it stopped, if it did.
async fn undone(coding: Coding, body: &[u8], piece: usize) -> (Vec<u8>, Option<String>) {
    let chunks: Vec<reqwest::Result<Bytes>> = body
        .chunks(piece.max(1))
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    let stopped = Stopped::default();
    let mut stream = decoded(coding, futures_util::stream::iter(chunks), stopped.clone());
    let mut out = Vec::new();
    while let Some(piece) = stream.next().await {
        out.extend_from_slice(&piece.unwrap());
    }
    let why = stopped.lock().unwrap().clone();
    (out, why)
}

#[test]
fn the_codings_dm_undoes_are_known_by_their_names() {
    assert_eq!(coding_of(&headers(" X-GZIP ")), Some(Coding::Gzip));
    assert_eq!(coding_of(&headers("deflate")), Some(Coding::Deflate));
    assert_eq!(coding_of(&headers("br")), Some(Coding::Brotli));
    assert_eq!(coding_of(&headers("zstd")), None);
    assert_eq!(coding_of(&reqwest::header::HeaderMap::new()), None);
}

#[tokio::test]
async fn deflate_and_brotli_bodies_are_undone() {
    let text = b"the same words, again and again, the same words".repeat(100);
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zlib.write_all(&text).unwrap();
    let packed = zlib.finish().unwrap();
    assert_eq!(undone(Coding::Deflate, &packed, 7).await, (text, None));

    // A brotli stream of one stored block holding "hello", then an empty last one.
    let mut br = vec![0x40, 0x00, 0x10];
    br.extend_from_slice(b"hello");
    br.push(0x03);
    assert_eq!(
        undone(Coding::Brotli, &br, 3).await,
        (b"hello".to_vec(), None)
    );
}

#[tokio::test]
async fn a_large_body_is_handed_on_in_slices() {
    let text = vec![b'a'; 3 * SLICE + 5];
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&text).unwrap();
    let packed = gz.finish().unwrap();
    let chunks = vec![Ok(Bytes::from(packed))];
    let mut stream = decoded(
        Coding::Gzip,
        futures_util::stream::iter(chunks),
        Stopped::default(),
    );
    let mut sizes = Vec::new();
    while let Some(piece) = stream.next().await {
        sizes.push(piece.unwrap().len());
    }
    assert!(sizes.iter().all(|n| *n <= SLICE), "{sizes:?}");
    assert_eq!(sizes.iter().sum::<usize>(), text.len());
}

#[tokio::test]
async fn a_body_cut_short_or_not_in_its_coding_is_said_to_be() {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(b"a body that will be cut short").unwrap();
    let packed = gz.finish().unwrap();
    let (_, why) = undone(Coding::Gzip, &packed[..packed.len() - 6], 64).await;
    let why = why.expect("a reason");
    assert!(why.contains("ended in the middle"), "{why}");

    let (out, why) = undone(Coding::Deflate, b"this was never compressed", 64).await;
    assert!(out.is_empty());
    let why = why.expect("a reason");
    assert!(why.contains("not valid Deflate"), "{why}");
}

#[tokio::test]
async fn an_error_of_the_connection_is_passed_on_and_ends_the_body() {
    let client = crate::reqwest_defaults();
    // A request that cannot be sent gives a reqwest error to pass on.
    let error = client.get("http://127.0.0.1:0/").send().await.unwrap_err();
    let chunks: Vec<reqwest::Result<Bytes>> = vec![Err(error), Ok(Bytes::from_static(b"x"))];
    let mut stream = decoded(
        Coding::Gzip,
        futures_util::stream::iter(chunks),
        Stopped::default(),
    );
    assert!(stream.next().await.unwrap().is_err());
    assert!(stream.next().await.is_none(), "nothing after the error");
}
