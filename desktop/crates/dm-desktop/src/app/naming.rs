// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// Where each row's file will go and what it will be called: the
/// application's (`dm_shell::app::names`), with the folder in force.
impl DmApp {
    /// Where a row not saved anywhere yet will go, for the Folder column to
    /// show until it is (`names::planned_folder`).
    pub fn planned_folder(
        &self,
        row: &Entry,
        categories: &mut dm_core::Categories,
    ) -> Option<String> {
        dm_shell::app::names::planned_folder(row, &self.applied.download_dir, categories)
    }

    /// For every row not yet placed, how many rows before it are bound for
    /// the same folder under the same name (`names::earlier_of_the_same_name`).
    pub fn earlier_of_the_same_name(&self) -> HashMap<Id, usize> {
        dm_shell::app::names::earlier_of_the_same_name(&self.rows)
    }

    /// The name the row's file will get in its folder.
    #[must_use]
    pub fn name_it_will_get(&self, row: &Entry, categories: &mut dm_core::Categories) -> String {
        self.name_it_will_get_after(row, categories, None)
    }

    /// [`Self::name_it_will_get`], where how many earlier rows take a free
    /// name first is already known (`names::name_it_will_get_after`).
    pub fn name_it_will_get_after(
        &self,
        row: &Entry,
        categories: &mut dm_core::Categories,
        known_before: Option<usize>,
    ) -> String {
        dm_shell::app::names::name_it_will_get_after(
            &self.rows,
            row,
            &self.applied.download_dir,
            categories,
            known_before,
        )
    }
}
