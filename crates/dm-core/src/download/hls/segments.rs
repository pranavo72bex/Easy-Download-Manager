// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A stream's segments fetched, each to a file of its own or into one, and cut or joined into the
//! file.

use super::*;

pub(super) async fn fetch_segments(
    client: &reqwest::Client,
    listed: &stream::Stream,
    keys: &Keys,
    options: &Options,
    downloaded: &AtomicU64,
    estimate: Option<&AtomicU64>,
) -> Result<(), Error> {
    // Workers taking the next unfetched index, rather than a stream of
    // futures: each worker is one plain future, which keeps this `Send`.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let worker = || async {
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(part) = listed.parts.get(index) else {
                return Ok::<(), Error>(());
            };
            fetch_one_segment(client, keys, options, downloaded, index, part).await?;
            // The stream's size, about: what its pieces so far came to, as many times over as
            // it has pieces (gaps row Y22). Surer with each piece; from the second.
            let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(estimate) = estimate.filter(|_| finished >= 2) {
                let so_far = downloaded.load(Ordering::Relaxed);
                let pieces = listed.parts.len() as u64;
                estimate.store(so_far / finished as u64 * pieces, Ordering::Relaxed);
            }
        }
    };
    // Connections per file, as a file's ranges take them: a fixed eight held a stream there
    // whatever the setting said (gaps row Y23).
    let at_once = options.connections.max(1);
    futures_util::future::try_join_all((0..at_once).map(|_| worker())).await?;
    Ok(())
}

async fn fetch_one_segment(
    client: &reqwest::Client,
    keys: &Keys,
    options: &Options,
    downloaded: &AtomicU64,
    index: usize,
    part: &stream::Part,
) -> Result<(), Error> {
    let done = part_path(&options.parts_dir, index);

    /* Written under a temporary name and moved when whole.
     *
     * A segment has no declared length, so a part file that is present
     * says nothing about whether it finished — half of one looks exactly
     * like all of one. Only a completed segment is ever given its real
     * name, which is what makes resuming a stream possible at all. */
    if let Ok(meta) = fs::metadata(&done).await {
        downloaded.fetch_add(meta.len(), Ordering::Relaxed);
        return Ok(());
    }

    options.control.check()?;

    let staging = done.with_extension("partial");
    let client = &crate::second_path::client_for_piece(client, &options.parts_dir, index);
    // One in the clear is written as it comes, not held whole first (gaps row Y17); so is one
    // whose samples are scrambled (SAMPLE-AES), put in the clear once joined (gaps row Y16).
    let key = part.key.as_ref().filter(|k| !part.init && !k.sample);
    if key.is_none() {
        let written = retry::run_up_to(&options.control, options.tries(), || {
            fetch_segment_to(client, &part.url, part.range, options, &staging)
        })
        .await?;
        downloaded.fetch_add(written, Ordering::Relaxed);
        fs::rename(&staging, &done).await?;
        return Ok(());
    }
    let fetched = retry::run_up_to(&options.control, options.tries(), || {
        fetch_segment_range(client, &part.url, part.range, options)
    })
    .await?;

    // Decrypted with its own key, where it has one (the initialisation segment never does).
    let bytes = match key {
        Some(declared) => {
            let key = keys.get(&declared.uri).map(Vec::as_slice).unwrap_or(&[]);
            stream::decrypt(key, &stream::iv_for(declared, part.sequence), &fetched)?
        }
        None => fetched,
    };

    downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    fs::write(&staging, &bytes).await?;
    fs::rename(&staging, &done).await?;
    Ok(())
}

/// One segment, whole or the bytes of a file that it is, held because it
/// has to be decrypted before it can be kept: exactly the bytes asked for,
/// or an error. A server that ignores the range and sends the whole file
/// would otherwise be written in as the piece.
pub(in crate::download) async fn fetch_segment_range(
    client: &reqwest::Client,
    url: &str,
    range: Option<(u64, u64)>,
    options: &Options,
) -> Result<Vec<u8>, Error> {
    /// The largest a segment is sized for up front. A segment grown by
    /// doubling from empty is copied about fourteen times on the way, so the
    /// hint is worth taking — but only as far as a real segment goes.
    /// Content-Length is the server's claim, and a claim of a terabyte would
    /// otherwise be an allocation of a terabyte.
    const HINT_CAP: u64 = 32 * 1024 * 1024;

    let res = open_segment(client, url, range, options).await?;
    let hint = res.content_length().unwrap_or(0).min(HINT_CAP);
    let mut body = Vec::with_capacity(hint as usize);
    read_segment(res, url, range, options, Sink::Memory(&mut body)).await?;
    Ok(body)
}

/// [`fetch_segment_range`] written to `path` as it comes, from its start, rather than held
/// (gaps row Y17): a segment in the clear, which needs no decrypting whole. Its length.
pub(in crate::download) async fn fetch_segment_to(
    client: &reqwest::Client,
    url: &str,
    range: Option<(u64, u64)>,
    options: &Options,
    path: &Path,
) -> Result<u64, Error> {
    use tokio::io::AsyncWriteExt as _;
    let res = open_segment(client, url, range, options).await?;
    let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, fs::File::create(path).await?);
    let written = read_segment(res, url, range, options, Sink::File(&mut file)).await?;
    file.flush().await?;
    file.get_mut().sync_data().await?;
    Ok(written)
}

/// Where a segment's bytes go as they come: held, or written to its file.
enum Sink<'a> {
    Memory(&'a mut Vec<u8>),
    File(&'a mut tokio::io::BufWriter<fs::File>),
}

impl Sink<'_> {
    async fn put(&mut self, chunk: &[u8]) -> Result<(), Error> {
        use tokio::io::AsyncWriteExt as _;
        match self {
            Sink::Memory(body) => body.extend_from_slice(chunk),
            Sink::File(file) => file.write_all(chunk).await?,
        }
        Ok(())
    }
}

/// A segment's request, sent: its range asked for where it is cut by one, and an answer that
/// does not honour it refused.
async fn open_segment(
    client: &reqwest::Client,
    url: &str,
    range: Option<(u64, u64)>,
    options: &Options,
) -> Result<reqwest::Response, Error> {
    let mut request = get(
        client,
        url,
        options.referrer.as_deref(),
        options.sign_in.as_ref(),
    );
    if let Some((first, last)) = range {
        request = request.header(reqwest::header::RANGE, format!("bytes={first}-{last}"));
    }
    let res = or_stop(&options.control, crate::auth::send(client, request, url)).await?;
    let res = Error::refused_or(res).await?;
    if range.is_some() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(Error::Internal(format!(
            "{url} did not honour the byte range this stream is cut by"
        )));
    }
    Ok(res)
}

/// A segment's body, each chunk put in `sink` as it comes, under the speed limit; its length,
/// which must be the range's where it is cut by one.
async fn read_segment(
    res: reqwest::Response,
    url: &str,
    range: Option<(u64, u64)>,
    options: &Options,
    mut sink: Sink<'_>,
) -> Result<u64, Error> {
    let mut length = 0u64;
    let mut chunks = res.bytes_stream();
    while let Some(chunk) = next_or_stop(&mut chunks, &options.control).await? {
        let chunk = chunk?;
        // A segment is seconds of video. One that keeps coming is not one,
        // and is not held, in memory or on the disk, until there is no room.
        if length + chunk.len() as u64 > SEGMENT_CAP as u64 {
            return Err(Error::Internal(format!(
                "{url} is far larger than a stream's segment should be"
            )));
        }
        sink.put(&chunk).await?;
        length += chunk.len() as u64;
        or_stop(&options.control, async {
            options.limiter.take(chunk.len() as u64).await;
            Ok(())
        })
        .await?;
    }
    if let Some((first, last)) = range {
        if length != last - first + 1 {
            return Err(Error::Internal(format!(
                "{url} answered a byte range with the wrong number of bytes"
            )));
        }
    }
    Ok(length)
}

/// A file name with the span it holds before its extension:
/// "show.mp4" -> "show (1m30s-2m05s).mp4".
pub(in crate::download) fn named_for_span(name: &str, span: crate::clip::Span) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => {
            format!("{stem} ({}).{ext}", span.words())
        }
        _ => format!("{name} ({})", span.words()),
    }
}

/// The fetched fragments of a fragmented MP4 (its initialisation segment
/// first) joined, then cut to `window` (milliseconds) by the muxer's edit
/// lists, into one file in the parts directory. With no window, the whole: a stream whose
/// initialisation segment changes part-way is made one MP4 of its runs (gaps row Y16).
pub(super) async fn cut_fragments(
    parts: &[PathBuf],
    window: Option<(u64, u64)>,
    options: &Options,
) -> Result<PathBuf, Error> {
    cut_fragments_with(
        parts,
        window,
        &super::hls_text::HlsTexts::default(),
        options,
    )
    .await
}

/// [`cut_fragments`], with an HLS stream's subtitles carried inside, timed from where the
/// fragments start (`tfdt`, gaps row Y20); none where `texts` is empty, as before.
pub(super) async fn cut_fragments_with(
    parts: &[PathBuf],
    window: Option<(u64, u64)>,
    texts: &super::hls_text::HlsTexts,
    options: &Options,
) -> Result<PathBuf, Error> {
    let whole = options.parts_dir.join("cut-input.mp4");
    let out = options.parts_dir.join("cut-output.mp4");
    let _ = fs::remove_file(&whole).await;
    let _ = fs::remove_file(&out).await;
    join_parts_copy(parts, &whole).await?;
    if texts.is_empty() {
        crate::mp4mux::join_cut_async(
            vec![whole.clone()],
            Vec::new(),
            out.clone(),
            window,
            options.shape,
        )
        .await
        .map_err(Error::Internal)?;
    } else {
        let texts = texts.clone();
        let place = move |zero| texts.timed(zero);
        let made = crate::mp4mux::join_cut_placed_async(
            vec![whole.clone()],
            place,
            out.clone(),
            window,
            options.shape,
        )
        .await;
        if let Err(why) = made {
            let _ = fs::remove_file(&whole).await;
            let _ = fs::remove_file(&out).await;
            return Err(Error::Internal(why));
        }
    }
    let _ = fs::remove_file(&whole).await;
    Ok(out)
}

/// The files one after another into `into`, leaving them in place.
pub(in crate::download) async fn join_parts_copy(
    parts: &[PathBuf],
    into: &Path,
) -> Result<(), Error> {
    use tokio::io::AsyncWriteExt;
    let mut out = tokio::io::BufWriter::new(fs::File::create(into).await?);
    for part in parts {
        let mut input = fs::File::open(part).await?;
        tokio::io::copy(&mut input, &mut out).await?;
    }
    out.flush().await?;
    Ok(())
}
