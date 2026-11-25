// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A finished file's SHA-256 is kept on its row, worked out just after it says Completed
//! (gaps row H5), so a check years later can say whether the file changed.

use std::time::Duration;

use axum::routing::get;
use dm_core::{client, DownloadList, State};
use sha2::{Digest, Sha256};

mod common;
use common::{options, serve, tempdir};

#[tokio::test]
async fn a_finished_files_digest_is_kept_on_its_row() {
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let served = body.clone();
    let addr = serve(axum::Router::new().route(
        "/kept.bin",
        get(move || {
            let body = served.clone();
            async move { body }
        }),
    ))
    .await;
    let dir = tempdir();
    let q = DownloadList::new(client(), options(dir.path()), 1);
    let id = q.add(&format!("http://{addr}/kept.bin")).await.unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, State::Completed, "{row:?}");
    let wanted: String = Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let began = std::time::Instant::now();
    let kept = loop {
        if let Some(kept) = q.get(id).await.and_then(|r| r.kept_sha256) {
            break kept;
        }
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "no digest kept on the row"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(kept, wanted);
}
