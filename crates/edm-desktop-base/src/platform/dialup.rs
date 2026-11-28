// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Dial-up connections, which `edm_shell::dialup` carries out: Windows' alone.

/// Whether Scheduler › Hang up modem when done is acted on here: Windows ends its dial-up
/// connections (`edm_shell::dialup::hang_up_later`); elsewhere there are none EDM knows.
pub(crate) const CAN_HANG_UP: bool = cfg!(windows);
