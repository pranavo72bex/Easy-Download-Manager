// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
        assert!(said.contains("couldn’t be written"), "{said}");
        assert!(said.contains("already there"), "{said}");
    }
    // Not allowed to: a folder no one may write in.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let shut = home.path().join("shut");
        std::fs::create_dir(&shut).unwrap();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o500)).unwrap();
        let said = at(SaveTo::OneFolder, &shut.join("x").display().to_string()).ready(&root, &cats);
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o700)).unwrap();
        // The superuser may write anywhere: there the folder is simply made.
        if let Err(said) = said {
            assert!(said.contains("not allowed"), "{said}");
        }
    }
}
