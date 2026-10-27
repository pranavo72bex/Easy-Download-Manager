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
