// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every download held for the computer's own state (gaps row W11): while a metered connection
//! or a low battery holds the queue nothing starts and what runs is paused, as the schedule's,
//! its parts kept; once the state ends the rows it held go on by themselves.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::response::Response;
use dm_core::{client, DownloadList, Hold, State};

use common::tempdir;

const BODY: &[u8] = b"the bytes of the file, all of them";

/// Serves the file after `wait`, so a test can find its download running.
async fn serve(wait: Duration) -> std::net::SocketAddr {
    let handler = move || async move {
        tokio::time::sleep(wait).await;
        Response::builder()
            .header("content-length", BODY.len())
            .body(Body::from(BODY))
            .unwrap()
    };
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

/// Waits up to five seconds for row `id` to be in `state`.
async fn until(q: &DownloadList, id: dm_core::Id, state: State) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if q.get(id).await.is_some_and(|row| row.state == state) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// A metered connection holds a download added while it lasts: paused by itself, not by a
/// person, with nothing fetched; once the connection is no longer metered it goes on.
#[tokio::test]
async fn a_download_waits_while_the_connection_is_metered_and_goes_on_after() {
    let addr = serve(Duration::ZERO).await;
    let dir = tempdir();
    let q = queue(dir.path());
    q.hold_for(Some(Hold::Metered));
    assert_eq!(q.held_by(), Some(Hold::Metered));
    assert!(
        !q.may_run(),
        "nothing may run while the connection is metered"
    );

    let id = q.add(&format!("http://{addr}/file.bin")).await.unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, State::Paused, "it ran on a metered connection");
    assert!(row.paused_by_schedule, "held by itself, to go on by itself");
    assert_eq!(row.progress.downloaded, 0);
    assert!(q.held().await);

    // The connection is no longer metered: what the watcher of the rules does.
    q.hold_for(None);
    assert!(q.may_run());
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().state, State::Completed);
}

/// A low battery puts a running download down, as the schedule closing does, and it goes on
/// once the computer is charging again.
#[tokio::test]
async fn a_running_download_is_put_down_on_a_low_battery_and_goes_on_after() {
    let addr = serve(Duration::from_millis(400)).await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/file.bin")).await.unwrap();
    let runner = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    assert!(until(&q, id, State::Running).await, "it never started");

    q.hold_for(Some(Hold::Battery));
    assert!(
        until(&q, id, State::Paused).await,
        "it ran on below the level"
    );
    assert!(q.get(id).await.unwrap().paused_by_schedule);
    runner.await.unwrap();

    q.hold_for(None);
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().state, State::Completed);
}

/// A download a person paused stays paused when the hold ends: only what the hold put down is
/// put back.
#[tokio::test]
async fn a_persons_pause_is_not_undone_when_the_hold_ends() {
    let addr = serve(Duration::ZERO).await;
    let dir = tempdir();
    let q = queue(dir.path());
    let mine = q.add(&format!("http://{addr}/mine.bin")).await.unwrap();
    q.pause(mine).await;
    q.hold_for(Some(Hold::Metered));
    let theirs = q.add(&format!("http://{addr}/theirs.bin")).await.unwrap();
    q.run().await;
    q.hold_for(None);
    q.run().await;
    assert_eq!(q.get(mine).await.unwrap().state, State::Paused);
    assert!(!q.get(mine).await.unwrap().paused_by_schedule);
    assert_eq!(q.get(theirs).await.unwrap().state, State::Completed);
}

/// Each hold says why in English, for the row to say after "Waiting: ".
#[test]
fn each_hold_says_why() {
    assert_eq!(Hold::Metered.said(), "the connection is metered");
    assert_eq!(Hold::Battery.said(), "the battery is low");
    // A phone's own rules (gaps row N10).
    assert_eq!(Hold::NotWifi.said(), "the phone is not on Wi-Fi");
    assert_eq!(Hold::Roaming.said(), "the phone is roaming");
    assert_eq!(Hold::DataSaver.said(), "Data Saver is on");
}
