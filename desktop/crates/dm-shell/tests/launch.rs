//! What dm does as it opens: the Main queue starts when the setting says
//! so, with nobody clicking "Start queue".

use dm_core::{AddOptions, Queue, State};
use dm_shell::settings::{self, Settings};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_main_queue_starts_at_launch_when_the_setting_says_so() {
    let home = tempfile::tempdir().unwrap();
    // The one test in this binary, so the process-wide home is its own.
    unsafe { std::env::set_var("DM_HOME", home.path()) };
    let saved = Settings {
        download_dir: home.path().join("out"),
        start_queue_on_launch: true,
        ..Settings::default()
    };
    saved.save().unwrap();

    // A row added for later, as the last run left it.
    let queue = Queue::new(
        dm_core::client(),
        saved.options(home.path().join("parts"), dm_core::Limiter::unlimited()),
        1,
    );
    queue.persist_to(settings::home().join("queue.json")).await;
    let id = queue
        .add_with(
            "http://127.0.0.1:9/later.bin",
            AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(queue.get(id).await.unwrap().state, State::Paused);
    drop(queue);

    let started = dm_shell::shell::start(&dm_shell::shell::orders(&[])).await;
    let row = started
        .queue
        .get(id)
        .await
        .expect("the saved row was not read back");
    assert_ne!(
        row.state,
        State::Paused,
        "the Main queue was not started as dm opened"
    );
    assert!(started.settings.start_queue_on_launch);
}
