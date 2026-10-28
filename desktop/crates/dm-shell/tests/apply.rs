//! A settings change is in force whole or not at all, and a cookies file
//! is read as dm opens and does not hold other changes hostage once gone.

use dm_shell::settings::Settings;
use dm_shell::shell;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cookies_file_is_read_at_launch_and_its_loss_blocks_nothing() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the one test in this binary, so the process-wide home is its own
    // and nothing else is reading the environment while it is set.
    unsafe { std::env::set_var("DM_HOME", home.path()) };
    let cookies = home.path().join("cookies.txt");
    std::fs::write(
        &cookies,
        "files.example\tFALSE\t/\tFALSE\t0\tsession\tabc\n",
    )
    .unwrap();
    let saved = Settings {
        download_dir: home.path().join("out"),
        cookies_file: cookies.display().to_string(),
        ..Settings::default()
    };
    saved.save().unwrap();

    // Read as dm opens, not first at the next change of a setting.
    let started = shell::start(&shell::orders(&[])).await;
    assert_eq!(dm_core::cookies::file(), Some(cookies.clone()));
    let (queue, held) = (started.queue, started.settings);

    // Moved away since: a change of language still goes through, and the
    // missing file is said, once.
    std::fs::remove_file(&cookies).unwrap();
    let mut next = held.clone();
    next.language = "de".into();
    let problems = shell::apply(&next, Some(&held), &queue)
        .await
        .expect("a missing cookies file refused an unrelated change");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("cookies.txt"), "{problems:?}");
    let mut again = next.clone();
    again.toolbar_style = "icons".into();
    let problems = shell::apply(&again, Some(&next), &queue).await.unwrap();
    assert!(problems.is_empty(), "said again: {problems:?}");

    // A change that cannot be made changes nothing: the connections beside
    // a proxy that cannot be built stay as they were.
    let before = queue.options().await.connections;
    let mut broken = again.clone();
    broken.connections = before + 1;
    broken.proxy.enabled = true;
    broken.proxy.host = "not a host name".into();
    assert!(shell::apply(&broken, Some(&again), &queue).await.is_err());
    assert_eq!(queue.options().await.connections, before);

    // And a change that refuses itself: a relative download folder.
    let mut relative = again.clone();
    relative.connections = before + 1;
    relative.download_dir = "Downloads".into();
    assert!(shell::apply(&relative, Some(&again), &queue).await.is_err());
    assert_eq!(queue.options().await.connections, before);
}
