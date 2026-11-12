// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's start: the login item put in step after the window is up.

use super::*;

/// A platform whose login item the system refuses to change.
struct Refusing;

impl Platform for Refusing {
    fn set_login_item(&self, _: bool) -> Result<(), String> {
        Err("the system said no".into())
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
        _: dm_shell::app::power::WhenDone,
        _: dm_shell::app::power::Report,
    ) -> Result<(), String> {
        Ok(())
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
    fn choose_file(&self, _: bool, _: &str) -> Option<std::path::PathBuf> {
        None
    }
}

/// Done on a thread of its own, the login item's refusal still reaches the
/// banner in the words it had when the start waited for it, and the window
/// is woken to read it (gaps row S16).
#[test]
fn a_login_item_refused_after_the_window_is_up_is_said_and_wakes_the_window() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = dm_shell::Settings {
        download_dir: dir.path().join("out"),
        start_on_login: true,
        ..Default::default()
    };
    let queue = dm_core::Queue::new(
        dm_core::client(),
        settings.options(dir.path().join("parts"), dm_core::Limiter::unlimited()),
        settings.concurrency,
    );
    let backend = Backend::new(
        runtime.handle().clone(),
        queue,
        settings,
        Vec::new(),
        Arc::new(Refusing),
    );
    let (woken, wakes) = std::sync::mpsc::channel();
    let woken = Mutex::new(woken);
    LoginItem(()).follow(
        &backend,
        Arc::new(move |wake| {
            let _ = dm_core::lock(&woken).send(wake);
        }),
    );
    let wake = wakes
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the window is woken");
    assert_eq!(wake, Wake::Now);
    assert_eq!(
        backend.problems(),
        vec![
            "could not change start-on-login: the system said no; start on login is not in force"
                .to_string()
        ]
    );
}
