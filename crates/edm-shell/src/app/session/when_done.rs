// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! When done asked from outside the window, and carried out once its countdown is over.

use crate::app::backend::Backend;
use crate::app::power::WhenDone;

/// The When done another EDM asked for (`EDM --when-done <word>`), left in
/// the inbox under `home`: taken, so it is acted on once. A word EDM does not
/// know is dropped.
pub fn when_done_asked(home: &std::path::Path) -> Option<WhenDone> {
    let file = crate::host::inbox(home).join(crate::shell::WHEN_DONE_FILE);
    let word = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    WhenDone::ALL.into_iter().find(|w| w.word() == word.trim())
}

/// Whether another EDM asked for a new download (`EDM --new-download`, the
/// jump list's Add URL), left in the inbox under `home`: taken, so Add URL
/// opens once.
pub fn new_download_asked(home: &std::path::Path) -> bool {
    std::fs::remove_file(crate::host::inbox(home).join(crate::shell::NEW_DOWNLOAD_FILE)).is_ok()
}

/// What carrying out a When done action that came due leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carried {
    /// EDM closes: the presentation does it, as only it can close itself.
    Quit,
    /// The computer was asked (to sleep, hibernate or shut down).
    Asked,
}

/// Carries out `action` once its countdown is over. Quit is the
/// presentation's to do; anything else is asked of the computer through the
/// platform, and a refusal that comes back later goes on `backend`'s problems,
/// as anything running apart from the window does: whoever set it is likely
/// away. An `Err` is a refusal said at once.
pub fn carry_out(
    backend: &Backend,
    action: crate::app::power::WhenDone,
) -> Result<Carried, String> {
    if action == crate::app::power::WhenDone::Quit {
        return Ok(Carried::Quit);
    }
    let notices = backend.notices();
    let report: crate::app::power::Report = Box::new(move |why| edm_core::lock(&notices).push(why));
    backend
        .platform
        .power(action, report)
        .map(|()| Carried::Asked)
}
