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

use crate::i18n::{t, tf};

/// Where dm's documentation is: the project's README.
pub const HELP: &str = "https://github.com/pranavo72bex/Easy-Download-Manager#readme";

/// The project's source, licence, privacy statement and code signing
/// policy, as About dm links them.
pub const SOURCE: &str = "https://github.com/pranavo72bex/Easy-Download-Manager";
/// The guide to dm's parts, where Help's topics point (`docs/user-guide.md`).
pub const GUIDE: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/blob/main/docs/user-guide.md";
/// Where a person asks for help or reports a fault.
pub const SUPPORT: &str = "https://github.com/pranavo72bex/Easy-Download-Manager/issues";
/// The project's licence.
pub const LICENCE: &str = "https://github.com/pranavo72bex/Easy-Download-Manager/blob/main/LICENSE";
/// The project's privacy statement.
pub const PRIVACY: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager/blob/main/PRIVACY.md";
/// The project's code signing policy.
pub const SIGNING: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager#code-signing-policy";

/// What the window says of the update check.
pub trait Said {
    /// What About says, in the language in force.
    fn shown(&self) -> String;
}

impl Said for Update {
    fn shown(&self) -> String {
        let running = env!("CARGO_PKG_VERSION");
        match self {
            Update::NotAsked => t("Not checked yet.").to_owned(),
            Update::Asking => t("Checking…").to_owned(),
            Update::Current => tf("This is the newest version ({}).", &[&running]),
            Update::Newer(v, Some(_)) => tf(
                "dm {} is available (you have {}). Download its installer here.",
                &[v, &running],
            ),
            Update::Newer(v, None) => tf(
                "dm {} is out (this is {}). It is on the project's releases page.",
                &[v, &running],
            ),
            Update::NoRelease => tf("No newer release yet (you have {}).", &[&running]),
            // In plain words where dm knows them (the raw
            // "error sending request for url (…)" offline).
            Update::Failed(why) => tf(
                "Could not reach the update server: {}",
                &[&crate::format::reason(why).map(t).unwrap_or(why.as_str())],
            ),
        }
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
