// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! macOS: `open` opens a file with its application, an address in the browser, and with `-R`
//! shows a file selected in Finder.

use std::path::Path;

/// Shows `path` selected in Finder.
pub(crate) fn reveal(path: &Path, _: &str) -> Result<(), String> {
    run(std::process::Command::new("open").arg("-R").arg(path))
}

/// Opens `path` with its application.
pub(crate) fn open(path: &Path, _: &str) -> Result<(), String> {
    run(std::process::Command::new("open").arg(path))
}

fn run(command: &mut std::process::Command) -> Result<(), String> {
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// No short forms here.
pub(crate) fn short_form(_: &str) -> Option<String> {
    None
}
