// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the app does around its window that other tests leave out: the
//! inbox watcher taking every kind of hand-off (and one it cannot remove),
//! links on the command line started with the queue, a change that needs a
//! new client, the queue's own clock starting a queue on time, and the
//! programs run after a download. dm's home is a temporary folder
//! (`DM_HOME`) for the whole binary; nothing opens a window or a file
//! manager.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dm_core::{Queue, State};
use dm_shell::host::{self, Request};
use dm_shell::settings::Settings;
use dm_shell::shell;

/// The environment and dm's home are the process's: tests take turns.
static ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// dm's home for this binary, made and named in `DM_HOME` once.
fn home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: under `ENV` (every test takes it before calling this), so
        // nothing else in this binary reads the environment meanwhile.
        unsafe {
            std::env::set_var("DM_HOME", home.path());
            // A throw-away home leaves nothing in the person's own store.
            std::env::set_var("DM_NO_SECRET_STORE", "1");
        }
        // Downloads, if any were made, land here too.
        let saved = Settings {
            download_dir: home.path().join("out"),
            ..Settings::default()
        };
        std::fs::write(
            home.path().join("settings.json"),
            serde_json::to_string(&saved).unwrap(),
        )
        .unwrap();
        home
    })
    .path()
}

fn queue(dir: &Path) -> Queue {
    let saved = Settings {
        download_dir: dir.join("out"),
        ..Settings::default()
    };
    let mut options = saved.options(dir.join("parts"), dm_core::Limiter::unlimited());
    options.attempts = 1;
    Queue::new(dm_core::client(), options, 1)
}

async fn until(what: &str, mut done: impl AsyncFnMut() -> bool) {
    let by = Instant::now() + Duration::from_secs(15);
    while !done().await {
        assert!(Instant::now() < by, "never: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn file(dir: &Path, name: &str, request: &Request) {
    std::fs::write(dir.join(name), serde_json::to_string(request).unwrap()).unwrap();
}

fn later(url: &str) -> Request {
    Request {
        url: url.into(),
        later: true,
        ..Request::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_inbox_watcher_takes_each_kind_of_hand_off_and_keeps_what_it_cannot_read() {
    let _turn = ENV.lock().await;
    let inbox = host::inbox(home());
    std::fs::create_dir_all(&inbox).unwrap();
    // A bell that cannot be put up: the inbox is polled as before.
    std::fs::create_dir_all(inbox.join(host::BELL).join("taken")).unwrap();
    for flag in [shell::QUIT, "show", "start-queue"] {
        std::fs::write(inbox.join(flag), b"").unwrap();
    }
    file(&inbox, "a.json", &later("http://127.0.0.1:9/a.bin"));
    file(
        &inbox,
        "b.json",
        &Request {
            ask: true,
            ..later("http://127.0.0.1:9/b.bin")
        },
    );
    file(
        &inbox,
        "c.json",
        &Request {
            from_browser: true,
            ..later("http://127.0.0.1:9/c.bin")
        },
    );
    std::fs::write(inbox.join("d.json"), b"{ not a request").unwrap();
    std::fs::create_dir_all(inbox.join("e.json")).unwrap();
    std::fs::write(inbox.join("note.txt"), b"not for the watcher").unwrap();

    let dir = tempfile::tempdir().unwrap();
    let queue = queue(dir.path());
    let (shown, show_rx) = std::sync::mpsc::channel::<()>();
    let (asked, ask_rx) = std::sync::mpsc::channel::<Request>();
    let (quitting, quit_rx) = std::sync::mpsc::channel::<()>();
    let watcher = tokio::spawn(shell::watch_inbox(
        queue.clone(),
        move || {
            let _ = shown.send(());
        },
        move |request| {
            let _ = asked.send(request);
        },
        move || {
            let _ = quitting.send(());
        },
    ));

    quit_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("asked to quit");
    show_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("asked to show itself");
    until("the two downloads were queued", async || {
        queue.list().await.len() == 2
    })
    .await;
    until("the unreadable one was kept aside", async || {
        inbox.join("d.unreadable").exists()
    })
    .await;
    let first = ask_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first.url, "http://127.0.0.1:9/b.bin");
    let mut queued: Vec<String> = queue.list().await.into_iter().map(|e| e.url).collect();
    queued.sort();
    assert_eq!(
        queued,
        ["http://127.0.0.1:9/a.bin", "http://127.0.0.1:9/c.bin"]
    );
    // Added for later, not started by the queue that was.
    assert!(queue.list().await.iter().all(|e| e.state == State::Paused));
    for gone in ["a.json", "b.json", "c.json", "d.json", "start-queue"] {
        assert!(!inbox.join(gone).exists(), "{gone} was left");
    }
    assert!(inbox.join("e.json").is_dir(), "what cannot be read is left");
    assert!(inbox.join("note.txt").exists());
    assert_eq!(
        std::fs::read_to_string(inbox.join("d.unreadable")).unwrap(),
        "{ not a request"
    );

    // While the window waits for a fresh link, every hand-off goes to it.
    shell::route_to_window(true);
    file(&inbox, "f.json", &later("http://127.0.0.1:9/f.bin"));
    file(
        &inbox,
        "g.json",
        &Request {
            from_browser: true,
            ..later("http://127.0.0.1:9/g.bin")
        },
    );
    let mut routed: Vec<String> = (0..2)
        .map(|_| ask_rx.recv_timeout(Duration::from_secs(10)).unwrap().url)
        .collect();
    shell::route_to_window(false);
    routed.sort();
    assert_eq!(
        routed,
        ["http://127.0.0.1:9/f.bin", "http://127.0.0.1:9/g.bin"]
    );
    assert_eq!(queue.list().await.len(), 2, "a routed hand-off was queued");
    watcher.abort();
    let _ = watcher.await;

    // A hand-off that cannot be removed is left alone rather than taken
    // again twice a second.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file(&inbox, "h.json", &later("http://127.0.0.1:9/h.bin"));
        std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o555)).unwrap();
        // As root no folder is locked: nothing to see.
        let locked = std::fs::write(inbox.join(".probe"), b"").is_err();
        if locked {
            let watcher = tokio::spawn(shell::watch_inbox(queue.clone(), || {}, |_| {}, || {}));
            tokio::time::sleep(Duration::from_millis(1500)).await;
            watcher.abort();
            let _ = watcher.await;
        }
        std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o755)).unwrap();
        if locked {
            assert!(inbox.join("h.json").exists());
            assert_eq!(queue.list().await.len(), 2, "taken though it stayed");
        }
        let _ = std::fs::remove_file(inbox.join("h.json"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn links_on_the_command_line_are_added_as_the_engine_starts() {
    let _turn = ENV.lock().await;
    let _ = home();
    let orders = shell::orders(&[
        "--later".into(),
        "http://127.0.0.1:9/one.bin".into(),
        "--name".into(),
        "two.bin".into(),
        "--later".into(),
        "http://127.0.0.1:9/two".into(),
    ]);
    let started = shell::start(&orders).await;
    let mut rows: Vec<(String, State)> = started
        .queue
        .list()
        .await
        .into_iter()
        .filter(|e| e.url.starts_with("http://127.0.0.1:9/"))
        .map(|e| (e.url, e.state))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        rows,
        [
            ("http://127.0.0.1:9/one.bin".to_string(), State::Paused),
            ("http://127.0.0.1:9/two".to_string(), State::Paused),
        ]
    );
    // What it was started with is read from dm's home.
    assert_eq!(started.settings.download_dir, home().join("out"));
}

/// A paste of several links where the queue turns some down: the rest are
/// added, and what was not is said, counted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paste_with_words_that_are_no_links_adds_the_links_and_counts_the_rest() {
    let _turn = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let queue = queue(dir.path());
    let added = shell::add(&queue, "notalink http://127.0.0.1:9/one.bin")
        .await
        .unwrap();
    assert_eq!(added.ids.len(), 1);
    assert_eq!(
        added.skipped,
        ["one of the links is not one dm can download"]
    );
    let added = shell::add(&queue, "two words http://127.0.0.1:9/two.bin")
        .await
        .unwrap();
    assert_eq!(
        added.skipped,
        ["2 of the links are not ones dm can download"]
    );
    // Nothing usable at all: refused, with why.
    assert_eq!(
        shell::add(&queue, "nothing").await.unwrap_err(),
        "one of the links is not one dm can download"
    );
    assert_eq!(queue.list().await.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_of_the_read_timeout_puts_a_new_client_in_force() {
    let _turn = ENV.lock().await;
    let _ = home();
    let dir = tempfile::tempdir().unwrap();
    let queue = queue(dir.path());
    let held = Settings {
        download_dir: dir.path().join("out"),
        ..Settings::default()
    };
    let next = Settings {
        read_timeout: held.read_timeout + 30,
        concurrency: 3,
        ..held.clone()
    };
    let problems = shell::apply(&next, Some(&held), &queue).await.unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(queue.options().await.parts_dir, next.parts_dir());
}

/// The queue's clock: a queue set to start at this minute starts on its
/// own, and the recheck queue's timer runs when it is due.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queue_set_to_start_now_is_started_by_the_queues_clock() {
    let _turn = ENV.lock().await;
    let app = axum::Router::new().fallback(|| async { vec![5u8; 2048] });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });

    let dir = tempfile::tempdir().unwrap();
    let queue = queue(dir.path());
    let id = queue
        .add_with(
            &format!("http://{addr}/on-time.bin"),
            dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(queue.get(id).await.unwrap().state, State::Paused);
    let (_, minute) = dm_core::now_local();
    queue.set_queue_starts(vec![(dm_core::MAIN_QUEUE.to_string(), minute)]);
    queue.set_recheck_every(Some(Duration::from_millis(1)));
    shell::watch_rules(&queue);
    until(
        "the queue started on time and finished its download",
        async || queue.get(id).await.unwrap().state == State::Completed,
    )
    .await;
    let saved = PathBuf::from(queue.get(id).await.unwrap().path);
    assert_eq!(std::fs::read(saved).unwrap(), vec![5u8; 2048]);
}

#[tokio::test]
async fn odds_and_ends_of_the_command_line_and_the_hand_off() {
    let _turn = ENV.lock().await;
    // A folder that is no folder dm can use: the link is still taken.
    let orders = shell::orders(&[
        "--save-to".into(),
        "~someone".into(),
        "https://e.example/f.zip".into(),
    ]);
    assert_eq!(orders.downloads.len(), 1);
    assert_eq!(orders.downloads[0].save_dir, None);

    // Later: into the Main queue.
    let options = shell::add_options(&later("https://e.example/f.zip"));
    assert_eq!(options.queue, dm_core::MAIN_QUEUE);
    assert!(options.later);

    // A form's own type is kept.
    let posted = shell::posted(
        "https://e.example/get ",
        Some(&dm_protocol::Posted {
            form: "a=1".into(),
            content_type: "text/plain".into(),
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(posted.content_type, "text/plain");
    assert_eq!(posted.body, b"a=1");
    assert_eq!(posted.url, "https://e.example/get");

    // A link that does not parse is refused in the parser's words.
    let why = shell::unusable("http://exa mple.com/f.zip").unwrap();
    assert!(why.contains("is not a link dm can read"), "{why}");

    // No origin to check: known. The built-in extension: known.
    assert!(shell::launcher_known(&["--native-host".into()]));
    assert!(shell::launcher_known(&[format!(
        "chrome-extension://{}/",
        dm_protocol::CHROME_EXTENSION_ID
    )]));

    // This process's own arguments, less its name.
    let ours: Vec<String> = std::env::args().skip(1).collect();
    assert_eq!(shell::args(), ours);

    // Handing to a running dm whose home cannot be written to: said, not a
    // crash, and nothing made.
    let dir = tempfile::tempdir().unwrap();
    let not_a_folder = dir.path().join("a file");
    std::fs::write(&not_a_folder, b"").unwrap();
    shell::hand_to_running(
        &not_a_folder,
        &shell::orders(&["https://e.example/f.zip".into()]),
    );
    assert!(not_a_folder.is_file());
}

#[tokio::test]
async fn a_file_that_is_gone_is_said_before_anything_is_opened() {
    let _turn = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone.bin");
    let gone = gone.to_str().unwrap();
    let said = "that file is no longer where dm saved it";
    assert_eq!(shell::open_file(gone), Err(said.into()));
    assert_eq!(shell::reveal_in_folder(gone), Err(said.into()));
    assert_eq!(shell::open_with(gone, Path::new("/")), Err(said.into()));
    // The file is there, the program is not.
    let there = dir.path().join("there.bin");
    std::fs::write(&there, b"").unwrap();
    let app = dir.path().join("No Such.app");
    assert_eq!(
        shell::open_with(there.to_str().unwrap(), &app),
        Err(format!("{} is not there", app.display()))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_program_run_after_a_download_is_given_the_file_last() {
    let _turn = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let done = dir.path().join("done.bin");
    std::fs::write(&done, b"").unwrap();
    let said = dir.path().join("said");
    let script = dir.path().join("after.sh");
    std::fs::write(
        &script,
        format!("printf '%s|' \"$@\" > '{}'\n", said.display()),
    )
    .unwrap();
    let command = format!("/bin/sh '{}' --first \"two words\"", script.display());
    assert_eq!(shell::run_after(&command, &done), Ok(()));
    until("the program ran", async || said.exists()).await;
    until("the program said what it was given", async || {
        std::fs::read_to_string(&said).is_ok_and(|s| s.ends_with("done.bin|"))
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(&said).unwrap(),
        format!("--first|two words|{}|", done.display())
    );
    // Nothing to run: nothing done.
    assert_eq!(shell::run_after("   ", &done), Ok(()));
    // A program that is not there: said, with the file it was for.
    let why = shell::run_after("/no/such/program --x", &done).unwrap_err();
    assert!(
        why.starts_with("\"/no/such/program\", to run after"),
        "{why}"
    );
    assert!(why.contains("could not start"), "{why}");
}
