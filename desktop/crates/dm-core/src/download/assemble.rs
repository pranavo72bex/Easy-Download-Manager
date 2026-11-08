// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// Concatenates part files into one, freeing each as it is consumed.
///
/// Copied through rather than read whole: `fs::read` allocates an entire part
/// before a byte of it is written, so peak memory tracked file size — a four
/// gigabyte download over eight connections held half a gigabyte at a time
/// for no reason.
///
/// Each part is removed as soon as it has been copied, which is the whole
/// difference between needing the file's size on disk and needing twice it.
/// Deleting them all at the end leaves every part in place while the finished
/// file grows alongside them; deleting each as it is consumed keeps the two
/// adding up to about one copy, since what the destination gains the parts
/// have just given up.
///
/// The cost is that a failure part-way through loses the parts already
/// copied, so the download starts again rather than resuming. That failure is
/// nearly always running out of room — which is the thing this stops
/// happening.
pub(super) async fn join_parts(parts: &[PathBuf], into: &Path) -> Result<u64, Error> {
    /* One part is not concatenated — it is moved.
     *
     * A ranged download's staged file, a stream gathered into one part: there
     * is nothing to join, the one part IS the file. Copying it doubled both the time and the peak
     * disk — the part and the growing copy both existed in full for the
     * length of the copy — for a rename that costs neither, when the two are
     * on one filesystem. Across filesystems a rename cannot move bytes, so it
     * falls back to the copy below. */
    if let [only] = parts {
        if fs::rename(only, into).await.is_ok() {
            return Ok(fs::metadata(into).await?.len());
        }
    }

    /* The file is dm's from here — `claim_path` took a name nothing had —
     * so a copy that fails part-way (the disk filled, nearly always) removes
     * what it had written rather than leaving a truncated file that looks
     * finished and, worse, holds the name the retry would otherwise take. */
    let copied = copy_parts(parts, into).await;
    if copied.is_err() {
        let _ = fs::remove_file(into).await;
    }
    copied
}

/// The finished file's bytes, and its name in its folder, on the disk before anything calls it
/// done: the queue records a completed row durably, and nothing resumes a finished file, so a
/// power cut soon after could leave a "completed" file whose last blocks never reached the disk
/// (engine review, 2026-11-08). One sync per finished file; a download's own writes are not synced
/// (`docs/decisions.md`, 2026-10-15). A file that cannot be synced is a disk error; its folder is
/// synced where the system allows it.
pub(super) async fn durable(path: &Path) -> Result<(), Error> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        // Opened for writing: Windows flushes a file only through a handle that may write it.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)?
            .sync_all()?;
        #[cfg(unix)]
        if let Some(dir) = path.parent().and_then(|p| std::fs::File::open(p).ok()) {
            let _ = dir.sync_all();
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
    .map_err(Error::Disk)
}

/* The first part becomes the file by a rename, and the rest are appended to
 * it on one blocking thread. The join used to copy every byte again through
 * async I/O 8 KiB at a time, each read and write a hop to the blocking pool:
 * on a fast link it took longer than the download did. `std::io::copy` into
 * a large `BufWriter` reads in large blocks, and on Linux becomes
 * `copy_file_range`, which the kernel does without copying through dm (and
 * Btrfs or XFS without copying at all). */
pub(super) async fn copy_parts(parts: &[PathBuf], into: &Path) -> Result<u64, Error> {
    let (parts, into) = (parts.to_vec(), into.to_path_buf());
    tokio::task::spawn_blocking(move || copy_parts_blocking(&parts, &into))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
}

pub(super) fn copy_parts_blocking(parts: &[PathBuf], into: &Path) -> Result<u64, Error> {
    use std::io::Write;
    /* Every part exists by the time this runs — a stream's segment is named
     * only once whole, a ranged download's staged file is made before its
     * first byte — so one that does not is one that was taken away, which is what a cancel does. Skipping it wrote the rest as
     * a shorter file and reported it Completed. */
    let missing =
        || Error::Internal("a staged part went missing while the file was being joined".into());
    let rest = match parts.split_first() {
        Some((first, rest)) if std::fs::rename(first, into).is_ok() => rest,
        Some((first, _)) if !first.exists() => return Err(missing()),
        // Across filesystems a rename cannot move bytes: every part is copied.
        _ => parts,
    };
    let out = if rest.len() == parts.len() {
        std::fs::File::create(into)?
    } else {
        std::fs::OpenOptions::new().append(true).open(into)?
    };
    let mut out = std::io::BufWriter::with_capacity(4 << 20, out);
    for part in rest {
        let mut source = std::fs::File::open(part).map_err(|_| missing())?;
        std::io::copy(&mut source, &mut out)?;
        drop(source);
        let _ = std::fs::remove_file(part);
    }
    out.flush()?;
    Ok(out.get_ref().metadata()?.len())
}

/// How much of each side of a seam is fetched a second time to be compared.
const SEAM_WINDOW: u64 = 2048;

/// How many seams are looked at, spread along the file: a request each, so
/// a file cut in two hundred pieces is not asked two hundred more questions.
const SEAMS_CHECKED: usize = 8;

/// Whether the bytes either side of some seams, fetched again, are the ones
/// on disk. A request that fails or is not answered as a range proves
/// nothing and is let pass: only bytes that differ are a finding.
async fn seams_agree(
    client: &reqwest::Client,
    job: &Job,
    segments: &[Segment],
    order: &[usize],
) -> bool {
    let pairs = order.len().saturating_sub(1);
    if pairs == 0 {
        return true;
    }
    let checks = pairs.min(SEAMS_CHECKED);
    for k in 0..checks {
        let at = k * pairs / checks;
        let (previous, index) = (order[at], order[at + 1]);
        let (before, after) = (&segments[previous], &segments[index]);
        if before.done == 0 || after.done == 0 {
            continue;
        }
        let seam = after.start;
        let low = seam - before.done.min(SEAM_WINDOW);
        let high = seam + after.done.min(SEAM_WINDOW) - 1;
        let Some(there) = span_again(client, job, index, low, high).await else {
            continue;
        };
        let Some(here) = span_on_disk(&job.staged(), low, high - low + 1).await else {
            continue;
        };
        if there != here {
            return false;
        }
    }
    true
}

/// `low..=high` of the file fetched afresh from the source of range `index`,
/// exactly as a range is asked for: `None` unless it comes back as that
/// range, whole.
async fn span_again(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    low: u64,
    high: u64,
) -> Option<Vec<u8>> {
    let source = job.source(index);
    let mut request = get(
        client,
        &source.url,
        job.referrer.as_deref(),
        job.sign_in.as_ref(),
    );
    request = job.headers.onto(
        request,
        crate::cookies::header_with(job.sign_in.as_ref(), &source.url).is_some(),
        job.referrer.is_some(),
    );
    request = request.header(reqwest::header::RANGE, format!("bytes={low}-{high}"));
    if let Some(validator) = source.validator.as_ref().filter(|v| !v.starts_with("W/")) {
        request = request.header(reqwest::header::IF_RANGE, validator);
    }
    let res = or_stop(
        &job.control,
        crate::auth::send(client, request, &source.url),
    )
    .await
    .ok()?;
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return None;
    }
    let wanted = high - low + 1;
    if res.content_length().is_some_and(|n| n != wanted) {
        return None;
    }
    let bytes = tokio::time::timeout(std::time::Duration::from_secs(15), res.bytes())
        .await
        .ok()?
        .ok()?;
    (bytes.len() as u64 == wanted).then(|| bytes.to_vec())
}

/// `length` bytes of the staged file from `at`.
async fn span_on_disk(path: &Path, at: u64, length: u64) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).await.ok()?;
    file.seek(std::io::SeekFrom::Start(at)).await.ok()?;
    let mut bytes = vec![0u8; usize::try_from(length).ok()?];
    tokio::io::AsyncReadExt::read_exact(&mut file, &mut bytes)
        .await
        .ok()?;
    Some(bytes)
}

/// Checks the ranges against each other and places the staged file as the
/// finished one.
pub(super) async fn assemble(
    client: &reqwest::Client,
    job: &Job,
    info: &Probe,
    options: &Options,
) -> Result<(PathBuf, u64, category::Category), Error> {
    let segments = job.segments.lock().await;

    // A length the server stated has to be the length that arrived. Segments
    // are written at the offsets they were handed out, so a server that
    // answers a range with other bytes leaves gaps that nothing above this
    // line would notice — the parts are all present and the result is short.
    if let Some(expected) = info.size {
        let got: u64 = segments.iter().map(|s| s.done).sum();
        if got != expected {
            return Err(Error::ShortFile { got, expected });
        }
    }

    // Each overlap slice against the matching bytes of the range before it.
    // A mismatch is a server that sent bytes from the wrong place under the
    // right label; the parts are thrown away so a Retry fetches them afresh
    // rather than reassembling the same wrong bytes.
    // In file order, which a split leaves different from part order.
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&i| segments[i].start);
    let mut dishonest = false;
    for pair in order.windows(2) {
        let (_previous, index) = (pair[0], pair[1]);
        let lead = {
            let guard = crate::lock(&job.overlaps[index]);
            guard.clone()
        };
        let Some(lead) = lead else { continue };
        if lead.is_empty() {
            continue;
        }
        let seam = segments[index].start;
        let n = lead.len() as u64;
        let disk_start = seam.saturating_sub(n);
        let Some(on_disk) = span_on_disk(&job.staged(), disk_start, n).await else {
            continue;
        };
        if on_disk != lead {
            dishonest = true;
            break;
        }
    }
    // The same seams looked at a second time, as reference A does: a few of
    // them fetched again over a fresh request and compared with what the
    // connections wrote.
    if !dishonest && !seams_agree(client, job, &segments, &order).await {
        dishonest = true;
    }
    if dishonest {
        drop(segments);
        let mut listing = fs::read_dir(&options.parts_dir).await?;
        while let Some(entry) = listing.next_entry().await? {
            let _ = fs::remove_file(entry.path()).await;
        }
        return Err(Error::DishonestRanges);
    }

    let name = if info.filename.is_empty() {
        "download".into()
    } else {
        info.filename.clone()
    };

    /* Every range is in place in the staged file, so it is the file: held
     * to the length that arrived (an unknown length was never known to
     * preallocate) and renamed into place, with nothing copied. */
    let length: u64 = info
        .size
        .unwrap_or_else(|| segments.iter().map(|s| s.done).sum());
    drop(segments);
    let staged = job.staged();
    fs::OpenOptions::new()
        .write(true)
        .open(&staged)
        .await?
        .set_len(length)
        .await?;
    finish(&name, info.content_type.as_deref(), &[staged], options).await
}

/// Everything that happens once the bytes are all on disk.
///
/// Shared by every route that ends in a file: ranged downloads, streams, FTP,
/// live recordings, video downloads and a torrent of one file. Ranged
/// downloads and streams used to carry their own copy of this, including the checksum block —
/// directly under a comment claiming there was one place that decided
/// whether a file is the file.
pub(crate) async fn finish(
    name: &str,
    content_type: Option<&str>,
    parts: &[PathBuf],
    options: &Options,
) -> Result<(PathBuf, u64, category::Category), Error> {
    // Sorted by what it turned out to be, which is only knowable now: the
    // name can come from a Content-Disposition the URL never hinted at.
    let category = category::classify(name, content_type);
    /* Nothing is joined for a download that was stopped after its last byte
     * arrived: the parts are being removed underneath the join, and a file
     * assembled from whichever were still there is short and looks finished.
     * A pause is honoured here as well, so a paused row does not become a
     * completed one a moment later. */
    options.control.check()?;
    let dir = options.categories.place(&options.dest_dir, category, name);
    fs::create_dir_all(&dir).await?;
    // Cleaned as any name from outside is: a chosen "../x" stays in the
    // folder, as the rename it replaces kept it.
    let chosen = options
        .save_name
        .as_deref()
        .map(crate::probe::sanitize)
        .filter(|n| !n.trim().is_empty());
    let titled = if chosen.is_none() && crate::naming::is_meaningless(name) {
        let extension = std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        options
            .title_hint
            .as_deref()
            .and_then(|t| crate::naming::title_name(t, extension))
            .map(|n| crate::probe::sanitize(&n))
    } else {
        None
    };
    let path = claim_path(
        &dir,
        chosen.as_deref().or(titled.as_deref()).unwrap_or(name),
    )
    .await?;

    let bytes = join_parts(parts, &path).await?;

    // Cancelled during the join: what it produced is not wanted either.
    if options.control.wanted() == crate::control::Wanted::Cancel {
        let _ = fs::remove_file(&path).await;
        return Err(Error::Cancelled);
    }

    durable(&path).await?;

    /* A file that fails its digest is removed rather than kept beside a
     * warning: one known to be wrong, sitting in the downloads folder looking
     * like every other one, is how it comes to be opened. */
    if let Some(expected) = &options.checksum {
        if let Err(e) = checksum::verify(&path, expected).await {
            // The row must not say the file is gone while it is still there.
            if let Err(io) = fs::remove_file(&path).await {
                return Err(Error::Internal(format!(
                    "{e}; and the file could not be removed from {}: {io}",
                    path.display()
                )));
            }
            return Err(e);
        }
    }

    // The parts are consumed; what was written down about them goes too,
    // so a finished download leaves its staging directory as empty as it
    // found it.
    // A gather or a remux cut short by a crash, on a run that placed the
    // stream without them, goes too.
    // One task for all of it: each call through the async file API is a hop
    // to the blocking pool, and a small file made seven or more of them.
    let dir = options.parts_dir.clone();
    let _ = tokio::task::spawn_blocking(move || {
        for note in ["plan.json", "rendition", "joined.ts.partial", REMUXED] {
            let _ = std::fs::remove_file(dir.join(note));
        }
        // And each range's ledger, which the bytes they spoke for have left.
        if let Ok(listing) = std::fs::read_dir(&dir) {
            for entry in listing.flatten() {
                if entry.file_name().to_string_lossy().starts_with("done-") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    })
    .await;
    Ok((path, bytes, category))
}
