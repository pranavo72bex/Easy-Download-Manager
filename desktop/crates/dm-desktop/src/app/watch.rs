//! The watched folder (Options › Downloads): a link saved where dm is told
//! to look is added, then the file is moved into a `done` folder inside it,
//! so it is not added twice and nothing is deleted.
//!
//! Files are read as a drop is (`dropped_links`): the first 64 KB of a text
//! kind, or a `.torrent` by its path. A file is never run, and one still
//! being written is left for the next look.

use super::*;

/// How often the folder is read.
pub(super) const WATCH_EVERY: Duration = Duration::from_secs(5);
/// A file touched more recently than this is still being written.
const SETTLED: Duration = Duration::from_secs(2);
/// Where handled files go, inside the watched folder.
const DONE: &str = "done";

impl DmApp {
    pub(super) fn take_watched(&mut self) {
        let folder = self.applied.watch_folder.trim().to_string();
        if folder.is_empty() {
            self.watch_skip.clear();
            return;
        }
        if self.watched_at.is_some_and(|at| at.elapsed() < WATCH_EVERY) {
            return;
        }
        self.watched_at = Some(Instant::now());
        let dir = std::path::PathBuf::from(&folder);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| watched_kind(p))
            .collect();
        files.sort();
        for path in files {
            if self.watch_skip.contains(&path) {
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
            let mut links = dropped_links(&crate::drops::DroppedPath(path.clone()));
            if links.is_empty() {
                self.watch_skip.insert(path);
                self.backend.note(format!(
                    "the watched folder has {name}, with no link in it; it is left where it is"
                ));
                continue;
            }
            // A link already in the list is not added a second time.
            links.retain(|url| crate::dialogs::already_listed(self, url).is_none());
            if !links.is_empty() {
                let _ = self.add(&links, Default::default());
            }
            if let Err(why) = move_aside(&dir, &path, &name) {
                self.watch_skip.insert(path);
                self.backend.note(format!(
                    "{name} in the watched folder was read but could not be moved to its {DONE} folder ({why}); it will not be read again until dm restarts"
                ));
            }
        }
    }
}

/// The kinds of file dm reads in the watched folder: the ones a drop reads.
fn watched_kind(path: &std::path::Path) -> bool {
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
fn move_aside(dir: &std::path::Path, path: &std::path::Path, name: &str) -> std::io::Result<()> {
    let done = dir.join(DONE);
    std::fs::create_dir_all(&done)?;
    let target = done.join(dm_core::download::free_name(&done, name));
    std::fs::rename(path, target)
}
