// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The project's pages About and Help link to, and what the window says of
//! the update check. The check itself (the manifest, the version, the
//! installer and its digest, the site rules) is the application's
//! (`dm_shell::app::update`), re-exported here.

use std::sync::{Arc, Mutex};

pub use dm_shell::app::update::*;

/// The project's pages About and Help link to (`dm_desktop_base::project`,
/// both windows).
pub use dm_desktop_base::project::{GUIDE, HELP, LICENCE, PRIVACY, SIGNING, SOURCE, SUPPORT};

/// What the window says of the update check.
pub trait Said {
    /// What About says, in the language in force.
    fn shown(&self) -> String;
}

impl Said for Update {
    fn shown(&self) -> String {
        dm_desktop_base::project::update_words(self, env!("CARGO_PKG_VERSION"))
    }
}

/// Asks whether a newer dm is out (`dm_shell::app::update::check`), with
/// this build's version, and repaints `ctx` once the answer is in.
pub fn check(
    backend: &dm_shell::app::backend::Backend,
    into: Arc<Mutex<Update>>,
    ctx: egui::Context,
) {
    dm_shell::app::update::check(backend, env!("CARGO_PKG_VERSION"), into, move || {
        ctx.request_repaint()
    });
}
