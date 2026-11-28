// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use std::sync::{Arc, Mutex};

/// A platform whose login item is a flag, refusing to set it on when told to.
struct Item {
    on: Mutex<Vec<bool>>,
    refuse: bool,
}

impl super::Platform for Item {
    fn set_login_item(&self, wanted: bool) -> Result<(), String> {
        if wanted && self.refuse {
            return Err("the system said no".into());
        }
        edm_core::lock(&self.on).push(wanted);
        Ok(())
    }
    fn notify(&self, _: &str, _: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn open(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn chime(&self) {}
    fn power(
        &self,
        _: crate::app::power::WhenDone,
        _: crate::app::power::Report,
    ) -> Result<(), String> {
        Ok(())
    }
    fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

fn backend(
    runtime: &tokio::runtime::Runtime,
    dir: &std::path::Path,
    start_on_login: bool,
    item: Arc<Item>,
) -> super::Backend {
    let settings = crate::Settings {
        general: crate::settings::GeneralSettings {
            start_on_login,
            ..Default::default()
        },
        folders: crate::settings::FolderSettings {
            download_dir: dir.join("out"),
            ..Default::default()
        },
        ..crate::Settings::default()
    };
    let queue = edm_core::DownloadList::new(
        edm_core::client(),
        settings.options(dir.join("parts"), edm_core::Limiter::unlimited()),
        settings.downloads.concurrency,
    );
    super::Backend::new(runtime.handle().clone(), queue, settings, Vec::new(), item)
}

/// The login item put in step at launch follows the setting as it is
/// when that runs, and what the system refuses comes back to be said
/// (gaps row S16: done off the window's thread once it is up).
#[test]
fn the_login_item_follows_the_setting_in_force_and_a_refusal_comes_back() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let item = Arc::new(Item {
        on: Mutex::default(),
        refuse: false,
    });
    let on = backend(&runtime, dir.path(), true, Arc::clone(&item));
    on.login_item_follows_setting().unwrap();
    let off = backend(&runtime, dir.path(), false, Arc::clone(&item));
    off.login_item_follows_setting().unwrap();
    assert_eq!(*edm_core::lock(&item.on), vec![true, false]);

    let refusing = Arc::new(Item {
        on: Mutex::default(),
        refuse: true,
    });
    let refused = backend(&runtime, dir.path(), true, Arc::clone(&refusing));
    assert_eq!(
        refused.login_item_follows_setting(),
        Err("the system said no".to_string())
    );
    assert!(edm_core::lock(&refusing.on).is_empty());
}

/// Launch's login item and a change of the setting in Options, at the
/// same time from two threads: whichever goes second, the item ends as
/// the setting in force says, never on beside a setting saying off.
#[test]
fn a_change_of_the_setting_meanwhile_is_the_one_that_stands() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for _ in 0..20 {
        let item = Arc::new(Item {
            on: Mutex::default(),
            refuse: false,
        });
        let backend = backend(&runtime, dir.path(), true, Arc::clone(&item));
        let launch = {
            let backend = backend.clone();
            std::thread::spawn(move || backend.login_item_follows_setting())
        };
        let mut off = backend.settings();
        off.general.start_on_login = false;
        // Saving the file may fail beside this crate's other tests; the
        // setting is in force before it is written.
        let _ = backend.set_settings(off);
        launch.join().unwrap().unwrap();
        assert!(!backend.settings().general.start_on_login);
        assert_eq!(edm_core::lock(&item.on).last(), Some(&false));
    }
}
