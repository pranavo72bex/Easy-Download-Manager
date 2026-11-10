// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's desktop program does that is not drawing: the system's
//! services (when the person is busy), with no window toolkit, so that any
//! window can use it. `dm_desktop` re-exports each module at its old path.

// Never crash on what comes from outside: no unwrap, expect or panic in
// shipped code (tests may), checked by clippy in CI.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
// As in dm-desktop: an `unsafe` block says why it is sound, a string is built
// with `format!` rather than `+`, and a loop is a loop rather than `for_each`.
#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::string_add, clippy::needless_for_each)]
pub mod quiet;
