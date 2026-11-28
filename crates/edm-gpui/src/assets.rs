// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's pictures, compiled in: a program that cannot find its own
//! files has none to show, so they travel inside it.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

// Every picture, by the path a view asks for (`assets/icons/`, gaps row X9).
use edm_assets::ICONS;

/// The pictures compiled into EDM.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(edm_assets::icon(path).map(Cow::Borrowed))
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
        for (command, _) in edm_shell::app::commands::TOOLBAR {
            let path = crate::window::toolbar::icon(command);
            assert!(
                matches!(Assets.load(path), Ok(Some(_))),
                "{} has no picture at {path}",
                command.id()
            );
        }
    }

    #[test]
    fn every_kind_has_its_picture() {
        for kind in edm_core::Category::ALL.into_iter().map(Some).chain([None]) {
            let path = crate::window::list::kind_path(kind);
            assert!(matches!(Assets.load(path), Ok(Some(_))), "{kind:?}: {path}");
        }
    }
}
