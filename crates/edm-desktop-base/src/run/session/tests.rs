// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What one tick asks of whoever drives it, given what the reads detected and the queue's
//! rows: in order, as the window and EDM with no window carry it out.

use super::*;
use edm_core::State;

/// A platform that does nothing.
struct Nothing;

impl edm_shell::app::backend::Platform for Nothing {
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
    fn power(&self, _: WhenDone, _: edm_shell::app::power::Report) -> Result<(), String> {
        Ok(())
    }
    fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

/// A backend over an empty queue of its own under `dir`, with `settings`.
pub(super) fn backend(
    runtime: &tokio::runtime::Runtime,
    dir: &std::path::Path,
    settings: edm_shell::Settings,
) -> Backend {
    let settings = edm_shell::Settings {
        folders: edm_shell::settings::FolderSettings {
            download_dir: dir.join("out"),
            ..settings.folders
        },
        ..settings
    };
    let queue = edm_core::DownloadList::new(
        edm_core::client(),
        settings.options(dir.join("parts"), edm_core::Limiter::unlimited()),
        settings.downloads.concurrency,
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
pub(super) fn window() -> Driver {
    Driver::window(false)
}

/// A finish and a failure: the failure told as the rows are gone through, then what
/// finished, what a screen reader hears, the finished file's notification and both sounds,
/// and the schedules looked at.
#[test]
fn a_finish_and_a_failure_are_told_in_order_with_their_sounds() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = edm_shell::Settings {
        general: edm_shell::settings::GeneralSettings {
            sound_on_complete: true,
            sound_on_failure: true,
            ..Default::default()
        },
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
    let settings = edm_shell::Settings {
        sounds: std::collections::BTreeMap::from([(
            "complete".to_string(),
            " done.wav ".to_string(),
        )]),
        general: edm_shell::settings::GeneralSettings {
            sound_on_complete: true,
            sound_on_failure: false,
            ..Default::default()
        },
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
    let settings = edm_shell::Settings {
        general: edm_shell::settings::GeneralSettings {
            show_progress: true,
            ..Default::default()
        },
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

/// A queue run dry plays its own sound and takes When done once: Exit ends EDM at once where
/// nobody sees a countdown, and counts down where a window shows it.
#[test]
fn a_queue_run_dry_plays_its_sound_and_takes_when_done() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = edm_shell::Settings {
        sounds: std::collections::BTreeMap::from([("queue_done".to_string(), "q.wav".to_string())]),
        ..Default::default()
    };
    let backend = backend(&runtime, dir.path(), settings);
    let nobody = Driver {
        nobody_sees: true,
        ..window()
    };
    let mut session = Session {
        dry: true,
        ..Session::default()
    };
    session.when_done = WhenDone::Quit;
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

/// With no window as with one, the queue run dry is noted at the read that saw it (gaps row A7):
/// a read between two ticks (a command over the control port, a hand-off) that took the change
/// left the tick's own look with nothing new, and When done never fired with no window.
#[test]
fn with_no_window_a_queue_run_dry_at_a_read_between_ticks_takes_when_done() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), Default::default());
    let mut session = Session::default();
    session.app.when_done = WhenDone::Quit;
    // The first read primes; the next, between two ticks, finds a download finished.
    session.note_dry(&[]);
    session.note_dry(&[row(1, State::Completed)]);
    // The tick's own rows show nothing new: it takes what the read noted.
    let effects = session.tick(&backend, &[], Driver::HEADLESS);
    assert_eq!(effects.last(), Some(&Effect::Quit), "{effects:?}");
    assert_eq!(session.when_done, WhenDone::Nothing, "taken");
}

/// The computer's state as a test sets it, in place of the computer's own calls (gaps row W11).
#[derive(Clone, Default)]
struct Fake(Arc<std::sync::Mutex<crate::metered::Reading>>);

impl crate::metered::Source for Fake {
    fn read(&self) -> crate::metered::Reading {
        *self.0.lock().unwrap()
    }
}

impl Fake {
    /// The state from now on, read at the session's next tick.
    fn set(&self, session: &mut Session, metered: Option<bool>, battery: Option<(bool, u8)>) {
        *self.0.lock().unwrap() = crate::metered::Reading {
            metered,
            battery: battery.map(|(discharging, percent)| crate::metered::Battery {
                discharging,
                percent: Some(percent),
            }),
        };
        session.read_state_from(Box::new(self.clone()));
    }
}

/// Settings whose Downloads page says `on_metered` and `battery_below`.
fn waiting(on_metered: &str, battery_below: u32) -> edm_shell::Settings {
    edm_shell::Settings {
        downloads: edm_shell::settings::DownloadSettings {
            on_metered: on_metered.into(),
            battery_below,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// On a metered connection with Options' "Wait", every download is held, its reason said on
/// the row; once the connection is not metered they go on. Nothing is asked.
#[test]
fn a_metered_connection_holds_the_downloads_until_it_ends() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), waiting("wait", 0));
    let (mut session, fake) = (Session::default(), Fake::default());
    fake.set(&mut session, Some(true), None);
    let effects = session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), Some(edm_core::Hold::Metered));
    assert!(!effects.contains(&Effect::AskMetered));
    crate::rows::set_held_by(backend.held_by());
    let mut held = (*row(1, State::Paused)).clone();
    held.paused_by_schedule = true;
    assert_eq!(
        crate::rows::status_in_english(&held),
        "Waiting: the connection is metered"
    );
    crate::rows::set_held_by(None);
    fake.set(&mut session, Some(false), None);
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), None);
}

/// "Ask": the window asks once while the connection stays metered, the downloads waiting
/// meanwhile; Download anyway lets them go on; the next metered period is asked again. With no
/// window nobody is asked, and they wait.
#[test]
fn ask_asks_once_a_metered_period_and_waits_with_no_window() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let headless = backend(&runtime, dir.path(), waiting("ask", 0));
    let backend = backend(&runtime, dir.path(), waiting("ask", 0));
    let (mut session, fake) = (Session::default(), Fake::default());
    fake.set(&mut session, Some(true), None);
    let asked = |effects: Vec<Effect>| effects.contains(&Effect::AskMetered);
    assert!(asked(session.tick(&backend, &[], window())));
    assert_eq!(backend.held_by(), Some(edm_core::Hold::Metered));
    assert!(!asked(session.tick(&backend, &[], window())), "asked twice");
    session.answer_metered(true);
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), None, "Download anyway");
    // Off the metered connection and back on it: asked again.
    fake.set(&mut session, Some(false), None);
    session.tick(&backend, &[], window());
    fake.set(&mut session, Some(true), None);
    assert!(asked(session.tick(&backend, &[], window())));
    session.answer_metered(false);
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), Some(edm_core::Hold::Metered), "Wait");

    let mut alone = Session::default();
    fake.set(&mut alone, Some(true), None);
    assert!(!asked(alone.tick(&headless, &[], Driver::HEADLESS)));
    assert_eq!(headless.held_by(), Some(edm_core::Hold::Metered));
}

/// On its battery below Options' level the downloads wait; charging, they go on. A state the
/// system does not say holds nothing.
#[test]
fn a_low_battery_holds_and_an_unknown_state_goes_on() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), waiting("wait", 30));
    let (mut session, fake) = (Session::default(), Fake::default());
    fake.set(&mut session, Some(false), Some((true, 12)));
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), Some(edm_core::Hold::Battery));
    fake.set(&mut session, Some(false), Some((false, 12)));
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), None, "charging");
    fake.set(&mut session, None, None);
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), None, "unknown");
}

/// A phone's state as a test sets it (gaps row N10): its own rules, read beside W11's.
#[derive(Clone, Default)]
struct PhoneFake(Arc<std::sync::Mutex<crate::phone::PhoneNetwork>>);

impl crate::metered::Source for PhoneFake {
    fn read(&self) -> crate::metered::Reading {
        crate::metered::phone::reading_of(&self.0.lock().unwrap())
    }

    fn phone(&self) -> crate::phone::PhoneNetwork {
        *self.0.lock().unwrap()
    }
}

/// On a phone with Options' Wi-Fi only, mobile data holds every download, its reason said on
/// the row; on Wi-Fi again they go on.
#[test]
fn wifi_only_holds_a_phones_downloads_off_wifi() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let settings = edm_shell::Settings {
        downloads: edm_shell::settings::DownloadSettings {
            wifi_only: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let backend = backend(&runtime, dir.path(), settings);
    let (mut session, fake) = (Session::default(), PhoneFake::default());
    *fake.0.lock().unwrap() = crate::phone::PhoneNetwork {
        metered: Some(true),
        wifi: Some(false),
        ..Default::default()
    };
    session.read_state_from(Box::new(fake.clone()));
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), Some(edm_core::Hold::NotWifi));
    crate::rows::set_held_by(backend.held_by());
    let mut held = (*row(1, State::Paused)).clone();
    held.paused_by_schedule = true;
    assert_eq!(
        crate::rows::status_in_english(&held),
        "Waiting: the phone is not on Wi-Fi"
    );
    crate::rows::set_held_by(None);
    *fake.0.lock().unwrap() = crate::phone::PhoneNetwork {
        metered: Some(false),
        wifi: Some(true),
        ..Default::default()
    };
    session.read_state_from(Box::new(fake.clone()));
    session.tick(&backend, &[], window());
    assert_eq!(backend.held_by(), None);
}

/// The browser popup's asks (gaps row C12): Open and Open folder are the effects every driver
/// carries out; a verb is done here and asks nothing; one that does not apply, or whose row is
/// gone, is nothing.
#[test]
fn the_popups_asks_are_the_windows_own_commands() {
    use edm_protocol::{act_does, Act};
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&runtime, dir.path(), edm_shell::Settings::default());
    let file = dir.path().join("f1.bin");
    std::fs::write(&file, b"x").unwrap();
    let path = file.to_string_lossy().into_owned();
    let mut done = Entry::clone(&row(1, State::Completed));
    done.path = path.clone();
    let rows = vec![
        Arc::new(done),
        row(2, State::Paused),
        row(3, State::Running),
    ];
    let act = |does: &str, id: Option<u64>| Act {
        does: does.into(),
        id,
    };
    let carried = |does: &str, id: Option<u64>| asked::carry_asked(&backend, &rows, &act(does, id));
    assert_eq!(
        carried(act_does::OPEN, Some(1)),
        Some(Effect::Open(path.clone()))
    );
    assert_eq!(
        carried(act_does::FOLDER, Some(1)),
        Some(Effect::Reveal(path))
    );
    assert_eq!(carried(act_does::RESUME, Some(2)), None);
    assert_eq!(carried(act_does::STOP, Some(3)), None);
    assert_eq!(carried(act_does::CLEAR_FINISHED, None), None);
    // Not where the window would grey it out, nor for a row gone.
    assert_eq!(carried(act_does::OPEN, Some(2)), None);
    assert_eq!(carried(act_does::OPEN, Some(9)), None);
}
