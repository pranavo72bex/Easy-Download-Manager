//! What the File and Downloads menus do to a row, beyond pause and resume:
//! "Add for later" into a named queue, a folder and name chosen before the
//! start, "Update download link", "Redownload", the order rows wait in,
//! "Move/Rename", and how each connection of a download is getting on.

mod common;

use axum::body::Body;
use axum::http::Uri;
use axum::response::Response;
use dm_core::{client, AddOptions, Queue, State, MAIN_QUEUE};

use common::tempdir;

const BODY: &[u8] = b"the bytes of the file, exactly";

async fn serve() -> std::net::SocketAddr {
    async fn handler(uri: Uri) -> Response {
        if uri.path().contains("gone") {
            return Response::builder()
                .status(404)
                .body(Body::from("no"))
                .unwrap();
        }
        Response::builder()
            .header("content-length", BODY.len())
            .body(Body::from(BODY))
            .unwrap()
    }
    common::serve(axum::Router::new().fallback(handler)).await
}

fn queue(dir: &std::path::Path) -> Queue {
    Queue::new(
        client(),
        dm_core::Options {
            connections: 1,
            ..common::options(dir)
        },
        2,
    )
}

#[tokio::test]
async fn download_later_waits_in_its_queue_until_the_queue_starts() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q
        .add_with(
            &format!("http://{addr}/later.bin"),
            AddOptions {
                queue: MAIN_QUEUE.into(),
                later: true,
                description: "for tonight".into(),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();

    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, State::Paused, "a later download started at once");
    assert_eq!(row.description, "for tonight");
    // The two built-in queues are always there.
    assert_eq!(q.queues().await, [MAIN_QUEUE, dm_core::RECHECK_QUEUE]);

    // Stopping a queue with nothing running stops nothing; starting it
    // starts its rows and no others.
    let other = q.add(&format!("http://{addr}/other.bin")).await.unwrap();
    q.pause(other).await;
    assert_eq!(q.start_queue(MAIN_QUEUE).await, 1);
    q.run().await;
    assert_eq!(q.get(id).await.unwrap().state, State::Completed);
    assert_eq!(q.get(other).await.unwrap().state, State::Paused);
}

#[tokio::test]
async fn stopping_a_queue_holds_only_its_rows() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let mine = q
        .add_with(
            &format!("http://{addr}/a.bin"),
            AddOptions {
                queue: "Night".into(),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    let theirs = q.add(&format!("http://{addr}/b.bin")).await.unwrap();
    assert_eq!(q.stop_queue("Night").await, 1);
    assert_eq!(q.get(mine).await.unwrap().state, State::Paused);
    assert_eq!(q.get(theirs).await.unwrap().state, State::Queued);
}

#[tokio::test]
async fn a_chosen_folder_and_name_are_where_the_file_lands() {
    let addr = serve().await;
    let dir = tempdir();
    let chosen = dir.path().join("chosen");
    let q = queue(dir.path());
    let id = q
        .add_with(
            &format!("http://{addr}/server-name.bin"),
            AddOptions {
                save_dir: Some(chosen.to_string_lossy().into_owned()),
                save_name: Some("mine.bin".into()),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    q.run().await;

    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, State::Completed, "{:?}", row.state);
    assert_eq!(std::path::Path::new(&row.path), chosen.join("mine.bin"));
    assert_eq!(row.filename, "mine.bin");
    assert_eq!(std::fs::read(&row.path).unwrap(), BODY);
}

#[tokio::test]
async fn a_chosen_name_never_replaces_a_file_already_there() {
    let addr = serve().await;
    let dir = tempdir();
    let chosen = dir.path().join("chosen");
    std::fs::create_dir_all(&chosen).unwrap();
    std::fs::write(chosen.join("mine.bin"), b"somebody's").unwrap();
    let q = queue(dir.path());
    let id = q
        .add_with(
            &format!("http://{addr}/x.bin"),
            AddOptions {
                save_dir: Some(chosen.to_string_lossy().into_owned()),
                save_name: Some("mine.bin".into()),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(
        std::fs::read(chosen.join("mine.bin")).unwrap(),
        b"somebody's"
    );
    assert_eq!(std::path::Path::new(&row.path), chosen.join("mine (1).bin"));
}

#[tokio::test]
async fn an_expired_link_is_refreshed_and_the_download_completes() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/gone.bin")).await.unwrap();
    q.run().await;
    assert!(matches!(q.get(id).await.unwrap().state, State::Failed(_)));

    assert!(
        !q.set_url(id, "javascript:alert(1)").await,
        "a non-link was taken"
    );
    assert!(q.set_url(id, &format!("http://{addr}/fresh.bin")).await);
    assert!(q.retry(id).await);
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, State::Completed);
    assert!(row.url.ends_with("/fresh.bin"));
}

#[tokio::test]
async fn redownload_fetches_again_from_nothing_beside_the_first_copy() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/again.bin")).await.unwrap();
    q.run().await;
    let first = q.get(id).await.unwrap().path;

    assert!(q.redownload(id).await);
    assert_eq!(q.get(id).await.unwrap().state, State::Queued);
    q.run().await;
    let second = q.get(id).await.unwrap();
    assert_eq!(second.state, State::Completed);
    assert_ne!(second.path, first, "the first copy was overwritten");
    assert_eq!(std::fs::read(&first).unwrap(), BODY);
    assert_eq!(std::fs::read(&second.path).unwrap(), BODY);
}

#[tokio::test]
async fn rows_move_up_and_down_the_order() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let a = q.add(&format!("http://{addr}/a")).await.unwrap();
    let b = q.add(&format!("http://{addr}/b")).await.unwrap();
    let c = q.add(&format!("http://{addr}/c")).await.unwrap();
    assert!(q.shift(c, -2).await);
    let order: Vec<_> = q.list().await.iter().map(|r| r.id).collect();
    assert_eq!(order, [c, a, b]);
    assert!(!q.shift(c, -1).await, "moved past the top");
}

#[tokio::test]
async fn a_finished_file_is_renamed_where_it_lies() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/old.bin")).await.unwrap();
    assert!(
        q.rename(id, "new.bin").await.is_err(),
        "renamed before it existed"
    );
    q.run().await;
    q.rename(id, "new.bin").await.unwrap();
    let row = q.get(id).await.unwrap();
    assert_eq!(row.filename, "new.bin");
    assert_eq!(std::fs::read(&row.path).unwrap(), BODY);
    assert!(!std::path::Path::new(&row.path)
        .with_file_name("old.bin")
        .exists());
    // A name that is a path is refused rather than followed.
    q.rename(id, "../../escaped.bin").await.unwrap();
    let row = q.get(id).await.unwrap();
    assert_eq!(row.filename, "escaped.bin");
    assert!(std::path::Path::new(&row.path).starts_with(dir.path()));
}

#[tokio::test]
async fn each_connection_is_read_from_its_part_on_disk() {
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add("http://127.0.0.1:9/staged.bin").await.unwrap();
    q.pause(id).await;
    assert!(q.connections(id).await.is_empty(), "nothing is staged yet");

    let parts = dir.path().join("parts").join(id.to_string());
    std::fs::create_dir_all(&parts).unwrap();
    let plan = dm_core::plan::plan(Some(4 * 1024 * 1024), 4, true);
    dm_core::plan::record(&parts, Some(4 * 1024 * 1024), true, None, &plan).unwrap();
    std::fs::write(parts.join("part-0000"), vec![0u8; 1024 * 1024]).unwrap();
    std::fs::write(parts.join("part-0002"), vec![0u8; 10]).unwrap();

    assert_eq!(q.resumable(id).await, Some(true));
    let seen = q.connections(id).await;
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[0].done, 1024 * 1024, "a finished range is whole");
    assert_eq!(seen[1].done, 0);
    assert_eq!(seen[2].done, 10);
    assert_eq!(seen[3].end, 4 * 1024 * 1024 - 1);
}

#[tokio::test]
async fn a_login_in_the_address_is_sent_as_basic_auth() {
    /* "Needs a sign-in" and Sign-ins put a user and password on
     * the download. dm carries them in the address, so this holds that the
     * engine really sends them — on the probe and on every range. */
    async fn guarded(headers: axum::http::HeaderMap) -> Response {
        let ok = headers.get("authorization").and_then(|v| v.to_str().ok())
            == Some(basic_header("me", &common::secrets::secret("password")).as_str());
        if !ok {
            return Response::builder()
                .status(401)
                .body(Body::from("who?"))
                .unwrap();
        }
        Response::builder()
            .header("content-length", BODY.len())
            .body(Body::from(BODY))
            .unwrap()
    }
    let addr = common::serve(axum::Router::new().fallback(guarded)).await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q
        .add(&common::with_login(
            &format!("http://{addr}/private.bin"),
            "me",
            &common::secrets::secret("password"),
        ))
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, State::Completed, "{:?}", row.state);
    assert_eq!(std::fs::read(&row.path).unwrap(), BODY);
}

/// Removing a selection wrote the whole queue file once per row —
/// 10,000 rows of a 6.5 MB file at 32 ms a save. A batch is one save.
#[tokio::test]
async fn removing_many_rows_writes_the_queue_once() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    q.persist_to(dir.path().join("queue.json")).await;
    let items = (0..200)
        .map(|n| {
            (
                format!("http://{addr}/row{n}.bin"),
                AddOptions {
                    later: true,
                    ..AddOptions::default()
                },
            )
        })
        .collect();
    let ids: Vec<_> = q.add_each(items).await.into_iter().flatten().collect();
    assert_eq!(ids.len(), 200);
    let kept = ids[0];

    let before = q.saves_written();
    q.remove_many(&ids[1..]).await;
    q.flush().await;
    assert_eq!(q.saves_written() - before, 1, "one save for the batch");
    let rows = q.list().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, kept);
    let saved = std::fs::read_to_string(dir.path().join("queue.json")).unwrap();
    assert!(
        !saved.contains("row1.bin"),
        "the file still lists a removed row"
    );
}

/// Changing only the case of a name on a case-insensitive disk
/// (macOS's and Windows' default) found the name "taken" by the file itself
/// and gave "report (1).pdf".
#[tokio::test]
async fn a_rename_that_changes_only_the_case_is_done_in_place() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let id = q.add(&format!("http://{addr}/Report.pdf")).await.unwrap();
    q.run().await;
    q.rename(id, "report.pdf").await.unwrap();
    let row = q.get(id).await.unwrap();
    assert_eq!(row.filename, "report.pdf");
    assert_eq!(std::fs::read(&row.path).unwrap(), BODY);
    let names: Vec<String> = std::fs::read_dir(std::path::Path::new(&row.path).parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["report.pdf"],
        "spelt as asked, nothing left beside it"
    );
}

/// The Basic header a login is sent as, worked out here rather than written
/// out: a literal one reads to a secret scanner as a leaked credential.
fn basic_header(user: &str, password: &str) -> String {
    use base64::Engine;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
    )
}

/// A file downloaded twice at once, the second time under a
/// chosen name. That one was placed under the server's name and renamed
/// after, so for a moment it held the name and pushed the first to
/// "same (1).bin".
#[tokio::test]
async fn a_chosen_name_does_not_take_the_servers_name_from_another_row() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let url = format!("http://{addr}/same.bin");
    let first = q.add(&url).await.unwrap();
    let second = q
        .add_with(
            &url,
            AddOptions {
                save_name: Some("two.bin".into()),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let (first, second) = (q.get(first).await.unwrap(), q.get(second).await.unwrap());
    let name = |p: &str| {
        std::path::Path::new(p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    assert_eq!(name(&second.path), "two.bin", "{:?}", second.state);
    assert_eq!(name(&first.path), "same.bin", "{:?}", first.state);
}

/// A chosen name is cleaned as a server's is: "../" keeps it in its folder.
#[tokio::test]
async fn a_chosen_name_cannot_leave_its_folder() {
    let addr = serve().await;
    let dir = tempdir();
    let chosen = dir.path().join("chosen");
    let q = queue(dir.path());
    let id = q
        .add_with(
            &format!("http://{addr}/x.bin"),
            AddOptions {
                save_dir: Some(chosen.to_string_lossy().into_owned()),
                save_name: Some("../escaped.bin".into()),
                ..AddOptions::default()
            },
        )
        .await
        .unwrap();
    q.run().await;
    let row = q.get(id).await.unwrap();
    assert_eq!(row.state, State::Completed, "{:?}", row.state);
    let path = std::path::Path::new(&row.path);
    assert_eq!(path.parent().unwrap(), chosen, "{}", row.path);
    assert!(!dir.path().join("escaped.bin").exists());
}

#[tokio::test]
async fn a_speed_limit_given_when_a_row_is_added_is_the_rows_own_and_is_kept() {
    let addr = serve().await;
    let dir = tempdir();
    let q = queue(dir.path());
    let url = format!("http://{addr}/limited.bin");
    let held = q
        .add_with(
            &url,
            AddOptions {
                later: true,
                speed_limit: 300,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let free = q
        .add_with(
            &format!("http://{addr}/free.bin"),
            AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(q.get(held).await.unwrap().speed_limit, 300);
    assert_eq!(
        q.get(free).await.unwrap().speed_limit,
        0,
        "no limit unless given"
    );
}
