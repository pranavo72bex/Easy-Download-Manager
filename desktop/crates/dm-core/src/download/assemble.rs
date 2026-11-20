// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

mod across;
mod seams;

use seams::leads_on_disk;
#[cfg(test)]
pub(super) use seams::SEAM_WINDOW;
pub(super) use seams::{ask_handed, ask_left, check_seams, hand_on, Handed};

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
///
/// A Cancel stops the join within a piece of [`JOIN_PIECE`] (gaps row 185), one part copied to
/// another drive within a piece of the system's copy, and the file it had begun goes with the
/// error; a pause does not: every byte is here, and a join cut short would lose the parts it had
/// used.
///
/// `spans`, where given, is each part's length in the file: a range's file can hold bytes past
/// its range (an attempt that ended past where a split later cut it), which are the next range's.
/// Each part is held to its span as it is joined, a rename's by a cut of the file it became and a
/// copy's by copying no further, where each range's file was opened and cut on its own first
/// (gaps row S5).
pub(super) async fn join_parts(
    parts: &[PathBuf],
    spans: Option<&[u64]>,
    into: &Path,
    control: &crate::control::Control,
) -> Result<u64, Error> {
    debug_assert!(spans.is_none_or(|s| s.len() == parts.len()));
    let first_span = spans.and_then(|s| s.first().copied());
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
            if let Some(span) = first_span {
                let (into, span) = (into.to_path_buf(), span);
                tokio::task::spawn_blocking(move || {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(into)?
                        .set_len(span)
                })
                .await
                .map_err(|e| {
                    Error::Internal(format!("the file could not be cut to size: {e}"))
                })??;
            }
            return Ok(fs::metadata(into).await?.len());
        }
        /* To another drive: on Windows by the system's own copy (CopyFileEx), which hands the
         * bytes over in its own large blocks and whose speed across drives was measured worth
         * having (row 166); elsewhere a piece at a time as the join copies (on Linux the kernel's
         * own copy). Either looks at the download's control between pieces, so a Cancel stops it
         * part-way (row 185). The part goes once the copy is whole. A turn of the computer's
         * heavy work (`heavy`). */
        let (from, to, control) = (only.clone(), into.to_path_buf(), control.clone());
        let copied = crate::heavy::run(move || -> Result<u64, Error> {
            if !from.exists() {
                return Err(Error::Internal(
                    "a staged part went missing while the file was being joined".into(),
                ));
            }
            let mut stop = || crate::heavy::stopped(&control);
            let bytes = across::copy_across(&from, &to, first_span, &mut stop)?;
            let _ = std::fs::remove_file(&from);
            Ok(bytes)
        })
        .await
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
    let copied = copy_parts(parts, spans, into, control).await;
    if copied.is_err() {
        let _ = fs::remove_file(into).await;
    }
    copied
}

/// The finished file's bytes, and its name in its folder, put on the disk without anything
/// waiting for it, so the row says Completed as soon as the system holds the whole file, as
/// reference A's does. Waited for, a file copied to a hard disk on another drive than dm's home
/// spent most of its finish on the flush: 512 MiB took 10 to 12.7 s where reference A took 5.7 to
/// 6.4 s (gaps row 166; the maintainer's choices, `docs/decisions.md` 2026-11-10).
/// What it gives up: a power cut in the seconds after Completed can leave the file's last blocks
/// unwritten, which a sync before Completed would rule out. Called once nothing more is
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
            crate::note!(
                warn,
                "dm: {} was not synced to the disk: {e}",
                path.display()
            );
        }
        #[cfg(unix)]
        if let Some(dir) = path.parent().and_then(|p| std::fs::File::open(p).ok()) {
            let _ = dir.sync_all();
        }
    });
}

/* The first part becomes the file by a rename, and the rest are appended to
 * it on one blocking thread. A join copying every byte again through async
 * I/O 8 KiB at a time, each read and write a hop to the blocking pool, would
 * take longer on a fast link than the download did. `std::io::copy` into
 * a large `BufWriter` reads in large blocks, and on Linux becomes
 * `copy_file_range`, which the kernel does without copying through dm (and
 * Btrfs or XFS without copying at all). */
pub(super) async fn copy_parts(
    parts: &[PathBuf],
    spans: Option<&[u64]>,
    into: &Path,
    control: &crate::control::Control,
) -> Result<u64, Error> {
    let (parts, into, control) = (parts.to_vec(), into.to_path_buf(), control.clone());
    let spans = spans.map(<[u64]>::to_vec);
    crate::heavy::run(move || copy_parts_blocking(&parts, spans.as_deref(), &into, &control))
        .await?
}

/// How much of a part is copied between two looks at whether the download was cancelled: about
/// a third of a second on a hard disk, a few hundredths on an SSD.
const JOIN_PIECE: u64 = 32 << 20;

/// The buffer a join writes through: large blocks, where `io::copy` alone moves 8 KiB at a time.
const JOIN_BUFFER: usize = 4 << 20;

/// Copies `source`, from where it stands, into `out`, as far as `left` or its end: a piece of
/// [`JOIN_PIECE`] at a time, each by `io::copy` (on Linux the kernel's own copy), with a look at
/// `stop` before each, so a Cancel ends it within a piece (gaps row 185).
fn copy_pieces(
    source: &mut std::fs::File,
    out: &mut std::io::BufWriter<std::fs::File>,
    mut left: u64,
    stop: &mut dyn FnMut() -> Result<(), Error>,
) -> Result<(), Error> {
    while left > 0 {
        stop()?;
        let piece = JOIN_PIECE.min(left);
        let copied = std::io::copy(&mut std::io::Read::take(&mut *source, piece), &mut *out)?;
        left -= copied;
        if copied < piece {
            break;
        }
    }
    Ok(())
}

pub(super) fn copy_parts_blocking(
    parts: &[PathBuf],
    spans: Option<&[u64]>,
    into: &Path,
    control: &crate::control::Control,
) -> Result<u64, Error> {
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
    let renamed = rest.len() < parts.len();
    let out = if renamed {
        // Opened to write, not to append: a handle that only appends may not change the file's
        // length on Windows ("Access is denied"), and the cut needs to.
        let mut out = std::fs::OpenOptions::new().write(true).open(into)?;
        // The first part, now the file, held to its span before the rest follow it.
        if let Some(span) = spans.and_then(|s| s.first()) {
            out.set_len(*span)?;
        }
        std::io::Seek::seek(&mut out, std::io::SeekFrom::End(0))?;
        out
    } else {
        std::fs::File::create(into)?
    };
    let mut out = std::io::BufWriter::with_capacity(JOIN_BUFFER, out);
    let skipped = usize::from(renamed);
    // The download's control, looked at between pieces.
    let mut stop = || crate::heavy::stopped(control);
    for (at, part) in rest.iter().enumerate() {
        let mut source = std::fs::File::open(part).map_err(|_| missing())?;
        // Copied no further than its span, where one is given.
        let left = spans
            .and_then(|s| s.get(at + skipped).copied())
            .unwrap_or(u64::MAX);
        copy_pieces(&mut source, &mut out, left, &mut stop)?;
        drop(source);
        let _ = std::fs::remove_file(part);
    }
    out.flush()?;
    Ok(out.get_ref().metadata()?.len())
}

/// Checks the ranges against each other and places the staged file as the
/// finished one.
pub(super) async fn assemble(
    job: &Job,
    info: &Probe,
    options: &Options,
) -> Result<(PathBuf, u64, category::Category), Error> {
    // A copy, the lock let go at once: the overlaps are read back from the
    // disk below, and every connection is over by now, so nothing changes it
    // meanwhile (gaps row 184).
    let segments = job.segments.lock().await.clone();

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
    crate::finish_trace::mark("assembling");
    let on_disk = leads_on_disk(job, &segments, &leads).await;
    crate::finish_trace::mark("leads read");
    // A slice whose bytes are not on disk proves nothing.
    let differing = leads
        .iter()
        .zip(&on_disk)
        .find(|((_, lead), there)| there.as_ref().is_some_and(|there| there != lead));
    if let Some(((at, lead), there)) = differing {
        crate::finish_trace::mark(&format!(
            "the {} bytes before {} differ from the next range's lead",
            lead.len(),
            at + lead.len() as u64
        ));
        // How, and between which ranges, for the trace alone (gaps row 206).
        if crate::finish_trace::on() {
            let seam = at + lead.len() as u64;
            crate::finish_trace::mark(&format!(
                "lead before {seam}: {}; ranges {}",
                seams::difference(lead, there.as_deref().unwrap_or_default()),
                seams::ranges_under(&segments, *at, seam)
            ));
        }
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
        // Each held to its span by the join itself, as it is copied (gaps row S5).
        let (parts, spans): (Vec<PathBuf>, Vec<u64>) = ranges.into_iter().unzip();
        return finish_held(
            &name,
            info.content_type.as_deref(),
            &parts,
            Some(&spans),
            options,
        )
        .await;
    }
    drop(segments);
    let staged = job.staged();
    fs::OpenOptions::new()
        .write(true)
        .open(&staged)
        .await?
        .set_len(length)
        .await?;
    crate::finish_trace::mark("held to its length");
    finish(&name, info.content_type.as_deref(), &[staged], options).await
}

/// Everything that happens once the bytes are all on disk.
///
/// Shared by every route that ends in a file: ranged downloads, streams, FTP,
/// live recordings, video downloads and a torrent of one file. No route
/// keeps its own copy of this, the checksum block included: this is the
/// one place that decides whether a file is the file.
pub(crate) async fn finish(
    name: &str,
    content_type: Option<&str>,
    parts: &[PathBuf],
    options: &Options,
) -> Result<(PathBuf, u64, category::Category), Error> {
    finish_held(name, content_type, parts, None, options).await
}

/// [`finish`], each part held to its span in `spans` as it is joined (`join_parts`).
async fn finish_held(
    name: &str,
    content_type: Option<&str>,
    parts: &[PathBuf],
    spans: Option<&[u64]>,
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
    crate::finish_trace::mark("named");

    let bytes = join_parts(parts, spans, &path, &options.control).await?;
    crate::finish_trace::mark("placed");

    // Cancelled during the join: what it produced is not wanted either.
    if options.control.wanted() == crate::control::Wanted::Cancel {
        let _ = fs::remove_file(&path).await;
        return Err(Error::Cancelled);
    }

    /* A file that fails its digest is removed rather than kept beside a
     * warning: one known to be wrong, sitting in the downloads folder looking
     * like every other one, is how it comes to be opened. */
    if let Some(expected) = &options.checksum {
        if let Err(e) = checksum::verify(&path, expected, &options.control).await {
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
        for note in [
            crate::paths::PLAN,
            "rendition",
            "joined.ts.partial",
            "joined.mp4.partial",
            REMUXED,
        ] {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each range's file held to its span by the join itself: the bytes an attempt wrote past
    /// where a split later cut its range are left out, whether the first part is renamed into
    /// place or every part is copied (gaps row S5).
    #[tokio::test]
    async fn each_part_is_joined_no_further_than_its_span() {
        let dir = std::env::temp_dir().join(format!("dm-join-spans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let control = crate::control::Control::new();
        let make = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        // Renamed: the first part becomes the file and is cut, the rest appended as far as
        // their spans.
        let parts = vec![make("a", b"aaaXX"), make("b", b"bbYY"), make("c", b"c")];
        let into = dir.join("joined");
        let bytes = join_parts(&parts, Some(&[3, 2, 1]), &into, &control)
            .await
            .unwrap();
        assert_eq!(bytes, 6);
        assert_eq!(std::fs::read(&into).unwrap(), b"aaabbc");
        assert!(parts.iter().all(|p| !p.exists()));
        // The blocking join of one part: renamed, then cut. (One part copied across drives,
        // where no rename can be made: `across`'s test.)
        let parts = [make("d", b"ddZZ"), make("e", b"eeeQ")];
        let into = dir.join("copied");
        let bytes = copy_parts_blocking(&parts[1..], Some(&[3]), &into, &control).unwrap();
        assert_eq!(bytes, 3);
        assert_eq!(std::fs::read(&into).unwrap(), b"eee");
        // One part alone: renamed and cut.
        let into = dir.join("one");
        let bytes = join_parts(&parts[..1], Some(&[2]), &into, &control)
            .await
            .unwrap();
        assert_eq!((bytes, std::fs::read(&into).unwrap()), (2, b"dd".to_vec()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
