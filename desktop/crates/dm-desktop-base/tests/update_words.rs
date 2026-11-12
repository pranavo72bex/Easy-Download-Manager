// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What About says of each answer of the update check, in plain words.
//! Moved from the egui window's tests.

use dm_desktop_base::i18n;
use dm_desktop_base::project::update_words;
use dm_shell::app::update::{self, Update};

#[test]
fn what_about_says_for_each_answer_of_the_check() {
    i18n::set(i18n::chosen("en"));
    // The version the window passes, its own: every dm crate has the same.
    let running = env!("CARGO_PKG_VERSION");
    let shown = |update: Update| update_words(&update, running);
    let installer = update::Installer {
        url: format!("{}desktop-v99.0.0/dm_99.0.0_aarch64.dmg", update::RELEASES),
        sha256: "ab".repeat(32),
    };
    assert_eq!(shown(Update::NotAsked), "Not checked yet.");
    assert_eq!(shown(Update::Asking), "Checking…");
    assert_eq!(
        shown(Update::Current),
        format!("This is the newest version ({running}).")
    );
    assert_eq!(
        shown(Update::Newer("99.0.0".into(), Some(installer))),
        format!("dm 99.0.0 is available (you have {running}). Download its installer here.")
    );
    assert_eq!(
        shown(Update::Newer("99.0.0".into(), None)),
        format!("dm 99.0.0 is out (this is {running}). It is on the project's releases page.")
    );
    assert_eq!(
        shown(Update::NoRelease),
        format!("No newer release yet (you have {running}).")
    );
    // A reason dm knows no plainer words for is said as it came.
    assert_eq!(
        shown(Update::Failed("the release names no version".into())),
        "Could not reach the update server: the release names no version"
    );
    assert_eq!(
        shown(Update::Failed(
            "the server answered 500 Internal Server Error".into()
        )),
        "Could not reach the update server: the server had a problem on its end. Try again later"
    );
    let offline = shown(Update::Failed(
        "error sending request for url (https://github.com/): dns error: failed to lookup address"
            .into(),
    ));
    assert!(
        offline.starts_with("Could not reach the update server: ") && !offline.contains("url ("),
        "{offline}"
    );
}
