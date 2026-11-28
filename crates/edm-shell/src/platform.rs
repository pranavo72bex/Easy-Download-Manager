// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What edm-shell asks of the system it runs on, each system its own way: `opener` (opening a
//! finished file and showing it in its folder, reached by `shell`), `dialup` (Windows' Remote
//! Access Service, reached by `crate::dialup`) and `access` (EDM's home kept to this account on
//! Windows, reached by `instance`) and `programs` (the system's own programs edm-shell runs: `ps`,
//! macOS's `open`, Windows' `reg.exe`, gaps row P8). A system's files sit here, chosen in this
//! module (gaps row A10).

pub(crate) mod access;
pub(crate) mod dialup;
pub(crate) mod opener;
pub(crate) mod programs;
