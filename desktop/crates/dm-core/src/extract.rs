//! Extracting a finished ZIP into a folder of its own.
//!
//! An archive is outside input, so it is read before it is trusted: every
//! entry is checked first, and nothing is written unless all of them pass.
//! An entry that would land outside the folder (`..`, an absolute or drive
//! path, a backslash trick), a symbolic link, a device file, a password-
//! protected entry, two names that are one on a case-folding disk, or totals
//! past the limits fail the whole archive. Nothing is repaired, nothing
//! extracted is opened or run, and the archive is left where it was.
//!
//! The destination is always a new folder beside the archive (`name`, then
//! `name (1)`…), so no file already on disk is ever replaced. Declared
//! sizes are not believed: the bytes actually written are counted against
//! the limits as well. A failure or a cancel removes the folder it made.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// How much an archive may ask for.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Bytes of all files together, once extracted.
    pub max_bytes: u64,
    /// Files and folders together.
    pub max_entries: usize,
    /// Extracted size over stored size, for an archive past `ratio_floor`
    /// bytes (a small file may compress very well without being a bomb).
    pub max_ratio: u64,
    pub ratio_floor: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: 64 << 30,
            max_entries: 200_000,
            max_ratio: 1000,
            ratio_floor: 1 << 30,
        }
    }
}

/// What an extraction made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub folder: PathBuf,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this is not a ZIP archive that can be read: {0}")]
    Unreadable(String),
    #[error("the archive has an entry that would leave its folder: {0}")]
    Escapes(String),
    #[error("the archive holds a link or special file, which dm does not extract: {0}")]
    Special(String),
    #[error("the archive is password-protected, which dm does not extract: {0}")]
    Encrypted(String),
    #[error("the archive lists the same name twice: {0}")]
    Duplicate(String),
    #[error("the archive is larger than dm will extract ({0})")]
    TooBig(&'static str),
    #[error("the archive was stopped")]
    Cancelled,
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

/// The folder an archive extracts into: its name without `.zip`, made
/// unique beside it.
pub fn destination_for(archive: &Path) -> PathBuf {
    destination_in(archive, archive.parent().unwrap_or_else(|| Path::new(".")))
}

/// [`destination_for`] inside a chosen `parent` folder instead.
pub fn destination_in(archive: &Path, parent: &Path) -> PathBuf {
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty() && s != "." && s != "..")
        .unwrap_or_else(|| "extracted".into());
    let mut candidate = parent.join(&stem);
    let mut n = 1;
    while candidate.exists() {
        candidate = parent.join(format!("{stem} ({n})"));
        n += 1;
    }
    candidate
}

/// Whether a file name is one this extracts.
pub fn is_zip(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".zip")
}

/// Whether a Unix mode, if the entry has one, is a file or a folder: not a
/// link, a device, a pipe or a socket.
fn plain_mode(mode: Option<u32>) -> bool {
    matches!(mode.map_or(0, |m| m & 0o170000), 0 | 0o100000 | 0o040000)
}

/// A path inside the archive that stays inside the folder: relative, with
/// no `..`, no drive, no stream suffix, no empty or NUL-bearing part, under
/// either slash.
fn safe_path(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('\0') || raw.starts_with('/') || raw.starts_with('\\') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            p if p.contains(':') => return None,
            p => out.push(p),
        }
    }
    // Only plain parts: a prefix or root can come from nothing else, but
    // this is the check the folder's safety rests on.
    if out.as_os_str().is_empty() || out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(out)
}

/// Extracts `archive` into a new folder beside it. `progress` is told bytes
/// done and bytes expected. `origin`, a URL, is written as the Mark of the
/// Web on each file where the system has one.
pub fn extract(
    archive: &Path,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<Extracted, Error> {
    extract_in(archive, None, limits, cancel, origin, progress)
}

/// [`extract`] into a new folder inside `parent`, which must exist; `None`
/// is beside the archive.
pub fn extract_in(
    archive: &Path,
    parent: Option<&Path>,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<Extracted, Error> {
    let folder = match parent {
        Some(parent) => destination_in(archive, parent),
        None => destination_for(archive),
    };
    extract_into(archive, &folder, limits, cancel, origin, progress)
}

/// [`extract`] into `folder`, which must not exist yet.
pub fn extract_into(
    archive: &Path,
    folder: &Path,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<Extracted, Error> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Unreadable(e.to_string()))?;
    if zip.len() > limits.max_entries {
        return Err(Error::TooBig("too many entries"));
    }

    // First pass: every entry is judged before anything is written.
    let mut plan: Vec<(usize, PathBuf, bool, u64)> = Vec::with_capacity(zip.len());
    let (mut total, mut stored) = (0u64, 0u64);
    let mut seen: HashSet<String> = HashSet::new();
    for i in 0..zip.len() {
        let entry = zip
            .by_index_raw(i)
            .map_err(|e| Error::Unreadable(e.to_string()))?;
        let raw = entry.name().to_string();
        let shown = raw.chars().take(120).collect::<String>();
        let Some(path) = safe_path(&raw) else {
            return Err(Error::Escapes(shown));
        };
        // `enclosed_name` is the zip crate's own opinion; both must agree.
        if entry.enclosed_name().is_none() {
            return Err(Error::Escapes(shown));
        }
        if entry.encrypted() {
            return Err(Error::Encrypted(shown));
        }
        if entry.is_symlink() || !plain_mode(entry.unix_mode()) {
            return Err(Error::Special(shown));
        }
        let key = path.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            return Err(Error::Duplicate(shown));
        }
        let dir = entry.is_dir();
        total = total.saturating_add(entry.size());
        stored = stored.saturating_add(entry.compressed_size());
        if total > limits.max_bytes {
            return Err(Error::TooBig("too many bytes"));
        }
        plan.push((i, path, dir, entry.size()));
    }
    if total > limits.ratio_floor && total / stored.max(1) > limits.max_ratio {
        return Err(Error::TooBig("it compresses far beyond anything real"));
    }

    // A file may not also be the folder of another entry.
    let dirs: HashSet<&Path> = plan
        .iter()
        .filter(|(_, _, dir, _)| *dir)
        .map(|(_, p, _, _)| p.as_path())
        .collect();
    for (_, path, dir, _) in &plan {
        if !*dir && dirs.contains(path.as_path()) {
            return Err(Error::Duplicate(path.display().to_string()));
        }
    }

    fs::create_dir(folder)?;
    let made = write_all(
        &mut zip, folder, &plan, total, limits, cancel, origin, progress,
    );
    match made {
        Ok(files) => Ok(Extracted {
            folder: folder.to_path_buf(),
            files,
            bytes: total,
        }),
        Err(e) => {
            // The folder is one this call made, so it is this call's to remove.
            let _ = fs::remove_dir_all(folder);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_all(
    zip: &mut zip::ZipArchive<File>,
    folder: &Path,
    plan: &[(usize, PathBuf, bool, u64)],
    expected: u64,
    limits: &Limits,
    cancel: &AtomicBool,
    origin: Option<&str>,
    progress: &dyn Fn(u64, u64),
) -> Result<usize, Error> {
    let (mut done, mut files) = (0u64, 0usize);
    let mut buf = vec![0u8; 256 * 1024];
    progress(0, expected);
    for (i, path, dir, declared) in plan {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let target = folder.join(path);
        if *dir {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut entry = zip
            .by_index(*i)
            .map_err(|e| Error::Unreadable(e.to_string()))?;
        // `create_new`: never replaces, never follows a link that is there.
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let mut wrote = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let n = entry
                .read(&mut buf)
                .map_err(|e| Error::Unreadable(e.to_string()))?;
            if n == 0 {
                break;
            }
            wrote += n as u64;
            // The header's size is a claim; the bytes are the fact.
            if wrote > *declared || done + wrote > limits.max_bytes {
                return Err(Error::TooBig("an entry is larger than it said"));
            }
            out.write_all(buf.get(..n).unwrap_or_default())?;
            progress(done + wrote, expected);
        }
        out.flush()?;
        drop(out);
        done += wrote;
        files += 1;
        if let Some(url) = origin {
            crate::motw::mark(&target, url, "");
        }
    }
    Ok(files)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn build(dir: &Path, name: &str, f: impl FnOnce(&mut ZipWriter<File>)) -> PathBuf {
        let path = dir.join(name);
        let mut w = ZipWriter::new(File::create(&path).unwrap());
        f(&mut w);
        w.finish().unwrap();
        path
    }

    fn run(archive: &Path) -> Result<Extracted, Error> {
        extract(
            archive,
            &Limits::default(),
            &AtomicBool::new(false),
            None,
            &|_, _| {},
        )
    }

    fn opts() -> SimpleFileOptions {
        SimpleFileOptions::default()
    }

    #[test]
    fn a_plain_archive_extracts_into_a_folder_of_its_own() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "pics.zip", |w| {
            w.start_file("a.txt", opts()).unwrap();
            w.write_all(b"hello").unwrap();
            w.add_directory("sub/", opts()).unwrap();
            w.start_file("sub/b.txt", opts()).unwrap();
            w.write_all(b"world").unwrap();
        });
        let got = run(&a).unwrap();
        assert_eq!(got.folder, t.path().join("pics"));
        assert_eq!((got.files, got.bytes), (2, 10));
        assert_eq!(fs::read(got.folder.join("sub/b.txt")).unwrap(), b"world");
        assert!(a.exists(), "the archive is kept");
        // A second time goes beside the first, replacing nothing.
        let again = run(&a).unwrap();
        assert_eq!(again.folder, t.path().join("pics (1)"));
        assert_eq!(fs::read(got.folder.join("a.txt")).unwrap(), b"hello");
    }

    #[test]
    fn entries_that_leave_the_folder_fail_the_whole_archive_before_any_write() {
        for bad in [
            "../evil.txt",
            "a/../../evil.txt",
            "/etc/evil.txt",
            "\\evil.txt",
            "..\\evil.txt",
            "a\\..\\..\\evil.txt",
            "C:\\evil.txt",
            "c:/evil.txt",
            "file.txt:stream",
        ] {
            let t = tempfile::tempdir().unwrap();
            let a = build(t.path(), "z.zip", |w| {
                w.start_file("fine.txt", opts()).unwrap();
                w.write_all(b"x").unwrap();
                w.start_file(bad, opts()).unwrap();
                w.write_all(b"x").unwrap();
            });
            assert!(
                matches!(run(&a), Err(Error::Escapes(_))),
                "{bad} should be refused"
            );
            assert!(!t.path().join("z").exists(), "{bad}: nothing is made");
            assert!(!t.path().join("evil.txt").exists());
        }
    }

    #[test]
    fn a_symbolic_link_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.add_symlink("link", "/etc/passwd", opts()).unwrap();
        });
        assert!(matches!(run(&a), Err(Error::Special(_))));
        assert!(!t.path().join("z").exists());
    }

    #[test]
    fn only_files_and_folders_pass_the_mode_check() {
        assert!(plain_mode(None) && plain_mode(Some(0o100644)) && plain_mode(Some(0o040755)));
        for special in [0o120777, 0o020644, 0o060644, 0o010644, 0o140644] {
            assert!(!plain_mode(Some(special)), "{special:o}");
        }
    }

    #[test]
    fn the_same_name_twice_or_in_another_case_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file("A.txt", opts()).unwrap();
            w.start_file("a.TXT", opts()).unwrap();
        });
        assert!(matches!(run(&a), Err(Error::Duplicate(_))));
    }

    #[test]
    fn a_file_that_is_also_a_folder_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file("x", opts()).unwrap();
            w.add_directory("x/", opts()).unwrap();
        });
        assert!(run(&a).is_err());
        assert!(!t.path().join("z").exists());
    }

    #[test]
    fn totals_past_the_limits_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            for n in 0..5 {
                w.start_file(format!("{n}"), opts()).unwrap();
            }
        });
        let few = Limits {
            max_entries: 3,
            ..Limits::default()
        };
        let no = AtomicBool::new(false);
        assert!(matches!(
            extract(&a, &few, &no, None, &|_, _| {}),
            Err(Error::TooBig(_))
        ));
        let b = build(t.path(), "b.zip", |w| {
            w.start_file("big", opts()).unwrap();
            w.write_all(&[7u8; 4096]).unwrap();
        });
        let small = Limits {
            max_bytes: 1000,
            ..Limits::default()
        };
        assert!(matches!(
            extract(&b, &small, &no, None, &|_, _| {}),
            Err(Error::TooBig(_))
        ));
        assert!(!t.path().join("b").exists());
    }

    #[test]
    fn a_bomb_is_refused_by_its_ratio() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file(
                "zeros",
                opts().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            w.write_all(&vec![0u8; 8 << 20]).unwrap();
        });
        let strict = Limits {
            max_ratio: 100,
            ratio_floor: 1 << 20,
            ..Limits::default()
        };
        assert!(matches!(
            extract(&a, &strict, &AtomicBool::new(false), None, &|_, _| {}),
            Err(Error::TooBig(_))
        ));
        assert!(!t.path().join("z").exists());
    }

    #[test]
    fn a_truncated_or_garbage_file_is_unreadable_not_a_panic() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file("a", opts()).unwrap();
            w.write_all(&[1u8; 5000]).unwrap();
        });
        let bytes = fs::read(&a).unwrap();
        fs::write(&a, bytes.get(..bytes.len() / 2).unwrap()).unwrap();
        assert!(matches!(run(&a), Err(Error::Unreadable(_))));
        let g = t.path().join("g.zip");
        fs::write(&g, b"not a zip at all").unwrap();
        assert!(matches!(run(&g), Err(Error::Unreadable(_))));
        let e = t.path().join("e.zip");
        fs::write(&e, b"").unwrap();
        assert!(matches!(run(&e), Err(Error::Unreadable(_))));
    }

    #[test]
    fn a_corrupt_entry_midway_removes_what_was_made() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file("first", opts()).unwrap();
            w.write_all(b"fine").unwrap();
            w.start_file(
                "second",
                opts().compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
            w.write_all(&[9u8; 3000]).unwrap();
        });
        // Flip a stored byte of the second entry: its checksum fails.
        let mut bytes = fs::read(&a).unwrap();
        let at = bytes.windows(8).rposition(|w| w == [9u8; 8]).unwrap();
        *bytes.get_mut(at).unwrap() = 1;
        fs::write(&a, bytes).unwrap();
        assert!(run(&a).is_err());
        assert!(
            !t.path().join("z").exists(),
            "a failure leaves no half folder"
        );
    }

    #[test]
    fn a_cancel_stops_and_removes_the_folder() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file("a", opts()).unwrap();
            w.write_all(b"x").unwrap();
        });
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            extract(&a, &Limits::default(), &cancel, None, &|_, _| {}),
            Err(Error::Cancelled)
        ));
        assert!(!t.path().join("z").exists());
    }

    #[test]
    fn a_file_already_in_the_way_is_never_replaced() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file("a", opts()).unwrap();
            w.write_all(b"new").unwrap();
        });
        let folder = t.path().join("chosen");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a"), b"mine").unwrap();
        // `extract_into` wants a folder that is not there yet.
        assert!(extract_into(
            &a,
            &folder,
            &Limits::default(),
            &AtomicBool::new(false),
            None,
            &|_, _| {}
        )
        .is_err());
        assert_eq!(fs::read(folder.join("a")).unwrap(), b"mine");
    }

    #[test]
    fn a_chosen_folder_gets_the_new_folder_and_a_missing_one_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        let a = build(t.path(), "z.zip", |w| {
            w.start_file("a", opts()).unwrap();
            w.write_all(b"x").unwrap();
        });
        let elsewhere = t.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let no = AtomicBool::new(false);
        let got = extract_in(
            &a,
            Some(&elsewhere),
            &Limits::default(),
            &no,
            None,
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(got.folder, elsewhere.join("z"));
        let gone = t.path().join("nowhere");
        assert!(extract_in(&a, Some(&gone), &Limits::default(), &no, None, &|_, _| {}).is_err());
        assert!(!gone.exists());
    }

    #[test]
    fn only_zip_names_are_offered() {
        assert!(is_zip("a.ZIP") && is_zip("x.y.zip"));
        assert!(!is_zip("a.zipx") && !is_zip("a.tar.gz") && !is_zip("zip"));
    }
}
