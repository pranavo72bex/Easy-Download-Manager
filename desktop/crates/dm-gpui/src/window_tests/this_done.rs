// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "When this download is done", in a download's own progress window (gaps row U36).

use dm_core::State;
use serde_json::json;

use crate::harness::world::world;

/// Chosen in its progress window, Open opens the file once the download is done, and only then;
/// a script cannot choose it.
#[test]
fn when_this_download_is_done_opens_it() {
    let world = world();
    let mut h = world.harness();
    h.call(
        "downloads.add",
        json!({ "urls": [world.url("file.zip")], "later": true }),
    )
    .unwrap();
    h.until("the row", |h| h.rows().len() == 1);
    let id = h.rows()[0].id;
    h.update(|view, cx| view.show_progress(id, cx));
    h.until_shown("When this download is done");
    // A script cannot choose to open a downloaded file, as it cannot open one itself.
    h.click("Do nothing");
    assert!(h.call("ui.click", json!({ "text": "Open" })).is_err());
    // A person's pick of it in the pop-up's menu.
    let dialog = h.cx.update(|cx| {
        crate::screen::Screens::all(cx)
            .into_iter()
            .find(|(_, _, dialog)| *dialog)
            .map(|(window, _, _)| window)
    });
    h.cx.dispatch_action(
        dialog.expect("the progress window"),
        crate::popup::Pick {
            field: "this-done",
            item: 0,
            index: 1,
        },
    );
    h.until("the choice to be kept", |h| {
        h.view(|view| view.session.this_done.get(&id).copied())
            == Some(dm_shell::app::session::ThisDone::Open)
    });
    assert!(
        h.fake.opened.lock().unwrap().is_empty(),
        "opened before it was done"
    );
    h.call("downloads.resume", json!({ "ids": [id] })).unwrap();
    h.until("it to finish and be opened", |h| {
        let row = h.rows()[0].clone();
        row.state == State::Completed && h.fake.opened.lock().unwrap().contains(&row.path)
    });
}
