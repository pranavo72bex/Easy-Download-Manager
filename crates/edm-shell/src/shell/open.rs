// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Opening a finished file or its folder, a long path in a form Windows' shell takes.

use super::*;

/// Windows' longest path that its shell (Explorer, the Recycle Bin, a
/// program started with a file) takes: `MAX_PATH`, its end included. EDM's
/// own reading and writing goes past it by itself (Rust's `std::fs` adds
/// `\\?\`), but GPUI's manifest, which EDM cannot replace, does not declare
/// `longPathAware`, and the shell would not take such a path either way.
pub const SHELL_PATH_MOST: usize = 260;

/// `path` as Windows' shell can take it: as it is when it is short enough,
/// else its 8.3 short form (`C:\DOWNLO~1\...`), which names the same file
/// in fewer letters (gaps row 191). As it is wherever no short form is to
/// be had (a volume with 8.3 names turned off), and on every other system.
pub fn for_shell(path: &str) -> String {
    if path.encode_utf16().count() < SHELL_PATH_MOST {
        return path.to_string();
    }
    opener::short_form(&verbatim(path))
        .map(|short| plain(&short))
        .unwrap_or_else(|| path.to_string())
}

/// `path` in Windows' verbatim form, which the system's calls take past
/// `MAX_PATH`: `\\?\C:\...`, or `\\?\UNC\server\share\...` for a share.
pub fn verbatim(path: &str) -> String {
    if path.starts_with(r"\\?\") {
        path.to_string()
    } else if let Some(share) = path.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{share}")
    } else {
        format!(r"\\?\{path}")
    }
}

/// `path` with its verbatim prefix taken off, as the shell reads paths.
pub fn plain(path: &str) -> String {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Opens the finished file's folder, with the file selected where the OS can.
///
/// A finished download the user cannot get to is a download half-delivered;
/// this is the "show in folder" every file manager has.
pub fn reveal_in_folder(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("that file is no longer where EDM saved it".into());
    }
    opener::reveal(p, path)
}

/// Opens a finished file with whatever the system opens it with.
pub fn open_file(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    // An address — Help's — is opened in the browser; a path must exist.
    let is_link = path.starts_with("https://") || path.starts_with("http://");
    if !is_link && !p.exists() {
        return Err("that file is no longer where EDM saved it".into());
    }
    opener::open(p, path)
}

/// The program and arguments that open `path` with `app`: on macOS through
/// `open -a`, which takes an application bundle; elsewhere the program
/// itself, given the file.
pub fn open_with_command(
    path: &Path,
    app: &Path,
    macos: bool,
) -> (PathBuf, Vec<std::ffi::OsString>) {
    if macos {
        (
            PathBuf::from("open"),
            vec!["-a".into(), app.as_os_str().into(), path.as_os_str().into()],
        )
    } else {
        (app.to_path_buf(), vec![path.as_os_str().into()])
    }
}

/// Opens a finished file with the application the person chose: "Open with".
pub fn open_with(path: &str, app: &Path) -> Result<(), String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("that file is no longer where EDM saved it".into());
    }
    if !app.exists() {
        return Err(format!("{} is not there", app.display()));
    }
    // A program on Windows is handed a long path's short form, which every
    // program reads, where one written before long paths would not open it.
    let shell_path = for_shell(path);
    let p = if opener::SHORT_FORM_FOR_PROGRAMS {
        Path::new(&shell_path)
    } else {
        p
    };
    let (program, args) = open_with_command(p, app, opener::OPEN_WITH_THROUGH_OPEN);
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{} could not be started: {e}", app.display()))
}
