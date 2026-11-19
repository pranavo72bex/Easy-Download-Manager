// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the list forgets of its own accord (gaps row W17): a download that keeps no record goes
//! once its file is whole, leaving nothing of itself in dm's home; and the finished downloads
//! older than "Forget finished downloads after … days" go, their files left where they are.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::response::Response;
use dm_core::{client, AddOptions, DownloadList, State, MAIN_QUEUE, RECHECK_QUEUE};

use common::tempdir;

const BODY: &[u8] = b"the bytes of the file, exactly";

/// Seconds in a day.
const DAY: u64 = 24 * 60 * 60;

async fn serve() -> std::net::SocketAddr {
    async fn handler() -> Response {
        Response::builder()
            .header("content-length", BODY.len())
            .body(Body::from(BODY))
            .unwrap()
    }
    common::serve(axum::Router::new().fallback(handler)).await
}

fn queue(dir: &Path) -> DownloadList {
    DownloadList::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir)
        },
        2,
    )
}

/// `persist_to`, and its first write waited for (it is made in the background).
async fn persisted(q: &DownloadList, path: PathBuf) {
    let before = q.saves_written();
    q.persist_to(path).await;
    let asked = Instant::now();
    while q.saves_written() == before {
        assert!(
            asked.elapsed() < Duration::from_secs(10),
            "the list read back was never written"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// The files directly in `home` whose bytes hold `what`.
fn holding(home: &Path, what: &str) -> Vec<PathBuf> {
    std::fs::read_dir(home)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            std::fs::read(p).is_ok_and(|b| b.windows(what.len()).any(|w| w == what.as_bytes()))
        })
        .collect()
}

/// A download that keeps no record is written down while it runs, as any is (so it can go on
/// after a restart); once its file is whole its row goes, and no file in dm's home keeps its
/// address, its page or its sign-in: not the queue file, not the journal of a long list, not
/// the secrets. The file stays.
#[tokio::test]
async fn a_download_that_keeps_no_record_leaves_no_row_and_nothing_of_itself_in_dms_home() {
    let addr = serve().await;
    let dir = tempdir();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let q = queue(dir.path());
    persisted(&q, home.join("queue.json")).await;
    // A long list, whose saves append what changed to a journal: the download's address reaches
    // the journal before its file is whole.
    let waiting = (0..600)
        .map(|n| {
            let with = AddOptions {
                later: true,
                queue: MAIN_QUEUE.into(),
                ..AddOptions::default()
            };
            (format!("http://{addr}/waiting-{n}.bin"), with)
        })
        .collect();
    q.add_each(waiting).await;
    q.flush().await;
    let id = q
        .add_with(
            &format!("http://{addr}/secret-private-file.bin"),
            AddOptions {
                keep_no_record: true,
                referrer: format!("http://{addr}/private-page"),
                cookies: "session=private-sign-in".into(),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    q.flush().await;
    assert!(
        !holding(&home, "secret-private-file").is_empty(),
        "written down while it runs"
    );

    q.run().await;
    let asked = Instant::now();
    while q.get(id).await.is_some() {
        assert!(
            asked.elapsed() < Duration::from_secs(10),
            "the row was kept"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    q.flush().await;
    assert_eq!(q.list().await.len(), 600, "the other rows stay");
    let file = dir.path().join("out").join("secret-private-file.bin");
    assert_eq!(std::fs::read(&file).unwrap(), BODY, "the file stays");
    for what in ["secret-private-file", "private-page", "private-sign-in"] {
        let left = holding(&home, what);
        assert!(left.is_empty(), "{what} is left in {left:?}");
    }
}

/// A row as the queue file keeps it: `state`, settled at `finished` (seconds since 1970), in
/// `queue`, its file `f<id>.bin` written under `dir`.
fn kept_row(id: u64, state: &State, finished: u64, queue: &str, dir: &Path) -> serde_json::Value {
    let path = dir.join(format!("f{id}.bin"));
    std::fs::write(&path, BODY).unwrap();
    serde_json::json!({
        "id": id,
        "url": format!("http://files.example/f{id}.bin"),
        "state": serde_json::to_value(state).unwrap(),
        "progress": {"downloaded": 30, "total": 30, "speed": 0.0, "eta": null},
        "filename": format!("f{id}.bin"),
        "finished_at": finished,
        "path": path.to_string_lossy(),
        "queue": queue,
    })
}

/// "Forget finished downloads after … days": a finished download older than that goes, its file
/// left; a newer one stays, as do a failed one (Retry resumes it from its parts), one in the
/// Recheck queue and anything at all where the days are 0 (never).
#[tokio::test]
async fn finished_downloads_older_than_the_days_set_are_forgotten_and_their_files_stay() {
    let dir = tempdir();
    let saved = dir.path().join("saved");
    std::fs::create_dir_all(&saved).unwrap();
    let now = 1_800_000_000;
    let failed = State::Failed("the server went away".into());
    let rows = serde_json::Value::Array(vec![
        kept_row(1, &State::Completed, now - 10 * DAY, "", &saved),
        kept_row(2, &State::Completed, now - 2 * DAY, "", &saved),
        kept_row(3, &failed, now - 10 * DAY, "", &saved),
        kept_row(4, &State::Completed, now - 10 * DAY, RECHECK_QUEUE, &saved),
    ]);
    let file = dir.path().join("queue.json");
    std::fs::write(&file, serde_json::to_vec(&rows).unwrap()).unwrap();
    let q = queue(dir.path());
    persisted(&q, file).await;

    assert_eq!(q.forget_finished_older_than(0, now).await, 0, "0 is never");
    assert_eq!(q.forget_finished_older_than(7, now).await, 1);
    let ids: Vec<u64> = q.list().await.iter().map(|r| r.id).collect();
    assert_eq!(ids, [2, 3, 4]);
    assert!(saved.join("f1.bin").is_file(), "the file stays");
    q.flush().await;
    let written = std::fs::read_to_string(dir.path().join("queue.json")).unwrap();
    assert!(!written.contains("f1.bin"), "{written}");
    // Looked at again (once a day), nothing more is old enough.
    assert_eq!(q.forget_finished_older_than(7, now).await, 0);
}
