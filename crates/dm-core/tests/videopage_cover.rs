// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What `videopage` tells about an address before it reads it.

/// A YouTube watch page is one whose own data lists its qualities; another
/// site's page is not.
#[test]
fn a_watch_page_is_told_from_another_page() {
    assert!(dm_core::videopage::is_watch_page(
        "https://www.youtube.com/watch?v=jNQXAC9IVRw"
    ));
    assert!(!dm_core::videopage::is_watch_page(
        "https://example.com/watch?v=jNQXAC9IVRw"
    ));
}
