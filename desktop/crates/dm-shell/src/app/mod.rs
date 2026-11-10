// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The application layer: what dm does, apart from how any window shows it.
//!
//! Everything here is free of the window toolkit (`cargo xtask layers` keeps
//! dm-shell so), so the egui window today, and any other presentation later,
//! drive the same application: they draw what it holds and hand it what a
//! person asked for. What belongs here: commands and their rules, the state a
//! window draws from, the changes it is told of, scheduling and settings
//! coordination. What does not: drawing, widgets, the window's own view state
//! (selection, sort, open dialogs), and carrying things out on this computer
//! (the tray, notifications, the system's power actions), which the desktop
//! implements behind the traits declared here. See `docs/architecture/`.

pub mod backend;
pub mod changes;
pub mod commands;
pub mod control_api;
pub mod detect;
pub mod events;
pub mod files;
pub mod handoff;
pub mod hints;
pub mod import;
pub mod links;
pub mod lookups;
pub mod names;
pub mod places;
pub mod platform;
pub mod power;
pub mod questions;
pub mod rules;
pub mod schedule;
pub mod search;
pub mod session;
pub mod snapshot;
pub mod update;
pub mod watch;
