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

/// No overlap byte fetched for a range (one fetched is 0–255).
pub(super) const NO_OVERLAP: u16 = u16::MAX;

/// The byte at `at` in a file, if it has one there.
pub(super) async fn byte_at(path: &Path, at: u64) -> Option<u8> {
    let mut file = fs::File::open(path).await.ok()?;
    if file.metadata().await.ok()?.len() <= at {
        return None;
    }
    file.seek(std::io::SeekFrom::Start(at)).await.ok()?;
    let mut byte = [0u8; 1];
    tokio::io::AsyncReadExt::read_exact(&mut file, &mut byte)
        .await
        .ok()?;
    Some(byte[0])
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

    // Each overlap byte against the last byte of the range before it. A
    // mismatch is a server that sent bytes from the wrong place under the
    // right label; the parts are thrown away so a Retry fetches them afresh
    // rather than reassembling the same wrong bytes.
    // In file order, which a split leaves different from part order.
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&i| segments[i].start);
    for pair in order.windows(2) {
        let (previous, index) = (pair[0], pair[1]);
        let byte = job.overlaps[index].load(Ordering::Relaxed);
        if byte == NO_OVERLAP {
            continue;
        }
        let before = {
            let p = &segments[previous];
            match p.done {
                0 => None,
                done => byte_at(&job.staged(), p.start + done - 1).await,
            }
        };
        if before.is_some_and(|b| u16::from(b) != byte) {
            drop(segments);
            let mut listing = fs::read_dir(&options.parts_dir).await?;
            while let Some(entry) = listing.next_entry().await? {
                let _ = fs::remove_file(entry.path()).await;
            }
            return Err(Error::DishonestRanges);
        }
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
/// live recordings, yt-dlp's output and a torrent of one file. Ranged
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
    let path = claim_path(&dir, chosen.as_deref().unwrap_or(name)).await?;

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
