// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Save to", where several links added together go, for every window and
//! every dialog that adds several at once ("Download links", "Links on the
//! clipboard"): each to its category's folder, all to one category's, or
//! all to one folder, typed or chosen, which is made when they are added if
//! it is not there yet.

use std::path::{Path, PathBuf};

use crate::i18n::{t, tf};

/// Where the links ticked are saved, as reference A offers: each to its
/// category's folder, all to one category's, or all to one folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveTo {
    /// Each file to the folder of its category.
    #[default]
    ByCategory,
    /// Every file to one category's folder (`SaveChoice::category`).
    OneCategory,
    /// Every file to one folder (`SaveChoice::folder`).
    OneFolder,
}

/// The choice a dialog's Save to holds.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveChoice {
    /// Which of the three.
    pub save_to: SaveTo,
    /// The category every file goes to, for `SaveTo::OneCategory`: a kind's
    /// word or a person's own category's name.
    pub category: String,
    /// The folder every file goes to, for `SaveTo::OneFolder`, as typed.
    pub folder: String,
}

impl Default for SaveChoice {
    /// Each to its category's folder, as dm sorts a download; Archives
    /// offered first for one category, as reference A offers it.
    fn default() -> SaveChoice {
        SaveChoice {
            save_to: SaveTo::ByCategory,
            category: dm_shell::app::names::category_word(dm_core::Category::Archives),
            folder: String::new(),
        }
    }
}

impl SaveChoice {
    /// The folder the links go to where the choice names one: `None` is each
    /// to the folder of its category, as dm sorts a download. A folder typed
    /// as a name alone ("Holiday") is one in the download folder `root`, as
    /// a person who types a name there means; `~` is the home folder.
    pub fn save_dir(&self, root: &Path, categories: &dm_core::Categories) -> Option<String> {
        match self.save_to {
            SaveTo::ByCategory => None,
            SaveTo::OneFolder => {
                let typed = self.folder.trim();
                if typed.is_empty() {
                    return None;
                }
                // Left as typed where it cannot be read as a folder: the
                // queue then refuses it in its own words.
                Some(
                    dm_shell::settings::full_folder(typed, Some(root))
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| typed.to_string()),
                )
            }
            SaveTo::OneCategory => Some(
                dm_shell::app::names::category_folder(&self.category, root, categories)
                    .display()
                    .to_string(),
            ),
        }
    }

    /// The folder a file called `name` will be saved to, as a Save to
    /// column says it.
    pub fn folder_of(&self, name: &str, root: &Path, categories: &dm_core::Categories) -> String {
        if let Some(dir) = self.save_dir(root, categories) {
            return dir;
        }
        categories
            .destination(root, dm_core::classify(name, None))
            .display()
            .to_string()
    }

    /// What Start now and Add for later check before adding: a folder is
    /// typed where every file goes to one, and the folder named is there,
    /// made now if it was not (with the folders above it), so a new folder
    /// typed or made in the system's panel holds the files at once, Add for
    /// later's too. The folder they go to, or the words to say in the
    /// dialog, which stays open: none typed, or one that cannot be made (dm
    /// not allowed to, a file of that name in the way).
    pub fn ready(
        &self,
        root: &Path,
        categories: &dm_core::Categories,
    ) -> Result<Option<String>, String> {
        if self.save_to == SaveTo::OneFolder && self.folder.trim().is_empty() {
            return Err(t("Choose the folder every file goes to.").to_string());
        }
        let Some(dir) = self.save_dir(root, categories) else {
            return Ok(None);
        };
        let path = PathBuf::from(&dir);
        // A folder not read as a full path is left to the queue, which says
        // why it refuses it.
        if path.is_absolute() {
            made(&path).map_err(|e| {
                tf(
                    "{} could not be written: {}",
                    &[&path.display(), &crate::format::file_error(&e)],
                )
            })?;
        }
        Ok(Some(dir))
    }
}

/// `path` as a folder: there already, or made with the folders above it. A
/// file of that name, here or above, is the error the system gives for a
/// name taken (`AlreadyExists`), whichever the system says, so it is told
/// the same way everywhere.
fn made(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    let in_the_way = path
        .ancestors()
        .find(|p| p.exists())
        .is_some_and(|p| !p.is_dir());
    if in_the_way {
        return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
    }
    std::fs::create_dir_all(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(save_to: SaveTo, folder: &str) -> SaveChoice {
        SaveChoice {
            save_to,
            folder: folder.into(),
            ..SaveChoice::default()
        }
    }

    #[test]
    fn each_to_its_category_unless_one_folder_or_category_is_chosen() {
        let home = tempfile::tempdir().unwrap();
        let root = &home.path().join("dl");
        let elsewhere = home.path().join("elsewhere").display().to_string();
        let cats = dm_core::Categories::default();
        assert_eq!(at(SaveTo::ByCategory, "").save_dir(root, &cats), None);
        assert_eq!(
            at(SaveTo::OneFolder, &elsewhere).save_dir(root, &cats),
            Some(elsewhere.clone())
        );
        // A name alone is a folder in the download folder.
        assert_eq!(
            at(SaveTo::OneFolder, " Holiday ").save_dir(root, &cats),
            Some(root.join("Holiday").display().to_string())
        );
        assert!(at(SaveTo::OneCategory, "")
            .save_dir(root, &cats)
            .is_some_and(|d| Path::new(&d).starts_with(root)));
        assert_eq!(
            at(SaveTo::OneFolder, &elsewhere).folder_of("a.zip", root, &cats),
            elsewhere
        );
        // Nothing is made by asking where.
        assert!(!home.path().join("elsewhere").exists());
    }

    #[test]
    fn a_new_folder_is_made_and_one_that_cannot_be_is_said() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("dl");
        let cats = dm_core::Categories::default();
        assert_eq!(
            at(SaveTo::OneFolder, "  ").ready(&root, &cats).unwrap_err(),
            "Choose the folder every file goes to."
        );
        assert_eq!(at(SaveTo::ByCategory, "").ready(&root, &cats), Ok(None));
        // Typed, two levels that are not there yet.
        let new = home.path().join("new").join("deeper");
        let dir = at(SaveTo::OneFolder, &new.display().to_string())
            .ready(&root, &cats)
            .unwrap();
        assert_eq!(dir, Some(new.display().to_string()));
        assert!(new.is_dir());
        // There already: as it is.
        assert!(at(SaveTo::OneFolder, &new.display().to_string())
            .ready(&root, &cats)
            .is_ok());
        // A file in the way, as the folder or above it.
        let file = home.path().join("taken");
        std::fs::write(&file, b"x").unwrap();
        for typed in [file.clone(), file.join("below")] {
            let said = at(SaveTo::OneFolder, &typed.display().to_string())
                .ready(&root, &cats)
                .unwrap_err();
            assert!(said.contains("could not be written"), "{said}");
            assert!(said.contains("already there"), "{said}");
        }
        // Not allowed to: a folder no one may write in.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let shut = home.path().join("shut");
            std::fs::create_dir(&shut).unwrap();
            std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o500)).unwrap();
            let said =
                at(SaveTo::OneFolder, &shut.join("x").display().to_string()).ready(&root, &cats);
            std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o700)).unwrap();
            // The superuser may write anywhere: there the folder is simply made.
            if let Err(said) = said {
                assert!(said.contains("not allowed"), "{said}");
            }
        }
    }
}
