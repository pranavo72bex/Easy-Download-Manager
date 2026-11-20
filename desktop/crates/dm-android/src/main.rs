// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's phone app as a window on a computer (`cargo run -p dm-android [home folder]`): the same
//! engine and the same Slint window as on the phone, at a phone's size, so the phone's screens
//! can be run and tested without an emulator (gaps rows N7, N13). dm's files go in the folder
//! given, else in `dm-phone` in the system's temporary folder, and downloads in its `Download`.
//! What only a phone does (notifications, the clipboard, opening a file) is said on the
//! terminal or refused in words.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use dm_android::{AndroidSystem, Engine};

/// The phone's services, as far as a computer stands in for them.
struct Computer;

impl AndroidSystem for Computer {
    fn notify(&self, title: String, body: String, _path: Option<String>, _failed: Option<u64>) {
        eprintln!("dm (notification): {title}: {body}");
    }

    fn clipboard(&self) -> Option<String> {
        None
    }

    fn set_clipboard(&self, _text: String) {}

    fn open(&self, path: String) -> Option<String> {
        Some(format!(
            "the phone's window on a computer does not open files; the file is {path}"
        ))
    }

    fn reveal(&self, path: String) -> Option<String> {
        Some(format!(
            "the phone's window on a computer shows no folders; the file is {path}"
        ))
    }

    fn sound(&self, _failed: bool) {}

    fn changed(&self) {
        dm_android::app::refresh();
    }
}

fn main() -> ExitCode {
    let home = std::env::args_os()
        .nth(1)
        .map_or_else(|| std::env::temp_dir().join("dm-phone"), PathBuf::from);
    let home = match std::path::absolute(&home) {
        Ok(home) => home,
        Err(e) => {
            eprintln!("dm: {}: {e}", home.display());
            return ExitCode::FAILURE;
        }
    };
    let downloads = home.join("Download");
    let engine = match Engine::start(
        home.to_string_lossy().into_owned(),
        downloads.to_string_lossy().into_owned(),
        Arc::new(Computer),
    ) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("dm: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ran = dm_android::app::run(Arc::clone(&engine));
    engine.shut_down();
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dm: {e}");
            ExitCode::FAILURE
        }
    }
}
