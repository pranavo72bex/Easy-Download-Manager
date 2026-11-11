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
        /* To another drive, by the system's own copy (CopyFileEx on Windows, copy_file_range
         * on Linux, a clone or fcopyfile on macOS), which hands the bytes over in the system's
         * own large blocks; the part goes once the copy is whole (gaps row 166). */
        let (from, to) = (only.clone(), into.to_path_buf());
        let copied = tokio::task::spawn_blocking(move || -> Result<u64, Error> {
            if !from.exists() {
                return Err(Error::Internal(
                    "a staged part went missing while the file was being joined".into(),
                ));
            }
            let bytes = std::fs::copy(&from, &to)?;
            let _ = std::fs::remove_file(&from);
            Ok(bytes)
        })
        .await
        .map_err(|e| Error::Internal(e.to_string()))
        .and_then(|copied| copied);
        if copied.is_err() {
            let _ = fs::remove_file(into).await;
        }
        return copied;
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

/// The finished file's bytes, and its name in its folder, put on the disk without anything
/// waiting for it, so the row says Completed as soon as the system holds the whole file, as
/// reference A's does. Waited for, a file copied to a hard disk on another drive than dm's home
/// spent most of its finish on the flush: 512 MiB took 10 to 12.7 s where reference A took 5.7 to
/// 6.4 s (gaps row 166, 2026-11-09; the maintainer's choices of 2026-11-10, `docs/decisions.md`).
/// What it gives up: a power cut in the seconds after Completed can leave the file's last blocks
/// unwritten, which the 2026-11-08 sync before Completed had ruled out. Called once nothing more is
/// done to the file (after the queue's renames and the mark of where it came from).
///
/// On Windows nothing is asked: the system's own write-behind puts the file on the disk within
/// seconds, as it does reference A's. A flush asked for (`FlushFileBuffers`, or
/// `NtFlushBuffersFileEx` as fdatasync) holds the file against every other open while it runs:
/// 4 to 8 s for 512 MiB on a hard disk, in which a stream added to it (the mark) and a look at
/// its size (the window's, on its own thread) both waited, the window frozen with them, and
/// Explorer or a double-click would too (measured 2026-11-10). Elsewhere, an fsync on a thread of
/// its own, which keeps nobody else from the file; its folder too. A file that cannot be synced
/// (moved or deleted that soon) is left to the system. A folder (a torrent's) is not synced here.
pub(crate) fn sync_after(path: PathBuf) {
    if cfg!(windows) || !path.is_file() {
        return;
    }
    tokio::task::spawn_blocking(move || {
        let synced = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|file| file.sync_all());
        if let Err(e) = synced {
            eprintln!("dm: {} was not synced to the disk: {e}", path.display());
        }
        #[cfg(unix)]
        if let Some(dir) = path.parent().and_then(|p| std::fs::File::open(p).ok()) {
            let _ = dir.sync_all();
        }
    });
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

/// How much of each side of a seam, and of the file's end, is fetched a second time and compared
/// with what the connections wrote. Another version of a file, or another place in it, differs
/// within any few hundred bytes of it, and the 64 bytes before each range (`LEAD_BYTES`) already
/// catch a server that labels a range with the wrong place. Each range's last 4 KiB asked apart
/// from 2 KiB either side of up to eight seams was 66 KB of a 64 MiB file fetched twice: all of
/// the 0.1% that tied reference A's floor in `bench` per_conn (gaps row 177). At a kibibyte a side
/// the checks were 15 KB, which with what the server had sent past the first range's end (up to
/// 18 KB) still came to 0.05% in one run of four.
pub(super) const SEAM_WINDOW: u64 = 512;

/// Once range `index` is whole: each seam between it and a neighbour that is whole too, and the
/// file's last bytes where it is the last range, asked for again and compared with the bytes on
/// disk, as reference A looks at its seams a second time. A seam is asked for once, by whichever
/// of its two ranges is whole second, so the end of every range is looked at in the same request
/// as the start of the next, while the download goes on. The seams used to be asked one after
/// another after the last byte, with every range's end asked apart as well: the finish waited out
/// eight round trips, and each range's end was fetched twice over (gaps rows 171 and 177).
///
/// A range's own checks, two at most (a seam either side, or a seam and the file's end), are
/// asked at once rather than one after the other, on whichever connections are free (the finished
/// ranges leave theirs open): the range whole last is the one the download waits for, and it
/// nearly always has two to ask, so its finish waited a round trip of the link more than it
/// needed. In `bench` per_conn, whose server answers a new request 4 ms after it comes, the last
/// byte of the file to Completed went from 14 to 18 ms to 9 to 12 (traced, 2026-11-11).
///
/// A request that fails or is not answered as a range proves nothing and is let pass; bytes that
/// differ, or the whole file answered under the validator (another version of it), are a finding,
/// and the file is fetched again on one connection (`Job::disagreed`).
pub(super) async fn check_seams(client: &reqwest::Client, job: &Job, index: usize) {
    use std::sync::atomic::Ordering::SeqCst;
    let (spans, segments) = {
        let segments = job.segments.lock().await;
        let Some(me) = segments.get(index).filter(|s| s.finished()) else {
            return;
        };
        let mut spans = Vec::new();
        // The range just before this one in the file, and the one just after it.
        let before = segments
            .iter()
            .position(|s| s.end.checked_add(1) == Some(me.start));
        let after = segments
            .iter()
            .position(|s| me.end.checked_add(1) == Some(s.start));
        for (lower, higher) in [(before, Some(index)), (Some(index), after)] {
            let (Some(lower), Some(higher)) = (lower, higher) else {
                continue;
            };
            let (low_side, high_side) = (&segments[lower], &segments[higher]);
            // A seam is known by the range that starts at it, whose start never moves.
            let claimed = || {
                job.seams_checked
                    .get(higher)
                    .is_some_and(|checked| !checked.swap(true, SeqCst))
            };
            if low_side.finished() && high_side.finished() && claimed() {
                let seam = high_side.start;
                spans.push((
                    seam - low_side.span().min(SEAM_WINDOW),
                    seam + high_side.span().min(SEAM_WINDOW) - 1,
                ));
            }
        }
        let last = job
            .size
            .is_some_and(|size| me.end.checked_add(1) == Some(size));
        if last && !job.end_checked.swap(true, SeqCst) {
            spans.push((me.end + 1 - me.span().min(SEAM_WINDOW), me.end));
        }
        if spans.is_empty() {
            return;
        }
        (spans, segments.clone())
    };
    let asked = spans
        .iter()
        .map(|&(low, high)| span_agrees(client, job, index, &segments, low, high));
    if !futures_util::future::join_all(asked)
        .await
        .into_iter()
        .all(|agrees| agrees)
    {
        job.disagreed.store(true, SeqCst);
    }
}

/// Whether `low..=high` of the file, fetched afresh from the source of range `index` exactly as
/// a range is asked for (with the headers a stream's reader says its server wants, and under the
/// validator), is the bytes on disk. True where the answer proves nothing.
async fn span_agrees(
    client: &reqwest::Client,
    job: &Job,
    index: usize,
    segments: &[Segment],
    low: u64,
    high: u64,
) -> bool {
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
    let conditional = source.validator.as_ref().filter(|v| !v.starts_with("W/"));
    if let Some(validator) = conditional {
        request = request.header(reqwest::header::IF_RANGE, validator);
    }
    let Ok(res) = or_stop(
        &job.control,
        crate::auth::send(client, request, &source.url),
    )
    .await
    else {
        return true;
    };
    /* Asked under the validator, the whole file instead of the range is the server saying the
     * file is no longer that version: the bytes on disk are not all of one file (a file
     * republished during the download, `zoo.rs` changes-midway). */
    if conditional.is_some() && res.status() == reqwest::StatusCode::OK {
        return false;
    }
    if res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return true;
    }
    let wanted = high - low + 1;
    if res.content_length().is_some_and(|n| n != wanted) {
        return true;
    }
    let Ok(Ok(there)) = tokio::time::timeout(std::time::Duration::from_secs(15), res.bytes()).await
    else {
        return true;
    };
    if there.len() as u64 != wanted {
        return true;
    }
    let Some(here) = bytes_at(job, segments, low, wanted).await else {
        return true;
    };
    there[..] == here[..]
}

/// `length` bytes of the file from `at`, read from where its ranges are: the staged file, or each
/// range's own file for the part of the span it holds. `None` where any of it is not on disk.
async fn bytes_at(job: &Job, segments: &[Segment], at: u64, length: u64) -> Option<Vec<u8>> {
    if !job.by_range {
        return span_on_disk(&job.staged(), at, length).await;
    }
    let end = at.checked_add(length)?;
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&i| segments[i].start);
    let mut bytes = Vec::with_capacity(usize::try_from(length).ok()?);
    for index in order {
        let segment = &segments[index];
        let (from, to) = (at.max(segment.start), end.min(segment.start + segment.done));
        if from >= to {
            continue;
        }
        // The ranges meet end to end; a gap before this one is bytes no range holds.
        if from != at + bytes.len() as u64 {
            return None;
        }
        let path = range_path(&job.parts, index);
        bytes.extend(span_on_disk(&path, from - segment.start, to - from).await?);
    }
    (bytes.len() as u64 == length).then_some(bytes)
}

/// The bytes on disk under each overlap slice, given as where it begins and the slice: `None` for
/// one not all there. Out of the staged file in one blocking task through one open, rather than an
/// open, a seek and a read for each slice, each of those a hop to the blocking pool while the
/// finished download waited: assembly before the file is placed went from 0.8 to 2.6 ms to 0.4 to
/// 1.0 on eight ranges (traced, 2026-11-11).
async fn leads_on_disk(
    job: &Job,
    segments: &[Segment],
    leads: &[(u64, Vec<u8>)],
) -> Vec<Option<Vec<u8>>> {
    if job.by_range {
        let mut found = Vec::with_capacity(leads.len());
        for (at, lead) in leads {
            found.push(bytes_at(job, segments, *at, lead.len() as u64).await);
        }
        return found;
    }
    if leads.is_empty() {
        return Vec::new();
    }
    let spans: Vec<(u64, usize)> = leads.iter().map(|(at, lead)| (*at, lead.len())).collect();
    let staged = job.staged();
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek};
        let mut file = std::fs::File::open(staged).ok();
        spans
            .into_iter()
            .map(|(at, length)| {
                let file = file.as_mut()?;
                file.seek(std::io::SeekFrom::Start(at)).ok()?;
                let mut bytes = vec![0u8; length];
                file.read_exact(&mut bytes).ok()?;
                Some(bytes)
            })
            .collect()
    })
    .await
    .unwrap_or_else(|_| vec![None; leads.len()])
}

/// `length` bytes of the file at `path` from `at`.
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
    // Each slice, and where the bytes it overlaps begin.
    let leads: Vec<(u64, Vec<u8>)> = order
        .windows(2)
        .filter_map(|pair| {
            let lead = crate::lock(&job.overlaps[pair[1]]).clone()?;
            let seam = segments[pair[1]].start;
            (!lead.is_empty()).then(|| (seam.saturating_sub(lead.len() as u64), lead))
        })
        .collect();
    let on_disk = leads_on_disk(job, &segments, &leads).await;
    // A slice whose bytes are not on disk proves nothing.
    let dishonest = leads
        .iter()
        .zip(&on_disk)
        .any(|((_, lead), there)| there.as_ref().is_some_and(|there| there != lead));
    if dishonest {
        drop(segments);
        release_staged(job, true).await;
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
    if job.by_range {
        /* Each range's file held to its range, in file order: an attempt that
         * ended past where a split later cut the range left bytes after it
         * that are the next range's. Appended into the finished file by
         * `finish`, each freed once copied. */
        let ranges: Vec<(PathBuf, u64)> = order
            .iter()
            .map(|&i| (range_path(&job.parts, i), segments[i].span()))
            .collect();
        drop(segments);
        for (path, span) in &ranges {
            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .await?
                .set_len(*span)
                .await?;
        }
        let parts: Vec<PathBuf> = ranges.into_iter().map(|(path, _)| path).collect();
        return finish(&name, info.content_type.as_deref(), &parts, options).await;
    }
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
    // Not synced here: whoever hands the file on starts it once nothing more is done to the
    // file (`sync_after`).

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
        // And each range's ledger, which the bytes they spoke for have left,
        // and a range's own file the join copied but could not free then.
        if let Ok(listing) = std::fs::read_dir(&dir) {
            for entry in listing.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("done-") || name.starts_with(RANGE) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    })
    .await;
    Ok((path, bytes, category))
}
