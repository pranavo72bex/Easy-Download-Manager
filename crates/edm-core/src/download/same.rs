// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The same file from another address (gaps row W18): once the server has said the file's
//! name and size (and its entity tag or digest, where it gives one), and before its body is
//! read, a finished download of that file in the list, or a file of that name and size in the
//! folder it would be saved in, stops it, so the window can ask whether to fetch it again.

use std::path::{Path, PathBuf};

use crate::probe::Probe;
use crate::{category, Error, FailKind};

use super::Options;

/// What a stopped download says, where it was the same file as one already here (gaps row
/// W18). The window asks about it in words of its own.
pub const SAME_FILE: &str = "a file of the same name and size is already in the list or in the \
                             folder, so it was not fetched again";

/// A finished download's file, which a download of the same file is compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameFile {
    /// Its name.
    pub name: String,
    /// Its size, in bytes.
    pub size: u64,
    /// The server's ETag or Last-Modified for it, where one was given.
    pub validator: Option<String>,
    /// The digest it was to have, where one was known.
    pub digest: Option<crate::Checksum>,
    /// Where it is.
    pub path: PathBuf,
}

/// What a download is compared with before its body is fetched (`Options::same_files`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SameFiles {
    /// The finished downloads' files, but those of the download's own address.
    pub finished: Vec<SameFile>,
    /// Files a person already chose to fetch again by their address ("Already in the list",
    /// contract row B17): not asked about a second time.
    pub passed_over: Vec<PathBuf>,
}

/// Whether two of the server's words for a file agree: either is missing, or both are the
/// same.
fn agree<T: PartialEq>(a: Option<&T>, b: Option<&T>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Whether two digests agree: either is missing, they are of different hashes (nothing to
/// compare), or they are the same.
fn digests_agree(a: Option<&crate::Checksum>, b: Option<&crate::Checksum>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) if a.algorithm == b.algorithm => a.expected == b.expected,
        _ => true,
    }
}

/// Whether `path` is a file of `size` bytes.
async fn file_of(path: &Path, size: u64) -> bool {
    match tokio::fs::metadata(path).await {
        Ok(meta) => meta.is_file() && meta.len() == size,
        Err(_) => false,
    }
}

/// Fails with [`FailKind::SameFile`] where `info` is the same file as a finished download
/// (`options.same_files`) or a file already in the folder it would go to; nothing is asked
/// where `options.same_files` is `None` (no one to ask, or a person said Download again).
pub(super) async fn already_here(info: &Probe, options: &Options) -> Result<(), Error> {
    let Some(known) = options.same_files.as_deref() else {
        return Ok(());
    };
    let Some(size) = info.size.filter(|size| *size > 0) else {
        return Ok(());
    };
    // The name it would be saved under: a person's own, else the server's.
    let name = options
        .save_name
        .as_deref()
        .map(crate::probe::sanitize)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| info.filename.clone());
    if name.trim().is_empty() {
        return Ok(());
    }
    let same = Err(Error::known(FailKind::SameFile, SAME_FILE));
    for file in &known.finished {
        if file.name == name
            && file.size == size
            && agree(file.validator.as_ref(), info.validator.as_ref())
            && digests_agree(file.digest.as_ref(), info.digest.as_ref())
            && file_of(&file.path, size).await
        {
            return same;
        }
    }
    let kind = category::classify(&name, info.content_type.as_deref());
    let path = options
        .categories
        .place(&options.dest_dir, kind, &name)
        .join(&name);
    if !known.passed_over.contains(&path) && file_of(&path, size).await {
        return same;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
