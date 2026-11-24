// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Downloads that fail: what each says, a retry, a fresh link, another folder and a sign-in.

use super::*;

#[test]
fn a_failed_download_plays_the_failure_sound_when_asked() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.general.sound_on_failure = true));
    // Nothing is served at this name: a 404, failed and not retried.
    h.add_url(&world.url("missing.bin"), "Start now");
    h.until("the row failed", failed);
    h.until("the sound", |h| opened(h).iter().any(|o| o == "alert"));
    assert!(!opened(&h).iter().any(|o| o == "chime"), "{:?}", opened(&h));
    // Once: a redraw does not sound it again.
    for _ in 0..5 {
        h.until("a wake", |_| true);
    }
    assert_eq!(opened(&h).iter().filter(|o| *o == "alert").count(), 1);
}

/// A server that wants a sign-in says where to give one, and Properties opens on its Login.
#[test]
fn a_refused_sign_in_says_where_to_give_one() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the refusal", failed);
    let row = h.rows()[0].clone();
    let said = dm_desktop_base::rows::status_of(&row);
    // In plain words, without the address or the server's page; where a sign-in goes is its
    // next step, in the details (gaps row V4).
    assert!(said.contains("sign in"), "{said}");
    assert!(!said.contains("http"), "{said}");
    let dm_core::State::Failed(raw) = &row.state else {
        panic!("not failed: {:?}", row.state)
    };
    let step = dm_desktop_base::format::next_step(raw).unwrap_or_default();
    assert!(step.contains("Properties › Login"), "{step}");
    h.select("secret.zip");
    // Retry, not Resume, on the toolbar.
    h.until_shown("Retry");
    h.run(Command::Properties);
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.type_text("user");
    h.click("OK");
    h.until("the sign-in to be kept with the row", |h| {
        h.rows()[0].url.contains("user@")
    });
}

/// A link that runs out part-way asks for a fresh one at once, and the fresh one goes on from
/// the parts already fetched.
#[test]
fn a_link_that_runs_out_part_way_asks_for_a_fresh_one() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| {
        s.advanced.attempts = 3;
        s.advanced.retry_wait = 0;
        s.downloads.record_minutes = 0;
    }));
    h.view(|view| view.backend.add(&world.url("expires.bin")).unwrap());
    h.until("the expired link to fail", failed);
    let row = h.rows()[0].clone();
    assert!(dm_shell::app::rules::link_expired(&row), "{:?}", row.state);
    h.until("a fresh link to be asked for", |h| {
        h.dialog_open("Update download link")
    });
    // The fresh link to the same file: it goes on, and finishes whole.
    h.type_text(&world.url("renewed.bin"));
    h.click("OK");
    h.until("the dialog to close", |h| {
        !h.dialog_open("Update download link")
    });
    h.until("the download to finish", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    let bytes = std::fs::read(&row.path).unwrap();
    assert_eq!(bytes.len(), 1_000_000);
    assert!(bytes.iter().all(|b| *b == 7));
}

/// A download whose folder cannot be reached (as when its drive is taken out) is paused with
/// why, not failed; the bar over the list offers another folder, and there it finishes, its
/// parts carried over.
#[test]
fn an_unreachable_folder_pauses_and_another_folder_finishes_it() {
    let world = world();
    let blocker = world.dir.path().join("not-a-drive");
    std::fs::write(&blocker, b"a file where the folder should be").unwrap();
    let download_dir = blocker.join("out");
    let mut h =
        world.harness_with(world.settings_with(|s| s.folders.download_dir = download_dir.clone()));
    h.view(|view| view.backend.add(&world.url("file.zip")).unwrap());
    h.until("the download to pause by itself", |h| {
        h.rows()
            .iter()
            .any(|r| r.state == State::Paused && !r.stalled.is_empty())
    });
    let row = h.rows()[0].clone();
    let status = dm_desktop_base::rows::status_of(&row);
    assert!(status.starts_with("Paused: "), "{status}");
    // The drive back, and a folder on it chosen: inside the download folder, as one chosen
    // while a script drives the window must be (`control_api::folder_refused`), which these
    // tests' clicks are.
    h.until_shown("Choose another folder…");
    std::fs::remove_file(&blocker).unwrap();
    let elsewhere = download_dir.join("elsewhere");
    crate::platform::services::choose_ahead(elsewhere.clone());
    h.click("Choose another folder…");
    h.until("the download to finish elsewhere", |h| completed(h) == 1);
    let row = h.rows()[0].clone();
    assert!(
        std::path::Path::new(&row.path).starts_with(&elsewhere),
        "{}",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap(), payload());
}

/// A failed row's status is a plain reason; the engine's words are kept for hover.
#[test]
fn a_cut_connection_is_said_plainly() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.advanced.attempts = 1));
    h.view(|view| view.backend.add(&world.url("drops.bin")).unwrap());
    h.until("the cut download to fail", failed);
    let row = h.rows()[0].clone();
    let status = dm_desktop_base::rows::status_of(&row);
    assert_eq!(
        status, "The connection dropped before the file was complete",
        "{:?}",
        row.state
    );
    assert!(dm_desktop_base::rows::failure_as_said(&row).is_some());
    // In the list's row and in Properties.
    h.until_shown(&status);
    h.select("drops.bin");
    h.run(Command::Properties);
    h.until("File properties", |h| h.dialog_open("File properties"));
    h.until("it in Properties too", |h| {
        h.texts().iter().filter(|t| **t == status).count() >= 2
    });
}

/// While a range waits to be tried again, the row says which attempt it is on.
#[test]
fn a_retry_says_its_attempt() {
    let world = world();
    let mut h = world.harness_with(world.settings_with(|s| s.advanced.attempts = 5));
    h.view(|view| view.backend.add(&world.url("drops.bin")).unwrap());
    // With when it tries next, in the words themselves (gaps row U7).
    h.until("the row to say it retries", |h| {
        h.rows().first().is_some_and(|r| {
            dm_desktop_base::rows::status_of(r).starts_with("Connection dropped (retry 2 of 5)")
        })
    });
    // It says when it tries again, not only that it will.
    h.until("when it tries again", |h| {
        h.texts().iter().any(|t| t.contains("next try in"))
    });
    // A new address is taken while it retries, and finishes it.
    let id = h.rows()[0].id;
    h.select("drops.bin");
    assert!(h.enabled(Command::UpdateLink));
    let good = world.url("file.zip");
    h.view(|view| view.backend.set_url(id, &good).unwrap());
    h.until("the new address to finish it", |h| {
        h.rows()
            .first()
            .is_some_and(|r| r.state == State::Completed && r.url == good)
    });
}

/// A download refused for want of a sign-in is retried with one saved in Options afterwards,
/// as a fresh copy of the address was.
#[test]
fn retry_uses_a_sign_in_saved_since_it_failed() {
    let world = world();
    let mut h = world.harness();
    h.view(|view| view.backend.add(&world.url("locked.zip")).unwrap());
    h.until("it to be refused", failed);
    let password = password();
    h.apply(|s| {
        s.site_logins = vec![dm_shell::settings::SiteLogin {
            host: "127.0.0.1".into(),
            username: "me".into(),
            password,
        }]
    });
    let id = h.rows()[0].id;
    h.view(|view| view.backend.retry(id));
    h.until("it to complete with the saved sign-in", |h| {
        completed(h) == 1
    });
}

/// A failure is announced as a finish is, saying why in plain words; it was only a sound, if
/// that was on.
#[test]
fn a_failed_download_is_announced_with_why() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the refusal", failed);
    h.until("the announcement", |h| {
        !h.fake.notified.lock().unwrap().is_empty()
    });
    for _ in 0..3 {
        h.until("a wake", |_| true);
    }
    let told = h.fake.notified.lock().unwrap().clone();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("secret.zip — "), "{told:?}");
    assert!(!told[0].contains("http"), "{told:?}");
}

/// A download that failed is counted above the list, with one button to try every failed one
/// again; hidden, the bar stays away until another fails.
#[test]
fn failed_downloads_are_counted_above_the_list_with_retry_all() {
    let world = world();
    let mut h = world.harness();
    h.add_url(&world.url("secret.zip"), "Start now");
    h.until("the failure", failed);
    h.until_shown("1 download didn't finish.");
    let asked = SECRET_ASKED.load(std::sync::atomic::Ordering::SeqCst);
    h.click("Retry all");
    // Tried again: asked of the server once more, and failed again, as that server still
    // wants a sign-in.
    h.until("the retry to reach the server", |_| {
        SECRET_ASKED.load(std::sync::atomic::Ordering::SeqCst) > asked
    });
    h.until("the second failure", failed);
    h.until_shown("1 download didn't finish.");
    h.click("Hide");
    h.until("the bar to go", |h| !h.shows("1 download didn't finish."));
}
