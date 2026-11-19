// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What one tick asks of whoever drives it, given what the reads detected and the queue's
//! rows: in order, as the window and dm with no window carry it out.

use super::*;
use dm_core::State;

/// A platform that does nothing.
struct Nothing;

impl dm_shell::app::backend::Platform for Nothing {
    fn set_login_item(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
    fn reveal(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn open(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn chime(&self) {}
    fn power(&self, _: WhenDone, _: dm_shell::app::power::Report) -> Result<(), String> {
        Ok(())
    }
    fn choose_app(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
    fn choose_file(&self, _: bool, _: &str) -> Option<std::path::PathBuf> {
        None
    }
}

/// A backend over an empty queue of its own under `dir`, with `settings`.
fn backend(
    runtime: &tokio::runtime::Runtime,
    dir: &std::path::Path,
    settings: dm_shell::Settings,
) -> Backend {
    let settings = dm_shell::Settings {
        download_dir: dir.join("out"),
        ..settings
    };
    let queue = dm_core::DownloadList::new(
        dm_core::client(),
        settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
        settings.concurrency,
    );
    Backend::new(
        runtime.handle().clone(),
        queue,
        settings,
        Vec::new(),
        Arc::new(Nothing),
    )
}

/// Download `id` in `state`, its file `f<id>.bin`.
fn row(id: Id, state: State) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id, "url": format!("https://e.com/f{id}.bin"),
            "state": serde_json::to_value(&state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": format!("f{id}.bin"),
            "path": format!("/downloads/f{id}.bin"),
        }))
        .unwrap(),
    )
}

/// The window's driver, in a test that does not hold the inbox.
fn window() -> Driver {
    Driver::window(false)
}

/// A finish and a failure: the failure told as the rows are gone through, then what
/// finished, what a screen reader hears, the finished file's notification and both sounds,
/// and the schedules looked at.
#[test]
fn a_finish_and_a_failure_are_told_in_order_with_their_sounds() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = dm_shell::Settings {
        sound_on_complete: true,
        sound_on_failure: true,
        ..Default::default()
    };
    let backend = backend(&runtime, dir.path(), settings);
    let rows = vec![
        row(1, State::Completed),
        row(2, State::Failed("boom".into())),
    ];
    let mut session = Session::default();
    session.note_read(Detected {
        finished: vec![1],
        failed: vec![2],
        expired: vec![2],
        ..Default::default()
    });
    let (done, failed) = (name_of(&rows[0]), name_of(&rows[1]));
    let why = crate::rows::status_of(&rows[1]);
    assert_eq!(
        session.tick(&backend, &rows, window()),
        vec![
            Effect::Telling { expired: Some(2) },
            Effect::Failed {
                id: 2,
                name: failed.clone(),
                why: why.clone(),
            },
            Effect::Finished(vec![1]),
            Effect::Told {
                finished: vec![done.clone()],
                failures: vec![(failed, why)],
            },
            Effect::NotifyFile {
                title: "Download finished",
                name: done,
                path: "/downloads/f1.bin".into(),
            },
            Effect::Play(Sound::Chime),
            Effect::Play(Sound::Alert),
            Effect::Scheduled { hang_up: false },
        ]
    );
    // Told once: the next tick has nothing to tell.
    assert_eq!(
        session.tick(&backend, &rows, window()),
        vec![
            Effect::Telling { expired: None },
            Effect::Told {
                finished: vec![],
                failures: vec![],
            },
            Effect::Scheduled { hang_up: false },
        ]
    );
}

/// A batch that finished is one notification, counted; a sound file chosen plays instead of
/// the system's; a sound turned off plays nothing.
#[test]
fn a_batch_is_one_notification_and_a_chosen_sound_is_its_file() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = dm_shell::Settings {
        sound_on_complete: true,
        sound_on_failure: false,
        sounds: std::collections::BTreeMap::from([(
            "complete".to_string(),
            " done.wav ".to_string(),
        )]),
        ..Default::default()
    };
    let backend = backend(&runtime, dir.path(), settings);
    let rows = vec![
        row(1, State::Completed),
        row(2, State::Completed),
        row(3, State::Failed("x".into())),
    ];
    let mut session = Session::default();
    session.note_read(Detected {
        finished: vec![1],
        ..Default::default()
    });
    session.note_read(Detected {
        finished: vec![2],
        failed: vec![3],
        ..Default::default()
    });
    let effects = session.tick(&backend, &rows, window());
    assert!(
        effects.contains(&Effect::Finished(vec![1, 2])),
        "{effects:?}"
    );
    assert!(effects.contains(&Effect::Notify {
        title: "Download finished",
        body: "2 downloads finished.".into(),
    }));
    let sounds: Vec<&Effect> = effects
        .iter()
        .filter(|e| matches!(e, Effect::Play(_)))
        .collect();
    assert_eq!(sounds, vec![&Effect::Play(Sound::File("done.wav".into()))]);
}

/// The one download a read found started elsewhere asks for its progress window where
/// Options asks for one, not for a batch; an address added again is named.
#[test]
fn a_download_started_elsewhere_and_an_address_added_again_are_asked_for() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = dm_shell::Settings {
        show_progress: true,
        ..Default::default()
    };
    let backend = backend(&runtime, dir.path(), settings);
    let rows = vec![
        row(1, State::Running),
        row(2, State::Queued),
        row(3, State::Queued),
    ];
    let mut session = Session::default();
    session.note_read(Detected {
        started_elsewhere: vec![1],
        added_again: Some(3),
        ..Default::default()
    });
    session.note_read(Detected {
        started_elsewhere: vec![2, 3],
        ..Default::default()
    });
    let effects = session.tick(&backend, &rows, window());
    assert_eq!(
        effects[..3],
        [
            Effect::Telling { expired: None },
            Effect::StartedElsewhere(1),
            Effect::AddedAgain {
                name: name_of(&rows[2]),
            },
        ]
    );
    assert!(
        !effects.contains(&Effect::StartedElsewhere(2)),
        "{effects:?}"
    );
}

/// A queue run dry plays its own sound and takes When done once: Exit ends dm at once where
/// nobody sees a countdown, and counts down where a window shows it.
#[test]
fn a_queue_run_dry_plays_its_sound_and_takes_when_done() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = dm_shell::Settings {
        sounds: std::collections::BTreeMap::from([("queue_done".to_string(), "q.wav".to_string())]),
        ..Default::default()
    };
    let backend = backend(&runtime, dir.path(), settings);
    let nobody = Driver {
        nobody_sees: true,
        ..window()
    };
    let mut session = Session::default();
    session.when_done = WhenDone::Quit;
    session.dry = true;
    let effects = session.tick(&backend, &[], nobody);
    assert_eq!(
        effects[effects.len() - 2..],
        [Effect::Play(Sound::File("q.wav".into())), Effect::Quit]
    );
    assert_eq!(session.when_done, WhenDone::Nothing, "taken");
    assert!(
        !session.tick(&backend, &[], nobody).contains(&Effect::Quit),
        "once"
    );

    session.when_done = WhenDone::Quit;
    session.dry = true;
    let effects = session.tick(&backend, &[], window());
    assert!(!effects.contains(&Effect::Quit), "{effects:?}");
    assert!(session
        .countdown
        .is_some_and(|(action, _)| action == WhenDone::Quit));
}

/// A download's own "When this download is done": its file opened, or When done's
/// countdown for what it chose.
#[test]
fn a_downloads_own_choice_opens_its_file_or_counts_down() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), Default::default());
    let rows = vec![row(1, State::Completed), row(2, State::Completed)];
    let mut session = Session::default();
    session.this_done.insert(1, ThisDone::Open);
    session.this_done.insert(2, ThisDone::Then(WhenDone::Sleep));
    let effects = session.tick(&backend, &rows, window());
    assert!(
        effects.contains(&Effect::Open("/downloads/f1.bin".into())),
        "{effects:?}"
    );
    assert!(session
        .countdown
        .is_some_and(|(action, _)| action == WhenDone::Sleep));
    assert!(session.this_done.is_empty(), "taken once");
}

/// A driver that does not read the inbox leaves its orders there.
#[test]
fn a_driver_that_does_not_read_the_inbox_takes_no_order() {
    assert!(Session::default().take_orders(window()).is_empty());
}

/// Only a notification, a sound and a file opened are carried out on the platform; the rest
/// is the driver's own.
#[test]
fn only_what_is_the_same_for_every_driver_is_carried_on_the_platform() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), Default::default());
    for shared in [
        Effect::Notify {
            title: "t",
            body: "b".into(),
        },
        Effect::Play(Sound::Chime),
        Effect::Open("/x".into()),
        Effect::Reveal("/x".into()),
    ] {
        assert_eq!(shared.on_platform(&backend), Some(Ok(())), "{shared:?}");
    }
    for own in [
        Effect::Quit,
        Effect::NewDownload,
        Effect::CannotWhenDone,
        Effect::Problem("p".into()),
        Effect::Scheduled { hang_up: true },
    ] {
        assert_eq!(own.on_platform(&backend), None, "{own:?}");
    }
}
