// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What edm-net asks of the system it runs on: `programs` (the system's own programs it runs,
//! macOS's `scutil` and `networksetup`, gaps row P8). A system's code sits here, chosen in this
//! module (gaps row A10).

pub(crate) mod programs;
