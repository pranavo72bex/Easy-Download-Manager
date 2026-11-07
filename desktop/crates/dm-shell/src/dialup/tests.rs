// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A dialler that is up or not, and fails its first `failing` dials.
struct Fake {
    up: bool,
    failing: usize,
    dialled: RefCell<Vec<(String, String, String)>>,
}

impl Fake {
    fn new(up: bool, failing: usize) -> Fake {
        Fake {
            up,
            failing,
            dialled: RefCell::default(),
        }
    }
}

impl Dialer for Fake {
    fn connected(&self, _: &str) -> bool {
        self.up
    }
    fn dial(&self, name: &str, user: &str, password: &str) -> Result<(), String> {
        let mut dialled = self.dialled.borrow_mut();
        dialled.push((name.into(), user.into(), password.into()));
        if dialled.len() <= self.failing {
            Err(format!("busy line {}", dialled.len()))
        } else {
            Ok(())
        }
    }
}

fn chosen() -> DialUp {
    DialUp {
        enabled: true,
        connection: "Office VPN".into(),
        username: "me".into(),
        password: "secret".into(),
        save_password: false,
        attempts: 3,
        wait_seconds: 7,
    }
}

fn always() -> bool {
    true
}

#[test]
fn an_older_settings_file_has_it_off_and_redials_as_reference_a_does() {
    let read: DialUp = serde_json::from_str("{}").unwrap();
    assert!(!read.wanted());
    assert!(!read.save_password, "a password is saved only when asked");
    // Reference A's: zero redial attempts (endlessly), thirty seconds between.
    assert_eq!((read.attempts, read.wait_seconds), (0, 30));
}

#[test]
fn nothing_is_dialled_when_it_is_off_none_is_chosen_or_it_is_up() {
    let mut off = chosen();
    off.enabled = false;
    let mut none = chosen();
    none.connection = "  ".into();
    for (dial, up) in [(off, false), (none, false), (chosen(), true)] {
        let fake = Fake::new(up, 0);
        let mut paused = Vec::new();
        let said = ensure(&dial, &fake, &always, &mut |d| paused.push(d), &|_| {});
        assert_eq!(said, Dialled::AlreadyUp);
        assert!(fake.dialled.borrow().is_empty() && paused.is_empty());
    }
}

#[test]
fn a_busy_line_is_dialled_again_after_the_time_set_and_each_redial_said() {
    let fake = Fake::new(false, 2);
    let mut paused = Vec::new();
    let heard = RefCell::new(Vec::new());
    let said = ensure(&chosen(), &fake, &always, &mut |d| paused.push(d), &|r| {
        heard.borrow_mut().push(r)
    });
    assert_eq!(said, Dialled::Connected);
    assert_eq!(fake.dialled.borrow().len(), 3);
    assert_eq!(
        fake.dialled.borrow()[0],
        (
            "Office VPN".to_string(),
            "me".to_string(),
            "secret".to_string()
        )
    );
    assert_eq!(paused, [Duration::from_secs(7), Duration::from_secs(7)]);
    let heard = heard.borrow();
    assert_eq!(
        heard.iter().map(|r| (r.attempt, r.of)).collect::<Vec<_>>(),
        [(2, 3), (3, 3)],
        "each redial is said as the try that comes next"
    );
    assert!(heard[0].why.contains("Office VPN") && heard[0].why.contains("busy line 1"));
    assert_eq!(heard[0].wait, Duration::from_secs(7));
}

#[test]
fn a_connection_that_never_comes_up_gives_up_after_the_tries_set_with_the_last_reason() {
    let fake = Fake::new(false, 99);
    let mut paused = Vec::new();
    let said = ensure(&chosen(), &fake, &always, &mut |d| paused.push(d), &|_| {});
    assert_eq!(
        fake.dialled.borrow().len(),
        3,
        "dialled more or fewer times than set"
    );
    assert_eq!(
        paused.len(),
        2,
        "a pause after the last try only delays the word"
    );
    match said {
        Dialled::GaveUp(why) => assert!(
            why.contains("Office VPN") && why.contains("3 tries") && why.contains("busy line 3"),
            "{why}"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn zero_tries_dials_endlessly_until_it_comes_up() {
    let mut endless = chosen();
    endless.attempts = 0;
    let fake = Fake::new(false, 40);
    let heard = Cell::new(0);
    let said = ensure(&endless, &fake, &always, &mut |_| {}, &|r| {
        assert_eq!(r.of, 0, "an endless redial has no last try to say");
        heard.set(heard.get() + 1);
    });
    assert_eq!(said, Dialled::Connected);
    assert_eq!(fake.dialled.borrow().len(), 41);
    assert_eq!(heard.get(), 40);
}

#[test]
fn an_endless_redial_ends_once_nobody_waits() {
    let mut endless = chosen();
    endless.attempts = 0;
    let fake = Fake::new(false, usize::MAX);
    let looks = Cell::new(0);
    let waiting = || {
        looks.set(looks.get() + 1);
        looks.get() <= 5
    };
    let said = ensure(&endless, &fake, &waiting, &mut |_| {}, &|_| {});
    assert_eq!(said, Dialled::NoLongerWanted);
    assert_eq!(fake.dialled.borrow().len(), 5);
}

/// `IN_FORCE` and the shared dial are the program's own: these tests take turns.
static TURN: Mutex<()> = Mutex::new(());

/// A dialler that can be used from several threads: fails every dial, slowly, counting them.
struct Slow {
    dials: AtomicUsize,
}

impl Dialer for Slow {
    fn connected(&self, _: &str) -> bool {
        false
    }
    fn dial(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        self.dials.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        Err("no answer".into())
    }
}

#[test]
fn downloads_that_start_together_wait_on_one_dial_and_fail_with_it() {
    let _turn = dm_core::lock(&TURN);
    let mut dial = chosen();
    dial.attempts = 2;
    dial.wait_seconds = 0;
    *dm_core::lock(&IN_FORCE) = Some(dial);
    let slow = Slow {
        dials: AtomicUsize::new(0),
    };
    let outcomes: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let waiters: Vec<_> = (0..3)
            .map(|_| scope.spawn(|| connect_with(&slow, &always, &mut |_| {}, &|_| {})))
            .collect();
        waiters.into_iter().map(|w| w.join().unwrap()).collect()
    });
    *dm_core::lock(&IN_FORCE) = None;
    assert_eq!(
        slow.dials.load(Ordering::SeqCst),
        2,
        "the downloads that waited dialled again"
    );
    for outcome in outcomes {
        let why = outcome.unwrap_err();
        assert!(
            why.contains("after 2 tries") && why.contains("no answer"),
            "{why}"
        );
    }
}

#[test]
fn turned_off_while_it_redials_the_download_goes_as_it_would_without_it() {
    let _turn = dm_core::lock(&TURN);
    let mut dial = chosen();
    dial.attempts = 0;
    *dm_core::lock(&IN_FORCE) = Some(dial);
    let fake = Fake::new(false, usize::MAX);
    let pauses = Cell::new(0);
    // The person turns it off in Options during the second wait between tries.
    let outcome = connect_with(
        &fake,
        &always,
        &mut |_| {
            pauses.set(pauses.get() + 1);
            if pauses.get() == 2 {
                *dm_core::lock(&IN_FORCE) = None;
            }
        },
        &|_| {},
    );
    *dm_core::lock(&IN_FORCE) = None;
    assert_eq!(outcome, Ok(()));
    assert_eq!(fake.dialled.borrow().len(), 2);
}

#[test]
fn a_download_no_longer_waited_for_stops_its_dial() {
    let _turn = dm_core::lock(&TURN);
    let mut dial = chosen();
    dial.attempts = 0;
    *dm_core::lock(&IN_FORCE) = Some(dial);
    let fake = Fake::new(false, usize::MAX);
    let looks = Cell::new(0);
    let waiting = || {
        looks.set(looks.get() + 1);
        looks.get() <= 3
    };
    let outcome = connect_with(&fake, &waiting, &mut |_| {}, &|_| {});
    *dm_core::lock(&IN_FORCE) = None;
    assert!(outcome.is_err());
    assert!(fake.dialled.borrow().len() <= 3);
}

/// Off only: switched on, the engine's hook would hold every other test's downloads in this
/// program while it is set.
#[test]
fn with_it_off_nothing_is_in_force() {
    let _turn = dm_core::lock(&TURN);
    let mut off = chosen();
    off.enabled = false;
    set(&off);
    assert!(in_force().is_none());
    set(&DialUp::default());
    assert!(in_force().is_none());
}

/// Windows' own: the list is read without failing, and a connection Windows does
/// not know is refused in Windows' words, not dialled for ever.
#[cfg(windows)]
#[test]
fn windows_names_a_connection_it_does_not_know() {
    let _ = connections();
    let unknown = DialUp {
        enabled: true,
        connection: "dm test connection that does not exist".into(),
        attempts: 2,
        wait_seconds: 0,
        ..DialUp::default()
    };
    let said = ensure(&unknown, &ras::Ras, &always, &mut |_| {}, &|_| {});
    match said {
        Dialled::GaveUp(why) => {
            assert!(why.contains("did not come up after 2 tries"), "{why}");
            assert!(!why.ends_with(": "), "Windows' reason is missing: {why}");
        }
        other => panic!("{other:?}"),
    }
}
