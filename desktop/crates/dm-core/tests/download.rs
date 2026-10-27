//! The engine against a real server.
//!
//! Bytes are compared, not statuses: a download manager that reports success
//! and writes the wrong file is worse than one that fails, and only the
//! contents can tell those apart. The failure modes here are the ones the
//! previous engine learned about the hard way — a server that answers a range
//! with something that is not that range, and one that quietly sends less
//! than it promised.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use dm_core::{client, download, Control, Error};

mod common;
use common::{options, tempdir};
use rand::RngCore;

const MB: usize = 1024 * 1024;

#[derive(Clone)]
struct Server {
    body: Arc<Vec<u8>>,
    /// How the server misbehaves, if at all.
    mode: Mode,
    /// A name to offer. Not a misbehaviour, so not a `Mode`.
    disposition: Option<&'static str>,
    hits: Arc<AtomicUsize>,
    /// Every range asked for, as (first, last).
    asked: Arc<std::sync::Mutex<Vec<(usize, usize)>>>,
}

type Asked = Arc<std::sync::Mutex<Vec<(usize, usize)>>>;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Honest,
    /// Ignores Range entirely, the way a server without resume does.
    NoRanges,
    /// Ranges over one representation and answers with another: every range
    /// comes back longer than it was asked for.
    Decoded,
    /// Answers ranges but each one short, so the parts never add up.
    Short,
    /// Refuses the first few requests the way an overloaded server does,
    /// then serves normally.
    Flaky,
}

async fn serve(mode: Mode, body: Vec<u8>) -> (SocketAddr, Arc<AtomicUsize>) {
    spawn(mode, body, None).await
}

async fn spawn(
    mode: Mode,
    body: Vec<u8>,
    disposition: Option<&'static str>,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let (addr, hits, _) = spawn_watched(mode, body, disposition).await;
    (addr, hits)
}

/// A server that also keeps every range it was asked for.
async fn spawn_watched(
    mode: Mode,
    body: Vec<u8>,
    disposition: Option<&'static str>,
) -> (SocketAddr, Arc<AtomicUsize>, Asked) {
    let hits = Arc::new(AtomicUsize::new(0));
    let asked = Asked::default();
    let state = Server {
        body: Arc::new(body),
        mode,
        disposition,
        hits: Arc::clone(&hits),
        asked: Arc::clone(&asked),
    };

    let app = axum::Router::new()
        .route("/f.bin", get(handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, hits, asked)
}

async fn handler(State(s): State<Server>, headers: HeaderMap) -> Response {
    let hit = s.hits.fetch_add(1, Ordering::SeqCst);
    let total = s.body.len();

    // Fewer refusals than retry::MAX_ATTEMPTS, so surviving them is what the
    // policy is for; more than the budget would be a server that is simply
    // down, which is a different test.
    if s.mode == Mode::Flaky && hit < 3 {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("retry-after", "0")
            .body(Body::from("busy"))
            .unwrap();
    }

    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .map(|v| {
            let mut parts = v.splitn(2, '-');
            let a: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
            let b = parts
                .next()
                .unwrap_or("")
                .parse::<usize>()
                .unwrap_or(total - 1);
            (a, b.min(total - 1))
        });

    // No range asked for, or a server that does not do them: the whole body.
    let Some((a, b)) = range.filter(|_| s.mode != Mode::NoRanges) else {
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-length", total)
            .body(Body::from(s.body.to_vec()))
            .unwrap();
    };

    s.asked.lock().unwrap().push((a, b));

    // Only the bytes differ between the modes, so only the bytes are chosen
    // here — the response around them is built once.
    let slice = match s.mode {
        // Three times as many bytes as the range covers.
        Mode::Decoded => s.body[(a * 3).min(total)..((b + 1) * 3).min(total)].to_vec(),
        // One byte less than asked for, every time.
        Mode::Short => s.body[a.min(total)..=b.saturating_sub(1).min(total - 1)].to_vec(),
        _ => s.body[a..=b].to_vec(),
    };

    let mut res = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("content-range", format!("bytes {a}-{b}/{total}"))
        .header("accept-ranges", "bytes")
        .header("etag", "\"v1\"");
    if let Some(disposition) = s.disposition {
        res = res.header("content-disposition", disposition);
    }
    res.body(Body::from(slice)).unwrap()
}

fn bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut v);
    v
}

/// Downloads from a test server into `dir`, the way every test does.
async fn fetch(addr: SocketAddr, dir: &std::path::Path) -> Result<dm_core::Finished, Error> {
    download(&client(), &format!("http://{addr}/f.bin"), &options(dir)).await
}

#[tokio::test]
async fn a_segmented_download_is_byte_correct() {
    let body = bytes(12 * MB);
    let (addr, hits) = serve(Mode::Honest, body.clone()).await;
    let dir = tempdir();
    let dir = dir.path();

    let done = fetch(addr, dir).await.expect("the download failed");

    assert_eq!(
        done.connections, 8,
        "twelve megabytes should split eight ways"
    );
    // One probe plus one request per range: the connections are real.
    assert!(
        hits.load(Ordering::SeqCst) >= 9,
        "only {} requests",
        hits.load(Ordering::SeqCst)
    );

    let saved = std::fs::read(&done.path).unwrap();
    assert_eq!(saved.len(), body.len());
    assert!(saved == body, "the saved bytes are not the server's");
}

#[tokio::test]
async fn a_server_without_ranges_still_produces_the_right_file() {
    let body = bytes(3 * MB);
    let (addr, _) = serve(Mode::NoRanges, body.clone()).await;
    let dir = tempdir();
    let dir = dir.path();

    let done = fetch(addr, dir).await.expect("the download failed");

    assert_eq!(
        done.connections, 1,
        "a server without ranges gets one connection"
    );
    assert!(std::fs::read(&done.path).unwrap() == body);
}

#[tokio::test]
async fn a_range_longer_than_asked_for_is_refused_rather_than_trimmed() {
    /* The corruption that is invisible without this check: trimming to
     * length gives every part exactly the planned size, so the file is the
     * right length and wrong throughout, and nothing reports a problem. */
    let body = bytes(6 * MB);
    let (addr, _) = serve(Mode::Decoded, body).await;
    let dir = tempdir();
    let dir = dir.path();

    let err = fetch(addr, dir)
        .await
        .expect_err("a dishonest range was accepted");

    assert!(matches!(err, Error::DishonestRanges), "got {err:?}");
    assert!(
        !dir.join("out").exists() || std::fs::read_dir(dir.join("out")).unwrap().count() == 0,
        "a corrupt file reached the download folder"
    );
}

#[tokio::test]
async fn a_file_shorter_than_promised_is_not_saved() {
    let body = bytes(6 * MB);
    let (addr, _) = serve(Mode::Short, body).await;
    let dir = tempdir();
    let dir = dir.path();

    let err = fetch(addr, dir).await.expect_err("a short file was saved");

    assert!(matches!(err, Error::ShortFile { .. }), "got {err:?}");
}

#[tokio::test]
async fn a_download_resumes_from_what_is_already_staged() {
    let body = bytes(8 * MB);
    let (addr, _, asked) = spawn_watched(Mode::Honest, body.clone(), None).await;
    let dir = tempdir();
    let dir = dir.path();
    let opts = options(dir);

    // A previous run that got the first range and stopped, under the plan
    // it wrote down when it started.
    std::fs::create_dir_all(&opts.parts_dir).unwrap();
    let planned = dm_core::plan::plan(Some(body.len() as u64), 8, true);
    dm_core::plan::record(
        &opts.parts_dir,
        Some(body.len() as u64),
        true,
        Some("\"v1\""),
        &planned,
    )
    .unwrap();
    let first = body.len() as u64 / 8;
    std::fs::write(opts.parts_dir.join("part-0000"), &body[..first as usize]).unwrap();

    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();

    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "resuming corrupted the file"
    );
    // The finished range was not fetched again: past the probe's one byte,
    // nothing is asked for before its last 64 KiB, which a resume fetches
    // again to compare with what is on disk.
    let again: Vec<_> = asked
        .lock()
        .unwrap()
        .iter()
        .copied()
        .filter(|&(a, b)| b > a && (a as u64) + 64 * 1024 < first)
        .collect();
    assert!(again.is_empty(), "it started over: {again:?}");
}

/// A previous run's plan and staged parts, as it left them at a power cut.
fn staged(opts: &dm_core::Options, body: &[u8], parts: &[(usize, Vec<u8>)]) {
    std::fs::create_dir_all(&opts.parts_dir).unwrap();
    let planned = dm_core::plan::plan(Some(body.len() as u64), 8, true);
    dm_core::plan::record(
        &opts.parts_dir,
        Some(body.len() as u64),
        true,
        Some("\"v1\""),
        &planned,
    )
    .unwrap();
    for (index, bytes) in parts {
        std::fs::write(opts.parts_dir.join(format!("part-{index:04}")), bytes).unwrap();
    }
}

#[tokio::test]
async fn a_part_whose_end_never_reached_the_disk_is_repaired_on_resume() {
    /* A power cut can leave a part's length on disk without its last bytes:
     * zeros where they should be. The length is the resume ledger, so the
     * resume trusted them and the file came out the right size, wrong, and
     * Completed. Now the end is fetched again, compared, and cut back to the
     * last byte that agrees. */
    let body = bytes(8 * MB);
    let (addr, _, _) = spawn_watched(Mode::Honest, body.clone(), None).await;
    let dir = tempdir();
    let opts = options(dir.path());
    let half = MB / 2;
    let mut torn = body[..half].to_vec();
    torn.extend(std::iter::repeat_n(0u8, 20 * 1024));
    // A part that had all its bytes, the last 10 KiB of them lost the same way.
    let mut whole = body[MB..2 * MB].to_vec();
    let at = whole.len() - 10 * 1024;
    whole[at..].fill(0);
    staged(&opts, &body, &[(0, torn), (1, whole)]);

    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();

    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "the zeros a power cut left were kept"
    );
}

#[tokio::test]
async fn a_power_cut_that_lost_a_block_before_written_ones_is_repaired_not_restarted() {
    /* A disk puts blocks down in its own order, so a power cut can lose a
     * block and keep the ones after it. The first difference then falls in
     * a block of zeros with written bytes beyond it: still a power cut, to
     * be repaired from there, not a changed file that starts everything
     * again. */
    let body = bytes(8 * MB);
    let (addr, _, asked) = spawn_watched(Mode::Honest, body.clone(), None).await;
    let dir = tempdir();
    let opts = options(dir.path());
    let mut part = body[MB..2 * MB].to_vec();
    let lost = part.len() - 20 * 1024;
    part[lost..lost + 4096].fill(0);
    staged(&opts, &body, &[(0, body[..MB / 2].to_vec()), (1, part)]);

    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();

    assert!(std::fs::read(&done.path).unwrap() == body);
    let from_start = asked.lock().unwrap().iter().any(|&(a, b)| a == 0 && b > a);
    assert!(!from_start, "a power cut was taken for a changed file");
}

#[tokio::test]
async fn a_power_cut_that_lost_more_than_the_checked_end_is_repaired_whole() {
    /* The end of a part is fetched again and compared, 64 KiB of it. A power
     * cut that lost a quarter of a megabyte left zeros before that window
     * too, which were trusted: right length, wrong bytes, Completed. Where
     * the window opens on zeros the part is cut back to where they begin. */
    let body = bytes(8 * MB);
    let (addr, _, asked) = spawn_watched(Mode::Honest, body.clone(), None).await;
    let dir = tempdir();
    let opts = options(dir.path());
    let lost = 256 * 1024;
    let mut half = body[..MB / 2].to_vec();
    let at = half.len() - lost;
    half[at..].fill(0);
    let mut whole = body[MB..2 * MB].to_vec();
    let at = whole.len() - lost;
    whole[at..].fill(0);
    staged(&opts, &body, &[(0, half), (1, whole)]);

    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();

    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "zeros before the checked window were kept"
    );
    let from_start = asked.lock().unwrap().iter().any(|&(a, b)| a == 0 && b > a);
    assert!(!from_start, "the lost bytes were fetched by starting over");
}

#[tokio::test]
async fn a_torn_end_after_a_files_own_zeros_is_not_followed_back_through_them() {
    /* A file can be zeros for megabytes — a disk image, a sparse archive.
     * A torn last block after such a run had the repair follow the zeros
     * back to where they began and fetch every one of them again. One step
     * of the repair goes back 4 MiB at most; here the run is 5 MiB, and the
     * file still comes out whole. */
    let part = 6 * MB;
    let mut body = bytes(8 * part);
    let run = part + MB / 2..part + 5 * MB + MB / 2;
    body[run.clone()].fill(0);
    let (addr, _, asked) = spawn_watched(Mode::Honest, body.clone(), None).await;
    let dir = tempdir();
    let opts = options(dir.path());
    // Part 1 staged up to 32 KiB past the run, those 32 KiB lost.
    let staged_to = run.end + 32 * 1024;
    let mut torn = body[part..staged_to].to_vec();
    let lost = torn.len() - 32 * 1024;
    torn[lost..].fill(0);
    staged(&opts, &body, &[(1, torn)]);

    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();

    assert!(std::fs::read(&done.path).unwrap() == body);
    let deepest = asked
        .lock()
        .unwrap()
        .iter()
        .map(|&(a, _)| a)
        .filter(|a| (part..2 * part).contains(a))
        .min()
        .unwrap();
    assert!(
        deepest >= run.start + MB / 2,
        "the repair went back to {deepest}, through the file's own zeros from {}",
        run.start
    );
}

#[tokio::test]
async fn a_part_that_agrees_with_the_server_is_kept() {
    let body = bytes(8 * MB);
    let (addr, _, asked) = spawn_watched(Mode::Honest, body.clone(), None).await;
    let dir = tempdir();
    let opts = options(dir.path());
    staged(&opts, &body, &[(0, body[..MB / 2].to_vec())]);

    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();

    assert!(std::fs::read(&done.path).unwrap() == body);
    // Asked again from 64 KiB before its end, not from its start.
    let from_start = asked.lock().unwrap().iter().any(|&(a, b)| a == 0 && b > a);
    assert!(!from_start, "the agreeing part was fetched again whole");
}

#[tokio::test]
async fn a_changed_connection_setting_does_not_corrupt_a_resumed_download() {
    /* Parts are matched to ranges by index, and the offset an index stands
     * for depends on the plan. Pause at eight connections, set four, resume:
     * part-0001 held bytes 1M–2M of an 8 MB file and the new plan would put
     * it at 2M–4M. The plan written beside the parts is the one the resume
     * finishes, whatever the setting now says. */
    let body = bytes(8 * MB);
    let (addr, _, asked) = spawn_watched(Mode::Honest, body.clone(), None).await;
    let dir = tempdir();
    let dir = dir.path();
    let mut opts = options(dir);

    std::fs::create_dir_all(&opts.parts_dir).unwrap();
    let planned = dm_core::plan::plan(Some(body.len() as u64), 8, true);
    dm_core::plan::record(
        &opts.parts_dir,
        Some(body.len() as u64),
        true,
        Some("\"v1\""),
        &planned,
    )
    .unwrap();
    for (i, segment) in planned.iter().enumerate().take(2) {
        std::fs::write(
            opts.parts_dir.join(format!("part-{i:04}")),
            &body[segment.start as usize..=segment.end as usize],
        )
        .unwrap();
    }

    opts.connections = 4;
    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();

    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "resuming under a different setting corrupted the file"
    );
    // The recorded eight-way plan was finished: each of its six unfinished
    // ranges was asked for from its own start (less the overlap byte), and
    // nothing before the two staged ones' end. A four-way plan would have
    // begun at 2 MB, 4 MB and 6 MB only. Halving a range under way may add
    // more requests, each inside a range of this plan.
    let asked = asked.lock().unwrap().clone();
    for segment in planned.iter().skip(2) {
        assert!(
            asked.iter().any(|&(a, _)| a as u64 + 1 == segment.start),
            "the resume abandoned the recorded plan: {} never asked for in {asked:?}",
            segment.start
        );
    }
    // Only each staged part's last 64 KiB, asked for again to compare.
    let staged_end = planned[1].end as usize;
    let tails: Vec<usize> = planned[..2]
        .iter()
        .map(|s| s.end as usize + 1 - 64 * 1024)
        .collect();
    assert!(
        !asked
            .iter()
            .any(|&(a, b)| b > a && a < staged_end && !tails.contains(&a)),
        "a staged part was fetched again: {asked:?}"
    );
    assert_eq!(
        done.connections, 4,
        "taken by the connections the setting now asks for"
    );
}

#[tokio::test]
async fn parts_staged_under_no_known_plan_are_not_trusted() {
    // A part with no plan beside it could have been written at any offset.
    // It is discarded and the download is right, at the cost of the bytes.
    let body = bytes(4 * MB);
    let (addr, _) = serve(Mode::Honest, body.clone()).await;
    let dir = tempdir();
    let dir = dir.path();
    let opts = options(dir);

    std::fs::create_dir_all(&opts.parts_dir).unwrap();
    // Wrong bytes at index 1 — what a part from another plan looks like.
    std::fs::write(opts.parts_dir.join("part-0001"), &body[..MB]).unwrap();

    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .unwrap();
    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "a part of unknown provenance was spliced in"
    );
}

#[tokio::test]
async fn the_name_the_server_offers_is_used() {
    let (addr, _) = spawn(
        Mode::Honest,
        bytes(2 * MB),
        Some("attachment; filename=\"report card.pdf\""),
    )
    .await;
    let dir = tempdir();
    let dir = dir.path();

    let done = fetch(addr, dir).await.unwrap();

    assert_eq!(done.path.file_name().unwrap(), "report card.pdf");
}

#[tokio::test]
async fn a_second_download_does_not_overwrite_the_first() {
    let body = bytes(MB);
    let (addr, _) = serve(Mode::Honest, body).await;
    let dir = tempdir();
    let dir = dir.path();
    let url = format!("http://{addr}/f.bin");

    let a = download(&client(), &url, &options(dir)).await.unwrap();
    // Parts are cleared after assembly, so this is a genuinely fresh run.
    let b = download(&client(), &url, &options(dir)).await.unwrap();

    assert_ne!(a.path, b.path, "the second download replaced the first");
    assert!(a.path.exists() && b.path.exists());
}

#[tokio::test]
async fn a_server_that_refuses_at_first_is_waited_out() {
    /* Reliability, not speed: a download that gives up on the first 503 is
     * one that fails whenever a CDN is briefly busy. The server here refuses
     * its first three requests, the probe among them, before relenting. */
    let body = bytes(4 * MB);
    let (addr, hits) = serve(Mode::Flaky, body.clone()).await;
    let dir = tempdir();
    let dir = dir.path();

    let done = fetch(addr, dir)
        .await
        .expect("a transient refusal ended the download");

    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "retrying corrupted the file"
    );
    assert!(
        hits.load(Ordering::SeqCst) > 3,
        "the refusals were never retried"
    );
}

#[tokio::test]
async fn an_answer_is_not_retried_forever() {
    // A 404 is an answer. Retrying it is how a download manager looks busy
    // while achieving nothing.
    let (addr, hits) = spawn(Mode::Honest, bytes(1024), None).await;
    let dir = tempdir();
    let dir = dir.path();

    let err = download(
        &client(),
        &format!("http://{addr}/missing.bin"),
        &options(dir),
    )
    .await
    .expect_err("a 404 was treated as a download");

    assert!(
        matches!(err, Error::Refused { status: 404, .. }),
        "got {err:?}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the 404 route was retried");
}

#[tokio::test]
async fn progress_is_reported_while_the_file_arrives() {
    /* Nothing above the engine can draw a bar or an ETA unless the engine
     * says how far along it is, so this checks the samples actually arrive
     * and move — not merely that a channel was wired up. */
    let body = bytes(12 * MB);
    let (addr, _) = serve(Mode::Honest, body.clone()).await;
    let dir = tempdir();
    let dir = dir.path();

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let collector = tokio::spawn(async move {
        let (mut seen, mut names) = (Vec::new(), Vec::new());
        while let Some(update) = rx.recv().await {
            match update {
                dm_core::Update::Progress(p) => seen.push(p),
                dm_core::Update::Named { name, .. } => names.push(name),
                dm_core::Update::Validator(_) | dm_core::Update::Retrying { .. } => {}
            }
        }
        (seen, names)
    });

    let done = dm_core::download_reporting(
        &client(),
        &format!("http://{addr}/f.bin"),
        &options(dir),
        Some(tx),
    )
    .await
    .expect("the download failed");

    let (seen, names) = collector.await.unwrap();
    assert!(!seen.is_empty(), "no progress was ever reported");
    assert_eq!(names, ["f.bin"], "the row was never given a name to show");

    // It only ever goes forwards.
    for pair in seen.windows(2) {
        assert!(
            pair[1].downloaded >= pair[0].downloaded,
            "progress went backwards"
        );
    }

    let last = seen.last().unwrap();
    assert_eq!(
        last.total,
        Some(body.len() as u64),
        "the size was never reported"
    );
    assert!(
        last.speed > 0.0,
        "a finished download reported no rate at all"
    );
    assert!(last.fraction().unwrap() > 0.0);

    // And the file is still correct, with the counting on top.
    assert!(std::fs::read(&done.path).unwrap() == body);
}

#[tokio::test]
async fn a_paused_download_stops_and_keeps_what_it_had() {
    /* Pausing is stopping, and resuming is starting again — what is staged
     * is the ledger, so the second attempt asks only for the rest. This
     * checks both halves: that it actually stops, and that finishing later
     * still produces the right bytes. */
    let body = bytes(12 * MB);
    let (addr, _) = serve(Mode::Honest, body.clone()).await;
    let dir = tempdir();
    let dir = dir.path();

    let mut opts = options(dir);
    let control = opts.control.clone();

    // Paused before it starts, so it stops at the first chunk boundary.
    control.pause();
    let err = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect_err("a paused download ran to completion");
    assert!(matches!(err, Error::Paused), "got {err:?}");

    let out = dir.join("out");
    assert!(
        !out.exists() || std::fs::read_dir(&out).unwrap().count() == 0,
        "a paused download saved a file"
    );

    // Resumed: a fresh control, and whatever reached disk is reused.
    opts.control = Control::new();
    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect("resuming failed");

    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "resuming corrupted the file"
    );
}

/// Ranges are written in place, into one staged file, and finishing is a
/// rename: a download on many connections stages no part files to copy
/// from, writes down how far each range got, and leaves nothing staged once
/// it is whole. The copy it replaced took longer than the fetch on a fast
/// link (2 GB on 8 connections: about 3.6 s end to end against 2.0 s now,
/// `qa/after.md`).
#[tokio::test]
async fn ranges_are_written_in_place_and_finishing_copies_nothing() {
    let body = bytes(12 * MB);
    let (addr, _) = serve(Mode::Honest, body.clone()).await;
    let dir = tempdir();
    let dir = dir.path();
    let mut opts = options(dir);
    opts.connections = 8;
    let names = |opts: &dm_core::Options| -> Vec<String> {
        std::fs::read_dir(&opts.parts_dir)
            .map(|it| {
                it.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };

    // Held to 2 MB/s, so a pause after a moment stops it part-way.
    opts.limiter = dm_core::Limiter::new(2 * MB as u64);
    let control = opts.control.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        control.pause();
    });
    let err = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect_err("a paused download ran to completion");
    assert!(matches!(err, Error::Paused), "got {err:?}");
    let staged = names(&opts);
    assert!(staged.iter().any(|n| n == "staged"), "{staged:?}");
    assert!(staged.iter().any(|n| n.starts_with("done-")), "{staged:?}");
    assert!(
        !staged.iter().any(|n| n.starts_with("part-")),
        "a part file was staged: {staged:?}"
    );
    assert_eq!(
        std::fs::metadata(opts.parts_dir.join("staged"))
            .unwrap()
            .len(),
        body.len() as u64,
        "the staged file is made as long as the file"
    );

    opts.control = Control::new();
    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect("resuming failed");
    assert!(
        std::fs::read(&done.path).unwrap() == body,
        "the file is wrong"
    );
    let left = names(&opts);
    assert!(
        !left
            .iter()
            .any(|n| n == "staged" || n.starts_with("done-") || n.starts_with("part-")),
        "left staged: {left:?}"
    );
}

#[tokio::test]
async fn a_cancelled_download_leaves_nothing_saved() {
    let (addr, _) = serve(Mode::Honest, bytes(12 * MB)).await;
    let dir = tempdir();
    let dir = dir.path();

    let opts = options(dir);
    opts.control.cancel();

    let err = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect_err("a cancelled download ran to completion");

    assert!(matches!(err, Error::Cancelled), "got {err:?}");
    let out = dir.join("out");
    assert!(!out.exists() || std::fs::read_dir(&out).unwrap().count() == 0);
}

#[tokio::test]
async fn stopping_is_not_something_to_retry() {
    // A pause that was waited out and tried again would take five attempts
    // to honour, which is not a pause.
    let (addr, hits) = serve(Mode::Honest, bytes(12 * MB)).await;
    let dir = tempdir();
    let dir = dir.path();

    let opts = options(dir);
    opts.control.pause();
    let _ = download(&client(), &format!("http://{addr}/f.bin"), &opts).await;

    // The probe, plus one request per range, and no second round.
    assert!(
        hits.load(Ordering::SeqCst) <= 9,
        "{} requests suggests the pause was retried",
        hits.load(Ordering::SeqCst)
    );
}

/// A server that answers the headers and then sends nothing, ever.
async fn serve_stalled() -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let app = axum::Router::new().route(
        "/f.bin",
        get(move |headers: HeaderMap| {
            let hits = Arc::clone(&counted);
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                // The probe asks for one byte and gets it; every range after
                // it gets headers and then silence.
                if headers
                    .get("range")
                    .is_some_and(|r| r.to_str().unwrap_or("") == "bytes=0-0")
                {
                    return Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-range", "bytes 0-0/8388608")
                        .header("accept-ranges", "bytes")
                        .body(Body::from(vec![0u8]))
                        .unwrap();
                }
                let stalled = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
                // Each range's own, so each connection stalls on an honest
                // answer rather than being refused for a dishonest one.
                let asked = headers
                    .get("range")
                    .and_then(|r| r.to_str().ok())
                    .and_then(|r| r.strip_prefix("bytes="))
                    .unwrap_or("0-1048575")
                    .to_string();
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header("content-range", format!("bytes {asked}/8388608"))
                    .header("accept-ranges", "bytes")
                    .body(Body::from_stream(stalled))
                    .unwrap()
            }
        }),
    );
    let addr = common::serve(app).await;
    (addr, hits)
}

#[tokio::test]
async fn a_pause_on_a_stalled_connection_is_honoured_within_the_read_timeout() {
    /* Every route honours a pause between chunks, so a server that sent
     * its headers and then nothing held the task forever with the flag
     * never read. The read timeout ends the stalled read; the retry loop
     * then sees the pause rather than making another request. */
    let (addr, hits) = serve_stalled().await;
    let dir = tempdir();
    let opts = options(dir.path());
    let control = opts.control.clone();
    let client = dm_core::builder()
        .read_timeout(Duration::from_millis(500))
        .build()
        .unwrap();

    let url = format!("http://{addr}/f.bin");
    let running = tokio::spawn(async move { download(&client, &url, &opts).await });

    // Paused once every range is in flight and stalled: the count has
    // stopped rising. A fixed wait raced the requests on a loaded machine,
    // counting one sent before the pause as one after it.
    let mut seen = usize::MAX;
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let now = hits.load(Ordering::SeqCst);
        if now == seen && now > 0 {
            break;
        }
        seen = now;
    }
    let before = hits.load(Ordering::SeqCst);
    control.pause();

    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the pause was never honoured: the stalled read held the task")
        .unwrap();
    assert!(matches!(outcome, Err(Error::Paused)), "got {outcome:?}");
    // The timeout ended the reads and the pause was seen before any retry
    // asked the server again.
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "a paused download made another request after its read timed out"
    );
}

/* ------------------------------------------------------ what it costs on disk */

/// Every byte under `dir`, however deeply nested.
fn bytes_under(dir: &std::path::Path) -> u64 {
    let mut seen = std::collections::HashSet::new();
    bytes_under_once(dir, &mut seen)
}

/// Each file counted once, by what it is on disk rather than by its name:
/// the walk reads one folder after another, and a file renamed from the
/// parts folder into the download folder between the two reads was counted
/// in both — a move sampled as twice the size, which failed a check now and
/// then on a loaded machine.
fn bytes_under_once(
    dir: &std::path::Path,
    seen: &mut std::collections::HashSet<(u64, u64)>,
) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => total += bytes_under_once(&entry.path(), seen),
            _ => {
                let Ok(meta) = entry.metadata() else { continue };
                // What a file is on disk: its device and inode where the
                // system says, its path elsewhere (a rename moves it, so a
                // walk there can still count a moving file twice).
                #[cfg(unix)]
                let key = {
                    use std::os::unix::fs::MetadataExt;
                    (meta.dev(), meta.ino())
                };
                #[cfg(not(unix))]
                let key = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    entry.path().hash(&mut h);
                    (0, h.finish())
                };
                if !seen.insert(key) {
                    continue;
                }
                total += meta.len();
            }
        }
    }
    total
}

#[tokio::test]
async fn a_download_never_needs_twice_its_own_size_on_disk() {
    /* Assembly used to copy every part into the finished file and only then
     * delete them, so at the moment it finished, the parts and the file both
     * existed in full: a four gigabyte download needed eight gigabytes free,
     * and failed on a disk that had six.
     *
     * Measured by watching the directory rather than by reading the code,
     * because the property is about what is on the disk at one instant and
     * nothing in the source says that out loud. */
    let size = 64 * MB;
    let body = bytes(size);
    let (addr, _) = serve(Mode::Honest, body.clone()).await;
    let dir = tempdir();
    let root = dir.path().to_path_buf();

    let watching = root.clone();
    let peak = Arc::new(AtomicU64::new(0));
    let sampler = {
        let peak = Arc::clone(&peak);
        tokio::spawn(async move {
            loop {
                peak.fetch_max(bytes_under(&watching), Ordering::Relaxed);
                tokio::time::sleep(Duration::from_micros(500)).await;
            }
        })
    };

    let done = download(&client(), &format!("http://{addr}/f.bin"), &options(&root))
        .await
        .expect("the download failed");
    peak.fetch_max(bytes_under(&root), Ordering::Relaxed);
    sampler.abort();

    assert_eq!(done.bytes, size as u64);
    let peak = peak.load(Ordering::Relaxed);
    let allowed = size as u64 + size as u64 / 4;
    assert!(
        peak <= allowed,
        "peak was {peak} bytes for a {size}-byte file — {:.2}x, and anything \
         approaching 2x means the parts are outliving the copy",
        peak as f64 / size as f64
    );
}

/// A hotlink-protected server: the file only to a request that names its page.
///
/// This is what a browser hand-off has to carry through. Without the referer,
/// the app is given a link the browser could fetch and it cannot — the file
/// silently lost. With it, the download works.
#[tokio::test]
async fn a_referer_is_sent_so_a_hotlink_protected_file_downloads() {
    async fn guard(headers: HeaderMap) -> Response {
        let ok = headers
            .get(axum::http::header::REFERER)
            .and_then(|v| v.to_str().ok())
            == Some("https://example.com/page");
        if ok {
            Response::builder()
                .header(axum::http::header::ACCEPT_RANGES, "bytes")
                .header(axum::http::header::CONTENT_LENGTH, "6")
                .body(Body::from("hotdog"))
                .unwrap()
        } else {
            Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::from("no hotlinking"))
                .unwrap()
        }
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, get(guard).into_make_service())
            .await
            .unwrap();
    });

    let dir = tempdir();
    let url = format!("http://{addr}/protected.bin");

    // Without the page, refused — exactly the silent loss to avoid.
    let refused = download(&client(), &url, &options(dir.path())).await;
    assert!(
        refused.is_err(),
        "the server should have refused with no referer"
    );

    // With the page, it downloads.
    let opts = dm_core::Options {
        referrer: Some("https://example.com/page".to_string()),
        ..options(dir.path())
    };
    let done = download(&client(), &url, &opts)
        .await
        .expect("referer download");
    let saved = std::fs::read(&done.path).unwrap();
    assert_eq!(
        saved, b"hotdog",
        "the referer download saved the wrong bytes"
    );
}

/// A single-connection download must not double disk at assembly either.
///
/// A server that will not serve ranges is fetched as one part, and that one
/// part IS the file — joining it used to mean copying it, so for the length of
/// the copy the part and the growing copy both existed in full: 2x the disk
/// for a file that needed none of it. Moved rather than copied, the peak stays
/// at one file's worth. Watched on the disk, because that is where the
/// property lives.
#[tokio::test]
async fn a_single_part_download_is_moved_into_place_not_copied() {
    let size = 48 * MB;
    let body = bytes(size);
    let (addr, _) = serve(Mode::NoRanges, body.clone()).await;
    let dir = tempdir();
    let root = dir.path().to_path_buf();

    let watching = root.clone();
    let peak = Arc::new(AtomicU64::new(0));
    let sampler = {
        let peak = Arc::clone(&peak);
        tokio::spawn(async move {
            loop {
                peak.fetch_max(bytes_under(&watching), Ordering::Relaxed);
                tokio::time::sleep(Duration::from_micros(500)).await;
            }
        })
    };

    let done = download(&client(), &format!("http://{addr}/f.bin"), &options(&root))
        .await
        .expect("the download failed");
    peak.fetch_max(bytes_under(&root), Ordering::Relaxed);
    sampler.abort();

    let saved = std::fs::read(&done.path).unwrap();
    assert_eq!(saved, body, "the moved file is not the bytes that arrived");

    let peak = peak.load(Ordering::Relaxed);
    // Room for the metadata sidecar and rounding, but nowhere near a second
    // copy. Copying would have peaked at ~2x here.
    let allowed = size as u64 + size as u64 / 8;
    assert!(
        peak <= allowed,
        "peak was {peak} for a {size}-byte single-part download — {:.2}x; a \
         move should never approach the 2x a copy costs",
        peak as f64 / size as f64
    );
}

#[tokio::test]
async fn a_pause_on_a_stalled_connection_is_honoured_at_once_whatever_the_read_timeout() {
    // The read timeout is a setting, up to ten minutes; a pause must not
    // wait for it.
    let (addr, _hits) = serve_stalled().await;
    let dir = tempdir();
    let opts = options(dir.path());
    let control = opts.control.clone();
    let client = dm_core::builder_with(Duration::from_secs(600), false)
        .build()
        .unwrap();
    let url = format!("http://{addr}/f.bin");
    let running = tokio::spawn(async move { download(&client, &url, &opts).await });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let asked = std::time::Instant::now();
    control.pause();
    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the pause waited for the read timeout")
        .unwrap();
    assert!(
        matches!(outcome, Err(dm_core::Error::Paused)),
        "{outcome:?}"
    );
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );
}

#[tokio::test]
async fn a_pause_before_the_server_answers_is_honoured_at_once() {
    // A server that takes the connection and never sends headers: the
    // request is held for the read timeout, up to ten minutes; a pause is not.
    async fn never() -> axum::response::Response {
        futures_util::future::pending::<()>().await;
        unreachable!()
    }
    let addr = common::serve(axum::Router::new().fallback(never)).await;
    let dir = tempdir();
    let opts = options(dir.path());
    let control = opts.control.clone();
    let client = dm_core::builder_with(Duration::from_secs(600), false)
        .build()
        .unwrap();
    let url = format!("http://{addr}/f.bin");
    let running = tokio::spawn(async move { download(&client, &url, &opts).await });
    tokio::time::sleep(Duration::from_millis(800)).await;
    let asked = std::time::Instant::now();
    control.pause();
    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the pause waited for the server")
        .unwrap();
    assert!(
        matches!(outcome, Err(dm_core::Error::Paused)),
        "{outcome:?}"
    );
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );
}

/// The probe's connection is the download's: the byte it asked for is read,
/// so the connection goes back to the pool and the first range is sent on
/// it, rather than on a new one with its own handshake.
#[tokio::test]
async fn the_probes_connection_carries_the_download() {
    let body = Arc::new(vec![7u8; 2 * MB]);
    let ports = Arc::new(std::sync::Mutex::new(
        std::collections::HashSet::<u16>::new(),
    ));
    let (data, seen) = (body.clone(), ports.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(
            move |axum::extract::ConnectInfo(from): axum::extract::ConnectInfo<SocketAddr>,
                  headers: HeaderMap| {
                let (data, seen) = (data.clone(), seen.clone());
                async move {
                    seen.lock().unwrap().insert(from.port());
                    let size = data.len();
                    let (a, b) = headers
                        .get("range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("bytes="))
                        .and_then(|v| v.split_once('-'))
                        .map(|(a, b)| {
                            (
                                a.parse::<usize>().unwrap(),
                                b.parse::<usize>().unwrap_or(size - 1).min(size - 1),
                            )
                        })
                        .unwrap_or((0, size - 1));
                    Response::builder()
                        .status(206)
                        .header("content-range", format!("bytes {a}-{b}/{size}"))
                        .header("content-length", b - a + 1)
                        .header("etag", "\"one\"")
                        .body(Body::from(data[a..=b].to_vec()))
                        .unwrap()
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });

    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 1,
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect("the download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
    assert_eq!(
        ports.lock().unwrap().len(),
        1,
        "the probe and the one range were made on separate connections"
    );
}

/// A server that serves ranges, and says which connections (by their client
/// port) it was asked on. `pace` holds each connection to about that many
/// bytes a second, as a server that limits each one does; `None` is as fast
/// as the socket takes it.
async fn serve_counting(
    body: Arc<Vec<u8>>,
    pace: Option<usize>,
) -> (
    SocketAddr,
    Arc<std::sync::Mutex<std::collections::HashSet<u16>>>,
) {
    let ports = Arc::new(std::sync::Mutex::new(
        std::collections::HashSet::<u16>::new(),
    ));
    let (data, seen) = (body, ports.clone());
    let app = axum::Router::new().route(
        "/f.bin",
        get(
            move |axum::extract::ConnectInfo(from): axum::extract::ConnectInfo<SocketAddr>,
                  headers: HeaderMap| {
                let (data, seen) = (data.clone(), seen.clone());
                async move {
                    seen.lock().unwrap().insert(from.port());
                    let size = data.len();
                    let (a, b) = headers
                        .get("range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.strip_prefix("bytes="))
                        .and_then(|v| v.split_once('-'))
                        .map(|(a, b)| {
                            (
                                a.parse::<usize>().unwrap(),
                                b.parse::<usize>().unwrap_or(size - 1).min(size - 1),
                            )
                        })
                        .unwrap_or((0, size - 1));
                    let piece = bytes::Bytes::copy_from_slice(&data[a..=b]);
                    let chunk = 64 * 1024;
                    let stream = futures_util::stream::unfold(0usize, move |at| {
                        let piece = piece.clone();
                        async move {
                            if at >= piece.len() {
                                return None;
                            }
                            let end = (at + chunk).min(piece.len());
                            if let Some(rate) = pace {
                                let wait = (end - at) as f64 / rate as f64;
                                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                            }
                            Some((Ok::<_, std::io::Error>(piece.slice(at..end)), end))
                        }
                    });
                    Response::builder()
                        .status(206)
                        .header("content-range", format!("bytes {a}-{b}/{size}"))
                        .header("content-length", b - a + 1)
                        .header("etag", "\"one\"")
                        .body(Body::from_stream(stream))
                        .unwrap()
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    (addr, ports)
}

/// A small file from a server that holds each connection to a megabyte a
/// second is fetched on more than one: the first connection is seen to be
/// slow, and the others join it, so it takes about a second and not three.
#[tokio::test]
async fn a_small_file_from_a_slow_server_is_fetched_on_helping_connections() {
    let body = Arc::new(vec![9u8; 3 * MB]);
    let (addr, ports) = serve_counting(body.clone(), Some(MB)).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 8,
        ..options(dir.path())
    };
    let started = std::time::Instant::now();
    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect("the download failed");
    let took = started.elapsed();
    assert_eq!(
        std::fs::read(&done.path).unwrap(),
        *body,
        "the file is not the file"
    );
    assert!(
        ports.lock().unwrap().len() > 2,
        "no helping connection joined: {} used",
        ports.lock().unwrap().len()
    );
    assert!(
        took < Duration::from_millis(2200),
        "3 MB at 1 MB/s on one connection takes three seconds; helped, {took:?}"
    );
}

/// The same small file from a server that is not slow is fetched on the one
/// connection the probe opened: a helper would cost a round trip to save
/// nothing, so none is ever opened.
#[tokio::test]
async fn a_small_file_from_a_fast_server_gets_no_helping_connection() {
    let body = Arc::new(vec![5u8; 3 * MB / 2]);
    let (addr, ports) = serve_counting(body.clone(), None).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 8,
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect("the download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
    assert_eq!(
        ports.lock().unwrap().len(),
        1,
        "a connection was opened that the fast server did not need"
    );
}

/// Pausing while helpers wait to see whether they are needed pauses the
/// download at once; they do not hold it up.
#[tokio::test]
async fn a_pause_while_helpers_are_still_waiting_is_honoured() {
    let body = Arc::new(vec![3u8; 3 * MB]);
    let (addr, _ports) = serve_counting(body, Some(MB)).await;
    let dir = tempdir();
    let control = Control::new();
    let opts = dm_core::Options {
        connections: 8,
        control: control.clone(),
        ..options(dir.path())
    };
    let url = format!("http://{addr}/f.bin");
    let task = tokio::spawn(async move { download(&client(), &url, &opts).await });
    tokio::time::sleep(Duration::from_millis(60)).await;
    control.pause();
    let paused = std::time::Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("the pause was not honoured")
        .unwrap();
    assert!(matches!(outcome, Err(Error::Paused)), "{outcome:?}");
    assert!(
        paused.elapsed() < Duration::from_secs(2),
        "the pause took {:?}",
        paused.elapsed()
    );
}

/// With a speed limit of dm's own set, the limit is the bottleneck, and a
/// helping connection would only share it out again: none is opened.
#[tokio::test]
async fn a_download_under_a_speed_limit_gets_no_helping_connection() {
    let body = Arc::new(vec![4u8; 3 * MB / 2]);
    let (addr, ports) = serve_counting(body.clone(), Some(MB)).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        connections: 8,
        limiter: dm_core::Limiter::new(2 * MB as u64),
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/f.bin"), &opts)
        .await
        .expect("the download failed");
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
    assert_eq!(
        ports.lock().unwrap().len(),
        1,
        "a connection was opened to share a limit out"
    );
}

/// A body with no length to plan the room for, that goes on and on (or a
/// small compressed one that unpacks to gigabytes), is stopped before it
/// fills the disk: the free space is looked at as it comes.
#[tokio::test]
async fn a_body_of_no_known_length_is_stopped_before_it_fills_the_disk() {
    use axum::{body::Body, routing::get, Router};
    use futures_util::stream;
    let app = Router::new().route(
        "/endless.bin",
        get(|| async {
            let chunk = bytes::Bytes::from(vec![7u8; 256 * 1024]);
            Body::from_stream(stream::iter(
                std::iter::repeat_n(chunk, 600).map(Ok::<_, std::io::Error>),
            ))
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    // A disk with 100 MB free: under the floor.
    let opts = dm_core::Options {
        room: dm_core::Room::with_free_space(|_| Some(100 * 1024 * 1024)),
        attempts: 1,
        ..options(dir.path())
    };
    let started = std::time::Instant::now();
    let result = download(&client(), &format!("http://{addr}/endless.bin"), &opts).await;
    let why = match result {
        Err(e) => e.to_string(),
        Ok(done) => panic!(
            "{} bytes of a body with no end were saved",
            std::fs::metadata(&done.path).map(|m| m.len()).unwrap_or(0)
        ),
    };
    assert!(why.contains("less than"), "{why}");
    // Stopped within a few looks of the start, not after the 150 MB sent.
    let on_disk: u64 = walk(dir.path())
        .iter()
        .filter_map(|p| p.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    assert!(
        on_disk < 100 * 1024 * 1024,
        "{on_disk} bytes were written before it stopped ({:?})",
        started.elapsed()
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            out.push(e.path());
            if e.path().is_dir() {
                out.extend(walk(&e.path()));
            }
        }
    }
    out
}

/// A redirect to an address the client will not follow (a local file, an
/// FTP server) is the download's failure, not an empty file that finished.
#[tokio::test]
async fn a_redirect_to_an_address_dm_does_not_follow_fails() {
    use axum::{routing::get, Router};
    let app = Router::new().route(
        "/to-file.bin",
        get(|| async {
            Response::builder()
                .status(302)
                .header("location", "file:///etc/hosts")
                .body(Body::empty())
                .unwrap()
        }),
    );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };
    match download(&client(), &format!("http://{addr}/to-file.bin"), &opts).await {
        Err(e) => assert!(e.to_string().contains("does not follow"), "{e}"),
        Ok(done) => panic!(
            "a redirect to a file: address was recorded as finished ({} bytes at {})",
            std::fs::metadata(&done.path).map(|m| m.len()).unwrap_or(0),
            done.path.display()
        ),
    }
}

/// A response that is a few hundred kilobytes on the wire and gigabytes when
/// undone (a decompression bomb) is stopped by the ratio between the two,
/// well before it fills anything; the same coding on a page of text, which is
/// ten times its size at most, is undone in full.
#[tokio::test]
async fn a_compressed_body_that_unpacks_to_far_more_than_it_sent_is_stopped() {
    use axum::{routing::get, Router};
    use std::io::Write;
    let bomb = {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let zeros = vec![0u8; 1024 * 1024];
        for _ in 0..100 {
            e.write_all(&zeros).unwrap();
        }
        e.finish().unwrap()
    };
    assert!(bomb.len() < 1024 * 1024, "the bomb is {} bytes", bomb.len());
    let text: Vec<u8> = (0..2_000_000u32)
        .flat_map(|i| format!("line {}\n", i % 977).into_bytes())
        .collect();
    let text_gz = {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&text).unwrap();
        e.finish().unwrap()
    };
    let plain = text.clone();
    let app = Router::new()
        .route(
            "/bomb.bin",
            get(move || {
                let body = bomb.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "gzip")
                        .body(Body::from(body))
                        .unwrap()
                }
            }),
        )
        .route(
            "/page.txt",
            get(move || {
                let body = text_gz.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "gzip")
                        .body(Body::from(body))
                        .unwrap()
                }
            }),
        );
    let addr = common::serve(app).await;
    let dir = tempdir();
    let opts = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };
    let why = match download(&client(), &format!("http://{addr}/bomb.bin"), &opts).await {
        Err(e) => e.to_string(),
        Ok(done) => panic!(
            "a bomb was saved: {} bytes",
            std::fs::metadata(&done.path).map(|m| m.len()).unwrap_or(0)
        ),
    };
    assert!(why.contains("unpacks to far more"), "{why}");
    let written: u64 = walk(dir.path())
        .iter()
        .filter_map(|p| p.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    assert!(
        written < 150 * 1024 * 1024,
        "{written} bytes were written before it stopped"
    );

    let dir = tempdir();
    let opts = dm_core::Options {
        attempts: 1,
        ..options(dir.path())
    };
    let done = download(&client(), &format!("http://{addr}/page.txt"), &opts)
        .await
        .expect("a compressed page of text was refused");
    assert_eq!(std::fs::read(&done.path).unwrap(), plain);
}
