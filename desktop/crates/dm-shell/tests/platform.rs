// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a platform gets from [`Platform`] when it says no more than it must.
//! Moved from the egui window's tests.

use std::sync::{Arc, Mutex};

use dm_shell::app::platform::Platform;
use dm_shell::app::power::{Report, WhenDone};

/// A system that says no: no login item, no power, no opening, no trash
/// (its file is left where it is). The rest of [`Platform`] is the trait's
/// own, which is what a platform that does not override it gets.
#[derive(Default)]
struct Refusing {
    said: Mutex<Vec<String>>,
}

impl Platform for Refusing {
    fn set_login_item(&self, _wanted: bool) -> Result<(), String> {
        Err("login items are not allowed here".into())
    }
    fn notify(&self, _title: &str, body: &str) {
        self.said.lock().unwrap().push(body.to_string());
    }
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, path: &str) -> Result<(), String> {
        Err(format!("cannot show {path}"))
    }
    fn open(&self, path: &str) -> Result<(), String> {
        Err(format!("cannot open {path}"))
    }
    fn chime(&self) {
        self.said.lock().unwrap().push("chime".into());
    }
    fn power(&self, action: WhenDone, _report: Report) -> Result<(), String> {
        Err(format!("{action:?} is not allowed here"))
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn open_with(&self, _path: &str, _app: &std::path::Path) -> Result<(), String> {
        Err("no".into())
    }
    fn choose_file(&self, _save: bool, _suggested: &str) -> Option<std::path::PathBuf> {
        None
    }
}

/// What a platform gets from the trait when it does not say otherwise: an
/// opener's failure reported, the finish's sound for everything, a folder
/// chooser that answers as a Cancel, and a file deleted for want of a Trash.
#[test]
fn a_platform_that_says_nothing_more_gets_the_traits_own_ways() {
    let platform = Refusing::default();
    let reported = Arc::new(Mutex::new(Vec::new()));
    let report = |into: &Arc<Mutex<Vec<String>>>| -> Report {
        let into = into.clone();
        Box::new(move |why| into.lock().unwrap().push(why))
    };
    platform.open_later("/x/a.zip", report(&reported));
    platform.reveal_later("/x/a.zip", report(&reported));
    assert_eq!(
        *reported.lock().unwrap(),
        ["cannot open /x/a.zip", "cannot show /x/a.zip"]
    );
    platform.play_file("/sounds/done.wav");
    platform.alert();
    platform.tick();
    platform.notify_file("Download finished", "a.zip", "/x/a.zip");
    assert_eq!(*platform.said.lock().unwrap(), ["chime", "chime", "a.zip"]);
    assert_eq!(platform.choose_folder(), None);
    assert!(platform.can_choose_folder());
    assert!(platform.can_choose_file());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gone.bin");
    std::fs::write(&file, b"x").unwrap();
    platform.trash(&file.display().to_string()).unwrap();
    assert!(!file.exists());
    assert!(platform.trash(&file.display().to_string()).is_err());
}
