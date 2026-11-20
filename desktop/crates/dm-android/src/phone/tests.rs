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
    fn ongoing(&self, title: &str, _: &str) -> Result<(), String> {
        self.said(format!("ongoing {title}"));
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
    let (phone, recorded) = phone();
    phone.keep_awake(true);
    phone.keep_awake(false);
    assert_eq!(
        calls(&recorded),
        [
            "awake true".to_owned(),
            format!("ongoing {ONGOING_TITLE}"),
            "end ongoing".to_owned(),
            "awake false".to_owned(),
        ]
    );
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
