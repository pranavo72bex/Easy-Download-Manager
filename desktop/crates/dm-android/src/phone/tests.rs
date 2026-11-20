// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The phone's services on the host, over a Java side that records each call.

use std::sync::Mutex;
use std::time::Instant;

use super::*;

/// The phone's Java side, recording each call.
#[derive(Default)]
struct Recorded {
    calls: Mutex<Vec<String>>,
    now: Mutex<i64>,
}

impl Recorded {
    fn said(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

impl Java for Arc<Recorded> {
    fn post(&self, title: &str, body: &str, open: Option<&str>) -> Result<(), String> {
        self.said(format!("post {title} / {body} / {open:?}"));
        Ok(())
    }
    fn post_failed(&self, title: &str, body: &str, row: u64) -> Result<(), String> {
        self.said(format!("failed {title} / {body} / Retry {row}"));
        Ok(())
    }
    fn index(&self, path: &str) -> Result<(), String> {
        self.said(format!("index {path}"));
        Ok(())
    }
    fn open(&self, path: &str) -> Result<(), String> {
        Err(format!("no app on this phone opens {path}"))
    }
    fn reveal(&self, path: &str) -> Result<(), String> {
        self.said(format!("reveal {path}"));
        Ok(())
    }
    fn clipboard(&self) -> Option<String> {
        Some(String::new())
    }
    fn set_clipboard(&self, text: &str) {
        self.said(format!("clipboard {text}"));
    }
    fn ongoing(&self, title: &str, body: &str) -> Result<(), String> {
        self.said(format!("ongoing {title} / {body}"));
        Ok(())
    }
    fn end_ongoing(&self) -> Result<(), String> {
        self.said("end ongoing".into());
        Ok(())
    }
    fn stay_awake(&self, on: bool) -> Result<(), String> {
        self.said(format!("awake {on}"));
        Err("no permission".into())
    }
    fn publish(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        self.said(format!("publish {}", path.display()));
        Ok(Some(
            PathBuf::from("/sdcard/Download/dm").join(path.file_name().unwrap()),
        ))
    }
    fn network(&self) -> PhoneNetwork {
        PhoneNetwork::default()
    }
    fn alarm(&self, at: Option<i64>) -> Result<(), String> {
        self.said(format!("alarm {at:?}"));
        Ok(())
    }
    fn now(&self) -> i64 {
        *self.now.lock().unwrap()
    }
}

fn phone() -> (Phone<Arc<Recorded>>, Arc<Recorded>) {
    let recorded = Arc::new(Recorded::default());
    let mut phone = Phone::new(Arc::clone(&recorded));
    phone.index_wait = Duration::ZERO;
    (phone, recorded)
}

fn calls(recorded: &Recorded) -> Vec<String> {
    recorded.calls.lock().unwrap().clone()
}

#[test]
fn a_finished_file_is_indexed_before_its_notification_and_a_tap_opens_it() {
    let (phone, recorded) = phone();
    phone.notify(Note {
        title: "Download complete".into(),
        body: "a.zip".into(),
        path: Some("/sdcard/Download/dm/a.zip".into()),
        retry: None,
    });
    let until = Instant::now() + Duration::from_secs(10);
    while calls(&recorded).len() < 2 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        calls(&recorded),
        [
            "index /sdcard/Download/dm/a.zip",
            "post Download complete / a.zip / Some(\"/sdcard/Download/dm/a.zip\")",
        ]
    );
}

#[test]
fn a_failure_is_posted_at_once_with_retry_and_a_message_without() {
    let (phone, recorded) = phone();
    phone.notify(Note {
        title: "Download failed".into(),
        body: "b.zip".into(),
        path: None,
        retry: Some(7),
    });
    phone.notify(Note {
        title: "Added".into(),
        body: "c.zip".into(),
        path: None,
        retry: None,
    });
    assert_eq!(
        calls(&recorded),
        [
            "failed Download failed / b.zip / Retry 7",
            "post Added / c.zip / None"
        ]
    );
}

#[test]
fn the_schedules_alarm_is_set_only_when_its_time_changes() {
    const MINUTE: i64 = 60_000;
    let noon = 1_800_000_000_000 - 1_800_000_000_000 % MINUTE;
    let (phone, recorded) = phone();
    *recorded.now.lock().unwrap() = noon + 5_000;
    phone.next_start(Some(30));
    // A minute later the same start is 29 minutes away: the same alarm, not set again.
    *recorded.now.lock().unwrap() = noon + MINUTE + 5_000;
    phone.next_start(Some(29));
    // The schedule changed: set again; then turned off: cleared, once.
    phone.next_start(Some(10));
    phone.next_start(None);
    phone.next_start(None);
    assert_eq!(
        calls(&recorded),
        [
            format!("alarm Some({})", noon + 29 * MINUTE),
            format!("alarm Some({})", noon + 10 * MINUTE),
            "alarm None".to_owned(),
        ]
    );
}

#[test]
fn keeping_awake_holds_the_wake_lock_and_the_ongoing_notification_together() {
    dm_desktop_base::i18n::set("en");
    let (phone, recorded) = phone();
    let downloading = Busy {
        running: 2,
        waiting: 1,
        bytes_per_second: 2048.0,
        ..Busy::default()
    };
    phone.busy(Some(downloading));
    // The same words again, or new ones within the second, change nothing.
    phone.busy(Some(downloading));
    phone.busy(Some(Busy {
        running: 1,
        ..downloading
    }));
    phone.busy(None);
    phone.busy(None);
    phone.keep_awake(true);
    phone.keep_awake(false);
    let rate = dm_desktop_base::format::rate(2048.0);
    assert_eq!(
        calls(&recorded),
        [
            "awake true".to_owned(),
            format!("ongoing 2 downloading at {rate} / 1 more is waiting to start."),
            "end ongoing".to_owned(),
            "awake false".to_owned(),
            "awake true".to_owned(),
            format!(
                "ongoing {} / {ONGOING_BODY}",
                dm_desktop_base::tray::words()[4]
            ),
            "end ongoing".to_owned(),
            "awake false".to_owned(),
        ]
    );
}

#[test]
fn the_ongoing_words_change_at_most_once_a_second() {
    let words = |title: &str| Some((title.to_owned(), ONGOING_BODY.to_owned()));
    let start = Instant::now();
    let mut ongoing = Ongoing::default();
    assert_eq!(ongoing.step(None, start), Step::Keep);
    assert_eq!(
        ongoing.step(words("a"), start),
        Step::Hold("a".into(), ONGOING_BODY.into())
    );
    assert_eq!(
        ongoing.step(words("b"), start + Duration::from_millis(500)),
        Step::Keep
    );
    assert_eq!(ongoing.step(words("a"), start + ONGOING_EVERY), Step::Keep);
    assert_eq!(
        ongoing.step(words("b"), start + ONGOING_EVERY),
        Step::Say("b".into(), ONGOING_BODY.into())
    );
    assert_eq!(ongoing.step(None, start + ONGOING_EVERY), Step::Let);
    assert_eq!(ongoing.step(None, start + ONGOING_EVERY), Step::Keep);
}

#[test]
fn the_rest_go_to_the_phone_and_the_trash_is_refused_in_words() {
    let (phone, recorded) = phone();
    assert_eq!(
        phone.open("/x/c.apk"),
        Err("no app on this phone opens /x/c.apk".into())
    );
    assert_eq!(phone.reveal("/x/c.apk"), Ok(()));
    // An empty clipboard is no text.
    assert_eq!(phone.clipboard(), None);
    phone.set_clipboard("https://example.com/a");
    assert!(phone.trash("/x/c.apk").is_err());
    assert_eq!(
        calls(&recorded),
        ["reveal /x/c.apk", "clipboard https://example.com/a"]
    );
}

/// Browse… on the phone offers the shared `Download/dm` first, then dm's own folder; a file dm
/// writes goes to the save folder, else the first of them; with none, both are refused in
/// words. On Android 10 and up only dm's own folder is offered, and a file dm writes may still
/// go to the shared one.
#[test]
fn the_folders_dm_writes_by_path_are_offered_shared_first() {
    let (bare, _) = phone();
    assert_eq!(bare.download_folders(), Ok(vec![]));
    assert!(bare.save_folder().is_err());
    let shared = PathBuf::from("/storage/emulated/0/Download/dm");
    let own = PathBuf::from("/storage/emulated/0/Android/data/com.dm.android/files/Download/dm");
    let both = phone().0.with_places(Places {
        shared: Some(shared.clone()),
        own: Some(own.clone()),
        save: None,
    });
    assert_eq!(
        both.download_folders(),
        Ok(vec![shared.clone(), own.clone()])
    );
    assert_eq!(both.save_folder(), Ok(shared.clone()));
    let ten = phone().0.with_places(Places {
        shared: None,
        own: Some(own.clone()),
        save: Some(shared.clone()),
    });
    assert_eq!(ten.download_folders(), Ok(vec![own.clone()]));
    assert_eq!(ten.save_folder(), Ok(shared));
    let own_only = phone().0.with_places(Places {
        shared: None,
        own: Some(own.clone()),
        save: None,
    });
    assert_eq!(own_only.save_folder(), Ok(own));
}

/// A finished file is published through the phone's Java side, which says where it now is.
#[test]
fn a_finished_file_is_published_where_the_phone_says() {
    let (phone, recorded) = phone();
    let file = Path::new("/e/files/Download/dm/a.zip");
    assert_eq!(
        phone.publish(file),
        Ok(Some(PathBuf::from("/sdcard/Download/dm/a.zip")))
    );
    assert_eq!(calls(&recorded), [format!("publish {}", file.display())]);
}

#[test]
fn a_move_between_wifi_and_mobile_data_is_told_to_the_engine_and_the_first_read_is_not() {
    let wifi = PhoneNetwork {
        wifi: Some(true),
        metered: Some(false),
        ..PhoneNetwork::default()
    };
    let mobile = PhoneNetwork {
        wifi: Some(false),
        metered: Some(true),
        ..PhoneNetwork::default()
    };
    assert!(!network_moved(None, wifi));
    assert!(!network_moved(Some(wifi), wifi));
    assert!(network_moved(Some(wifi), mobile));
    assert!(network_moved(Some(mobile), wifi));
    // A part read as unknown is no move.
    assert!(!network_moved(Some(wifi), PhoneNetwork::default()));
    // The battery changing is no move.
    let low = PhoneNetwork {
        battery_percent: Some(10),
        ..wifi
    };
    assert!(!network_moved(Some(wifi), low));
}
