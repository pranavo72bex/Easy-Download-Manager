// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The ways dm runs: `modes` (what the command line asks for, as one
//! `Mode`, and the modes with no window), `boot` (dm started as every window
//! starts it), `headless` (dm with no window at all), `session` (the
//! application's tick, which the window and `headless` both drive),
//! `control` (the control port's server) and `probe` (the window methods it asks, as a
//! contract any window answers), `cli` (the command line, a client of the
//! control port) and `mcp` (the MCP server an AI assistant talks to, over
//! the control port too). Each is reached from the crate's root as before
//! (`dm_desktop_base::boot`).

pub mod boot;
pub mod cli;
pub mod control;
pub mod headless;
pub mod mcp;
pub mod modes;
pub mod probe;
pub mod session;
