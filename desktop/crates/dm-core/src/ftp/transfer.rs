//! Downloading a file in parts, and resuming them.

use super::*;

/// Fetches an FTP file into `options.dest_dir`, over several connections
/// where the server takes `REST`.
pub async fn download_ftp(
    url: &str,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    // Through the proxy, where one is set and this site is not bypassed;
    // a proxy FTP cannot go through is a refusal, never a direct route.
    let mut target = Target::parse(url)?;
    if let Some(why) = options.no_route_through_system_proxy(url, "FTP") {
        return Err(Error::Internal(why));
    }
    target.route = Route::parse(options.proxy_for(url))?;
    let target = target;
    let options = &Options {
        connections: options.connections_for(url),
        ..options.clone()
    };
    let described = retry::run_up_to(&options.control, options.tries(), || {
        or_stop(&options.control, describe(&target))
    })
    .await?;
    let target = Target {
        path: described.absolute.clone().unwrap_or(target.path.clone()),
        ..target
    };
    let name = crate::probe::filename_for(None, url, None);

    fs::create_dir_all(&options.parts_dir).await?;
    fs::create_dir_all(&options.dest_dir).await?;
    /* An empty file has no range to fetch: planned as one byte it was
     * waited for and never came, and the download failed (or, set to keep
     * trying, never ended). It is placed as it is. */
    if described.size == Some(0) {
        let part = part_path(&options.parts_dir, 0);
        fs::write(&part, b"").await?;
        let (path, bytes, category) =
            crate::download::finish(&name, None, &[part], options).await?;
        return Ok(Finished {
            left_behind: None,
            path,
            bytes,
            connections: 1,
            category,
            verified: options.checksum.is_some(),
            mirrors: 1,
        });
    }
    let segments = planned(&described, options).await?;

    let downloaded = Arc::new(AtomicU64::new(0));
    let mut ranges = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let part = part_path(&options.parts_dir, index);
        let on_disk = fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
        if on_disk > segment.span() {
            if let Ok(file) = fs::OpenOptions::new().write(true).open(&part).await {
                let _ = file.set_len(segment.span()).await;
            }
        }
        let done = on_disk.min(segment.span());
        downloaded.fetch_add(done, Ordering::Relaxed);
        ranges.push((segment.clone(), part));
    }

    if let Some(tx) = &report {
        let _ = tx
            .send(Update::Named {
                name: name.clone(),
                category: category::classify(&name, None),
            })
            .await;
        if let Some(validator) = &described.validator {
            let _ = tx.send(Update::Validator(validator.clone())).await;
        }
    }
    let reporter =
        report.map(|tx| crate::download::watch(Arc::clone(&downloaded), described.size, tx));

    let workers = options
        .connections
        .clamp(1, plan::MAX_CONNECTIONS)
        .min(ranges.len());
    let ranges = Arc::new(ranges);
    let next = Arc::new(AtomicUsize::new(0));
    let mut running = Vec::new();
    for _ in 0..workers {
        let (target, ranges, next, downloaded, options) = (
            target.clone(),
            Arc::clone(&ranges),
            Arc::clone(&next),
            Arc::clone(&downloaded),
            options.clone(),
        );
        let resumable = described.resumable;
        running.push(tokio::spawn(async move {
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((segment, part)) = ranges.get(index) else {
                    return Ok::<(), Error>(());
                };
                // As for HTTP: without REST each attempt starts from nothing.
                let on_disk = || {
                    if resumable {
                        std::fs::metadata(part).map_or(0, |m| m.len())
                    } else {
                        0
                    }
                };
                // A part a power cut tore can end in bytes the disk never
                // got: its length is not the ledger. Checked once per range
                // and run, not at each retry of this run's own writing; a
                // server that cannot be asked is left to the fetch to say.
                if resumable {
                    if let Err(stopped @ (Error::Paused | Error::Cancelled)) =
                        repair_torn_part(&target, segment, part, &options, &downloaded).await
                    {
                        return Err(stopped);
                    }
                }
                retry::run_making_progress(&options.control, options.tries(), on_disk, || {
                    fetch_range(&target, segment, part, resumable, &options, &downloaded)
                })
                .await?;
            }
        }));
    }
    let mut outcome = Ok(());
    for worker in running {
        let result = worker
            .await
            .map_err(|e| Error::Internal(e.to_string()))
            .and_then(|r| r);
        if outcome.is_ok() {
            outcome = result;
        }
    }
    if let Some((handle, finished)) = reporter {
        let _ = finished.send(());
        let _ = handle.await;
    }
    outcome?;

    if let Some(expected) = described.size {
        let got = downloaded.load(Ordering::Relaxed);
        if got != expected {
            return Err(Error::ShortFile { got, expected });
        }
    }
    let parts: Vec<PathBuf> = ranges.iter().map(|(_, part)| part.clone()).collect();
    let (path, bytes, category) = crate::download::finish(&name, None, &parts, options).await?;
    Ok(Finished {
        left_behind: None,
        path,
        bytes,
        connections: workers,
        category,
        verified: options.checksum.is_some(),
        mirrors: 1,
    })
}

/// The plan recorded beside the parts where it is this file's, or a new one.
pub(super) async fn planned(
    described: &Described,
    options: &Options,
) -> Result<Vec<Segment>, Error> {
    let dir = options.parts_dir.clone();
    let recorded = tokio::task::spawn_blocking(move || plan::recorded(&dir))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    if let Some(recorded) = recorded {
        if recorded.size == described.size
            && recorded.resumable
            && described.resumable
            && recorded.validator == described.validator
        {
            return Ok(recorded.segments());
        }
    }
    let mut listing = fs::read_dir(&options.parts_dir).await?;
    while let Some(entry) = listing.next_entry().await? {
        let _ = fs::remove_file(entry.path()).await;
    }
    let segments = plan::pieces(described.size, options.connections, described.resumable);
    let (dir, size, resumable, validator, written) = (
        options.parts_dir.clone(),
        described.size,
        described.resumable,
        described.validator.clone(),
        segments.clone(),
    );
    tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &written)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(segments)
}

pub(super) fn part_path(parts_dir: &Path, index: usize) -> PathBuf {
    parts_dir.join(format!("part-{index:04}"))
}

/// Cuts a part back to the last byte the server agrees with, where its last
/// block is not what the server has (a power cut, a block never written).
pub(super) async fn repair_torn_part(
    target: &Target,
    segment: &Segment,
    part: &Path,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<(), Error> {
    let already = fs::metadata(part)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
        .min(segment.span());
    if already == 0 {
        return Ok(());
    }
    let agreed = agreed_end(target, segment, part, already, options).await?;
    if agreed < already {
        let file = fs::OpenOptions::new().write(true).open(part).await?;
        file.set_len(agreed).await?;
        downloaded.fetch_sub(already - agreed, Ordering::Relaxed);
    }
    Ok(())
}

/// How much of the last 64 KiB of a part the server agrees with: the part's
/// last block is read from the server again, and what is on disk is believed
/// up to the first byte that differs (or past what the server gives).
pub(super) async fn agreed_end(
    target: &Target,
    segment: &Segment,
    part: &Path,
    already: u64,
    options: &Options,
) -> Result<u64, Error> {
    const TAIL: u64 = 64 * 1024;
    let tail = already.min(TAIL);
    let from = already - tail;
    let mut ours = vec![0u8; tail as usize];
    let mut file = fs::File::open(part).await?;
    file.seek(std::io::SeekFrom::Start(from)).await?;
    file.read_exact(&mut ours).await?;
    let (_session, mut data) = or_stop(&options.control, async {
        let mut session = Session::open(target).await?;
        let data = session.retrieve(&target.path, segment.start + from).await?;
        Ok((session, data))
    })
    .await?;
    let mut theirs = Vec::with_capacity(ours.len());
    let mut buffer = vec![0u8; 16 * 1024];
    while theirs.len() < ours.len() {
        let read = or_stop(&options.control, timed(data.read(&mut buffer))).await?;
        if read == 0 {
            break;
        }
        let room = ours.len() - theirs.len();
        theirs.extend_from_slice(buffer.get(..read.min(room)).unwrap_or_default());
    }
    let same = ours.iter().zip(&theirs).take_while(|(a, b)| a == b).count();
    Ok(from + same as u64)
}

/// One attempt at one range, from where its part file ends.
pub(super) async fn fetch_range(
    target: &Target,
    segment: &Segment,
    part: &Path,
    resumable: bool,
    options: &Options,
    downloaded: &AtomicU64,
) -> Result<(), Error> {
    let span = segment.span();
    let mut already = fs::metadata(part)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
        .min(span);
    if already >= span {
        return Ok(());
    }
    // Without REST a transfer starts at the beginning, so it is kept from
    // there rather than written after what an earlier attempt left.
    if !resumable && already > 0 {
        let file = fs::OpenOptions::new().write(true).open(part).await?;
        file.set_len(0).await?;
        downloaded.fetch_sub(already, Ordering::Relaxed);
        already = 0;
    }

    let (mut session, mut data) = or_stop(&options.control, async {
        let mut session = Session::open(target).await?;
        let data = session
            .retrieve(&target.path, segment.start + already)
            .await?;
        Ok((session, data))
    })
    .await?;

    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .await?;
    let mut file = tokio::io::BufWriter::with_capacity(256 * 1024, file);
    file.seek(std::io::SeekFrom::Start(already)).await?;

    let mut written = already;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if let Err(stopped) = options.control.check() {
            file.flush().await?;
            return Err(stopped);
        }
        // Raced against the control, as HTTP's reads are: a silent server
        // does not hold a pause for the read timeout.
        let read = match or_stop(&options.control, timed(data.read(&mut buffer))).await {
            Ok(read) => read,
            Err(e) => {
                file.flush().await?;
                return Err(e);
            }
        };
        if read == 0 {
            break;
        }
        let keep = (read as u64).min(span - written) as usize;
        file.write_all(buffer.get(..keep).unwrap_or_default())
            .await?;
        written += keep as u64;
        downloaded.fetch_add(keep as u64, Ordering::Relaxed);
        if let Err(stopped) = or_stop(&options.control, async {
            options.limiter.take(keep as u64).await;
            Ok(())
        })
        .await
        {
            file.flush().await?;
            return Err(stopped);
        }
        if written >= span {
            // This range is whole; the rest of the stream is another's.
            break;
        }
    }
    file.flush().await?;
    drop(data);

    // A file of unknown length ends where its transfer ends — and only a
    // transfer the server says is complete: a data connection closed early,
    // with 426 or 451 on the control one, is a short file, not the file.
    if segment.end == u64::MAX {
        let (code, text) = or_stop(&options.control, session.reply()).await?;
        return match code {
            226 | 250 => Ok(()),
            _ => Err(refusal(code, &text, &target.path)),
        };
    }
    if written < span {
        return Err(Error::Incomplete {
            got: written,
            expected: span,
        });
    }
    Ok(())
}
