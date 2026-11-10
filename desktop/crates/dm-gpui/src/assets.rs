// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's pictures, compiled in: a program that cannot find its own
//! files has none to show, so they travel inside it.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// The pictures named, each at `icons/<name>.svg`, compiled in.
macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../assets/icons/", $name, ".svg")) as &[u8],
        )),*]
    };
}

/// Every picture, by the path a view asks for.
const ICONS: &[(&str, &[u8])] = icons!(
    // The toolbar's.
    "add",
    "resume",
    "stop",
    "stop-all",
    "delete",
    "delete-completed",
    "options",
    "scheduler",
    "start-queue",
    "stop-queue",
    "grabber",
    // Each file kind's, before a row's name (B45).
    "kind-videos",
    "kind-audio",
    "kind-documents",
    "kind-apps",
    "kind-archives",
    "kind-pictures",
    "kind-other",
    // A pop-up button's, as the system draws one (up and down).
    "chevron-up-down",
    // The top bar's search box.
    "search",
    // The top bar's button that shows or hides the categories, as the
    // system's sidebar button.
    "sidebar",
);

/// The pictures compiled into dm.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_toolbar_command_has_its_picture() {
        for (command, _) in dm_shell::app::commands::TOOLBAR {
            let path = crate::toolbar::icon(command);
            assert!(
                matches!(Assets.load(path), Ok(Some(_))),
                "{} has no picture at {path}",
                command.id()
            );
        }
    }

    #[test]
    fn every_kind_has_its_picture() {
        for kind in dm_core::Category::ALL.into_iter().map(Some).chain([None]) {
            let (path, _) = crate::list::kind_picture(kind);
            assert!(matches!(Assets.load(path), Ok(Some(_))), "{kind:?}: {path}");
        }
    }
}
