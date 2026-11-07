// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A download waits for the dial-up or VPN connection a person chose before it
//! goes out, as reference A's did (gaps row 93, G84): nothing is sent until the
//! connection is up, a connection that will not come up fails the row in its
//! words with nothing sent, a redial is said on the row while it waits, and a
//! pause while it waits pauses it. The hook is the program's own, so these
//! tests are a program of their own, taking turns.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::State;
use axum::response::Response;
use dm_core::{client, set_connect_first, Queue, Redial, State as Row};

mod common;
use common::tempdir;

static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// When each request arrived.
type Asked = Arc<Mutex<Vec<Instant>>>;

async fn serve() -> (std::net::SocketAddr, Asked) {
    let asked: Asked = Arc::default();
    async fn handler(State(asked): State<Asked>) -> Response {
        asked.lock().unwrap().push(Instant::now());
        Response::builder()
            .header("content-length", 1024)
            .body(Body::from(vec![7u8; 1024]))
            .unwrap()
    }
    let app = axum::Router::new()
        .fallback(handler)
        .with_state(Arc::clone(&asked));
    (common::serve(app).await, asked)
}

fn queue(dir: &std::path::Path) -> Queue {
    Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir)
        },
        1,
    )
}

async fn row(q: &Queue, id: dm_core::Id) -> Row {
    q.get(id).await.unwrap().state
}

#[tokio::test]
async fn a_download_waits_for_the_connection_and_goes_once_it_is_up() {
    let _turn = TURN.lock().await;
    let up_at: Arc<Mutex<Option<Instant>>> = Arc::default();
    let marks = Arc::clone(&up_at);
    set_connect_first(Some(Arc::new(move |_, _| {
        std::thread::sleep(Duration::from_millis(400));
        *marks.lock().unwrap() = Some(Instant::now());
        Ok(())
    })));
    let (addr, asked) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(20), q.run())
        .await
        .unwrap();
    set_connect_first(None);
    assert_eq!(row(&q, id).await, Row::Completed);
    let up = up_at
        .lock()
        .unwrap()
        .expect("the download did not wait for the connection");
    let asked = asked.lock().unwrap();
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|at| *at >= up),
        "a request went out before the connection was up"
    );
}

#[tokio::test]
async fn a_connection_that_will_not_come_up_fails_the_download_with_nothing_sent() {
    let _turn = TURN.lock().await;
    set_connect_first(Some(Arc::new(|_, _| {
        Err("the connection Office VPN did not come up after 2 tries: no answer".into())
    })));
    let (addr, asked) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(20), q.run())
        .await
        .unwrap();
    set_connect_first(None);
    match row(&q, id).await {
        Row::Failed(why) => assert!(
            why.contains("Office VPN") && why.contains("did not come up"),
            "{why}"
        ),
        other => panic!("not failed in the connection's words: {other:?}"),
    }
    assert!(
        asked.lock().unwrap().is_empty(),
        "a request went out without the connection"
    );
}

#[tokio::test]
async fn a_redial_is_said_on_the_row_while_it_waits() {
    let _turn = TURN.lock().await;
    set_connect_first(Some(Arc::new(|_, redialling| {
        redialling(Redial {
            attempt: 2,
            of: 3,
            why: "the connection Office VPN did not come up: no answer".into(),
            wait: Duration::from_secs(5),
        });
        std::thread::sleep(Duration::from_millis(800));
        Ok(())
    })));
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    let running = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    let mut seen = None;
    for _ in 0..40 {
        if let Some(retry) = q.retrying().await.get(&id) {
            seen = Some((retry.0, retry.1, retry.2.clone()));
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::timeout(Duration::from_secs(20), running)
        .await
        .unwrap()
        .unwrap();
    set_connect_first(None);
    let (attempt, of, why) = seen.expect("the redial was not said on the row");
    assert_eq!((attempt, of), (2, 3));
    assert!(why.contains("did not come up"), "{why}");
    assert_eq!(row(&q, id).await, Row::Completed);
}

#[tokio::test]
async fn a_pause_while_it_waits_pauses_the_download_and_ends_the_dialling() {
    let _turn = TURN.lock().await;
    let ended = Arc::new(AtomicUsize::new(0));
    let saw = Arc::clone(&ended);
    // Redials endlessly, as zero tries does, for as long as someone waits.
    set_connect_first(Some(Arc::new(move |waiting, _| {
        while waiting() {
            std::thread::sleep(Duration::from_millis(20));
        }
        saw.fetch_add(1, Ordering::SeqCst);
        Err("no one waits".into())
    })));
    let (addr, asked) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    let running = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    q.pause(id).await;
    tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("the queue went on waiting for a paused download")
        .unwrap();
    for _ in 0..50 {
        if ended.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    set_connect_first(None);
    assert_eq!(row(&q, id).await, Row::Paused);
    assert_eq!(ended.load(Ordering::SeqCst), 1, "the dialling went on");
    assert!(asked.lock().unwrap().is_empty());
}
