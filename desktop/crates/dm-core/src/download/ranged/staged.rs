//! What a staged file needs around its ranges: when the ledger is written,
//! how much of a part's end is checked on a resume, and the repair of a part
//! a power cut tore.

use super::*;

/// How often a range's ledger is written down while it downloads: every
/// this many bytes, or every second, whichever comes first.
pub(in crate::download) const LEDGER_EVERY: u64 = 4 * 1024 * 1024;

pub(in crate::download) const LEDGER_EVERY_SECOND: Duration = Duration::from_secs(1);

/// How much of a part's end is fetched again and compared on a resume: more
/// than a filesystem leaves unwritten at a power cut, little enough to cost
/// nothing beside the rest.
pub(in crate::download) const RESUME_CHECK: u64 = 64 * 1024;

/// How long the other connections get to put what they hold on disk after a
/// pause or a cancel, before the ones still going are stopped.
pub(in crate::download) const STOP_GRACE: Duration = Duration::from_secs(1);

/// The unit a disk writes in, and so the unit a power cut loses.
pub(in crate::download) const DISK_BLOCK: u64 = 4096;

/// Removes the plan and the part files staged under `dir`.
pub(in crate::download) async fn discard_staged(dir: &Path) {
    let _ = fs::remove_file(crate::plan::record_path(dir)).await;
    if let Ok(mut listing) = fs::read_dir(dir).await {
        while let Ok(Some(entry)) = listing.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("part-")
                || name.starts_with("done-")
                || name == STAGED
                || name.starts_with(crate::queue::PREVIEW)
            {
                let _ = fs::remove_file(entry.path()).await;
            }
        }
    }
}

/// After the file is found changed: the plan its parts were fetched under
/// goes, so the next attempt adopts none of them and starts afresh, rather
/// than resuming into a file that no longer exists on the server.
pub(in crate::download) async fn forget_plan(job: &Job) {
    let _ = fs::remove_file(crate::plan::record_path(&job.parts)).await;
}

/// How far back past the checked window one repair follows a run of zeros
/// in a part. Far more than a disk block or the write buffer a power cut
/// loses at once, and still little to fetch again where the zeros were the
/// file's own; a longer loss is followed a step of this at a time.
pub(in crate::download) const ZERO_RUN_STEP: u64 = 4 * 1024 * 1024;

/// Where the run of zeros that ends at `end` in this file begins, looking
/// no further back than `floor`: `end` itself when the byte before it is not
/// zero, `floor` when the run reaches it. Read backwards a block of reads at
/// a time, so a long run costs reads, not memory.
pub(in crate::download) async fn zeros_ending_at(
    path: &Path,
    end: u64,
    floor: u64,
) -> Result<u64, Error> {
    const STEP: u64 = 64 * 1024;
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; STEP as usize];
    let mut at = end;
    while at > floor {
        let from = at.saturating_sub(STEP).max(floor);
        let len = (at - from) as usize;
        let chunk = buf.get_mut(..len).unwrap_or(&mut []);
        file.seek(std::io::SeekFrom::Start(from)).await?;
        tokio::io::AsyncReadExt::read_exact(&mut file, chunk).await?;
        if let Some(last) = chunk.iter().rposition(|&b| b != 0) {
            return Ok(from + last as u64 + 1);
        }
        at = from;
    }
    Ok(floor)
}

/// Reads `tail.len()` bytes off the stream against `tail`: how many agree
/// from the front, and whatever the stream gave past them (from the first
/// byte that differed, or past the end of `tail`), still to be written.
pub(in crate::download) async fn compare_tail<S>(
    stream: &mut S,
    tail: &[u8],
    control: &Control,
) -> Result<(u64, Option<bytes::Bytes>), Error>
where
    S: futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let mut seen = 0usize;
    while seen < tail.len() {
        let Some(chunk) = next_or_stop(stream, control).await? else {
            // Ended early: what agreed so far is kept, and the rest of the
            // range is short, which the caller reports as such.
            break;
        };
        let chunk = chunk.map_err(Error::Network)?;
        let theirs = chunk
            .get(..chunk.len().min(tail.len() - seen))
            .unwrap_or(&[]);
        let ours = tail.get(seen..seen + theirs.len()).unwrap_or(&[]);
        if let Some(at) = theirs.iter().zip(ours).position(|(a, b)| a != b) {
            return Ok(((seen + at) as u64, Some(chunk.slice(at..))));
        }
        seen += theirs.len();
        if theirs.len() < chunk.len() {
            return Ok((seen as u64, Some(chunk.slice(theirs.len()..))));
        }
    }
    Ok((seen as u64, None))
}
