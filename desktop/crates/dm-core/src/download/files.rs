// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// Where one segment of a stream is staged, and where an earlier dm staged
/// one range of a download (`adopt_part`).
///
/// The name is a contract across runs — a part file the next run cannot find
/// is a range silently fetched again — so it is derived in one place rather
/// than re-typed wherever a part is opened.
pub(super) fn part_path(parts_dir: &Path, index: usize) -> PathBuf {
    parts_dir.join(format!("part-{index:04}"))
}

/// The file a ranged download is written into, every range at its own
/// offset.
///
/// Ranges used to go into a part file each, and the parts were copied into
/// the finished file at the end: every byte written twice, and on a fast
/// link the copy took longer than the fetch (2 GB: 4.4 s fetched, then 7 to
/// 9 s joining, `qa/baseline.md`). Written in place, finishing is a rename.
pub(crate) const STAGED: &str = "staged";

pub(crate) fn staged_path(parts_dir: &Path) -> PathBuf {
    parts_dir.join(STAGED)
}

/// What the file of one range is called, before its index, where each range is written into a
/// plain file of its own ([`range_path`]).
pub(crate) const RANGE: &str = "range-";

/// The file range `index` is written into, front to back, where the download folder is on
/// another disk than the parts folder, and the ranges are appended into the finished file in file
/// order at the end (reference A's layout). The finish there is a copy whichever way the bytes are
/// staged, and on Windows one sparse staged file wrote and copied slower than plain files: in a
/// probe of eight threads writing 16 KiB at a time, 512 MiB took 1.8 to 1.95 s into one sparse file
/// against 0.42 s into eight plain ones, and 1.6 s against 1.1 s to copy (gaps row 166; the
/// maintainer's choice of 2026-11-10). On one disk the staged file stays, finished by a rename.
pub(crate) fn range_path(parts_dir: &Path, index: usize) -> PathBuf {
    parts_dir.join(format!("{RANGE}{index:04}"))
}

/// Whether a download's ranges go into a plain file each ([`range_path`]) rather than into one
/// staged file: only one of known length fetched by ranges, whose download folder is on another
/// disk than its parts folder. What an earlier run left decides first, so a resume goes on in the
/// layout its bytes are in: a staged file, or an earlier dm's part files, keep the staged file.
/// `DM_STAGE_BY_RANGE` set to `1` or `0` decides instead, for tests and for measuring the two.
pub(crate) fn stage_by_range(parts_dir: &Path, dest_dir: &Path, by_ranges: bool) -> bool {
    let mut ranges = false;
    if let Ok(listing) = std::fs::read_dir(parts_dir) {
        for entry in listing.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == STAGED || name.starts_with("part-") {
                return false;
            }
            ranges |= name.starts_with(RANGE);
        }
    }
    if ranges {
        return by_ranges;
    }
    match std::env::var("DM_STAGE_BY_RANGE").as_deref() {
        Ok("1") => by_ranges,
        Ok("0") => false,
        _ => by_ranges && !super::space::same_disk(parts_dir, dest_dir),
    }
}

/// `file` made sparse on Windows, so it takes its room only as bytes land,
/// as files on other systems do. A staged file is made as long as the
/// download first (`ranged::preallocate`); NTFS then wrote zeros over
/// everything before the first byte a range wrote far into it, so a 4.5 GiB
/// file's last range waited 95 s on this computer's disk (2026-11-07) and dm
/// took twice reference A's time (gaps row 158). A file system without sparse
/// files (FAT) says no, and the file is as it was before.
#[cfg(windows)]
pub(crate) fn sparse(file: &impl std::os::windows::io::AsRawHandle) {
    let mut returned = 0u32;
    // SAFETY: the handle is `file`'s own and open for the call; FSCTL_SET_SPARSE
    // with no input buffer sets the flag, and no output is asked for.
    unsafe {
        windows_sys::Win32::System::IO::DeviceIoControl(
            file.as_raw_handle(),
            windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(test)]
mod tests;

/// The ledger of range `index`: how many of its bytes are on disk, in
/// decimal. A part file's length used to be this number; in one shared
/// file, where every range's region exists from the start, it is written
/// down instead.
pub(crate) fn ledger_path(parts_dir: &Path, index: usize) -> PathBuf {
    parts_dir.join(format!("done-{index:04}"))
}

/// Writes down how much of a range is on disk. Behind the bytes, never ahead
/// of them: written after they are handed to the disk, and one that lags
/// costs only the bytes fetched again. Always as many digits, written over
/// the last in one write, so a shorter number never leaves a longer one's
/// tail behind it (which would read as more than is there).
pub(super) async fn put(ledger: &mut fs::File, done: u64) -> Result<(), Error> {
    use tokio::io::AsyncWriteExt;
    ledger.seek(std::io::SeekFrom::Start(0)).await?;
    ledger.write_all(format!("{done:020}").as_bytes()).await?;
    ledger.flush().await?;
    Ok(())
}

/// What range `index`'s ledger says, if it has one that reads. One cut
/// short by a crash reads as nothing, which is safe: a ledger behind the
/// bytes costs a fetch, never a wrong file.
pub(crate) fn read_ledger(parts_dir: &Path, index: usize) -> Option<u64> {
    std::fs::read_to_string(ledger_path(parts_dir, index))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The index a staged file's name gives it: `part-0003`, `range-0003` or `done-0003`.
pub(super) fn staged_index(name: &str) -> Option<usize> {
    name.strip_prefix("part-")
        .or_else(|| name.strip_prefix(RANGE))
        .or_else(|| name.strip_prefix("done-"))
        .and_then(|n| n.parse().ok())
}

/// A part file an earlier dm left, copied into the staged file at its
/// range's offset and written down in the range's ledger: a download begun
/// before ranges were written in place resumes rather than starting again.
pub(super) fn adopt_part(
    parts_dir: &Path,
    index: usize,
    start: u64,
    span: u64,
) -> std::io::Result<u64> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let part = part_path(parts_dir, index);
    let mut source = std::fs::File::open(&part)?;
    let mut staged = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(staged_path(parts_dir))?;
    staged.seek(SeekFrom::Start(start))?;
    let copied = std::io::copy(&mut (&mut source).take(span), &mut staged)?;
    staged.flush()?;
    std::fs::write(ledger_path(parts_dir, index), format!("{copied:020}"))?;
    drop(source);
    std::fs::remove_file(&part)?;
    Ok(copied)
}

/// Whether files can be made in the download folder, said with its path.
///
/// The folder was first written to when the finished file was placed, so a
/// read-only folder — a removed disk, a permission changed, a sandbox — cost
/// the whole transfer and then said "disk: Permission denied", naming
/// neither the folder nor what to do about it.
pub(super) async fn writable(dir: &Path) -> Result<(), Error> {
    let said = |e: std::io::Error| {
        Error::Internal(format!(
            "the folder {} cannot be written to ({e}); choose another for this download \
             (Move/Rename) or for every download (Options › Folders)",
            dir.display()
        ))
    };
    fs::create_dir_all(dir).await.map_err(said)?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let probe = dir.join(format!(".dm-write-check-{}-{n}", std::process::id()));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .await
        .map_err(said)?;
    let _ = fs::remove_file(&probe).await;
    Ok(())
}

/// `name`, then `name (1)`, `name (2)`… — the paths a file may be saved at.
pub(crate) fn candidates<'a>(dir: &'a Path, name: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    std::iter::once(dir.join(name))
        .chain((1..10_000).map(move |n| dir.join(format!("{stem} ({n}){ext}"))))
}

/// The name a file called `name` would be saved under in `dir` as things
/// stand: `name` if nothing has it, else the first `name (n)` free. A
/// guess for showing — [`claim_path`] decides when the file is placed.
pub fn free_name(dir: &Path, name: &str) -> String {
    candidates(dir, name)
        .find(|path| std::fs::symlink_metadata(path).is_err())
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| name.to_string())
}

/// The name the `nth` download of `name` into `dir` will get, counting from
/// 0, where `nth` others of that name are already waiting to be placed
/// there: each takes the next free one in turn.
pub fn nth_free_name(dir: &Path, name: &str, nth: usize) -> String {
    candidates(dir, name)
        .filter(|path| std::fs::symlink_metadata(path).is_err())
        .nth(nth)
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| name.to_string())
}

pub(super) fn all_taken(dir: &Path, name: &str) -> Error {
    Error::Internal(format!(
        "ten thousand files named {name} already in {}; nothing will be overwritten",
        dir.display()
    ))
}

/// A folder nothing is using, taken by creating it empty — for a torrent of
/// several files, which arrives as a tree. The same race as [`claim_path`]:
/// a look and then a move could land on a folder made in between.
///
/// An existing file is somebody's, and a download manager that overwrites one
/// is worse than one that fails — so past ten thousand of the same name it
/// fails, rather than falling back to the one path it knows is taken.
pub(crate) async fn claim_dir(dir: &Path, name: &str) -> Result<PathBuf, Error> {
    for candidate in candidates(dir, name) {
        match fs::create_dir(&candidate).await {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(all_taken(dir, name))
}

/// A path nothing is using, taken by creating it empty.
///
/// Looking and then writing let two downloads finishing at once with the
/// same name both choose `f.bin`, and the second rename replaced the first;
/// a file of the person's own made in that moment went the same way.
/// `create_new` asks the filesystem, which answers one of them only.
pub(crate) async fn claim_path(dir: &Path, name: &str) -> Result<PathBuf, Error> {
    for candidate in candidates(dir, name) {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .await
        {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(all_taken(dir, name))
}
