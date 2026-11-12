// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A dial-up or VPN connection connected before a download goes out (Options ›
//! Advanced › Dial Up / VPN, Windows), as reference A's last Options tab offers
//! (gaps row 93, G84; the maintainer's word of 2026-11-07: "If reference A has
//! then implement it").
//!
//! As reference A's, measured beside it on 2026-11-08 with a VPN to this
//! computer that refuses: off until "Use Windows Dial Up / VPN Networking" is
//! on; the connection is one Windows knows; a download that starts while it is
//! not up waits while it is dialled, redialled as many times as set (zero:
//! endlessly) with the time set between, and stops with "Cannot connect to the
//! Internet." when every try failed, sent nothing meanwhile. Two things differ:
//! reference A asks for a user name and password in a box of its own when
//! Options has none, where dm dials with the ones Windows keeps for the
//! connection; and its Apply button, which hands them to Windows, is not here:
//! dm keeps its own (`docs/decisions.md`, 2026-11-08).
//!
//! The Scheduler's "Hang up modem when done", reference A's too, hangs the same
//! connection up once the scheduled downloads are done ([`hang_up`]).

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;

use dm_core::Redial;

#[cfg(windows)]
mod ras;

#[cfg(test)]
mod tests;

/// What Options says about the connection to dial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialUp {
    /// "Use Windows Dial Up / VPN Networking": off unless the person turns it on.
    pub enabled: bool,
    /// The connection's name as Windows lists it.
    pub connection: String,
    /// Its user name; empty, with no password either, takes the one Windows keeps.
    pub username: String,
    /// Its password. Kept in the system's store between runs when
    /// `save_password` is on, never in the settings file (`settings/secrets.rs`).
    pub password: String,
    /// Off, as reference A's: the password typed is forgotten when dm closes.
    pub save_password: bool,
    /// How many times dm dials before it gives up; zero dials endlessly.
    pub attempts: u32,
    /// Seconds between one try and the next.
    pub wait_seconds: u64,
}

impl Default for DialUp {
    fn default() -> Self {
        // Reference A's: zero redial attempts (endlessly), 30 seconds between.
        DialUp {
            enabled: false,
            connection: String::new(),
            username: String::new(),
            password: String::new(),
            save_password: false,
            attempts: 0,
            wait_seconds: 30,
        }
    }
}

impl DialUp {
    /// Whether a connection is to be dialled: switched on, with one chosen.
    pub fn wanted(&self) -> bool {
        self.enabled && !self.connection.trim().is_empty()
    }
}

/// The most tries Options takes.
pub const MOST_ATTEMPTS: u32 = 999;
/// The longest wait Options takes between two tries, in seconds.
pub const MOST_WAIT_SECONDS: u64 = 3600;

/// What dials, so the rule below can be tried without a modem.
pub trait Dialer {
    /// Whether `name` is connected now.
    fn connected(&self, name: &str) -> bool;
    /// Dials `name` once, waiting until it is up or has failed. An empty `user`
    /// and `password` take the ones Windows keeps for it.
    fn dial(&self, name: &str, user: &str, password: &str) -> Result<(), String>;
    /// Hangs `name` up if Windows has it open, up or still dialling, and waits
    /// for it to go: `Ok(true)` once it has, `Ok(false)` when it was not open.
    fn hang_up(&self, name: &str) -> Result<bool, String>;
}

/// How [`ensure`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialled {
    /// Up already, or nothing to dial: nothing was dialled.
    AlreadyUp,
    /// Dialled, and it came up.
    Connected,
    /// Every try failed: why, in words.
    GaveUp(String),
    /// Nobody waits for it any more (`waiting` turned false).
    NoLongerWanted,
}

/// Connects `dial.connection` unless it is up already: dialled up to
/// `attempts` times (endlessly for zero), `wait_seconds` apart, for as long as
/// `waiting()` says somebody waits. `redialling` hears each failed try that
/// another follows; `pause` waits between two.
pub fn ensure(
    dial: &DialUp,
    dialer: &dyn Dialer,
    waiting: &dyn Fn() -> bool,
    pause: &mut dyn FnMut(Duration),
    redialling: &dyn Fn(Redial),
) -> Dialled {
    let name = dial.connection.trim();
    if !dial.wanted() || dialer.connected(name) {
        return Dialled::AlreadyUp;
    }
    let of = dial.attempts.min(MOST_ATTEMPTS);
    let wait = Duration::from_secs(dial.wait_seconds.min(MOST_WAIT_SECONDS));
    let mut tried = 0u32;
    loop {
        if !waiting() {
            return Dialled::NoLongerWanted;
        }
        tried = tried.saturating_add(1);
        let why = match dialer.dial(name, &dial.username, &dial.password) {
            Ok(()) => return Dialled::Connected,
            Err(why) => why,
        };
        if of != 0 && tried >= of {
            return Dialled::GaveUp(format!(
                "the connection {name} did not come up after {tried} tries: {why}"
            ));
        }
        redialling(Redial {
            attempt: tried.saturating_add(1),
            of,
            why: format!("the connection {name} did not come up: {why}"),
            wait,
        });
        pause(wait);
    }
}

/// The connection the settings in force name; `None` for none.
static IN_FORCE: Mutex<Option<DialUp>> = Mutex::new(None);

/// One dial at a time, and the end of the last: downloads that start together
/// wait on one dial, and those that waited while it failed fail with it rather
/// than each dialling every try again.
// Dialled on Windows alone (and in the tests, everywhere): elsewhere nothing reads these.
#[cfg(any(windows, test))]
static DIALLING: Mutex<()> = Mutex::new(());
#[cfg(any(windows, test))]
static LAST: Mutex<(u64, Option<String>)> = Mutex::new((0, None));

/// The tests' turn at what is in force (`IN_FORCE`, the shared dial): every test in the crate
/// that applies settings reaches `set` through `shell::apply`, and one did between a dial-up
/// test's setting and its dials (gaps row S20). Taken again on a thread that holds it.
#[cfg(test)]
pub(crate) fn turn() -> Turn {
    if HOLDS.with(std::cell::Cell::get) {
        return Turn(None);
    }
    let guard = dm_core::lock(&TURN);
    HOLDS.with(|h| h.set(true));
    Turn(Some(guard))
}

#[cfg(test)]
static TURN: Mutex<()> = Mutex::new(());

#[cfg(test)]
thread_local! {
    static HOLDS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// [`turn`], held until dropped.
#[cfg(test)]
pub(crate) struct Turn(Option<std::sync::MutexGuard<'static, ()>>);

#[cfg(test)]
impl Drop for Turn {
    fn drop(&mut self) {
        if self.0.take().is_some() {
            HOLDS.with(|h| h.set(false));
        }
    }
}

/// Takes the settings' connection as the one to dial (`shell::apply`), and
/// hands the engine the wait every download makes for it.
pub fn set(dial: &DialUp) {
    #[cfg(test)]
    let _turn = turn();
    let wanted = dial.wanted().then(|| dial.clone());
    let any = wanted.is_some();
    *dm_core::lock(&IN_FORCE) = wanted;
    dm_core::set_connect_first(any.then(|| {
        std::sync::Arc::new(|waiting: &dyn Fn() -> bool, redialling: &dyn Fn(Redial)| {
            before_download(waiting, redialling)
        }) as dm_core::ConnectFirst
    }));
}

#[cfg(any(windows, test))]
fn in_force() -> Option<DialUp> {
    dm_core::lock(&IN_FORCE).clone()
}

/// What a download waits on before it goes out (the engine's `connect_first`):
/// `Ok` once the connection in force is up, or when none is; why, when it will
/// not come up.
fn before_download(waiting: &dyn Fn() -> bool, redialling: &dyn Fn(Redial)) -> Result<(), String> {
    #[cfg(windows)]
    {
        connect_with(
            &ras::Ras,
            waiting,
            &mut |wait| pause_while(wait, waiting),
            redialling,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (waiting, redialling);
        Ok(())
    }
}

/// [`before_download`] with its dialler and pause given, for the tests.
#[cfg(any(windows, test))]
fn connect_with(
    dialer: &dyn Dialer,
    waiting: &dyn Fn() -> bool,
    pause: &mut dyn FnMut(Duration),
    redialling: &dyn Fn(Redial),
) -> Result<(), String> {
    loop {
        let Some(dial) = in_force() else {
            return Ok(());
        };
        let asked = dm_core::lock(&LAST).0;
        let _one = dm_core::lock(&DIALLING);
        {
            // A dial that ended while this one waited its turn and failed: its word is this one's.
            let last = dm_core::lock(&LAST);
            if last.0 != asked {
                if let Some(why) = &last.1 {
                    return Err(why.clone());
                }
            }
        }
        // The setting changed meanwhile: dialled as it says now.
        let same = || in_force().as_ref() == Some(&dial);
        let still = || waiting() && same();
        let outcome = ensure(&dial, dialer, &still, pause, redialling);
        let failed = match &outcome {
            Dialled::GaveUp(why) => Some(why.clone()),
            _ => None,
        };
        {
            let mut last = dm_core::lock(&LAST);
            last.0 = last.0.wrapping_add(1);
            last.1 = failed.clone();
        }
        match outcome {
            Dialled::AlreadyUp | Dialled::Connected => return Ok(()),
            Dialled::GaveUp(why) => return Err(why),
            Dialled::NoLongerWanted if !waiting() => return Err("no longer waited for".into()),
            Dialled::NoLongerWanted => continue,
        }
    }
}

/// Waits `wait`, or less once nobody waits any more.
#[cfg(windows)]
fn pause_while(wait: Duration, waiting: &dyn Fn() -> bool) {
    let until = std::time::Instant::now() + wait;
    while waiting() {
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(200)));
    }
}

/// How [`hang_up`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HungUp {
    /// Options › Advanced › Dial Up / VPN sets no connection: nothing to hang up.
    NoneSet,
    /// The connection was not connected.
    WasDown(String),
    /// The connection was connected, and is hung up.
    Done(String),
    /// Windows did not hang the connection up: its name, and why.
    Failed(String, String),
}

/// Hangs up the connection Options › Advanced › Dial Up / VPN sets, if it is
/// up: Scheduler › Hang up modem when done, as reference A's. Whoever
/// connected it, as the person asked for that connection to go; never in the
/// middle of a dial of dm's, but after it. Windows takes a moment to end a
/// connection, so this is called off the window's thread.
pub fn hang_up() -> HungUp {
    #[cfg(windows)]
    {
        hang_up_with(&ras::Ras)
    }
    #[cfg(not(windows))]
    {
        HungUp::NoneSet
    }
}

/// [`hang_up`] with its dialler given, for the tests.
#[cfg(any(windows, test))]
fn hang_up_with(dialer: &dyn Dialer) -> HungUp {
    let Some(dial) = in_force() else {
        return HungUp::NoneSet;
    };
    let _one = dm_core::lock(&DIALLING);
    let name = dial.connection.trim().to_owned();
    match dialer.hang_up(&name) {
        Ok(true) => HungUp::Done(name),
        Ok(false) => HungUp::WasDown(name),
        Err(why) => HungUp::Failed(name, why),
    }
}

/// [`hang_up`] off the caller's thread (it waits on the system), its answer
/// left in `into` for the presentation to say. Where no thread can be
/// started it is done here, and the answer given back.
pub fn hang_up_later(into: std::sync::Arc<std::sync::Mutex<Option<HungUp>>>) -> Option<HungUp> {
    let slot = std::sync::Arc::clone(&into);
    let spawned = std::thread::Builder::new()
        .name("dm-hang-up".into())
        .spawn(move || {
            let hung = hang_up();
            *dm_core::lock(&slot) = Some(hung);
        });
    match spawned {
        Ok(_) => None,
        Err(_) => Some(hang_up()),
    }
}

/// The connections Windows knows, by name, for Options to offer; none
/// elsewhere.
pub fn connections() -> Vec<String> {
    #[cfg(windows)]
    {
        ras::entries()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}
