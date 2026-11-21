// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Who may reach dm's home: on Windows its access list (`owner_only`); Unix's 0700 is set
//! where the home is made (`instance`).

#[cfg(windows)]
pub(crate) mod owner_only;
