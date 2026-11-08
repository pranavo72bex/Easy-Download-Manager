// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The watched folder (Options › Downloads): a link saved where dm is told to
//! look is added, then the file is moved into a `done` folder inside it, so it
//! is not added twice and nothing is deleted.
//!
//! Files are read as a drop is (`links::links_at`): the first 64 KB of a text
//! kind, or a `.torrent` by its path. A file is never run, and one still being
//! written is left for the next look. The application reads the folder and
//! moves what it read; the presentation adds the links it is handed, so they
//! go in as its own adds do.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dm_core::Entry;

use super::backend::Backend;
use super::links::{links_at, DROP_KINDS};

/// How often the folder is read.
pub const WATCH_EVERY: Duration = Duration::from_secs(5);
/// A file touched more recently than this is still being written.
const SETTLED: Duration = Duration::from_secs(2);
/// Where handled files go, inside the watched folder.
const DONE: &str = "done";

/// What the watched folder remembers between looks.
#[derive(Debug, Default)]
pub struct Watched {
    /// When the folder was last read.
    at: Option<Instant>,
    /// Files that could not be read or moved: not read again until dm
    /// restarts, so one file does not repeat its complaint every look.
    skip: HashSet<PathBuf>,
}

impl Watched {
    /// Reads `folder` if a look is due, and moves each file it read into the
    /// `done` folder. Returns, for each file read, its links not already in
    /// `rows` (nor in a file read before it in this look), for the
    /// presentation to add. What could not be read or moved is noted on
    /// `backend`'s problems.
    pub fn look(
        &mut self,
        folder: &str,
        rows: &[Arc<Entry>],
        backend: &Backend,
    ) -> Vec<Vec<String>> {
        let folder = folder.trim();
        if folder.is_empty() {
            self.skip.clear();
            return Vec::new();
        }
        if self.at.is_some_and(|at| at.elapsed() < WATCH_EVERY) {
            return Vec::new();
        }
        self.at = Some(Instant::now());
        let dir = PathBuf::from(folder);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| watched_kind(p))
            .collect();
        files.sort();
        let mut taken: HashSet<String> = HashSet::new();
        let mut to_add = Vec::new();
        for path in files {
            if self.skip.contains(&path) {
                continue;
            }
            // A plain file only: not a link to somewhere else, not a folder.
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if !meta.is_file() || still_being_written(&meta) {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut links = links_at(&path, || None);
            if links.is_empty() {
                self.skip.insert(path);
                backend.note(format!(
                    "the watched folder has {name}, with no link in it; it is left where it is"
                ));
                continue;
            }
            // A link already in the list is not added a second time, nor one
            // a file read before this one in the same look already gave.
            links.retain(|url| {
                super::rules::already_listed(rows, url).is_none() && taken.insert(url.clone())
            });
            if !links.is_empty() {
                to_add.push(links);
            }
            if let Err(why) = move_aside(&dir, &path, &name) {
                self.skip.insert(path);
                backend.note(format!(
                    "{name} in the watched folder was read but could not be moved to its {DONE} folder ({why}); it will not be read again until dm restarts"
                ));
            }
        }
        to_add
    }
}

/// The kinds of file dm reads in the watched folder: the ones a drop reads.
fn watched_kind(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'));
    let kind = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    !hidden
        && kind
            .as_deref()
            .is_some_and(|k| k == "torrent" || DROP_KINDS.contains(&k))
}

fn still_being_written(meta: &std::fs::Metadata) -> bool {
    meta.modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < SETTLED)
}

/// Moves a handled file into `done`, under a free name.
fn move_aside(dir: &Path, path: &Path, name: &str) -> std::io::Result<()> {
    let done = dir.join(DONE);
    std::fs::create_dir_all(&done)?;
    let target = done.join(dm_core::free_name(&done, name));
    std::fs::rename(path, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file read is moved aside; a link already listed, or one an earlier
    /// file of the same look gave, is not handed over twice; a hidden file or
    /// another kind is left alone.
    #[test]
    fn the_folder_is_read_once_each_link_is_handed_over_once_and_files_are_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(60);
        let write = |name: &str, text: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        };
        write("a.txt", "https://h.example/one https://h.example/listed");
        write("b.txt", "https://h.example/one https://h.example/two");
        write(".hidden.txt", "https://h.example/hidden");
        write("c.pdf", "https://h.example/pdf");
        let rows: Vec<Arc<Entry>> = vec![Arc::new(
            serde_json::from_value(serde_json::json!({
                "id": 1, "url": "https://h.example/listed",
                "state": serde_json::to_value(dm_core::State::Paused).unwrap(),
                "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
                "filename": "f",
            }))
            .unwrap(),
        )];
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let state = tempfile::tempdir().unwrap();
        let backend = crate::app::backend::testing::backend(&runtime, state.path());
        let mut watched = Watched::default();
        let handed = watched.look(&dir.path().display().to_string(), &rows, &backend);
        assert_eq!(
            handed,
            vec![
                vec!["https://h.example/one".to_string()],
                vec!["https://h.example/two".to_string()]
            ]
        );
        assert!(dir.path().join(DONE).join("a.txt").is_file());
        assert!(dir.path().join(DONE).join("b.txt").is_file());
        assert!(dir.path().join(".hidden.txt").is_file() && dir.path().join("c.pdf").is_file());
        // Not due again at once.
        write("d.txt", "https://h.example/three");
        assert!(watched
            .look(&dir.path().display().to_string(), &rows, &backend)
            .is_empty());
    }
}
