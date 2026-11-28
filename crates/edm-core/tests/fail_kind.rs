// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A failure's kind in `queue.json` (gaps row A9): kept beside the state, whose sentence keeps
//! the shape every EDM reads, so a queue saved now still opens in an older EDM; a queue saved
//! before kinds gives each failure its kind from its sentence, once, as it is read.

use edm_core::{DownloadList, Entry, FailKind, State};
use serde_json::json;

mod common;
use common::tempdir;

fn failed(id: u64, said: &str) -> serde_json::Value {
    json!({
        "id": id,
        "url": format!("http://127.0.0.1:9/{id}.zip"),
        "state": {"kind": "failed", "reason": said},
        "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
        "filename": format!("{id}.zip"),
    })
}

#[tokio::test]
async fn an_old_queue_file_gives_each_failure_its_kind_as_it_is_read() {
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let page = "that link answered with a web page rather than the file — it may have expired, \
                or need a sign-in";
    let old = serde_json::Value::Array(vec![
        failed(
            1,
            "the server answered 404 Not Found for http://127.0.0.1:9/1.zip",
        ),
        failed(2, page),
        failed(3, "the server refused the connection"),
    ]);
    std::fs::write(&store, old.to_string()).unwrap();
    let q = DownloadList::new(edm_core::client(), common::options(dir.path()), 0);
    q.persist_to(store).await;
    let kind = |row: Option<Entry>| row.expect("the row was not restored").fail_kind;
    assert_eq!(kind(q.get(1).await), Some(FailKind::Refused(404)));
    assert_eq!(kind(q.get(2).await), Some(FailKind::WebPage));
    assert_eq!(kind(q.get(3).await), Some(FailKind::Other));
    // The sentence a person reads is the one saved, word for word.
    let row = q.get(2).await.unwrap();
    assert!(
        matches!(&row.state, State::Failed(said) if said.starts_with("that link answered")),
        "{:?}",
        row.state
    );
}

#[test]
fn a_new_row_reads_back_equal_and_keeps_the_state_an_older_edm_reads() {
    let said = "the server answered 403 Forbidden for https://e.com/a.zip";
    let mut row: Entry = serde_json::from_value(failed(4, said)).unwrap();
    assert_eq!(row.fail_kind, None);
    assert_eq!(row.failed_as(), Some(FailKind::Refused(403)));
    row.fail_kind = Some(FailKind::Refused(403));
    let saved = serde_json::to_value(&row).unwrap();
    // The state as every EDM before kinds wrote and reads it; the kind a field beside it, which
    // a reader that does not know it passes over.
    assert_eq!(saved["state"], json!({"kind": "failed", "reason": said}));
    assert_eq!(saved["fail_kind"], "refused 403");
    let back: Entry = serde_json::from_value(saved).unwrap();
    assert_eq!(back.state, row.state);
    assert_eq!(back.fail_kind, row.fail_kind);
    // A row that has not failed writes no kind.
    let mut queued = row;
    queued.state = State::Queued;
    queued.fail_kind = None;
    let saved = serde_json::to_value(&queued).unwrap();
    assert!(saved.get("fail_kind").is_none(), "{saved}");
    assert_eq!(queued.failed_as(), None);
}
