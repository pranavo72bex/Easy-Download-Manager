// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Files written whole or not at all, and paths as a person reads them.

/// Writes a file whole, or not at all.
///
/// Written to a sibling and moved, because the files dm keeps — the queue,
/// the settings, a request handed over by a browser and the rest — are read
/// back by something that must never see half of one.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, |w| w.write_all(bytes), true)
}

/// As [`write_atomic`], for a file produced piece by piece: nothing the size
/// of the whole file is held in memory.
pub fn write_atomic_with(
    path: &std::path::Path,
    fill: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
) -> std::io::Result<()> {
    replace(path, fill, true)
}

/// As [`write_atomic`], without waiting for the disk: for a file whose
/// stale or empty copy after a power cut costs time but not correctness.
pub fn write_atomic_unsynced(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, |w| w.write_all(bytes), false)
}

pub(crate) fn replace(
    path: &std::path::Path,
    fill: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
    durable: bool,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A sibling of its own per write: two saves at once shared one `.tmp`,
    // and whichever renamed second found it gone and reported a failure
    // that had not happened.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{n}.tmp", std::process::id()));
    let temp = path.with_file_name(name);
    /* On disk before it takes the name. Renamed with its bytes still in the
     * page cache, a power cut could leave the name on an empty or zeroed
     * file: the queue then unreadable, and every download in it forgotten. */
    let written = (|| {
        use std::io::Write;
        // Readable by this account alone: dm's files hold a proxy's
        // password, site logins and a site's handed-over session.
        let mut open = std::fs::OpenOptions::new();
        open.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
        let mut file = open.open(&temp)?;
        let mut buffered = std::io::BufWriter::new(&mut file);
        fill(&mut buffered)?;
        buffered.flush()?;
        drop(buffered);
        if durable {
            file.sync_all()?;
        }
        Ok(())
    })();
    if let Err(e) = written.and_then(|()| std::fs::rename(&temp, path)) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    // And the rename itself, which lives in the directory. Best effort:
    // not every system lets a directory be opened to sync it.
    #[cfg(unix)]
    if let Some(dir) = path
        .parent()
        .filter(|_| durable)
        .and_then(|p| std::fs::File::open(p).ok())
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// A path as Windows programs read it: `canonicalize` there gives the
/// `\\?\C:\…` form, which a browser registered with it — and `cmd`, which
/// starts the host — can mishandle. A UNC share keeps its form.
pub fn plain_path(path: std::path::PathBuf) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => std::path::PathBuf::from(rest),
        _ => path,
    }
}
