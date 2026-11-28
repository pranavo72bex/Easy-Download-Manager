// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A saved proxy that is turned on with no host is not gone around in
//! silence at launch: it is turned off for every route, and the window is
//! told why.

use edm_shell::settings::Settings;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_saved_proxy_with_no_host_is_turned_off_and_said_at_launch() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the one test in this binary, so the process-wide home is its own
    // and nothing else is reading the environment while it is set.
    unsafe { std::env::set_var("EDM_HOME", home.path()) };
    let mut saved = Settings {
        folders: edm_shell::settings::FolderSettings {
            download_dir: home.path().join("out"),
            ..Default::default()
        },
        ..Settings::default()
    };
    saved.advanced.proxy.enabled = true;
    saved.advanced.proxy.kind = edm_core::ProxyKind::Http;
    saved.advanced.proxy.host = String::new();
    saved.save().unwrap();

    let started = edm_shell::shell::start(&edm_shell::shell::orders(&[])).await;
    assert!(
        !started.settings.advanced.proxy.enabled,
        "the unusable proxy stayed on for the tools while EDM's own went direct"
    );
    assert!(
        started
            .notices
            .iter()
            .any(|n| n.contains("saved proxy could not be used") && n.contains("no host")),
        "nothing said: {:?}",
        started.notices
    );
    assert_eq!(
        started
            .settings
            .options(home.path().into(), edm_core::Limiter::unlimited())
            .proxy,
        None
    );
    // And on disk, where the browser's native host reads it: it found the
    // proxy still on and refused every hand-off while the app went direct.
    let (on_disk, _) = Settings::load();
    assert!(
        !on_disk.advanced.proxy.enabled,
        "the host still reads the broken proxy as on"
    );
}
