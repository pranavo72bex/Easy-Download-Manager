// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download's files on disk: room checked, folders moved or copied, names made free, the bytes
//! staged and the preview of one being fetched.

use super::*;

/// Why a download stopped for want of room or of its folder, where that is
/// why: a full disk (as the system says it, or EDM's own check before it
/// started), a folder it may not write to, or one that can no longer be
/// made, as when its drive is taken out. `None` for any other failure.
pub(super) fn out_of_room(error: &Error, dirs: [&std::path::Path; 2]) -> Option<String> {
    let text = error.to_string();
    // A full disk as the system says it, or EDM's own look for room and at the download folder
    // before a byte is fetched: by the kind the error carries.
    let full = matches!(
        error.fail_kind(),
        crate::FailKind::DiskFull | crate::FailKind::NoRoom
    ) || {
        // The system's own words for a full disk, where its error reached EDM inside another
        // sentence (a copy's, a join's) and so carries no kind: text from outside EDM.
        let lower = text.to_ascii_lowercase();
        ["no space left", "disk full", "disk is full"]
            .iter()
            .any(|w| lower.contains(w))
    };
    if full {
        return Some(text);
    }
    if !matches!(error, Error::Disk(_)) {
        return None;
    }
    dirs.iter()
        .find(|dir| !dir.as_os_str().is_empty() && std::fs::create_dir_all(dir).is_err())
        .map(|dir| {
            format!(
                "the folder {} cannot be reached (was its drive taken out?)",
                dir.display()
            )
        })
}

/// Where a row's unfinished parts go in a folder a person chose for it.
pub const UNFINISHED: &str = ".edm-unfinished";

/// Moves a folder, across disks if it has to: copied whole, then the
/// original removed.
pub(super) async fn move_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(());
    }
    copy_dir(from, to).await?;
    let _ = tokio::fs::remove_dir_all(from).await;
    Ok(())
}

pub(super) fn copy_dir<'a>(
    from: &'a std::path::Path,
    to: &'a std::path::Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(to).await?;
        let mut listing = tokio::fs::read_dir(from).await?;
        while let Some(entry) = listing.next_entry().await? {
            let target = to.join(entry.file_name());
            if entry.file_type().await?.is_dir() {
                copy_dir(&entry.path(), &target).await?;
            } else {
                tokio::fs::copy(entry.path(), &target).await?;
            }
        }
        Ok(())
    })
}

/// Moves `from` to `name` beside it, under a free name if that one is taken.
pub(super) async fn rename_free(from: &std::path::Path, name: &str) -> std::io::Result<PathBuf> {
    let name = crate::probe::sanitize(name);
    if name.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "that is not a name a file can have",
        ));
    }
    let dir = from.parent().unwrap_or(std::path::Path::new("."));
    // Placed under the chosen name already, or the free one beside it
    // where that was taken: renamed again, "name (1)" became "name (2)".
    if crate::download::candidates(dir, &name).any(|c| c == from) {
        return Ok(from.to_path_buf());
    }
    if let Some(to) = rename_case_only(from, dir, &name).await? {
        return Ok(to);
    }
    // Claimed by creating it, so nothing else can take the name between
    // the choice and the move; the move then replaces that empty claim.
    let to = crate::download::claim_path(dir, &name)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    if let Err(e) = tokio::fs::rename(from, &to).await {
        let _ = tokio::fs::remove_file(&to).await;
        return Err(e);
    }
    Ok(to)
}

/// A rename that changes only the case of a name ("Report.pdf" to
/// "report.pdf"), done in place. On a case-insensitive disk (macOS's and
/// Windows' by default) the new name is "taken" by the file itself, so the
/// free-name rule gave "report (1).pdf". `None` when this is not such a
/// rename, or the disk holds the new name as another file.
pub(super) async fn rename_case_only(
    from: &std::path::Path,
    dir: &std::path::Path,
    name: &str,
) -> std::io::Result<Option<PathBuf>> {
    let Some(old) = from.file_name().map(|f| f.to_string_lossy().into_owned()) else {
        return Ok(None);
    };
    if old.to_lowercase() != name.to_lowercase() {
        return Ok(None);
    }
    let to = dir.join(name);
    if tokio::fs::symlink_metadata(&to).await.is_err() {
        // Nothing answers to the new name: an ordinary rename will do.
        return Ok(None);
    }
    // Something answers to it. If no entry is spelt that way, it is this
    // file found by a case-insensitive disk; otherwise it is another file.
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name().to_string_lossy() == name {
            return Ok(None);
        }
    }
    // Through a name of its own: a disk that ignores case may also ignore
    // a rename to what it takes for the same name.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let between = dir.join(format!(".edm-rename-{}-{nanos}", std::process::id()));
    tokio::fs::rename(from, &between).await?;
    if let Err(e) = tokio::fs::rename(&between, &to).await {
        let _ = tokio::fs::rename(&between, from).await;
        return Err(e);
    }
    Ok(Some(to))
}

/// Whether the server's file is no longer the one a finished row fetched.
/// Only what both sides know is compared; nothing known is no change.
pub fn differs(row: &Entry, now: &crate::Probe) -> bool {
    let validator = matches!((&row.validator, &now.validator), (Some(a), Some(b)) if a != b);
    let length = matches!((row.progress.total, now.size), (Some(a), Some(b)) if a != b);
    validator || length
}

/// What an earlier run left under `dir` that the download limit counted
/// then: the bytes of its part files; none for a folder that is empty or
/// missing; and `None` for one holding only another route's files (a
/// torrent's pieces), whose own first report says how
/// much of them there is.
pub(super) async fn staged_bytes(dir: &std::path::Path) -> Option<u64> {
    let Ok(mut listing) = tokio::fs::read_dir(dir).await else {
        return Some(0);
    };
    let (mut parts, mut total, mut others) = (0, 0, 0);
    while let Ok(Some(entry)) = listing.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("part-") {
            parts += 1;
            total += entry.metadata().await.map(|m| m.len()).unwrap_or(0);
        } else if let Some(done) = name.strip_prefix("done-") {
            // A range's ledger: what is on disk of it in the staged file.
            parts += 1;
            if let Ok(index) = done.parse::<usize>() {
                total += crate::download::read_ledger(dir, index).unwrap_or(0);
            }
        } else if name != crate::paths::PLAN
            && name != crate::download::STAGED
            // A range's own file: its ledger above says what of it is on disk.
            && !name.starts_with(crate::download::RANGE)
            && !name.starts_with(PREVIEW)
        {
            others += 1;
        }
    }
    (parts > 0 || others == 0).then_some(total)
}

/// The largest file a torrent being fetched has under `parts`, in its own folders: none of
/// EDM's own (its plan, ledgers, range files, staged file, a recording or a preview made before).
fn torrent_file_in(parts: &std::path::Path) -> Option<PathBuf> {
    fn ours(name: &str) -> bool {
        name == crate::paths::PLAN
            || name == crate::download::STAGED
            || name == "live.recording"
            || name.starts_with(crate::download::RANGE)
            || name.starts_with("done-")
            || name.starts_with("part-")
            || name.starts_with(PREVIEW)
            || name.starts_with('.')
    }
    fn walk(dir: &std::path::Path, depth: usize, best: &mut Option<(u64, PathBuf)>) {
        let Ok(listing) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in listing.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if ours(&name) {
                continue;
            }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() && depth < 8 {
                walk(&path, depth + 1, best);
            } else if meta.is_file()
                && meta.len() > 0
                && best.as_ref().is_none_or(|(len, _)| meta.len() > *len)
            {
                *best = Some((meta.len(), path));
            }
        }
    }
    let mut best = None;
    walk(parts, 0, &mut best);
    best.map(|(_, path)| path)
}

/// The file a player is given for [`DownloadList::preview`]: what is fetched of
/// `filename` under `parts`, under a name that ends as `filename` does.
pub(super) fn preview_of(parts: &std::path::Path, filename: &str) -> Result<PathBuf, String> {
    // The staged file of a ranged download, the first range's own file of
    // one going to another disk (which holds the file's beginning), a
    // stream's recording, or the one part of a download over one connection.
    let first_range = crate::download::range_path(parts, 0);
    let source = [
        parts.join(crate::download::STAGED),
        first_range,
        parts.join("live.recording"),
        parts.join("part-0000"),
    ]
    .into_iter()
    .find(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
    // A torrent's files are under its own names: the largest, which its pieces fetched first
    // and last then in order make playable from its start as it comes (gaps row C1).
    .or_else(|| torrent_file_in(parts))
    .ok_or("nothing of it is on disk to preview yet")?;
    // The file's own ending where it has one (a torrent's), else the row's.
    let extension = source
        .extension()
        .filter(|_| !source.starts_with(parts.join(crate::download::STAGED)))
        .filter(|e| !e.is_empty())
        .or_else(|| std::path::Path::new(filename).extension())
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let to = parts.join(if extension.is_empty() {
        "preview".to_string()
    } else {
        format!("{PREVIEW}.{extension}")
    });
    let _ = std::fs::remove_file(&to);
    if std::fs::hard_link(&source, &to).is_err() {
        std::fs::copy(&source, &to).map_err(|e| format!("could not make a preview: {e}"))?;
    }
    Ok(to)
}
