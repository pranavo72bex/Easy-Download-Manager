// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The dial-up and VPN connections the system dials: Windows' Remote Access Service (`ras`).

#[cfg(windows)]
pub(crate) mod ras;
