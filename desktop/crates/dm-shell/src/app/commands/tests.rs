// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// What the window hands off its thread is what `verb` carries out:
/// a command left out would still be waited for, one put in by
/// mistake would answer nothing.
#[test]
fn is_verb_names_exactly_what_verb_carries_out() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, dir.path());
    for command in Command::ALL {
        assert_eq!(
            verb(*command, &backend, &[], dm_core::MAIN_QUEUE).is_some(),
            is_verb(*command),
            "{command:?}"
        );
    }
}

#[test]
fn every_command_has_its_own_name_and_label() {
    let mut ids: Vec<_> = Command::ALL.iter().map(|c| c.id()).collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "two commands share a name");
    for c in Command::ALL {
        assert_eq!(Command::from_id(c.id()), Some(*c));
        assert!(!c.label().is_empty());
    }
    assert_eq!(Command::from_id("no.such.command"), None);
}

/// The keys that act on the list's selection are kept from it behind a
/// dialog; the others, which open something, are not.
#[test]
fn only_the_selections_keys_act_on_the_list() {
    for command in [
        Command::Remove,
        Command::SelectAll,
        Command::Properties,
        Command::DownloadNow,
        Command::StopDownload,
        Command::FindNext,
        Command::FindPrevious,
    ] {
        assert!(command.acts_on_the_list(), "{command:?}");
    }
    for command in [
        Command::AddUrl,
        Command::Options,
        Command::Find,
        Command::Quit,
    ] {
        assert!(!command.acts_on_the_list(), "{command:?}");
    }
}

#[test]
fn no_two_commands_share_a_key() {
    let chords: Vec<_> = Command::ALL.iter().flat_map(|c| c.chords()).collect();
    assert!(chords.len() >= 12, "only {} keys", chords.len());
    for (i, a) in chords.iter().enumerate() {
        assert!(!chords[i + 1..].contains(a), "{a:?} runs two commands");
    }
}

#[test]
fn the_numbered_series_holds_more_than_add_url() {
    let series = Command::AddSeries.chords()[0];
    let add = Command::AddUrl.chords()[0];
    assert_eq!((series.key, add.key), (Key::N, Key::N));
    assert!(series.held() > add.held());
}

#[test]
fn the_presets_hold_their_speeds_and_only_they_do() {
    assert_eq!(Command::SpeedLow.speed_preset(), Some(SPEED_LOW));
    assert_eq!(Command::SpeedUnlimited.speed_preset(), Some(0));
    let presets = Command::ALL
        .iter()
        .filter(|c| c.speed_preset().is_some())
        .count();
    assert_eq!(presets, 4);
}

struct Seen {
    retrying: bool,
    gone: bool,
    plays: bool,
}

impl Looks for Seen {
    fn retrying(&self, _: Id) -> bool {
        self.retrying
    }
    fn gone(&self, _: &Entry) -> bool {
        self.gone
    }
    fn plays(&self, _: &Entry) -> bool {
        self.plays
    }
}

const PLAIN: Seen = Seen {
    retrying: false,
    gone: false,
    plays: false,
};

fn row(id: u64, state: State, path: &str, downloaded: u64) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": format!("https://example.com/{id}"),
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": downloaded, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
            "path": path,
        }))
        .unwrap(),
    )
}

#[test]
fn row_commands_follow_the_chosen_rows_states() {
    let paused = row(1, State::Paused, "", 10);
    let running = row(2, State::Running, "", 10);
    let done = row(3, State::Completed, "/tmp/a.zip", 10);
    let rows = vec![paused.clone(), running.clone(), done.clone()];
    let on = |c, chosen: &[&Entry]| applies(c, chosen, &rows, false, &PLAIN);
    assert!(on(Command::DownloadNow, &[&paused]));
    assert!(!on(Command::DownloadNow, &[&running]));
    assert!(on(Command::StopDownload, &[&running]));
    // Nothing is removed from under its own writer.
    assert!(!on(Command::Remove, &[&paused, &running]));
    assert!(on(Command::Remove, &[&paused]));
    assert!(!on(Command::Remove, &[]));
    assert!(on(Command::DeleteFile, &[&done]));
    assert!(!on(Command::DeleteFile, &[&paused]));
    // At the top or the bottom, a move would do nothing.
    assert!(!on(Command::MoveUp, &[&paused]));
    assert!(on(Command::MoveDown, &[&paused]));
    assert!(!on(Command::MoveDown, &[&done]));
    assert!(on(Command::ExtractHere, &[&done]));
    assert!(on(Command::ResumeAll, &[]));
    assert!(on(Command::StopAll, &[]));
    assert!(on(Command::AddUrl, &[]));
}

#[test]
fn what_the_presentation_sees_decides_the_rest() {
    let done = row(3, State::Completed, "/tmp/a.zip", 10);
    let partial = row(4, State::Paused, "", 10);
    let rows = vec![done.clone(), partial.clone()];
    let gone = Seen {
        gone: true,
        ..PLAIN
    };
    assert!(!applies(Command::OpenFile, &[&done], &rows, false, &gone));
    assert!(!applies(Command::ExtractTo, &[&done], &rows, false, &gone));
    // Open folder stays: it says the file is gone.
    assert!(applies(Command::OpenFolder, &[&done], &rows, false, &gone));
    let plays = Seen {
        plays: true,
        ..PLAIN
    };
    assert!(applies(Command::Preview, &[&partial], &rows, false, &plays));
    assert!(!applies(
        Command::Preview,
        &[&partial],
        &rows,
        false,
        &PLAIN
    ));
    let running = row(5, State::Running, "", 1);
    let retrying = Seen {
        retrying: true,
        ..PLAIN
    };
    assert!(!applies(
        Command::UpdateLink,
        &[&running],
        &rows,
        false,
        &PLAIN
    ));
    assert!(applies(
        Command::UpdateLink,
        &[&running],
        &rows,
        false,
        &retrying
    ));
    assert!(!applies(Command::FindNext, &[], &rows, false, &PLAIN));
    assert!(applies(Command::FindNext, &[], &rows, true, &PLAIN));
}

#[test]
fn the_speed_commands_ask_for_their_limit_and_keep_the_last() {
    let mut last = 0;
    let off = Settings {
        speed_limit: 0,
        ..Settings::default()
    };
    // Back on with nothing kept: at least 1 KB/s.
    let on = limited(Command::SpeedLimiterToggle, &off, &mut last).unwrap();
    assert_eq!(on.speed_limit, 1);
    let preset = limited(Command::SpeedMedium, &on, &mut last).unwrap();
    assert_eq!(
        (preset.speed_limit, preset.last_speed_limit, last),
        (SPEED_MEDIUM, 1, 1)
    );
    // Off keeps the limit it turned off, and on brings it back.
    let off_again = limited(Command::SpeedLimiterToggle, &preset, &mut last).unwrap();
    assert_eq!((off_again.speed_limit, last), (0, SPEED_MEDIUM));
    let back = limited(Command::SpeedLimiterToggle, &off_again, &mut last).unwrap();
    assert_eq!(back.speed_limit, SPEED_MEDIUM);
    assert!(limited(Command::Find, &back, &mut last).is_none());
}

#[test]
fn the_when_done_commands_name_their_choice() {
    assert_eq!(when_done(Command::WhenDoneSleep), Some(WhenDone::Sleep));
    assert_eq!(
        when_done(Command::WhenDoneShutDown),
        Some(WhenDone::ShutDown)
    );
    let named = Command::ALL
        .iter()
        .filter(|c| when_done(**c).is_some())
        .count();
    assert_eq!(named, 5);
}

#[test]
fn the_applications_verbs_act_on_the_queue_and_the_rest_are_not_its() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::app::backend::testing::backend(&runtime, dir.path());
    // Held for later, so nothing is fetched.
    let later = dm_core::AddOptions {
        later: true,
        ..dm_core::AddOptions::default()
    };
    let a = backend
        .add_with("http://127.0.0.1:9/a.bin", later.clone())
        .unwrap();
    let b = backend.add_with("http://127.0.0.1:9/b.bin", later).unwrap();
    let row = |id| backend.list().into_iter().find(|r| r.id == id).unwrap();
    assert_eq!(
        verb(Command::MoveDown, &backend, &[row(a)], ""),
        Some(Ran::Done)
    );
    let order: Vec<Id> = backend.list().iter().map(|r| r.id).collect();
    assert_eq!(order, [b, a]);
    assert_eq!(
        verb(Command::MoveUp, &backend, &[], ""),
        Some(Ran::Done),
        "nothing chosen moves nothing"
    );
    assert_eq!(
        verb(Command::DeleteCompleted, &backend, &[], ""),
        Some(Ran::Cleared(0))
    );
    assert_eq!(
        verb(Command::StopQueue, &backend, &[], "No such queue"),
        Some(Ran::QueueStopped(0))
    );
    for presentations in [Command::AddUrl, Command::Properties, Command::SortByName] {
        assert_eq!(verb(presentations, &backend, &[row(a)], ""), None);
    }
}

#[test]
fn limit_speed_comes_back_at_the_limit_in_force_else_the_last_one_else_512() {
    let mut settings = Settings {
        speed_limit: 300,
        last_speed_limit: 80,
        ..Settings::default()
    };
    assert_eq!(remembered_limit(&settings), 300);
    settings.speed_limit = 0;
    assert_eq!(remembered_limit(&settings), 80);
    settings.last_speed_limit = 0;
    assert_eq!(remembered_limit(&settings), DEFAULT_LIMIT);
}

#[test]
fn each_when_done_has_its_command_and_back() {
    for w in WhenDone::ALL {
        assert_eq!(when_done(when_done_command(w)), Some(w), "{w:?}");
    }
}

#[test]
fn the_toolbar_keeps_a_persons_order_and_hides_what_they_hid() {
    let order: Vec<String> = [
        Command::Options.id(),
        "no.such.command",
        Command::Options.id(),
        Command::AddUrl.id(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let buttons = toolbar_in(&order);
    assert_eq!(buttons.len(), TOOLBAR.len(), "each once, none lost");
    assert_eq!(
        buttons[0].0,
        Command::Options,
        "the person's first is first"
    );
    assert_eq!(buttons[1].0, Command::AddUrl);
    assert_eq!(
        buttons[2].0,
        Command::DownloadNow,
        "then the rest as they come"
    );
    let settings = Settings {
        toolbar_hidden: vec![Command::Scheduler.id().to_string()],
        ..Settings::default()
    };
    let shown = toolbar_shown(&settings);
    assert!(!shown.iter().any(|(c, _)| *c == Command::Scheduler));
    assert_eq!(shown.len(), TOOLBAR.len() - 1);
}
