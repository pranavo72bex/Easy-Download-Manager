// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A file kept to this account: on a Unix system its mode is set to the owner's alone; on
//! Windows a file in the account's own folders is already its own, so nothing is changed.

use std::path::Path;

/// Makes `path` readable and writable by its owner alone (mode `0600`): the control port's
/// token, one written looser by an older EDM or by hand (`control::token`).
#[cfg(unix)]
pub(crate) fn owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Nothing to change: the file is left as it is.
#[cfg(not(unix))]
pub(crate) fn owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
