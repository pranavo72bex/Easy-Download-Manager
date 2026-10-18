//! What the schedule or the download limit held is started again by the
//! app itself when they let it go, with no person touching the queue: the
//! scheduler stops once nothing is queued or running, and a held row is
//! neither.

use std::time::Duration;

use axum::routing::get;
use dm_core::{Categories, Control, Limiter, Options, Queue, State};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_download_goes_on_when_the_limit_lets_it() {
    let files = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let served = files.local_addr().unwrap();
    let app = axum::Router::new().route("/:name", get(|| async { vec![7u8; 4096] }));
    tokio::spawn(async move { axum::serve(files, app).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let queue = Queue::new(
        dm_core::client(),
        Options {
            connections: 1,
            control: Control::new(),
            dest_dir: dir.path().join("out"),
            parts_dir: dir.path().join("parts"),
            limiter: Limiter::unlimited(),
            categories: Categories::default(),
            checksum: None,
            quality: dm_core::stream::Quality::Best,
            media: false,
            referrer: None,
            proxy: None,
            container: None,
            attempts: 5,
            host_limits: Vec::new(),
            proxy_bypass: Vec::new(),
            subtitles: false,
            room: Default::default(),
            remux: false,
            server_dates: false,
            retry_wait: 0,
            sign_in: None,
            system_proxy: false,
        },
        1,
    );
    queue.set_allowance(dm_core::quota::Allowance {
        bytes: 1,
        period: Duration::from_secs(1),
    });
    dm_shell::shell::watch_rules(&queue);

    let first = queue.add(&format!("http://{served}/a.bin")).await.unwrap();
    let second = queue.add(&format!("http://{served}/b.bin")).await.unwrap();
    queue.run().await;
    assert_eq!(queue.get(first).await.unwrap().state, State::Completed);
    assert_eq!(queue.get(second).await.unwrap().state, State::Paused);

    // Nobody calls run() from here on.
    for _ in 0..80 {
        if queue.get(second).await.unwrap().state == State::Completed {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the held download was never started again");
}
