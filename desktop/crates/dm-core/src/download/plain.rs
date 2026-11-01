use super::*;
use crate::probe::Probe;
use tokio::io::AsyncWriteExt;

/// The most of a file's index (`moov`) that is read.
const INDEX_CAP: u64 = 64 * 1024 * 1024;
/// The most one request asks for.
const SLICE: u64 = 4 * 1024 * 1024;

/// Whether the file at this address is an MP4 a part can be cut from.
pub(super) fn is_mp4(name: &str, content_type: Option<&str>) -> bool {
    let lower = name.to_ascii_lowercase();
    [".mp4", ".m4v", ".mov", ".m4a"]
        .iter()
        .any(|ext| lower.ends_with(ext))
        || content_type.is_some_and(|t| {
            let t = t.to_ascii_lowercase();
            t.starts_with("video/mp4")
                || t.starts_with("audio/mp4")
                || t.starts_with("video/quicktime")
        })
}

/// A part of a plain MP4 file: its index asked for by range, the frames
/// between the two times (from the last picture that can start the part)
/// asked for by range, and the part built by dm's MP4 writer with its edit
/// list trimming it to the second. Nothing else of the file is fetched.
pub(super) async fn download_mp4_part(
    client: &reqwest::Client,
    url: &str,
    info: &Probe,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let span = options
        .span
        .ok_or_else(|| Error::Internal("no part was asked for".into()))?;
    let size = match (info.resumable, info.size) {
        (true, Some(size)) if size > 0 => size,
        _ => {
            return Err(Error::Internal(
                "a part of a file can be cut only from a server that serves byte ranges and \
                 says how long the file is"
                    .into(),
            ))
        }
    };
    let name = hls::named_for_span(&info.filename, span);
    fs::create_dir_all(&options.parts_dir).await?;
    hls::keep_only_parts_of(
        &options.parts_dir,
        &format!("mp4 part {url}\n{size}\n{}", span.words()),
    )
    .await?;
    fs::create_dir_all(&options.dest_dir).await?;
    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: name.clone(),
                category: category::classify(&name, info.content_type.as_deref()),
            })
            .await;
    }

    let made = options.parts_dir.join("part.mp4");
    if !fs::metadata(&made).await.is_ok_and(|m| m.len() > 0) {
        let (from_ms, to_ms) = span.millis();
        let index = find_index(client, url, size, options).await?;
        let index_file = options.parts_dir.join("index.mp4");
        fs::write(&index_file, &index).await?;
        let planned = {
            let index_file = index_file.clone();
            tokio::task::spawn_blocking(move || {
                crate::mp4mux::plan_plain_part(&index_file, from_ms, to_ms)
            })
            .await
            .map_err(|e| Error::Internal(format!("reading the index stopped: {e}")))?
            .map_err(Error::Internal)?
        };

        // The frames, one range after another, into one compact file.
        let data = options.parts_dir.join("part.data");
        let downloaded = Arc::new(AtomicU64::new(0));
        let meter = report.map(|tx| watch(Arc::clone(&downloaded), Some(planned.bytes()), tx));
        let gathered = async {
            let mut out = tokio::io::BufWriter::new(fs::File::create(&data).await?);
            for (first, last) in &planned.ranges {
                let mut at = *first;
                while at <= *last {
                    let to = (at + SLICE - 1).min(*last);
                    let body = retry::run_up_to(&options.control, options.tries(), || {
                        hls::fetch_segment_range(client, url, Some((at, to)), options)
                    })
                    .await?;
                    out.write_all(&body).await?;
                    downloaded.fetch_add(body.len() as u64, Ordering::Relaxed);
                    at = to + 1;
                }
            }
            out.flush().await?;
            Ok::<(), Error>(())
        }
        .await;
        if let Some((handle, finished)) = meter {
            let _ = finished.send(());
            let _ = handle.await;
        }
        gathered?;
        options.control.check()?;

        let partial = options.parts_dir.join("part.partial.mp4");
        let _ = fs::remove_file(&partial).await;
        let (data_file, out_file, shape) = (data.clone(), partial.clone(), options.shape);
        let written = tokio::task::spawn_blocking(move || {
            crate::mp4mux::write_plain_part(planned, &data_file, &out_file, (from_ms, to_ms), shape)
        })
        .await
        .map_err(|e| Error::Internal(format!("building the part stopped: {e}")))?;
        if let Err(why) = written {
            let _ = fs::remove_file(&partial).await;
            return Err(Error::Internal(format!(
                "the part could not be built: {why}"
            )));
        }
        fs::rename(&partial, &made).await?;
        let _ = fs::remove_file(&data).await;
        let _ = fs::remove_file(&index_file).await;
    }

    let (path, bytes, category) =
        finish(&name, info.content_type.as_deref(), &[made], options).await?;
    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: 1,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// The file's `moov` box, found by walking the top-level boxes with a
/// request for each header: usually after the first few, or at the very end.
async fn find_index(
    client: &reqwest::Client,
    url: &str,
    size: u64,
    options: &Options,
) -> Result<Vec<u8>, Error> {
    let odd = || Error::Internal("this file's boxes do not add up, so no part can be cut".into());
    let mut at = 0u64;
    for _ in 0..64 {
        if at + 8 > size {
            break;
        }
        let head =
            hls::fetch_segment_range(client, url, Some((at, (at + 15).min(size - 1))), options)
                .await?;
        let small = head
            .get(0..4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_be_bytes)
            .ok_or_else(odd)?;
        let kind = head.get(4..8).ok_or_else(odd)?;
        let total = match small {
            0 => size - at,
            1 => head
                .get(8..16)
                .and_then(|b| <[u8; 8]>::try_from(b).ok())
                .map(u64::from_be_bytes)
                .ok_or_else(odd)?,
            n => u64::from(n),
        };
        if total < 8 || at.saturating_add(total) > size {
            return Err(odd());
        }
        if kind == b"moov" {
            if total > INDEX_CAP {
                return Err(Error::Internal("this file's index is too large".into()));
            }
            return hls::fetch_segment_range(client, url, Some((at, at + total - 1)), options)
                .await;
        }
        at += total;
    }
    Err(Error::Internal(
        "this file has no index (moov) to cut a part by".into(),
    ))
}
