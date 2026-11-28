// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where EDM keeps its things, and folders read as full paths.

use std::path::PathBuf;

/// A folder typed or read as a full path: `~` is the home folder, and a
/// relative one is no folder — a Dock launch starts in `/`, where nothing
/// can be written, and every download would fail. `None` for those and for
/// nothing at all.
pub fn full_path(typed: &str) -> Option<PathBuf> {
    let typed = typed.trim();
    match typed.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => {
            edm_core::folders::home().map(|home| home.join(rest.trim_start_matches(['/', '\\'])))
        }
        _ if !typed.is_empty() && std::path::Path::new(typed).is_absolute() => {
            Some(PathBuf::from(typed))
        }
        _ => None,
    }
}

/// A folder as EDM writes it into a field a person reads: the home folder as `~` on macOS and
/// Linux, where people know it, so a long path keeps its own end in sight (gaps row U17);
/// [`full_path`] reads it back. As it is on Windows, where `~` means nothing to Explorer.
pub fn home_short(path: &str) -> String {
    if cfg!(windows) {
        return path.to_string();
    }
    let Some(home) = edm_core::folders::home() else {
        return path.to_string();
    };
    match std::path::Path::new(path).strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string(),
    }
}

/// A folder for one download, as a full path: `~` is the home folder, and a
/// relative one is taken against `base` — the folder of the command that
/// asked, which a running EDM handed the request does not share (a Dock
/// launch's is `/`). With no base, a relative folder is `None`: a row must
/// never hold one, since Open and Open folder read it later from wherever
/// EDM happens to be.
pub fn full_folder(typed: &str, base: Option<&std::path::Path>) -> Option<PathBuf> {
    full_path(typed).or_else(|| {
        let typed = typed.trim();
        (!typed.is_empty() && !typed.starts_with('~'))
            .then_some(base?.join(typed))
            .filter(|p| p.is_absolute())
    })
}

/// Where EDM keeps what belongs to it rather than to the person using it.
pub fn home() -> PathBuf {
    /* An override, so that driving a real build does not share a queue, a
     * settings file and an inbox with the copy somebody actually uses. There
     * is no other way to run this twice on one machine. */
    if let Some(elsewhere) = edm_core::env::get_os("EDM_HOME") {
        return PathBuf::from(elsewhere);
    }
    // This crate's own tests save settings through the backend: written into
    // the real home, they replaced the person's download folder with a test's
    // temporary one. They get a home of their own, one a run.
    if cfg!(test) {
        return std::env::temp_dir().join(format!("edm-shell-tests-{}", std::process::id()));
    }
    let home = edm_core::folders::local_data()
        .unwrap_or_else(std::env::temp_dir)
        .join(edm_core::paths::HOME_FOLDER);
    edm_core::paths::adopt_old_home(home, edm_core::paths::OLD_HOME_FOLDER)
}

/// The file that makes a copy of EDM portable: put beside the program, it
/// keeps EDM's settings, list and window beside it too, in [`PORTABLE_DATA`],
/// rather than in the computer's place for them.
pub const PORTABLE_MARKER: &str = "portable";
/// The folder a portable EDM keeps what belongs to it in.
pub const PORTABLE_DATA: &str = edm_core::paths::PORTABLE_HOME_FOLDER;

/// Where a portable copy of EDM in `program_dir` keeps its things, or `None`
/// where there is no [`PORTABLE_MARKER`] beside it. On macOS the program is
/// inside `EDM.app`, so the marker beside the app counts too.
pub fn portable_home_in(program_dir: &std::path::Path) -> Option<PathBuf> {
    let mut beside = vec![program_dir.to_path_buf()];
    // EDM.app/Contents/MacOS/edm-desktop: the folder holding EDM.app.
    if let Some(app) = program_dir
        .ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
    {
        beside.extend(app.parent().map(std::path::Path::to_path_buf));
    }
    beside
        .into_iter()
        .find(|dir| dir.join(PORTABLE_MARKER).is_file())
        .map(|dir| {
            edm_core::paths::adopt_old_home(
                dir.join(PORTABLE_DATA),
                edm_core::paths::OLD_PORTABLE_HOME_FOLDER,
            )
        })
}

/// Makes this run portable when a [`PORTABLE_MARKER`] is beside the
/// program, by giving it its own home as `EDM_HOME` does — so a portable
/// copy, like a test's, leaves the browsers' registration and the login
/// items to the installed one. To be called first thing, before any other
/// thread starts. An explicit `EDM_HOME` wins. Returns the home it chose.
pub fn enter_portable() -> Option<PathBuf> {
    if edm_core::env::get_os("EDM_HOME").is_some_and(|h| !h.is_empty()) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let home = portable_home_in(exe.parent()?)?;
    std::env::set_var("EDM_HOME", &home);
    Some(home)
}
