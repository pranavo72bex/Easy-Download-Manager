// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished torrent's files put in place: its folder named, and its tree moved, or copied across
//! volumes.

use super::*;

/// Moves what a torrent produced into the download folder.
///
/// One file is placed exactly as any other download is — sorted into its
/// category folder and checked against the digest it was given, by the same
/// code. A tree of files keeps the structure the torrent names, under the
/// download folder itself: category sorting is for one file with one kind.
/// A digest cannot be checked over a tree, and a row that carries one is
/// failed rather than completed with the digest quietly unchecked.
pub(super) async fn place(
    staged: &Path,
    name: &str,
    options: &Options,
) -> Result<(PathBuf, category::Category, bool, Option<String>), Error> {
    if staged.is_file() {
        // Under the file's own name, not the torrent's: the two differ when
        // a torrent of one file is named for its release.
        let file_name = staged
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string());
        let (path, _, category) =
            crate::download::finish(&file_name, None, &[staged.to_path_buf()], options).await?;
        return Ok((path, category, options.checksum.is_some(), None));
    }
    if options.checksum.is_some() {
        return Err(Error::Internal(
            "a checksum cannot be checked over a torrent of several files".into(),
        ));
    }
    options.control.check()?;
    tokio::fs::create_dir_all(&options.dest_dir)
        .await
        .map_err(Error::Disk)?;
    let folder = folder_name(name);
    let dest = crate::download::claim_dir(&options.dest_dir, &folder).await?;
    match move_tree(staged, &dest).await {
        Ok(left_behind) => Ok((dest, category::classify(&folder, None), false, left_behind)),
        Err(e) => {
            // Only the empty folder this claimed; never anything with files in.
            let _ = tokio::fs::remove_dir(&dest).await;
            Err(e)
        }
    }
}

/// The folder a torrent of several files lands in, from the name the
/// torrent gives itself — which is the torrent's to say, and so cleaned as
/// any remote name is: `../.config/autostart` must not leave the download
/// folder, nor arrive hidden.
pub(super) fn folder_name(name: &str) -> String {
    let cleaned = crate::probe::sanitize(name);
    if cleaned.is_empty() {
        "torrent".into()
    } else {
        cleaned
    }
}

/// Moves a directory: a rename where the two are on one filesystem, a copy
/// and a removal where they are not. What it says, when it says anything,
/// is a copy left behind that could not be removed: the files are in place,
/// so that is a problem to note, not a failed download.
async fn move_tree(from: &Path, to: &Path) -> Result<Option<String>, Error> {
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(None);
    }
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_across(&from, &to))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?
        .map_err(Error::Disk)
}

/// A move between disks. Copied straight into `to`, a copy that
/// failed midway left half a tree in the download folder looking finished;
/// and a copy that succeeded but whose source could not be removed failed
/// the row beside a whole copy, so Retry fetched it again as "name (1)".
/// The copy goes to a hidden name beside `to` and is renamed into place
/// only when whole; it is removed if it fails.
pub(super) fn copy_across(from: &Path, to: &Path) -> std::io::Result<Option<String>> {
    let parent = to.parent().unwrap_or(Path::new("."));
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = parent.join(format!(".{name}.edm-moving"));
    // One left by a move that was interrupted is no use to anyone.
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = copy_tree(from, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    // Onto the empty folder claimed for it: Unix replaces an empty folder,
    // Windows does not, so that goes first there.
    if std::fs::rename(&staging, to).is_err() {
        let _ = std::fs::remove_dir(to);
        if let Err(e) = std::fs::rename(&staging, to) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    }
    Ok(std::fs::remove_dir_all(from).err().map(|e| {
        format!(
            "{} is complete, but the copy it was moved from could not be removed from {}: {e}",
            to.display(),
            from.display()
        )
    }))
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
