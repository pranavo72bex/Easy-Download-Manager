// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The answers the page's own script kept about media (`globalThis.__dmBodies`).

use super::*;

/// The kept bodies, oldest first.
pub(super) fn kept_texts() -> Vec<String> {
    let Ok(kept) = Reflect::get(&js_sys::global(), &"__dmBodies".into()) else {
        return Vec::new();
    };
    kept.dyn_ref::<Array>()
        .map(|list| list.iter().filter_map(|b| b.as_string()).collect())
        .unwrap_or_default()
}
