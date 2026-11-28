// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Tests: what a list says of a download with its secrets left out.

use super::*;

#[test]
fn a_redacted_row_carries_no_secret_and_keeps_the_rest() {
    let mut row: Entry = serde_json::from_value(serde_json::json!({
        "id": 7,
        "url": concat!("https://me:", "pa55", "@files.example/a.zip"),
        "state": serde_json::to_value(State::Paused).unwrap(),
        "progress": {"downloaded": 5, "total": null, "speed": 0.0, "eta": null},
        "filename": "a.zip",
        "description": "the big one",
    }))
    .unwrap();
    row.cookies = "sid=1".into();
    row.headers = vec![("Authorization".into(), "Bearer t".into())];
    row.post = Some(crate::cookies::Posted {
        url: "https://files.example/form".into(),
        body: b"user=me".to_vec(),
        content_type: "application/x-www-form-urlencoded".into(),
    });
    let shown = row.redacted();
    assert!(shown.cookies.is_empty() && shown.headers.is_empty() && shown.post.is_none());
    assert_eq!(
        shown.url,
        concat!("https://me:", "•••", "@files.example/a.zip")
    );
    assert_eq!(
        (
            shown.id,
            shown.filename.as_str(),
            shown.description.as_str()
        ),
        (7, "a.zip", "the big one")
    );
    let said = serde_json::to_string(&shown).unwrap();
    assert!(!said.contains("pa55") && !said.contains("sid=1") && !said.contains("Bearer"));
    // An address with no password is kept as it is.
    assert_eq!(
        shown_url("https://files.example/a.zip"),
        "https://files.example/a.zip"
    );
}
