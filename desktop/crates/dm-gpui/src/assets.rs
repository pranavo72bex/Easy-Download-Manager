// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's pictures, compiled in: a program that cannot find its own
//! files has none to show, so they travel inside it.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Every picture, by the path a view asks for.
const ICONS: [(&str, &[u8]); 11] = [
    ("icons/add.svg", include_bytes!("../assets/icons/add.svg")),
    (
        "icons/resume.svg",
        include_bytes!("../assets/icons/resume.svg"),
    ),
    ("icons/stop.svg", include_bytes!("../assets/icons/stop.svg")),
    (
        "icons/stop-all.svg",
        include_bytes!("../assets/icons/stop-all.svg"),
    ),
    (
        "icons/delete.svg",
        include_bytes!("../assets/icons/delete.svg"),
    ),
    (
        "icons/delete-completed.svg",
        include_bytes!("../assets/icons/delete-completed.svg"),
    ),
    (
        "icons/options.svg",
        include_bytes!("../assets/icons/options.svg"),
    ),
    (
        "icons/scheduler.svg",
        include_bytes!("../assets/icons/scheduler.svg"),
    ),
    (
        "icons/start-queue.svg",
        include_bytes!("../assets/icons/start-queue.svg"),
    ),
    (
        "icons/stop-queue.svg",
        include_bytes!("../assets/icons/stop-queue.svg"),
    ),
    (
        "icons/grabber.svg",
        include_bytes!("../assets/icons/grabber.svg"),
    ),
];

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
}
