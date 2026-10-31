//! The queue: several downloads, a limit, and the controls between states.
//!
//! Byte-correctness is the engine's business and covered there; what matters
//! here is that the limit is honoured, that a paused download stops without
//! taking the queue down with it, and that a failure is one row's problem.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Response;
use dm_core::{client, Queue, State as DownloadState};

mod common;
use common::tempdir;

#[derive(Clone)]
struct Server {
    live: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    /// How often `flaky` has been asked for: the first time is refused.
    flaky: Arc<AtomicUsize>,
    /// Every path asked for, so a test can say what was never fetched.
    asked: Arc<std::sync::Mutex<Vec<String>>>,
}

/// Serves a small file slowly, and records how many requests overlap.
async fn serve() -> (SocketAddr, Arc<AtomicUsize>) {
    let (addr, peak, _) = serve_recording().await;
    (addr, peak)
}

async fn serve_recording() -> (
    SocketAddr,
    Arc<AtomicUsize>,
    Arc<std::sync::Mutex<Vec<String>>>,
) {
    let peak = Arc::new(AtomicUsize::new(0));
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = Server {
        live: Arc::new(AtomicUsize::new(0)),
        peak: Arc::clone(&peak),
        flaky: Arc::new(AtomicUsize::new(0)),
        asked: Arc::clone(&asked),
    };

    // A fallback rather than a routed path: the parameter syntax changed
    // between axum versions, and this test cares about the queue, not routing.
    async fn handler(State(s): State<Server>, uri: Uri) -> Response {
        s.asked.lock().unwrap().push(uri.path().to_string());
        // Gone the first time and there after: what a retry is for. A 404,
        // which the engine's own retries do not cover, so only a person's
        // Retry gets it the second look.
        let gone_once = uri.path().contains("flaky") && s.flaky.fetch_add(1, Ordering::SeqCst) == 0;
        if uri.path().contains("missing") || gone_once {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("no"))
                .unwrap();
        }

        let live = s.live.fetch_add(1, Ordering::SeqCst) + 1;
        s.peak.fetch_max(live, Ordering::SeqCst);
        // Long enough that concurrent downloads genuinely overlap.
        tokio::time::sleep(Duration::from_millis(120)).await;
        s.live.fetch_sub(1, Ordering::SeqCst);

        Response::builder()
            .status(StatusCode::OK)
            .header("content-length", 1024)
            .body(Body::from(vec![7u8; 1024]))
            .unwrap()
    }

    let addr = common::serve(axum::Router::new().fallback(handler).with_state(state)).await;
    (addr, peak, asked)
}

fn queue(dir: &std::path::Path, concurrency: usize) -> Queue {
    Queue::new(
        client(),
        // One connection by default: these tests are about the queue's
        // states, not about how a single file is split.
        dm_core::Options {
            connections: 1,
            ..common::options(dir)
        },
        concurrency,
    )
}

/// The window waits to be told rather than looking every second while
/// nothing moves: a row added or taken away tells it.
#[tokio::test]
async fn a_change_to_the_queue_is_told() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let told = std::sync::Arc::new(AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&told);
    q.on_change(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let id = q
        .add_with(
            "http://127.0.0.1:9/f.bin",
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let after_add = told.load(Ordering::SeqCst);
    assert!(after_add > 0, "adding a row told nothing");
    q.remove(id).await;
    assert!(
        told.load(Ordering::SeqCst) > after_add,
        "removing it told nothing"
    );
}

/// A sound the player fetched apart is kept on the row, and only where it is
/// a web address: anything else would be fetched by dm on a page's word.
#[tokio::test]
async fn a_sound_is_kept_on_the_row_only_if_it_is_a_web_address() {
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let mut kept = Vec::new();
    for sound in ["https://cdn.example/s", "file:///etc/passwd", "ftp://h/s"] {
        let id = q
            .add_with(
                "http://127.0.0.1:9/picture",
                dm_core::AddOptions {
                    later: true,
                    sound: Some(sound.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        kept.push(
            q.list()
                .await
                .into_iter()
                .find(|e| e.id == id)
                .unwrap()
                .sound,
        );
    }
    assert_eq!(
        kept,
        [Some("https://cdn.example/s".to_string()), None, None]
    );
}

#[tokio::test]
async fn downloads_run_together_but_not_all_at_once() {
    let (addr, peak) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);

    for n in 0..6 {
        q.add(&format!("http://{addr}/f{n}.bin")).await.unwrap();
    }
    q.run().await;

    let peak = peak.load(Ordering::SeqCst);
    assert!(
        peak > 1,
        "the queue ran everything one at a time (peak {peak})"
    );
    assert!(peak <= 2, "the concurrency limit was ignored (peak {peak})");

    assert!(q
        .list()
        .await
        .iter()
        .all(|e| e.state == DownloadState::Completed));
}

#[tokio::test]
async fn the_order_things_were_added_is_the_order_they_are_listed() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let mut ids = Vec::new();
    for n in 0..4 {
        ids.push(q.add(&format!("http://{addr}/f{n}.bin")).await.unwrap());
    }

    assert_eq!(q.list().await.iter().map(|e| e.id).collect::<Vec<_>>(), ids);
}

#[tokio::test]
async fn a_failure_is_one_rows_problem() {
    /* The queue exists to keep going. A 404 in the middle of a batch that
     * stopped the others would make it worse than downloading them by hand. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);

    q.add(&format!("http://{addr}/ok1.bin")).await.unwrap();
    let bad = q.add(&format!("http://{addr}/missing.bin")).await.unwrap();
    q.add(&format!("http://{addr}/ok2.bin")).await.unwrap();
    q.run().await;

    let rows = q.list().await;
    assert!(matches!(
        q.get(bad).await.unwrap().state,
        DownloadState::Failed(_)
    ));
    assert_eq!(
        rows.iter()
            .filter(|e| e.state == DownloadState::Completed)
            .count(),
        2,
        "the other downloads did not finish"
    );

    // And the reason survives, rather than just the fact.
    let DownloadState::Failed(why) = q.get(bad).await.unwrap().state else {
        panic!()
    };
    assert!(why.contains("404"), "the reason was lost: {why}");
}

#[tokio::test]
async fn a_paused_download_holds_its_place_and_can_be_resumed() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let held = q.add(&format!("http://{addr}/held.bin")).await.unwrap();
    q.pause(held).await;
    q.add(&format!("http://{addr}/other.bin")).await.unwrap();

    q.run().await;

    // The paused one stayed put; the queue served the rest and stopped.
    assert_eq!(q.get(held).await.unwrap().state, DownloadState::Paused);
    assert!(
        !q.busy().await,
        "the queue kept spinning on a paused download"
    );

    // Resumed, it runs.
    q.resume(held).await;
    assert_eq!(q.get(held).await.unwrap().state, DownloadState::Queued);
    q.run().await;
    assert_eq!(q.get(held).await.unwrap().state, DownloadState::Completed);
}

#[tokio::test]
async fn a_failed_download_is_retried_from_a_clean_row_and_a_settled_one_is_not() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let flaky = q.add(&format!("http://{addr}/flaky.bin")).await.unwrap();
    let fine = q.add(&format!("http://{addr}/fine.bin")).await.unwrap();
    q.run().await;
    assert!(matches!(
        q.get(flaky).await.unwrap().state,
        DownloadState::Failed(_)
    ));

    // Only a failure has anything to retry; the window is told so it does
    // not wake the scheduler for nothing.
    assert!(
        !q.retry(fine).await,
        "a finished download was put back in line"
    );
    assert!(q.retry(flaky).await);

    // Back in line as a fresh row: no leftover reason, no leftover count
    // sitting at whatever the failed run reached, no finish time.
    let row = q.get(flaky).await.unwrap();
    assert_eq!(row.state, DownloadState::Queued);
    assert_eq!(row.progress.downloaded, 0);
    assert_eq!(row.finished_at, None);

    q.run().await;
    assert_eq!(q.get(flaky).await.unwrap().state, DownloadState::Completed);
    assert_eq!(
        std::fs::read(dir.path().join("out/flaky.bin"))
            .unwrap()
            .len(),
        1024
    );
}

#[tokio::test]
async fn pausing_everything_leaves_nothing_running_and_starting_it_puts_it_back() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let mut ids = Vec::new();
    for i in 0..3 {
        ids.push(q.add(&format!("http://{addr}/all-{i}.bin")).await.unwrap());
    }

    // One command for the lot, and it says how many it touched — the window
    // greys the button out when that would be nought.
    assert_eq!(q.pause_all().await, 3);
    for id in &ids {
        assert_eq!(q.get(*id).await.unwrap().state, DownloadState::Paused);
    }

    // Nothing to pause a second time, so nothing is reported.
    assert_eq!(q.pause_all().await, 0);

    q.run().await;
    assert!(
        !q.busy().await,
        "the queue ran a download that had been paused"
    );

    // One added for later, waiting for its queue to be started, is not
    // something Resume all starts.
    let later = q
        .add_with(
            &format!("http://{addr}/later.bin"),
            dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(q.resume_all().await, 3);
    q.run().await;
    for id in &ids {
        assert_eq!(q.get(*id).await.unwrap().state, DownloadState::Completed);
    }
    assert_eq!(q.get(later).await.unwrap().state, DownloadState::Paused);

    // A settled row is not something start-all reaches back into.
    assert_eq!(q.resume_all().await, 0);
}

#[tokio::test]
async fn a_row_knows_what_kind_of_file_it_is_before_it_finishes() {
    /* The rail counts by kind, and a count that only filled in at assembly
     * would leave everything in flight uncounted — which is most of what
     * anyone is looking at. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let id = q.add(&format!("http://{addr}/clip.mp4")).await.unwrap();
    q.run().await;

    let entry = q.get(id).await.unwrap();
    assert_eq!(entry.category, Some(dm_core::Category::Videos));
}

#[tokio::test]
async fn a_cancelled_download_is_not_started() {
    let (addr, _, asked) = serve_recording().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let gone = q.add(&format!("http://{addr}/gone.bin")).await.unwrap();
    q.cancel(gone).await;
    q.run().await;

    assert_eq!(q.get(gone).await.unwrap().state, DownloadState::Cancelled);
    // Not fetched and thrown away: the state alone cannot tell those apart,
    // since a cancelled row stays cancelled whatever its task did.
    let asked = asked.lock().unwrap().clone();
    assert!(
        !asked.iter().any(|p| p.contains("gone")),
        "a cancelled download was fetched: {asked:?}"
    );
}

#[tokio::test]
async fn a_removed_download_is_forgotten() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.remove(id).await;

    assert!(q.get(id).await.is_none());
    assert!(q.list().await.is_empty());
}

#[tokio::test]
async fn progress_reaches_the_row() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert_eq!(
        row.progress.downloaded, 1024,
        "the row never saw its own bytes"
    );
    assert!(
        !row.filename.is_empty(),
        "the row never learned its filename"
    );
}

#[tokio::test]
async fn two_schedulers_do_not_between_them_exceed_the_limit() {
    /* The window spawns a scheduler on every add, so several run() loops
     * overlap routinely. If each takes its own view of how many are running
     * and then starts more, the limit is advisory and the link is oversold. */
    let (addr, peak) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);

    for n in 0..8 {
        q.add(&format!("http://{addr}/f{n}.bin")).await.unwrap();
    }

    // Four at once, the way four quick pastes would.
    let runs: Vec<_> = (0..4)
        .map(|_| {
            let q = q.clone();
            tokio::spawn(async move { q.run().await })
        })
        .collect();
    for r in runs {
        r.await.unwrap();
    }

    let peak = peak.load(Ordering::SeqCst);
    assert!(peak <= 2, "the limit was oversold: {peak} ran at once");
}

#[tokio::test]
async fn a_link_added_before_the_saved_queue_loads_is_kept() {
    /* A browser hand-off on a cold start is queued while the saved queue is
     * still being read. Both were numbered from 1, and restoring wrote the
     * old row over the new one: the handed link vanished and the old row was
     * listed twice. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path(), 1);
    first.persist_to(store.clone()).await;
    let old = first.add(&format!("http://{addr}/old.bin")).await.unwrap();
    first.pause(old).await;
    first.flush().await;

    let second = queue(dir.path(), 1);
    second
        .add_all(&[&format!("http://{addr}/handed.bin")])
        .await;
    second.persist_to(store.clone()).await;

    let rows = second.list().await;
    let urls: Vec<&str> = rows.iter().map(|r| r.url.as_str()).collect();
    assert_eq!(rows.len(), 2, "a row was lost or doubled: {urls:?}");
    assert!(urls.iter().any(|u| u.ends_with("/old.bin")), "{urls:?}");
    assert!(urls.iter().any(|u| u.ends_with("/handed.bin")), "{urls:?}");
    let old_row = rows.iter().find(|r| r.url.ends_with("/old.bin")).unwrap();
    assert_eq!(
        old_row.id, old,
        "the restored row must keep its id: its parts are named by it"
    );
    assert_ne!(rows[0].id, rows[1].id, "two rows share an id");

    // And the next id is past both.
    let fresh = second
        .add(&format!("http://{addr}/fresh.bin"))
        .await
        .unwrap();
    assert!(rows.iter().all(|r| r.id != fresh), "an id was reissued");
}

#[tokio::test]
async fn a_restored_row_does_not_claim_a_rate_from_before_the_restart() {
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    std::fs::write(
        &store,
        r#"[{"id":3,"url":"http://127.0.0.1:9/f.bin","state":{"kind":"running"},
            "progress":{"downloaded":10,"total":100,"speed":5000.0,"eta":{"secs":9,"nanos":0}},
            "filename":"f.bin"}]"#,
    )
    .unwrap();
    let q = queue(dir.path(), 0);
    q.persist_to(store).await;
    let row = q.get(3).await.expect("the row was not restored");
    assert_eq!(row.state, DownloadState::Queued);
    assert_eq!(row.progress.speed, 0.0);
    assert!(row.progress.eta.is_none());
    assert_eq!(
        row.progress.downloaded, 10,
        "what had arrived is still counted"
    );
}

/// A login written into an address, and the cookies a browser handed over,
/// are not written into the queue file: it is shown, sent with a report and
/// backed up on its own. They are kept beside it and come back with the row.
#[tokio::test]
async fn a_login_and_cookies_stay_out_of_the_queue_file_and_come_back() {
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let first = queue(dir.path(), 0);
    first.persist_to(store.clone()).await;
    let url = common::with_login("http://127.0.0.1:9/private.bin", "me", "open sesame");
    let id = first
        .add_with(
            &url,
            dm_core::queue::AddOptions {
                cookies: "session=given-by-the-browser".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let plain = first.add("http://127.0.0.1:9/plain.bin").await.unwrap();
    first.flush().await;

    let written = std::fs::read_to_string(&store).unwrap();
    for secret in ["sesame", "me@", "given-by-the-browser"] {
        assert!(
            !written.contains(secret),
            "{secret} is in the queue file:\n{written}"
        );
    }
    assert!(written.contains("/private.bin"), "{written}");
    let kept = std::fs::read_to_string(dir.path().join("secrets.json")).unwrap();
    assert!(
        kept.contains("sesame") && kept.contains("given-by-the-browser"),
        "{kept}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("secrets.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "secrets.json is {mode:o}");
    }

    let second = queue(dir.path(), 0);
    second.persist_to(store.clone()).await;
    let row = second.get(id).await.unwrap();
    assert_eq!(row.url, url, "the login did not come back with the row");
    assert_eq!(row.cookies, "session=given-by-the-browser");
    let plain_row = second.get(plain).await.unwrap();
    assert_eq!(plain_row.url, "http://127.0.0.1:9/plain.bin");
    assert!(plain_row.cookies.is_empty());

    // Once no row has a secret, nothing is kept beside the queue.
    second.remove(id).await;
    second.flush().await;
    assert!(
        !dir.path().join("secrets.json").exists(),
        "secrets.json outlived its row"
    );
}

#[tokio::test]
async fn the_queue_survives_the_app_closing() {
    /* Part files outlive the process; the row pointing at them did not. That
     * made a reboot mid-download lose a transfer whose bytes were still on
     * disk — the one failure a resume-capable engine has no excuse for. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path(), 1);
    first.persist_to(store.clone()).await;
    let kept = first.add(&format!("http://{addr}/kept.bin")).await.unwrap();
    first.pause(kept).await;
    // What the app does as it ends.
    first.flush().await;

    // A second run of the app, against the same store.
    let second = queue(dir.path(), 1);
    second.persist_to(store.clone()).await;

    let rows = second.list().await;
    assert_eq!(rows.len(), 1, "the queue did not survive");
    assert_eq!(
        rows[0].id, kept,
        "the download came back under a different id"
    );
    assert_eq!(
        rows[0].state,
        DownloadState::Paused,
        "its state was not kept"
    );

    // And a new download does not reuse the restored id.
    let fresh = second
        .add(&format!("http://{addr}/fresh.bin"))
        .await
        .unwrap();
    assert_ne!(fresh, kept, "an id was handed out twice");
}

#[tokio::test]
async fn a_download_that_was_running_comes_back_ready_to_resume() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path(), 1);
    first.persist_to(store.clone()).await;
    let id = first.add(&format!("http://{addr}/f.bin")).await.unwrap();
    first.run().await;
    first.flush().await;

    // Forge the state the app would have been killed in. Asserted, because
    // `replace` on a string that does not contain the pattern returns it
    // unchanged — so without this the test would forge nothing and still pass
    // by reading back a queue that had never left Queued.
    let text = std::fs::read_to_string(&store).unwrap();
    assert!(
        text.contains(r#""kind": "completed""#),
        "the finished download never reached the file: {text}"
    );
    let text = text.replace(r#""kind": "completed""#, r#""kind": "running""#);
    std::fs::write(&store, text).unwrap();

    let second = queue(dir.path(), 1);
    second.persist_to(store).await;

    // Queued, not Running: nothing is running in a process that just started.
    assert_eq!(second.get(id).await.unwrap().state, DownloadState::Queued);
}

#[tokio::test]
async fn cancelling_reclaims_the_bytes_it_staged() {
    // A cancelled download that leaves its parts behind costs its full size
    // on disk for ever, and nothing ever comes back for them.
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    let parts = dir.path().join("parts").join(id.to_string());
    std::fs::create_dir_all(&parts).unwrap();
    std::fs::write(parts.join("part-0000"), vec![0u8; 4096]).unwrap();

    q.cancel(id).await;
    assert!(!parts.exists(), "the staged bytes were left behind");
}

#[tokio::test]
async fn an_unreadable_queue_file_does_not_stop_the_app_opening() {
    // Recoverable: an empty list can be fixed from inside the app, a refusal
    // to start cannot.
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    std::fs::write(&store, "{ this is not json").unwrap();

    let q = queue(dir.path(), 1);
    q.persist_to(store).await;
    assert!(q.list().await.is_empty());
}

#[tokio::test]
async fn a_finished_download_is_still_finished_after_a_restart() {
    // The scheduler settles a download in memory. If that never reaches the
    // file, a restart reads the row back as it was queued and downloads the
    // whole thing again — which is what it did.
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path(), 1);
    first.persist_to(store.clone()).await;
    let id = first.add(&format!("http://{addr}/f.bin")).await.unwrap();
    first.run().await;
    assert_eq!(first.get(id).await.unwrap().state, DownloadState::Completed);
    first.flush().await;

    let second = queue(dir.path(), 1);
    second.persist_to(store).await;
    assert_eq!(
        second.get(id).await.unwrap().state,
        DownloadState::Completed
    );
}

#[tokio::test]
async fn a_failure_is_still_a_failure_after_a_restart() {
    // Same defect, other outcome: a row that failed for a reason nobody will
    // fix by waiting should not be retried on every launch for ever.
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path(), 1);
    first.persist_to(store.clone()).await;
    let id = first
        .add(&format!("http://{addr}/missing.bin"))
        .await
        .unwrap();
    first.run().await;
    assert!(matches!(
        first.get(id).await.unwrap().state,
        DownloadState::Failed(_)
    ));
    first.flush().await;

    let second = queue(dir.path(), 1);
    second.persist_to(store).await;
    assert!(matches!(
        second.get(id).await.unwrap().state,
        DownloadState::Failed(_)
    ));
}

#[tokio::test]
async fn a_download_added_while_others_run_is_picked_up_by_the_running_scheduler() {
    // The window and the browser watcher both start a scheduler and neither
    // waits for it. Whatever arrives mid-flight has to be taken by the
    // scheduler already looping, or it waits for the queue to drain first —
    // which is what happened, and it is the ordinary case, not a corner.
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);

    for n in 0..4 {
        q.add(&format!("http://{addr}/f{n}.bin")).await.unwrap();
    }
    let running = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };

    // Added after the scheduler is already serving the queue — and it is,
    // or this test would be checking a fresh scheduler rather than the
    // running one.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let late = q.add(&format!("http://{addr}/late.bin")).await.unwrap();
    assert!(
        q.scheduling(),
        "the first scheduler had already left; nothing here tests a pick-up"
    );

    // A second scheduler finds one already running and returns at once, so
    // the late arrival is the running one's responsibility.
    let second = std::time::Instant::now();
    q.run().await;
    assert!(
        second.elapsed() < Duration::from_millis(50),
        "the second run served the queue itself instead of returning"
    );
    running.await.unwrap();

    assert_eq!(
        q.get(late).await.unwrap().state,
        DownloadState::Completed,
        "a download added while the queue was busy was never started"
    );
}

/* ------------------------------------------- categories, checks, history */

/// The digest of the 1024 bytes of 7 the test server serves.
fn served_sha256() -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(vec![7u8; 1024]);
    hex::encode(hasher.finalize())
}

#[tokio::test]
async fn a_finished_file_is_sorted_into_its_category_folder() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let mut options = q.options().await;
    options.categories = dm_core::Categories {
        enabled: true,
        ..Default::default()
    };
    q.set_options(options).await;

    let id = q.add(&format!("http://{addr}/holiday.mkv")).await.unwrap();
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert_eq!(row.category, Some(dm_core::Category::Videos));
    assert!(
        dir.path().join("out/Videos/holiday.mkv").exists(),
        "landed at {} instead",
        row.path
    );
}

#[tokio::test]
async fn sorting_off_leaves_the_file_in_the_download_folder() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let id = q.add(&format!("http://{addr}/holiday.mkv")).await.unwrap();
    q.run().await;

    // Still classified — the row says what it is either way — but not moved.
    assert_eq!(
        q.get(id).await.unwrap().category,
        Some(dm_core::Category::Videos)
    );
    assert!(dir.path().join("out/holiday.mkv").exists());
    assert!(!dir.path().join("out/Videos").exists());
}

#[tokio::test]
async fn a_download_that_matches_its_published_digest_is_marked_verified() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.set_checksum(id, dm_core::Checksum::parse(&served_sha256()))
        .await;
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert!(
        row.verified,
        "a matching digest was not recorded as checked"
    );
}

#[tokio::test]
async fn a_download_that_fails_its_digest_fails_and_leaves_no_file() {
    /* A file known to be wrong must not sit in the downloads folder looking
     * like every other one. Reporting "done" beside a warning is how people
     * end up installing it. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.set_checksum(id, dm_core::Checksum::parse(&"0".repeat(64)))
        .await;
    q.run().await;

    let row = q.get(id).await.unwrap();
    match &row.state {
        DownloadState::Failed(why) => assert!(
            why.contains("sha256") && why.contains("was published"),
            "unhelpful reason: {why}"
        ),
        other => panic!("a corrupt download was reported as {other:?}"),
    }
    assert!(!row.verified);
    assert!(
        !dir.path().join("out/f.bin").exists(),
        "the file that failed its digest was left behind"
    );
}

#[tokio::test]
async fn a_finished_row_records_when_it_finished() {
    // Without this the queue is a list of what is happening, not a history.
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let ok = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    let bad = q.add(&format!("http://{addr}/missing.bin")).await.unwrap();
    q.run().await;

    for id in [ok, bad] {
        assert!(
            q.get(id).await.unwrap().finished_at.is_some_and(|t| t > 0),
            "a settled row has no finishing time"
        );
    }
}

#[tokio::test]
async fn a_finished_download_leaves_no_parts_folder_behind() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, dm_core::State::Completed);
    assert!(
        !std::path::Path::new(&row.parts_dir).exists(),
        "{} is still there",
        row.parts_dir
    );
}

#[tokio::test]
async fn clearing_the_history_keeps_what_has_not_finished() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let done = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.run().await;
    let waiting = q.add(&format!("http://{addr}/later.bin")).await.unwrap();
    q.pause(waiting).await;

    assert_eq!(q.clear_finished().await, 1);
    assert!(
        q.get(done).await.is_none(),
        "the finished row is still there"
    );
    assert!(
        q.get(waiting).await.is_some(),
        "a paused row was cleared too"
    );
}

#[tokio::test]
async fn clearing_the_history_keeps_a_failed_row_and_its_parts() {
    /* "Clear finished" took Failed rows too, and deleted their staged parts
     * with no question: what Retry would have resumed from. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let done = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    let failed = q.add(&format!("http://{addr}/missing.bin")).await.unwrap();
    let cancelled = q.add(&format!("http://{addr}/never.bin")).await.unwrap();
    q.cancel(cancelled).await;
    q.run().await;
    let row = q.get(failed).await.unwrap();
    assert!(
        matches!(row.state, DownloadState::Failed(_)),
        "{:?}",
        row.state
    );
    // Parts a failure part-way through leaves behind.
    let parts = std::path::Path::new(&row.parts_dir);
    std::fs::create_dir_all(parts).unwrap();
    std::fs::write(parts.join("part-0000"), b"staged").unwrap();

    assert_eq!(q.clear_finished().await, 2);
    assert!(q.get(done).await.is_none(), "the completed row stayed");
    assert!(q.get(cancelled).await.is_none(), "the cancelled row stayed");
    assert!(q.get(failed).await.is_some(), "the failed row was cleared");
    assert_eq!(
        std::fs::read(parts.join("part-0000")).unwrap(),
        b"staged",
        "the failed row's staged parts were deleted"
    );
}

/* ------------------------------------------------------------- schedule */

fn closed_now() -> dm_core::Schedule {
    // A window that cannot contain the present moment, whenever it is run.
    dm_core::Schedule {
        enabled: true,
        from: "00:00".into(),
        to: "00:01".into(),
        days: vec![],
        wake: false,
    }
}

#[tokio::test]
async fn nothing_starts_while_the_schedule_is_closed() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);
    q.set_schedule(closed_now());

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.run().await;

    assert_eq!(
        q.get(id).await.unwrap().state,
        DownloadState::Paused,
        "a download ran outside its window"
    );
    assert!(!dir.path().join("out/f.bin").exists());
}

/// One row a person starts now runs to the end with the schedule
/// still closed, and the schedule holds the rest as before.
#[tokio::test]
async fn one_row_started_outside_the_schedule_runs_while_the_rest_wait() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);
    q.set_schedule(closed_now());

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    let other = q.add(&format!("http://{addr}/g.bin")).await.unwrap();
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().state, DownloadState::Paused);

    assert!(q.start_outside_schedule(id).await);
    // The scheduler ticking under the closed window does not put it down.
    let serving = tokio::spawn({
        let q = q.clone();
        async move { q.run().await }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while q.get(id).await.unwrap().state != DownloadState::Completed {
        assert!(
            std::time::Instant::now() < deadline,
            "the row started outside the schedule did not finish: {:?}",
            q.get(id).await.unwrap().state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    serving.await.unwrap();
    assert!(dir.path().join("out/f.bin").exists());
    let held = q.get(other).await.unwrap();
    assert_eq!(held.state, DownloadState::Paused);
    assert!(
        held.paused_by_schedule,
        "the other row is still the schedule's"
    );
    // A row already finished is not started again.
    assert!(!q.start_outside_schedule(id).await);
}

#[tokio::test]
async fn the_window_opening_puts_back_what_it_stopped() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);
    q.set_schedule(closed_now());

    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().state, DownloadState::Paused);

    q.set_schedule(dm_core::Schedule::default());
    q.run().await;

    assert_eq!(q.get(id).await.unwrap().state, DownloadState::Completed);
}

#[tokio::test]
async fn what_the_schedule_paused_is_started_after_a_restart() {
    /* Which pauses were the schedule's was kept beside the row and not with
     * it, so a restart overnight turned them into a person's pauses, and the
     * window opening in the morning started nothing. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");

    let first = queue(dir.path(), 2);
    first.persist_to(store.clone()).await;
    first.set_schedule(closed_now());
    let id = first.add(&format!("http://{addr}/f.bin")).await.unwrap();
    first.run().await;
    assert_eq!(first.get(id).await.unwrap().state, DownloadState::Paused);
    first.flush().await;

    let second = queue(dir.path(), 2);
    second.persist_to(store).await;
    second.set_schedule(dm_core::Schedule::default());
    second.run().await;

    assert_eq!(
        second.get(id).await.unwrap().state,
        DownloadState::Completed,
        "the schedule's pause was taken for a person's after the restart"
    );
}

#[tokio::test]
async fn a_deliberate_pause_survives_the_window_reopening() {
    /* Obeying a schedule must not quietly undo a decision somebody made.
     * Both are State::Paused on the wire, so only the queue knows which is
     * which — and getting it wrong restarts a download at one in the morning
     * that was stopped on purpose. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);

    let mine = q.add(&format!("http://{addr}/mine.bin")).await.unwrap();
    let theirs = q.add(&format!("http://{addr}/theirs.bin")).await.unwrap();
    q.pause(mine).await;

    q.set_schedule(closed_now());
    q.run().await;
    q.set_schedule(dm_core::Schedule::default());
    q.run().await;

    assert_eq!(
        q.get(mine).await.unwrap().state,
        DownloadState::Paused,
        "the schedule restarted a download that was paused on purpose"
    );
    assert_eq!(q.get(theirs).await.unwrap().state, DownloadState::Completed);
}

/* ------------------------------------------------------- download limit */

#[tokio::test]
async fn a_spent_download_limit_holds_the_queue_until_its_period_rolls_over() {
    /* The download limit: no more than so many bytes every so many hours.
     * The first download spends the allowance; the next is held as the
     * scheduler holds one, and goes on once the period rolls over. Driven
     * without wall-clock waits: the first is run to completion alone so it
     * has spent the allowance before the second is added, and the rollover
     * is a zero-length period, which `Meter::roll` clears at once. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    q.set_allowance(dm_core::quota::Allowance {
        bytes: 1,
        period: Duration::from_secs(3600),
    });

    let first = q.add(&format!("http://{addr}/first.bin")).await.unwrap();
    q.run().await;
    assert_eq!(q.get(first).await.unwrap().state, DownloadState::Completed);
    assert!(
        q.allowance_spent().is_some(),
        "the first spent the allowance"
    );
    assert!(!q.may_run(), "nothing more may run while it is spent");

    // Added now, with the allowance already spent, the second is held.
    let second = q.add(&format!("http://{addr}/second.bin")).await.unwrap();
    q.run().await;
    assert_eq!(
        q.get(second).await.unwrap().state,
        DownloadState::Paused,
        "the second ran past a spent allowance"
    );
    assert!(q.held().await);

    // The period rolls over (here, a zero-length one): downloading goes on.
    q.set_allowance(dm_core::quota::Allowance {
        bytes: 1,
        period: Duration::ZERO,
    });
    assert!(q.may_run(), "the period rolled over");
    q.run().await;
    assert_eq!(q.get(second).await.unwrap().state, DownloadState::Completed);
}

#[tokio::test]
async fn a_pause_of_a_persons_is_not_undone_by_the_download_limit() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let mine = q.add(&format!("http://{addr}/mine.bin")).await.unwrap();
    q.pause(mine).await;
    assert!(
        !q.held().await,
        "a person's pause is not the limit's to lift"
    );
}

/* ---------------------------------------------------------- speed limit */

#[tokio::test]
async fn a_speed_limit_slows_the_queue_down() {
    /* Measured against the clock rather than by asking the limiter what it
     * thinks: the question is whether the cap reaches the bytes, and every
     * way of checking that short of timing it tests the wiring instead. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 4);

    // 1024 bytes a file, four files, capped at 1024 a second.
    q.set_speed_limit(1024).await;
    for n in 0..4 {
        q.add(&format!("http://{addr}/f{n}.bin")).await.unwrap();
    }

    let started = std::time::Instant::now();
    q.run().await;
    let took = started.elapsed();

    assert!(
        took >= Duration::from_millis(2500),
        "4 KB at 1 KB/s took {took:?}, so the cap was not applied"
    );
}

#[tokio::test]
async fn lifting_the_limit_takes_effect_on_what_is_already_running() {
    // A cap people set mid-download and watch do nothing is a broken one.
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 4);

    q.set_speed_limit(1).await;
    assert_eq!(q.speed_limit().await, 1);
    for n in 0..4 {
        q.add(&format!("http://{addr}/f{n}.bin")).await.unwrap();
    }
    let running = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };

    tokio::time::sleep(Duration::from_millis(300)).await;
    q.set_speed_limit(0).await;

    // Without the lift this would sit at one byte a second for hours.
    tokio::time::timeout(Duration::from_secs(20), running)
        .await
        .expect("lifting the cap did not release the downloads")
        .unwrap();
}

#[tokio::test]
async fn a_downloads_own_limit_slows_it_alone_and_is_kept() {
    // 16 KB in four pieces, so the limit has pieces to space out.
    let pieces = || async {
        use futures_util::StreamExt;
        // Apart, so each arrives as a read of its own.
        let parts = futures_util::stream::iter(0..4).then(|_| async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok::<_, std::io::Error>(bytes::Bytes::from(vec![9u8; 4096]))
        });
        Response::builder()
            .header("content-length", 16 * 1024)
            .body(Body::from_stream(parts))
            .unwrap()
    };
    let addr = common::serve(axum::Router::new().fallback(pieces)).await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);
    let slow = q.add(&format!("http://{addr}/slow.bin")).await.unwrap();
    let free = q.add(&format!("http://{addr}/free.bin")).await.unwrap();
    q.pause(slow).await;
    q.pause(free).await;
    q.set_own_limit(slow, 8).await;
    assert_eq!(q.get(slow).await.unwrap().speed_limit, 8);

    let timed = |id| {
        let q = q.clone();
        async move {
            let started = std::time::Instant::now();
            q.resume(id).await;
            q.run().await;
            started.elapsed()
        }
    };
    let unlimited = timed(free).await;
    let limited = timed(slow).await;
    // Three pieces wait half a second each at 8 KB/s.
    assert!(
        limited >= Duration::from_millis(1200) && unlimited < Duration::from_millis(1000),
        "own limit {limited:?}, none {unlimited:?}"
    );
    // Kept with the row, as the queue is saved.
    assert_eq!(q.get(slow).await.unwrap().speed_limit, 8);
    q.set_own_limit(slow, 0).await;
    assert_eq!(q.get(slow).await.unwrap().speed_limit, 0);
}

#[tokio::test]
async fn a_queue_starts_on_its_own_once_a_day_at_its_time() {
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    q.set_queue_starts(vec![
        ("Night".into(), 120),
        ("".into(), 60),
        ("Odd".into(), 9999),
    ]);
    let day = 739_000;
    assert!(q.starts_due(day, 119).is_empty(), "before its time");
    assert_eq!(q.starts_due(day, 121), ["Night"]);
    assert!(q.starts_due(day, 122).is_empty(), "twice in a day");
    // A change of settings keeps what already started today.
    q.set_queue_starts(vec![("Night".into(), 120)]);
    assert!(q.starts_due(day, 123).is_empty());
    // Hours late, as after a computer that slept through it: not started.
    assert!(q.starts_due(day + 1, 600).is_empty());
    assert_eq!(q.starts_due(day + 2, 120), ["Night"]);
}

/// A download into a folder whose disk has no room is paused with why,
/// not failed; given another folder it goes on there and finishes, its
/// staging folder there gone once it is whole.
#[tokio::test]
async fn a_full_disk_pauses_and_another_folder_finishes_it() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let full = dir.path().join("full");
    let roomy = dir.path().join("roomy");
    let base = common::options(dir.path());
    // The download folder and dm's own parts folder are on the full disk.
    let (watched, parts) = (full.clone(), base.parts_dir.clone());
    let q = Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            dest_dir: full.clone(),
            room: dm_core::download::Room::with_free_space(move |at| {
                (at.starts_with(&watched) || at.starts_with(&parts)).then_some(10)
            }),
            ..base
        },
        1,
    );
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Paused, "{:?}", row.state);
    assert!(
        row.stalled.starts_with("not enough room"),
        "{}",
        row.stalled
    );

    q.relocate(id, roomy.clone()).await.unwrap();
    assert!(q.get(id).await.unwrap().stalled.is_empty());
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed, "{:?}", row.state);
    assert!(
        std::path::Path::new(&row.path).starts_with(&roomy),
        "{}",
        row.path
    );
    assert_eq!(std::fs::read(&row.path).unwrap().len(), 1024);
    assert!(!roomy.join(dm_core::queue::UNFINISHED).exists());
}

/* ------------------------------------------------------------- metalink */

/// A server that hosts a mirror list and the mirrors it names.
///
/// `/a.meta4` serves whatever document the test puts in `doc` — filled in
/// after the address is known, since the mirrors have to point back here.
/// Any path containing `stale` is served short, so a mirror that disagrees
/// about the file can be told apart from one that agrees.
#[derive(Clone)]
struct Mirrors {
    body: Arc<Vec<u8>>,
    doc: Arc<std::sync::Mutex<String>>,
    hits: Arc<std::sync::Mutex<Vec<String>>>,
}

async fn serve_metalink(body: Vec<u8>) -> (SocketAddr, Mirrors) {
    let state = Mirrors {
        body: Arc::new(body),
        doc: Arc::new(std::sync::Mutex::new(String::new())),
        hits: Arc::new(std::sync::Mutex::new(Vec::new())),
    };

    async fn handler(
        State(s): State<Mirrors>,
        uri: Uri,
        headers: axum::http::HeaderMap,
    ) -> Response {
        let path = uri.path().to_string();
        if path.ends_with(".meta4") {
            return Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/metalink4+xml")
                .body(Body::from(s.doc.lock().unwrap().clone()))
                .unwrap();
        }
        if path.contains("missing") {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap();
        }

        let served: Vec<u8> = if path.contains("stale") {
            s.body[..s.body.len() / 2].to_vec()
        } else {
            s.body.to_vec()
        };
        let total = served.len() as u64;

        let range = headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
            .map(str::to_owned);

        match range {
            Some(spec) => {
                let (from, to) = spec.split_once('-').unwrap_or(("0", ""));
                let start: u64 = from.parse().unwrap_or(0);
                let end: u64 = to.parse().unwrap_or(total - 1).min(total - 1);

                /* Only real range fetches are recorded, never the one-byte
                 * probe. Every mirror is probed whatever happens, so counting
                 * probes would have said all three mirrors were in use even
                 * when every byte came from the first — which is exactly what
                 * an earlier version of this test did say. */
                if end > start {
                    s.hits.lock().unwrap().push(path.clone());
                }
                let slice = served[start as usize..=end as usize].to_vec();
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header("content-length", slice.len())
                    .header("content-range", format!("bytes {start}-{end}/{total}"))
                    // Per-mirror, as real mirrors are: sending one server's
                    // validator to another is how a resume gets refused.
                    .header("etag", format!("\"{path}\""))
                    .body(Body::from(slice))
                    .unwrap()
            }
            None => {
                s.hits.lock().unwrap().push(path.clone());
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-length", total)
                    .body(Body::from(served))
                    .unwrap()
            }
        }
    }

    let app = axum::Router::new()
        .fallback(handler)
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, state)
}

fn document(addr: SocketAddr, mirrors: &[&str], digest: Option<&str>) -> String {
    let urls: String = mirrors
        .iter()
        .enumerate()
        .map(|(i, name)| format!("<url priority=\"{}\">http://{addr}/{name}</url>", i + 1))
        .collect();
    let hash = digest
        .map(|d| format!("<hash type=\"sha-256\">{d}</hash>"))
        .unwrap_or_default();
    format!(
        "<metalink xmlns=\"urn:ietf:params:xml:ns:metalink\">\
         <file name=\"mirrored.bin\">{hash}{urls}</file></metalink>"
    )
}

fn sha256_of(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Bytes that differ everywhere, so a file spliced from the wrong source is
/// caught by content and not only by length.
fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// Large enough to be cut into one segment per mirror.
///
/// `plan` will not make a segment smaller than a megabyte, so a file under
/// `connections` megabytes is fetched over one connection from one place —
/// and a mirror test on one of those proves nothing at all.
const MIRRORED: usize = 8 * 1024 * 1024;

#[tokio::test]
async fn a_metalink_is_fetched_from_every_mirror_that_agrees() {
    let body = pattern(MIRRORED);
    let (addr, server) = serve_metalink(body.clone()).await;
    *server.doc.lock().unwrap() = document(addr, &["one.bin", "two.bin", "three.bin"], None);

    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let mut options = q.options().await;
    options.connections = 6;
    q.set_options(options).await;

    let id = q.add(&format!("http://{addr}/a.meta4")).await.unwrap();
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed, "{:?}", row.state);
    assert_eq!(row.filename, "mirrored.bin", "the document names the file");
    assert_eq!(row.mirrors, 3, "the bytes came from one mirror only");

    // Every mirror actually served bytes — the point of a mirror list, and
    // not something probing them all would demonstrate.
    let hits = server.hits.lock().unwrap().clone();
    for mirror in ["one.bin", "two.bin", "three.bin"] {
        assert!(
            hits.iter().any(|h| h.contains(mirror)),
            "{mirror} served no ranges: {hits:?}"
        );
    }

    let saved = std::fs::read(dir.path().join("out/mirrored.bin")).unwrap();
    assert_eq!(saved.len(), body.len());
    assert_eq!(
        saved, body,
        "the file assembled from three mirrors is wrong"
    );
}

#[tokio::test]
async fn a_mirror_that_disagrees_about_the_file_is_left_out() {
    /* Two copies of different lengths are a stale mirror and a current one,
     * not mirrors. Splicing ranges from both produces a file of exactly the
     * right size and wrong throughout, which is the one thing this engine
     * must never produce. */
    let body = pattern(MIRRORED);
    let (addr, server) = serve_metalink(body.clone()).await;
    *server.doc.lock().unwrap() = document(addr, &["one.bin", "stale.bin", "two.bin"], None);

    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let mut options = q.options().await;
    options.connections = 6;
    q.set_options(options).await;

    let id = q.add(&format!("http://{addr}/a.meta4")).await.unwrap();
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert_eq!(row.mirrors, 2, "the short mirror was used anyway");
    assert_eq!(
        std::fs::read(dir.path().join("out/mirrored.bin")).unwrap(),
        body
    );
}

#[tokio::test]
async fn a_metalink_checksum_is_checked_without_being_asked_for() {
    // The document is published by whoever published the file; its digest is
    // the whole reason a Metalink is worth reading rather than guessing.
    let body = pattern(256 * 1024);
    let (addr, server) = serve_metalink(body.clone()).await;
    *server.doc.lock().unwrap() = document(addr, &["one.bin"], Some(&sha256_of(&body)));

    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/a.meta4")).await.unwrap();
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert!(row.verified, "the document's digest was never checked");
}

#[tokio::test]
async fn a_metalink_whose_digest_does_not_match_fails_the_download() {
    let body = pattern(256 * 1024);
    let (addr, server) = serve_metalink(body).await;
    *server.doc.lock().unwrap() = document(addr, &["one.bin"], Some(&"0".repeat(64)));

    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/a.meta4")).await.unwrap();
    q.run().await;

    match q.get(id).await.unwrap().state {
        DownloadState::Failed(why) => assert!(why.contains("sha256"), "{why}"),
        other => panic!("a file that failed its published digest was {other:?}"),
    }
    assert!(!dir.path().join("out/mirrored.bin").exists());
}

#[tokio::test]
async fn a_metalink_naming_nothing_reachable_fails_with_a_reason() {
    let (addr, server) = serve_metalink(pattern(1024)).await;
    *server.doc.lock().unwrap() = document(addr, &["missing.bin"], None);

    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/a.meta4")).await.unwrap();
    q.run().await;

    match q.get(id).await.unwrap().state {
        // The reason names the mirror that answered, not the document's own
        // URL, which is where a bare failure would send people looking.
        DownloadState::Failed(why) => assert!(
            why.contains("missing.bin"),
            "the reason should name the mirror: {why}"
        ),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn a_document_that_is_not_a_metalink_fails_rather_than_being_downloaded() {
    // Otherwise a broken mirror list is saved as a file called a.meta4 and
    // reported as a successful download.
    let (addr, server) = serve_metalink(pattern(1024)).await;
    *server.doc.lock().unwrap() = "<html>not a metalink at all</html>".into();

    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/a.meta4")).await.unwrap();
    q.run().await;

    assert!(matches!(
        q.get(id).await.unwrap().state,
        DownloadState::Failed(_)
    ));
    // "Rather than being downloaded" is the half that matters.
    let saved: Vec<_> = std::fs::read_dir(dir.path().join("out"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(
        saved.is_empty(),
        "the document was saved as a file: {saved:?}"
    );
}

/// A server whose ranges never arrive: headers, then silence.
/// How many stalled bodies are open at once, and the most there were.
#[derive(Default)]
struct Open {
    now: AtomicUsize,
    most: AtomicUsize,
}

/// Held by a stalled body for as long as the server is sending it: dropped
/// when the client lets the connection go.
struct Holding(Arc<Open>);

impl Drop for Holding {
    fn drop(&mut self) {
        self.0.now.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A server that sends a range's headers and then nothing, counting the
/// ranges asked for and how many it is holding open at once.
async fn serve_stalled_counting() -> (SocketAddr, Arc<AtomicUsize>, Arc<Open>) {
    use axum::http::HeaderMap;
    let hits = Arc::new(AtomicUsize::new(0));
    let open = Arc::new(Open::default());
    let (counted, held) = (Arc::clone(&hits), Arc::clone(&open));
    let app = axum::Router::new().fallback(axum::routing::get(move |headers: HeaderMap| {
        let (hits, open) = (Arc::clone(&counted), Arc::clone(&held));
        async move {
            let probe = headers
                .get("range")
                .is_some_and(|r| r.to_str().unwrap_or("") == "bytes=0-0");
            if !probe {
                hits.fetch_add(1, Ordering::SeqCst);
            }
            let (status, range, body) = if probe {
                (
                    StatusCode::PARTIAL_CONTENT,
                    "bytes 0-0/4194304",
                    Body::from(vec![0u8]),
                )
            } else {
                let now = open.now.fetch_add(1, Ordering::SeqCst) + 1;
                open.most.fetch_max(now, Ordering::SeqCst);
                let holding = Holding(Arc::clone(&open));
                let stalled = futures_util::stream::unfold(holding, |holding| async move {
                    futures_util::future::pending::<()>().await;
                    Some((Ok::<Vec<u8>, std::io::Error>(Vec::new()), holding))
                });
                (
                    StatusCode::PARTIAL_CONTENT,
                    "bytes 0-4194303/4194304",
                    Body::from_stream(stalled),
                )
            };
            Response::builder()
                .status(status)
                .header("content-range", range)
                .header("accept-ranges", "bytes")
                .body(body)
                .unwrap()
        }
    }));
    let addr = common::serve(app).await;
    (addr, hits, open)
}

#[tokio::test]
async fn a_resumed_row_waits_for_its_previous_run_to_stop_before_starting_again() {
    /* A pause is recorded the moment it is asked for; the task honours it a
     * moment later. A resume in between used to start a second task on the
     * same part files while the first still held them. Measured at the
     * server: the row is started again, and never two of its requests are
     * open at once. */
    let (addr, hits, open) = serve_stalled_counting().await;
    let dir = tempdir();
    let q = Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir.path())
        },
        1,
    );

    let id = q.add(&format!("http://{addr}/stalled.bin")).await.unwrap();
    let serving = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    let asked = async {
        while hits.load(Ordering::SeqCst) < 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), asked)
        .await
        .expect("the range was never requested");

    q.pause(id).await;
    q.resume(id).await;
    assert_eq!(q.get(id).await.unwrap().state, DownloadState::Queued);

    let started_again = async {
        while hits.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), started_again)
        .await
        .expect("the resumed row was never started again");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        open.most.load(Ordering::SeqCst),
        1,
        "a second task was started while the first still held the part files"
    );

    q.cancel(id).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), serving).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_queue_that_cannot_be_written_says_so_rather_than_pretending() {
    use std::os::unix::fs::PermissionsExt;
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store_dir = dir.path().join("store");
    std::fs::create_dir_all(&store_dir).unwrap();
    let q = queue(dir.path(), 1);
    q.persist_to(store_dir.join("queue.json")).await;
    assert!(q.problems().is_empty(), "nothing has gone wrong yet");

    // The directory stops taking writes: a disk that filled, a permission
    // that changed. Every change from here is one the window shows and the
    // next launch will not.
    std::fs::set_permissions(&store_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(store_dir.join("probe"), b"").is_ok() {
        return; // root writes past permissions (a container): nothing to test
    }
    q.add(&format!("http://{addr}/f.bin")).await.unwrap();
    // A change waits a moment for its write (`Queue::flush` is what the app
    // does as it ends); the failure is said when the write is made.
    q.flush().await;
    let problems = q.problems();
    std::fs::set_permissions(&store_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("could not be written"), "{problems:?}");
}

#[tokio::test]
async fn an_unreadable_queue_file_is_kept_aside_and_said_rather_than_overwritten() {
    /* A queue file that cannot be parsed used to be skipped and then, on the
     * first save, overwritten with the empty list — so a release that changed
     * a field destroyed every queue on first launch, with the parts those
     * rows pointed at orphaned for good. */
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    std::fs::write(&store, "[{\"id\": \"not a number\"}]").unwrap();

    let q = queue(dir.path(), 1);
    q.persist_to(store.clone()).await;

    assert!(q.list().await.is_empty());
    let kept = std::fs::read_to_string(dir.path().join("queue.json.unreadable"))
        .expect("the unreadable queue was not kept aside");
    assert!(kept.contains("not a number"));
    let problems = q.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("could not be read"), "{problems:?}");
    // Said once: asking again reports nothing new.
    assert!(q.problems().is_empty());
}

#[tokio::test]
async fn what_is_not_a_url_is_refused_at_the_one_door() {
    /* The window checked and the browser hand-off did not, so anything a
     * browser sent became a row that could only ever fail. The check belongs
     * where both of them arrive. */
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    for refused in ["", "   ", "not a url", "file:///etc/passwd", "javascript:x"] {
        assert!(q.add(refused).await.is_none(), "accepted {refused:?}");
    }
    assert!(
        q.add("  https://e.com/f.bin  ").await.is_some(),
        "surrounding space"
    );
    assert!(q.add("http://e.com/f.bin").await.is_some());
    // FTP is fetched too, and FTP over TLS.
    assert!(q.add("ftp://e.com/f.bin").await.is_some());
    assert!(q.add("ftps://e.com/f.bin").await.is_some());
    assert_eq!(
        q.list().await.len(),
        4,
        "only the four real URLs became rows"
    );
}

/// A magnet is a download the queue takes, so that the torrent engine gets to
/// see it — and a non-URL is still refused, so a mistyped line is not a row.
#[tokio::test]
async fn the_queue_takes_a_magnet_but_not_a_stray_line() {
    let dir = tempdir();
    let queue = Queue::new(client(), common::options(dir.path()), 2);

    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";
    assert!(
        queue.add(magnet).await.is_some(),
        "a magnet should be queued for the torrent engine"
    );
    assert!(
        queue.add("not a url at all").await.is_none(),
        "a line that is neither a link nor a magnet is not a download"
    );

    // It is a row like any other, in whatever state it settled into — the
    // point is only that the queue accepted it rather than dropping it.
    let rows = queue.list().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].url, magnet);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saves_at_the_same_time_do_not_report_a_failure_that_did_not_happen() {
    /* Every save wrote the same `queue.tmp`, so two at once raced: the
     * second rename found the file gone and the window was told the queue
     * could not be written when it had been. */
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let q = queue(dir.path(), 0);
    q.persist_to(store.clone()).await;
    let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();

    let mut saves = Vec::new();
    for i in 0..400 {
        let q = q.clone();
        saves.push(tokio::spawn(async move {
            if i % 2 == 0 {
                q.pause(id).await;
            } else {
                q.resume(id).await;
            }
        }));
    }
    for save in saves {
        save.await.unwrap();
    }

    q.flush().await;
    assert!(q.problems().is_empty(), "{:?}", q.problems());
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "temporary files were left behind");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&store).unwrap()).unwrap();
    assert_eq!(saved.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_paused_row_still_stopping_holds_its_slot() {
    /* A paused row's task keeps its connection until it notices the pause.
     * It was not counted as running, so a limit of one let a second
     * download start beside it. Measured at the server: never two open. */
    let (addr, hits, open) = serve_stalled_counting().await;
    let dir = tempdir();
    let q = Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir.path())
        },
        1,
    );

    let first = q.add(&format!("http://{addr}/a.bin")).await.unwrap();
    let serving = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    let asked = async {
        while hits.load(Ordering::SeqCst) < 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), asked)
        .await
        .expect("the first range was never asked for");

    q.pause(first).await;
    let second = q.add(&format!("http://{addr}/b.bin")).await.unwrap();

    let started = async {
        while hits.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .expect("the second download never started");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        open.most.load(Ordering::SeqCst),
        1,
        "a second download started while the first still held the only slot"
    );

    q.cancel(second).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), serving).await;
}

#[tokio::test]
async fn the_synchronization_queue_fetches_again_only_what_changed_on_the_server() {
    use axum::response::IntoResponse;
    // One file that is republished partway through, and one that is not.
    let version = Arc::new(AtomicUsize::new(1));
    let app = axum::Router::new()
        .route(
            "/changing.txt",
            axum::routing::get({
                let version = Arc::clone(&version);
                move || {
                    let v = version.load(std::sync::atomic::Ordering::SeqCst);
                    async move {
                        (
                            [("etag", format!("\"v{v}\""))],
                            format!("version {v} of the file"),
                        )
                            .into_response()
                    }
                }
            }),
        )
        .route(
            "/steady.txt",
            axum::routing::get(|| async {
                ([("etag", "\"s1\"")], "never changes").into_response()
            }),
        );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);
    let changing = q.add(&format!("http://{addr}/changing.txt")).await.unwrap();
    let steady = q.add(&format!("http://{addr}/steady.txt")).await.unwrap();
    q.run().await;
    for id in [changing, steady] {
        assert_eq!(q.get(id).await.unwrap().state, DownloadState::Completed);
        assert!(q.set_queue(id, dm_core::RECHECK_QUEUE).await);
    }
    assert_eq!(
        q.get(changing).await.unwrap().validator.as_deref(),
        Some("\"v1\""),
        "the server's ETag is kept with the row"
    );

    // Nothing changed: nothing is fetched again.
    assert_eq!(q.synchronize(dm_core::RECHECK_QUEUE).await, (2, vec![]));

    let first_path = q.get(changing).await.unwrap().path;
    version.store(2, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        q.synchronize(dm_core::RECHECK_QUEUE).await,
        (2, vec![changing])
    );
    q.run().await;
    let row = q.get(changing).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert_eq!(
        std::fs::read_to_string(&row.path).unwrap(),
        "version 2 of the file"
    );
    assert_eq!(row.validator.as_deref(), Some("\"v2\""));
    assert_eq!(
        row.path, first_path,
        "replaced in place, not saved beside it"
    );
    assert!(q
        .queues()
        .await
        .contains(&dm_core::RECHECK_QUEUE.to_string()));
}

#[tokio::test]
async fn the_recheck_queue_asks_again_on_its_own_timer_and_only_then() {
    use axum::response::IntoResponse;
    let version = Arc::new(AtomicUsize::new(1));
    let app =
        axum::Router::new().route(
            "/doc.txt",
            axum::routing::get({
                let version = Arc::clone(&version);
                move || {
                    let v = version.load(std::sync::atomic::Ordering::SeqCst);
                    async move {
                        ([("etag", format!("\"v{v}\""))], format!("edition {v}")).into_response()
                    }
                }
            }),
        );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let doc = q.add(&format!("http://{addr}/doc.txt")).await.unwrap();
    q.run().await;
    assert!(q.set_queue(doc, dm_core::RECHECK_QUEUE).await);

    // No timer: never on its own.
    assert_eq!(q.recheck_if_due().await, None);
    q.set_recheck_every(Some(Duration::from_millis(300)));
    assert_eq!(q.recheck_if_due().await, None, "not yet time");
    version.store(2, std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        q.recheck_if_due().await,
        Some((1, vec![doc])),
        "due, and the change seen"
    );
    assert_eq!(q.recheck_if_due().await, None, "the clock restarted");
    q.run().await;
    assert_eq!(
        std::fs::read_to_string(q.get(doc).await.unwrap().path).unwrap(),
        "edition 2"
    );
    // Turned off again: nothing more.
    q.set_recheck_every(None);
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(q.recheck_if_due().await, None);
}

/// A file that takes about two seconds to arrive, chunk by chunk.
fn slow_router() -> axum::Router {
    use axum::response::IntoResponse;
    use futures_util::StreamExt as _;
    axum::Router::new().route(
        "/slow.bin",
        axum::routing::get(|| async {
            let chunks =
                futures_util::stream::iter((0..20).map(|i| {
                    Ok::<_, std::io::Error>(bytes::Bytes::from(vec![i as u8; 64 * 1024]))
                }))
                .then(|c| async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    c
                });
            (
                [("content-length", (20 * 64 * 1024).to_string())],
                axum::body::Body::from_stream(chunks),
            )
                .into_response()
        }),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queue_with_a_limit_of_its_own_runs_that_many_while_the_rest_of_the_room_goes_on() {
    use std::collections::HashMap;
    let addr = common::serve(slow_router()).await;
    let dir = tempdir();
    let q = queue(dir.path(), 3);
    q.set_queue_limits(HashMap::from([(dm_core::MAIN_QUEUE.to_string(), 1)]));
    let later = || dm_core::AddOptions {
        later: true,
        queue: dm_core::MAIN_QUEUE.into(),
        ..Default::default()
    };
    let a = q
        .add_with(&format!("http://{addr}/slow.bin?a"), later())
        .await
        .unwrap();
    let b = q
        .add_with(&format!("http://{addr}/slow.bin?b"), later())
        .await
        .unwrap();
    let c = q.add(&format!("http://{addr}/slow.bin?c")).await.unwrap();
    q.start_queue(dm_core::MAIN_QUEUE).await;
    let running = q.clone();
    let scheduler = tokio::spawn(async move { running.run().await });
    // Watched the whole way, not looked at once: the Main queue never has
    // two running, and the unqueued row runs beside one of them.
    let (mut most_main, mut beside) = (0, false);
    while !scheduler.is_finished() {
        let rows = q.list().await;
        let running = |id| {
            rows.iter()
                .any(|r| r.id == id && r.state == DownloadState::Running)
        };
        let main = [a, b].into_iter().filter(|id| running(*id)).count();
        most_main = most_main.max(main);
        beside |= main == 1 && running(c);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(most_main, 1, "the Main queue ran more than its limit");
    assert!(
        beside,
        "the unqueued row was held back by another queue's limit"
    );
    let _ = scheduler.await;
    for id in [a, b, c] {
        assert_eq!(q.get(id).await.unwrap().state, DownloadState::Completed);
    }
    assert_eq!(
        q.queue_limits(),
        HashMap::from([(dm_core::MAIN_QUEUE.to_string(), 1)])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_keeps_the_parts_folder_it_began_in_when_the_setting_changes() {
    let addr = common::serve(slow_router()).await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/slow.bin")).await.unwrap();
    let running = q.clone();
    let scheduler = tokio::spawn(async move { running.run().await });
    // Some of it staged, then paused.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while q.get(id).await.unwrap().progress.downloaded < 128 * 1024 {
        assert!(std::time::Instant::now() < deadline, "nothing arrived");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    q.pause(id).await;
    let _ = scheduler.await;
    let first = dir.path().join("parts").join(id.to_string());
    assert!(first.exists(), "nothing staged under the first folder");
    let row = q.get(id).await.unwrap();
    assert_eq!(
        row.parts_dir,
        first.display().to_string(),
        "the row did not record its folder"
    );

    // The parts folder moves in the settings; this row stays put and
    // finishes from what it has, and its parts are never looked for anew.
    let elsewhere = dir.path().join("elsewhere");
    q.set_options(dm_core::Options {
        parts_dir: elsewhere.clone(),
        ..q.options().await
    })
    .await;
    q.resume(id).await;
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed, "{row:?}");
    let saved = std::fs::read(&row.path).unwrap();
    assert_eq!(saved.len(), 20 * 64 * 1024);
    assert!(
        saved
            .chunks(64 * 1024)
            .enumerate()
            .all(|(i, c)| c.iter().all(|b| *b == i as u8)),
        "the file is not the file"
    );
    assert!(
        !elsewhere.join(id.to_string()).exists(),
        "the parts were started again under the new folder"
    );
    // A row added now stages under the new folder.
    let next = q
        .add(&format!("http://{addr}/slow.bin?next"))
        .await
        .unwrap();
    let running = q.clone();
    let scheduler = tokio::spawn(async move { running.run().await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        elsewhere.join(next.to_string()).exists(),
        "a new row did not stage under the folder set"
    );
    q.cancel(next).await;
    let _ = scheduler.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_row_removes_the_parts_where_it_began_after_the_folder_changed() {
    let addr = common::serve(slow_router()).await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/slow.bin")).await.unwrap();
    let running = q.clone();
    let scheduler = tokio::spawn(async move { running.run().await });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while q.get(id).await.unwrap().progress.downloaded < 128 * 1024 {
        assert!(std::time::Instant::now() < deadline, "nothing arrived");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    q.pause(id).await;
    let _ = scheduler.await;
    let first = dir.path().join("parts").join(id.to_string());
    assert!(first.exists());
    q.set_options(dm_core::Options {
        parts_dir: dir.path().join("elsewhere"),
        ..q.options().await
    })
    .await;
    q.remove(id).await;
    assert!(
        !first.exists(),
        "the parts were orphaned where the row began"
    );
}

#[tokio::test]
async fn the_recheck_queue_remembers_when_it_last_ran_across_a_restart() {
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    // Last run two hours ago, by the wall clock, kept beside the queue.
    let two_hours_ago = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        - 2 * 3600 * 1000;
    std::fs::write(dir.path().join("recheck-at"), two_hours_ago.to_string()).unwrap();
    let q = queue(dir.path(), 1);
    q.set_recheck_every(Some(Duration::from_secs(3600)));
    q.persist_to(store.clone()).await;
    assert!(
        q.recheck_if_due().await.is_some(),
        "an hourly recheck two hours on did not run"
    );
    assert!(q.recheck_if_due().await.is_none());
    let kept: u64 = std::fs::read_to_string(dir.path().join("recheck-at"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(kept > two_hours_ago, "the run was not written down");

    // A launch after it: not due again yet.
    let again = queue(dir.path(), 1);
    again.set_recheck_every(Some(Duration::from_secs(3600)));
    again.persist_to(store).await;
    assert!(again.recheck_if_due().await.is_none());
}

/// A last run written while the clock was set decades ahead held
/// the recheck until that date. It is stale: the count starts from now.
#[tokio::test]
async fn a_recheck_time_in_the_future_is_stale_not_waited_out() {
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let forty_years_on = now + 40 * 365 * 24 * 3600 * 1000;
    std::fs::write(dir.path().join("recheck-at"), forty_years_on.to_string()).unwrap();
    let q = queue(dir.path(), 1);
    q.set_recheck_every(Some(Duration::from_millis(1)));
    q.persist_to(store).await;
    assert!(
        q.recheck_if_due().await.is_none(),
        "reset, counted from now"
    );
    let kept: u64 = std::fs::read_to_string(dir.path().join("recheck-at"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(kept < forty_years_on, "the stale time was kept on disk");
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        q.recheck_if_due().await.is_some(),
        "held by a time decades away"
    );
}

#[tokio::test]
async fn the_stalled_server_sees_a_dropped_response_let_go() {
    // What the two tests below measure with: a client dropping a stalled
    // response is seen by the server as one fewer held open.
    let (addr, _, open) = serve_stalled_counting().await;
    let res = client()
        .get(format!("http://{addr}/x.bin"))
        .header("range", "bytes=0-")
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(open.now.load(Ordering::SeqCst), 1);
    drop(res);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while open.now.load(Ordering::SeqCst) != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the server never saw it let go"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_rows_sign_in_stays_with_it_for_redownload_and_recheck_until_it_is_deleted() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let q = queue(dir.path(), 1);
    q.persist_to(store.clone()).await;
    let id = q
        .add_with(
            &format!("http://{addr}/a.bin"),
            dm_core::AddOptions {
                cookies: "session=1".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().state, DownloadState::Completed);
    // Kept: Redownload and the recheck queue ask the same server again.
    assert_eq!(q.get(id).await.unwrap().cookies, "session=1");
    assert!(q.redownload(id).await);
    assert_eq!(
        q.get(id).await.unwrap().cookies,
        "session=1",
        "Redownload lost the sign-in"
    );
    q.run().await;
    // Gone with the row, and from the file the queue is kept in.
    q.remove(id).await;
    q.flush().await;
    let saved = std::fs::read_to_string(&store).unwrap();
    assert!(!saved.contains("session=1"), "{saved}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_rechecks_at_once_do_not_both_fetch_again() {
    use axum::response::IntoResponse;
    let version = Arc::new(AtomicUsize::new(1));
    let fetched = Arc::new(AtomicUsize::new(0));
    let app = axum::Router::new().route(
        "/doc.txt",
        axum::routing::get({
            let (version, fetched) = (Arc::clone(&version), Arc::clone(&fetched));
            move |headers: axum::http::HeaderMap| {
                let v = version.load(std::sync::atomic::Ordering::SeqCst);
                if headers.get("range").is_none() {
                    fetched.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                async move {
                    // Slow enough that the two checks overlap.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    ([("etag", format!("\"v{v}\""))], format!("edition {v}")).into_response()
                }
            }
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let doc = q.add(&format!("http://{addr}/doc.txt")).await.unwrap();
    q.run().await;
    assert!(q.set_queue(doc, dm_core::RECHECK_QUEUE).await);
    version.store(2, std::sync::atomic::Ordering::SeqCst);
    let (a, b) = tokio::join!(
        q.synchronize(dm_core::RECHECK_QUEUE),
        q.synchronize(dm_core::RECHECK_QUEUE)
    );
    let changed = a.1.len() + b.1.len();
    assert_eq!(changed, 1, "both checks fetched it again: {a:?} {b:?}");
}

#[tokio::test]
async fn pausing_everything_while_the_schedule_holds_it_is_not_undone_by_the_window_opening() {
    /* The schedule had paused the row, so "Pause all" passed over it, and
     * the row stayed the schedule's: the window opening started a download
     * a person had just paused. The same through "Stop queue". */
    for stop in ["pause all", "stop queue"] {
        let (addr, _) = serve().await;
        let dir = tempdir();
        let q = queue(dir.path(), 2);
        q.set_schedule(closed_now());
        let id = q.add(&format!("http://{addr}/f.bin")).await.unwrap();
        q.run().await;
        assert!(q.get(id).await.unwrap().paused_by_schedule);

        let taken = match stop {
            "pause all" => q.pause_all().await,
            _ => q.stop_queue("").await,
        };
        assert_eq!(taken, 1, "{stop} did not count the row it took over");
        q.set_schedule(dm_core::Schedule::default());
        q.run().await;

        assert_eq!(
            q.get(id).await.unwrap().state,
            DownloadState::Paused,
            "the window opening undid {stop}"
        );
    }
}

#[tokio::test]
async fn a_deleted_row_holds_its_slot_until_its_run_lets_go() {
    /* Deleting a running row dropped it and its task at once, so nothing
     * counted the connection the task still held: with a limit of one, the
     * next row started beside it. Measured at the server. */
    let (addr, hits, open) = serve_stalled_counting().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let first = q.add(&format!("http://{addr}/a.bin")).await.unwrap();
    let serving = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    let asked = async {
        while hits.load(Ordering::SeqCst) < 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), asked)
        .await
        .expect("the first range was never asked for");

    q.remove(first).await;
    let second = q.add(&format!("http://{addr}/b.bin")).await.unwrap();
    let serving_again = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    let started = async {
        while hits.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .expect("the next download never started");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        open.most.load(Ordering::SeqCst),
        1,
        "the next download started while the deleted one still held the only slot"
    );

    q.cancel(second).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), serving).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), serving_again).await;
}

#[tokio::test]
async fn a_cleared_row_holds_its_slot_until_its_run_lets_go() {
    /* Clearing finished rows dropped a cancelled row whose run still held
     * a stalled read, and nothing counted that connection: with a limit of
     * one, the next row started beside it. Measured at the server. */
    let (addr, hits, open) = serve_stalled_counting().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);

    let first = q.add(&format!("http://{addr}/a.bin")).await.unwrap();
    let serving = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    let asked = async {
        while hits.load(Ordering::SeqCst) < 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), asked)
        .await
        .expect("the first range was never asked for");

    q.cancel(first).await;
    assert_eq!(q.clear_finished().await, 1);
    let second = q.add(&format!("http://{addr}/b.bin")).await.unwrap();
    let serving_again = {
        let q = q.clone();
        tokio::spawn(async move { q.run().await })
    };
    let started = async {
        while hits.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .expect("the next download never started");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        open.most.load(Ordering::SeqCst),
        1,
        "the next download started while the cleared one still held the only slot"
    );

    q.cancel(second).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), serving).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), serving_again).await;
}

/// A file republished on demand that can also be made to vanish, its ETag
/// naming its version.
fn versioned(version: Arc<AtomicUsize>, gone: Arc<std::sync::atomic::AtomicBool>) -> axum::Router {
    use axum::response::IntoResponse;
    axum::Router::new().fallback(axum::routing::get(move || {
        let v = version.load(Ordering::SeqCst);
        let gone = gone.load(Ordering::SeqCst);
        async move {
            if gone {
                return StatusCode::NOT_FOUND.into_response();
            }
            ([("etag", format!("\"v{v}\""))], format!("version {v}")).into_response()
        }
    }))
}

#[tokio::test]
async fn a_recheck_that_cannot_fetch_the_new_version_keeps_the_old_file() {
    /* The old copy was deleted as soon as a change was seen, before the new
     * one was fetched; a server that then failed left neither. */
    let version = Arc::new(AtomicUsize::new(1));
    let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let addr = common::serve(versioned(Arc::clone(&version), Arc::clone(&gone))).await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/doc.txt")).await.unwrap();
    q.run().await;
    let first = q.get(id).await.unwrap();
    assert_eq!(first.state, DownloadState::Completed);
    assert!(q.set_queue(id, dm_core::RECHECK_QUEUE).await);

    version.store(2, Ordering::SeqCst);
    assert_eq!(q.synchronize(dm_core::RECHECK_QUEUE).await, (1, vec![id]));
    gone.store(true, Ordering::SeqCst);
    q.run().await;

    let row = q.get(id).await.unwrap();
    let DownloadState::Failed(reason) = &row.state else {
        panic!("the failed fetch was not said: {:?}", row.state);
    };
    assert!(reason.contains("old one is kept"), "{reason}");
    assert_eq!(row.path, first.path, "the row lost the file it has");
    assert_eq!(
        std::fs::read_to_string(&first.path).unwrap(),
        "version 1",
        "the old file went before the new one had arrived"
    );

    // Back, and retried: the new version takes the old one's place.
    gone.store(false, Ordering::SeqCst);
    assert!(q.retry(id).await);
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert_eq!(row.path, first.path, "saved beside the old one");
    assert_eq!(std::fs::read_to_string(&row.path).unwrap(), "version 2");
    let saved = std::fs::read_dir(dir.path().join("out")).unwrap().count();
    assert_eq!(saved, 1, "the old and new versions were both kept");
}

#[tokio::test]
async fn a_recheck_asks_about_a_row_whose_scheme_is_written_in_capitals() {
    let version = Arc::new(AtomicUsize::new(1));
    let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let addr = common::serve(versioned(Arc::clone(&version), gone)).await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("HTTP://{addr}/doc.txt")).await.unwrap();
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().state, DownloadState::Completed);
    assert!(q.set_queue(id, dm_core::RECHECK_QUEUE).await);

    version.store(2, Ordering::SeqCst);
    assert_eq!(q.synchronize(dm_core::RECHECK_QUEUE).await, (1, vec![id]));
}

#[tokio::test]
async fn a_recheck_of_a_metalink_asks_about_the_file_not_the_document() {
    /* The row keeps a mirror's ETag and the file's length; the recheck
     * compared them with the mirror list's own, which always differ, and
     * fetched every Metalink again on every check. */
    let body = pattern(4096);
    let (addr, server) = serve_metalink(body.clone()).await;
    *server.doc.lock().unwrap() = document(addr, &["one.bin"], None);
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let id = q.add(&format!("http://{addr}/a.meta4")).await.unwrap();
    q.run().await;
    let first = q.get(id).await.unwrap();
    assert_eq!(first.state, DownloadState::Completed);
    assert!(q.set_queue(id, dm_core::RECHECK_QUEUE).await);
    let fetched = server.hits.lock().unwrap().len();

    assert_eq!(q.synchronize(dm_core::RECHECK_QUEUE).await, (1, vec![]));
    assert_eq!(server.hits.lock().unwrap().len(), fetched);
    assert_eq!(std::fs::read(&first.path).unwrap(), body);

    // The document now names a different file: that is a change.
    *server.doc.lock().unwrap() = document(addr, &["stale.bin"], None);
    assert_eq!(q.synchronize(dm_core::RECHECK_QUEUE).await, (1, vec![id]));
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert_eq!(row.path, first.path);
    assert_eq!(std::fs::read(&row.path).unwrap(), body[..body.len() / 2]);
}

#[tokio::test]
async fn a_recheck_leaves_alone_a_row_made_from_a_web_page() {
    /* A yt-dlp row's address is the page, whose length is never the
     * video's and which has no ETag of the video's: compared, every such
     * row was fetched again on every check. Nothing comparable: not
     * checked, not fetched. Stood up from a saved queue, since making the
     * row needs a yt-dlp. */
    use axum::response::IntoResponse;
    let app = axum::Router::new().fallback(axum::routing::get(|| async {
        (
            [("content-type", "text/html; charset=utf-8")],
            "<html>a watch page</html>",
        )
            .into_response()
    }));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let video = dir.path().join("clip.mp4");
    std::fs::write(&video, vec![1u8; 5000]).unwrap();
    let store = dir.path().join("queue.json");
    let row = serde_json::json!([{
        "id": 1,
        "url": format!("http://{addr}/watch?v=x"),
        "state": {"kind": "completed"},
        "progress": {"downloaded": 5000, "total": 5000, "speed": 0.0, "eta": null},
        "filename": "clip.mp4",
        "path": video.display().to_string(),
        "queue": dm_core::RECHECK_QUEUE,
    }]);
    std::fs::write(&store, row.to_string()).unwrap();
    let q = queue(dir.path(), 1);
    q.persist_to(store).await;
    assert_eq!(q.list().await.len(), 1, "the saved row was not read");

    assert_eq!(q.synchronize(dm_core::RECHECK_QUEUE).await, (0, vec![]));
    assert_eq!(q.get(1).await.unwrap().state, DownloadState::Completed);
    assert!(video.exists());
}

/// A download begun goes on into the folder it began in, when
/// the download folder is changed before it runs again.
#[tokio::test]
async fn a_download_goes_on_into_the_folder_it_began_in() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    // Its first run fails, as a stopped or interrupted one ends.
    let id = q.add(&format!("http://{addr}/flaky.bin")).await.unwrap();
    q.run().await;
    let began = q.get(id).await.unwrap().began_in;
    assert!(!began.is_empty(), "no folder recorded at its start");

    // The download folder moved meanwhile.
    let elsewhere = tempdir();
    let mut options = q.options().await;
    options.dest_dir = elsewhere.path().to_path_buf();
    q.set_options(options).await;

    assert!(q.retry(id).await);
    q.run().await;
    let done = q.get(id).await.unwrap();
    assert_eq!(done.state, DownloadState::Completed);
    assert!(
        std::path::Path::new(&done.path).starts_with(&began),
        "{} is not in the folder it began in, {began}",
        done.path
    );
}

/// With one download at a time, a row whose server keeps
/// answering 500 lends its place while it waits between attempts, so the
/// next row in line is fetched instead of waiting behind it — and the
/// failing row is still trying when the other finishes.
#[tokio::test]
async fn a_row_retrying_a_failing_server_lets_the_next_one_run() {
    let app = axum::Router::new()
        .route(
            "/broken.bin",
            axum::routing::get(|| async {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("down"))
                    .unwrap()
            }),
        )
        .route(
            "/good.bin",
            axum::routing::get(|| async { Response::new(Body::from(vec![7u8; 4096])) }),
        );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let q = Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            attempts: 10,
            ..common::options(dir.path())
        },
        1,
    );
    let broken = q.add(&format!("http://{addr}/broken.bin")).await.unwrap();
    let good = q.add(&format!("http://{addr}/good.bin")).await.unwrap();
    let runner = q.clone();
    let running = tokio::spawn(async move { runner.run().await });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let state = q.get(good).await.map(|e| e.state);
        if state == Some(DownloadState::Completed) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the good row never ran behind the failing one: {state:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        q.get(broken).await.map(|e| e.state),
        Some(DownloadState::Running),
        "the failing row should still be trying"
    );
    q.cancel(broken).await;
    let _ = tokio::time::timeout(Duration::from_secs(10), running).await;
}

#[tokio::test]
async fn a_finished_download_taken_off_the_list_by_mistake_is_put_back_where_it_was() {
    let (addr, _) = serve().await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let first = q.add(&format!("http://{addr}/one.bin")).await.unwrap();
    let second = q.add(&format!("http://{addr}/two.bin")).await.unwrap();
    let third = q.add(&format!("http://{addr}/three.bin")).await.unwrap();
    q.run().await;

    let rows = q.finished_rows(&[second]).await;
    assert_eq!(rows.len(), 1);
    q.remove(second).await;
    assert!(q.get(second).await.is_none());

    q.put_back(rows).await;
    let ids: Vec<_> = q.list().await.iter().map(|r| r.id).collect();
    assert_eq!(ids, [first, second, third], "not back in its place");
    let row = q.get(second).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert!(
        std::path::Path::new(&row.path).is_file(),
        "its file is not where the row says"
    );

    // Put back twice, it is not listed twice; and an unfinished row has
    // nothing to put back, since its fetched bytes go with it.
    let again = q.finished_rows(&[second]).await;
    q.put_back(again).await;
    assert_eq!(q.list().await.len(), 3);
    let held = q.add(&format!("http://{addr}/held.bin")).await.unwrap();
    q.pause(held).await;
    assert!(q.finished_rows(&[held]).await.is_empty());
}

/// A place freed by a download that has finished is filled at once, not at
/// the scheduler's next tick (fifty milliseconds): with one place and tiny
/// files, a queue is as fast as the files, not as slow as the ticks. Over a
/// thousand small files this was 131 files a second, however quick each was.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_finished_download_hands_its_place_to_the_next_at_once() {
    use axum::routing::get;
    const FILES: usize = 60;
    let app = axum::Router::new().fallback(get(|| async {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-length", 512)
            .body(Body::from(vec![3u8; 512]))
            .unwrap()
    }));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    for n in 0..FILES {
        q.add(&format!("http://{addr}/tiny{n}.bin")).await.unwrap();
    }
    let started = std::time::Instant::now();
    q.run().await;
    let took = started.elapsed();
    assert!(q
        .list()
        .await
        .iter()
        .all(|e| e.state == DownloadState::Completed));
    println!(
        "{FILES} tiny files, one place: {:.2}s ({:.0} files/s)",
        took.as_secs_f64(),
        FILES as f64 / took.as_secs_f64()
    );
    /* Waiting for the tick after each file puts a floor under this: one
     * place, a tick every 50 ms, so no less than about 50 ms a file at any
     * load, 3 s here. Handed on at once it is the files alone (0.2 s on an
     * idle machine, several seconds with every core busy, where this is
     * not a test to read anything into); the bound is just under the floor,
     * so the tick always fails it. */
    assert!(
        took < Duration::from_millis(FILES as u64 * 45),
        "{FILES} tiny files took {took:?}: places are refilled by the tick, not when they free"
    );
}

/// What `run` returns after is already owed to the disk: a `flush` straight
/// after finds the last download's state waiting, however soon the scheduler
/// found nothing left to do (a finished download wakes it at once).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_last_state_is_on_disk_when_run_returns() {
    use axum::routing::get;
    let app = axum::Router::new().fallback(get(|| async {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-length", 64)
            .body(Body::from(vec![1u8; 64]))
            .unwrap()
    }));
    let addr = common::serve(app).await;
    for round in 0..40 {
        let dir = tempdir();
        let store = dir.path().join("queue.json");
        let first = queue(dir.path(), 1);
        first.persist_to(store.clone()).await;
        let id = first
            .add(&format!("http://{addr}/r{round}.bin"))
            .await
            .unwrap();
        first.run().await;
        first.flush().await;
        let second = queue(dir.path(), 1);
        second.persist_to(store).await;
        assert_eq!(
            second.get(id).await.unwrap().state,
            DownloadState::Completed,
            "round {round}: the finished state was not on disk after run and flush"
        );
    }
}

/// A row that is not downloading has no speed and no time left, however it
/// stopped: paused (with the reports of what was still in flight arriving
/// after), finished or cancelled. The last run's figures stayed on it, and
/// a finished download read as moving to anything that read the list.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_row_that_is_not_downloading_has_no_speed_or_time_left() {
    use axum::routing::get;
    use futures_util::StreamExt as _;
    const SIZE: usize = 1_600_000;
    let app = axum::Router::new().fallback(get(|| async {
        let pieces = (0..SIZE / 32_000).map(|_| vec![9u8; 32_000]);
        Response::builder()
            .status(StatusCode::OK)
            .header("content-length", SIZE)
            .body(Body::from_stream(futures_util::stream::iter(pieces).then(
                |piece| async move {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    Ok::<_, std::io::Error>(bytes::Bytes::from(piece))
                },
            )))
            .unwrap()
    }));
    let addr = common::serve(app).await;
    let dir = tempdir();
    let q = queue(dir.path(), 2);
    let id = q.add(&format!("http://{addr}/moving.bin")).await.unwrap();
    let started = q.clone();
    tokio::spawn(async move { started.run().await });
    // Running, with a speed to speak of.
    let mut moving = false;
    for _ in 0..200 {
        let row = q.get(id).await.unwrap();
        if row.state == DownloadState::Running && row.progress.speed > 1.0 {
            moving = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(moving, "the download never showed a speed");

    q.pause(id).await;
    // Long enough for what was in flight to be reported.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Paused);
    assert_eq!(row.progress.speed, 0.0, "a paused row kept its speed");
    assert_eq!(row.progress.eta, None, "a paused row kept its time left");

    q.resume(id).await;
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, DownloadState::Completed);
    assert_eq!(row.progress.speed, 0.0, "a finished row kept its speed");
    assert_eq!(row.progress.eta, None, "a finished row kept its time left");
}

/// "Delete file after": kept on the row from the add and from the menu,
/// across a restart, and due counted from when the row finished.
#[tokio::test]
async fn a_row_keeps_when_its_file_is_to_be_deleted() {
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let first = queue(dir.path(), 1);
    first.persist_to(store.clone()).await;
    let id = first
        .add_with(
            "http://127.0.0.1:9/f.bin",
            dm_core::AddOptions {
                later: true,
                delete_after: Some(600),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let row = |rows: Vec<dm_core::Entry>| rows.into_iter().find(|e| e.id == id).unwrap();
    assert_eq!(row(first.list().await).delete_after, Some(600));
    first.flush().await;

    let second = queue(dir.path(), 1);
    second.persist_to(store.clone()).await;
    let mut kept = row(second.list().await);
    assert_eq!(kept.delete_after, Some(600), "not kept across a restart");

    // Not due before it has finished well, then counted from its finish.
    assert_eq!(kept.delete_due_in(1_000), None, "due while not finished");
    kept.state = DownloadState::Completed;
    kept.finished_at = Some(1_000);
    assert_eq!(kept.delete_due_in(1_100), Some(500));
    assert_eq!(kept.delete_due_in(1_600), Some(0));
    assert_eq!(kept.delete_due_in(9_999), Some(0));

    // Turned off from the menu, and 0 is no time at all.
    assert!(second.set_delete_after(id, Some(0)).await);
    assert_eq!(row(second.list().await).delete_after, None);
    assert!(second.set_delete_after(id, Some(3600)).await);
    assert_eq!(row(second.list().await).delete_after, Some(3600));
    assert!(second.set_delete_after(id, None).await);
    assert_eq!(row(second.list().await).delete_after, None);
}

#[tokio::test]
async fn a_shared_list_hands_back_an_unchanged_row_as_the_same_row() {
    let dir = tempdir();
    let q = queue(dir.path(), 1);
    let mut ids = Vec::new();
    for i in 0..3 {
        let options = dm_core::AddOptions {
            later: true,
            ..Default::default()
        };
        let address = format!("http://127.0.0.1:9/f{i}.bin");
        ids.push(q.add_with(&address, options).await.unwrap());
    }
    let before = q.list_shared().await;
    assert!(q.set_description(ids[1], "changed").await);
    let after = q.list_shared().await;
    assert!(
        std::sync::Arc::ptr_eq(&before[0], &after[0]),
        "an unchanged row was copied"
    );
    assert!(
        std::sync::Arc::ptr_eq(&before[2], &after[2]),
        "an unchanged row was copied"
    );
    assert!(
        !std::sync::Arc::ptr_eq(&before[1], &after[1]),
        "a changed row came back as it was"
    );
    // The list read before the change still says what it said.
    assert_eq!(before[1].description, "");
    assert_eq!(after[1].description, "changed");
    // And the copying list agrees with the shared one.
    let copied = q.list().await;
    assert_eq!(copied.len(), 3);
    assert!(copied
        .iter()
        .zip(&after)
        .all(|(c, s)| c.id == s.id && c.description == s.description));
}

/// A long list (past the length written whole at every save): what changed
/// goes to the journal, and a restart that never wrote the whole list again
/// (a dm killed) reads every change back.
#[tokio::test]
async fn a_long_list_comes_back_whole_from_its_file_and_journal() {
    let dir = tempdir();
    let store = dir.path().join("queue.json");
    let first = queue(dir.path(), 1);
    first.persist_to(store.clone()).await;
    let mut ids = Vec::new();
    for i in 0..600 {
        let options = dm_core::AddOptions {
            later: true,
            ..Default::default()
        };
        let address = format!("http://127.0.0.1:9/f{i}.bin");
        ids.push(first.add_with(&address, options).await.unwrap());
    }
    first.flush().await;
    // A write of the whole list, so what follows is the journal's.
    let whole = std::fs::metadata(&store).unwrap().len();
    assert!(first.set_description(ids[3], "a note").await);
    assert!(first.set_queue(ids[7], "Night").await);
    first.remove(ids[11]).await;
    let login = common::with_login("https://127.0.0.1:9/late.bin", "me", "open sesame");
    let added = first
        .add_with(
            &login,
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    first.flush().await;
    let journal = dir.path().join("queue.journal");
    assert!(journal.exists(), "no journal for a long list");
    let written = std::fs::read_to_string(&journal).unwrap();
    assert!(!written.contains("sesame"), "a password in the journal");
    let expected = first.list().await;
    // Killed: the queue file is the one written before the changes.
    drop(first);
    let second = queue(dir.path(), 1);
    second.persist_to(store.clone()).await;
    let back = second.list().await;
    assert_eq!(back.len(), expected.len());
    for (b, e) in back.iter().zip(&expected) {
        assert_eq!(
            (b.id, &b.url, &b.description, &b.queue),
            (e.id, &e.url, &e.description, &e.queue)
        );
    }
    assert!(
        back.iter().any(|r| r.id == added && r.url == login),
        "the login was lost"
    );
    assert!(
        !back.iter().any(|r| r.id == ids[11]),
        "a removed row came back"
    );
    // The start folded the journal into the file.
    second.flush().await;
    assert!(!journal.exists(), "the journal outlived a start");
    assert!(std::fs::metadata(&store).unwrap().len() != whole);
}
