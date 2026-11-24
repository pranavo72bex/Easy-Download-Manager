// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's crates' tests share (gaps row X10): each crate names it in its
//! `[dev-dependencies]`, so it is never built into the program, and no test
//! reaches into another crate's folder by `#[path]` (`cargo xtask layers`).

pub mod load;
pub mod secrets;

pub use load::allowing_for_load;
