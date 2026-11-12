// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Undoing a `Content-Encoding` a server applied to a body.
//!
//! dm asks for the file as the server keeps it (`Accept-Encoding: identity`),
//! but a server may compress what it was not asked to, and a browser would
//! undo that, so it is undone here. Not by reqwest: once it decodes, it says
//! nothing of how many bytes came over the wire, and a small response that
//! unpacks to gigabytes (a decompression bomb) could not be told from a large
//! page of text. Here the two counts are kept, and a body that unpacks to
//! more than [`FLOOR`] and more than [`RATIO`] times what was sent is stopped.

use std::collections::VecDeque;
use std::io::Write;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};

/// What is let through unconditionally, however well it compresses.
pub(crate) const FLOOR: u64 = 64 * 1024 * 1024;
/// Text compresses ten- or twenty-fold; a bomb a thousand-fold.
pub(crate) const RATIO: u64 = 100;
/// How much compressed input is fed to the decoder at a time, so no step
/// expands to more than a few megabytes in memory.
const STEP: usize = 4096;
/// How much is handed on at a time.
const SLICE: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coding {
    Gzip,
    Deflate,
    Brotli,
}

/// The coding a response's body is in, if it is in one dm undoes.
pub(crate) fn coding_of(headers: &reqwest::header::HeaderMap) -> Option<Coding> {
    let said = headers
        .get(reqwest::header::CONTENT_ENCODING)?
        .to_str()
        .ok()?
        .trim()
        .to_ascii_lowercase();
    match said.as_str() {
        "gzip" | "x-gzip" => Some(Coding::Gzip),
        "deflate" => Some(Coding::Deflate),
        "br" => Some(Coding::Brotli),
        _ => None,
    }
}

/// Why a decoded stream ended before its end, if it did: read by whoever read
/// the stream once it has ended.
pub(crate) type Stopped = Arc<Mutex<Option<String>>>;

enum Decoder {
    Gzip(flate2::write::GzDecoder<Vec<u8>>),
    Deflate(flate2::write::ZlibDecoder<Vec<u8>>),
    Brotli(Box<brotli_decompressor::DecompressorWriter<Vec<u8>>>),
}

impl Decoder {
    fn new(coding: Coding) -> Decoder {
        match coding {
            Coding::Gzip => Decoder::Gzip(flate2::write::GzDecoder::new(Vec::new())),
            Coding::Deflate => Decoder::Deflate(flate2::write::ZlibDecoder::new(Vec::new())),
            Coding::Brotli => Decoder::Brotli(Box::new(
                brotli_decompressor::DecompressorWriter::new(Vec::new(), 4096),
            )),
        }
    }

    fn feed(&mut self, input: &[u8]) -> std::io::Result<Vec<u8>> {
        match self {
            Decoder::Gzip(d) => {
                d.write_all(input)?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Deflate(d) => {
                d.write_all(input)?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Brotli(d) => {
                d.write_all(input)?;
                Ok(std::mem::take(d.get_mut()))
            }
        }
    }

    fn finish(&mut self) -> std::io::Result<Vec<u8>> {
        match self {
            Decoder::Gzip(d) => {
                d.try_finish()?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Deflate(d) => {
                d.try_finish()?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Brotli(d) => {
                d.flush()?;
                Ok(std::mem::take(d.get_mut()))
            }
        }
    }
}

struct State<S> {
    inner: S,
    decoder: Decoder,
    wire: u64,
    decoded: u64,
    out: VecDeque<Bytes>,
    over: bool,
    stopped: Stopped,
}

/// `inner`'s bytes, undone. The stream ends early (and says why in `stopped`)
/// on a body that is not what it claims to be or that unpacks beyond reason.
pub(crate) fn decoded<S>(
    coding: Coding,
    inner: S,
    stopped: Stopped,
) -> Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Send + Unpin + 'static,
{
    let state = State {
        inner,
        decoder: Decoder::new(coding),
        wire: 0,
        decoded: 0,
        out: VecDeque::new(),
        over: false,
        stopped,
    };
    Box::pin(futures_util::stream::unfold(
        state,
        move |mut st| async move {
            loop {
                if let Some(piece) = st.out.pop_front() {
                    return Some((Ok(piece), st));
                }
                if st.over {
                    return None;
                }
                match st.inner.next().await {
                    None => {
                        st.over = true;
                        match st.decoder.finish() {
                            Ok(rest) => {
                                st.decoded += rest.len() as u64;
                                push(&mut st.out, rest);
                            }
                            Err(e) => stop(
                                &st.stopped,
                                format!("the body ended in the middle of its compression ({e})"),
                            ),
                        }
                    }
                    Some(Err(e)) => {
                        st.over = true;
                        return Some((Err(e), st));
                    }
                    Some(Ok(chunk)) => {
                        st.wire += chunk.len() as u64;
                        for part in chunk.chunks(STEP) {
                            match st.decoder.feed(part) {
                                Ok(out) => {
                                    st.decoded += out.len() as u64;
                                    push(&mut st.out, out);
                                }
                                Err(e) => {
                                    st.over = true;
                                    stop(
                                        &st.stopped,
                                        format!("the body is not valid {coding:?} ({e})"),
                                    );
                                    break;
                                }
                            }
                            if st.decoded > FLOOR && st.decoded > st.wire.saturating_mul(RATIO) {
                                st.over = true;
                                st.out.clear();
                                stop(
                                &st.stopped,
                                format!(
                                    "the server's compressed response unpacks to far more than it sent ({} bytes from {}): stopped, as a decompression bomb would not be",
                                    st.decoded, st.wire
                                ),
                            );
                                break;
                            }
                        }
                    }
                }
            }
        },
    ))
}

fn push(out: &mut VecDeque<Bytes>, data: Vec<u8>) {
    let mut data = Bytes::from(data);
    while data.len() > SLICE {
        out.push_back(data.split_to(SLICE));
    }
    if !data.is_empty() {
        out.push_back(data);
    }
}

fn stop(stopped: &Stopped, why: String) {
    if let Ok(mut slot) = stopped.lock() {
        slot.get_or_insert(why);
    }
}

#[cfg(test)]
mod tests {
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
}
