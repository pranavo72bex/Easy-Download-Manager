// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A connection that must be up before a download goes out: the dial-up or VPN
//! connection a person chose (dm-shell's `dialup.rs`, Windows; gaps row 93, G84).
//!
//! The engine holds only the hook. A download waits on it before its first
//! request, and is never sent without the connection: reference A's stopped
//! with "Cannot connect to the Internet." when every redial failed, and sent
//! nothing meanwhile (measured 2026-11-08), and for a VPN a download sent
//! around it would come from the address it was chosen to hide. A pause or a
//! cancel while it waits is heard within a fifth of a second.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Control, Error, Wanted};

/// A try of the connection that failed, with another to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redial {
    /// The try that comes next (the first try is 1), as a range's retry says it.
    pub attempt: u32,
    /// How many tries there may be; 0 for no end.
    pub of: u32,
    /// What failed, in words.
    pub why: String,
    /// How long until the next try.
    pub wait: Duration,
}

/// Blocks until the connection is up (`Ok`) or will not come up (`Err`, in
/// words). `waiting()` turns false once nobody waits for it (the download was
/// paused or cancelled): the hook then stops at its next look, and what it
/// returns is not used. `redialling` hears each failed try that another follows.
pub type ConnectFirst =
    Arc<dyn Fn(&dyn Fn() -> bool, &dyn Fn(Redial)) -> Result<(), String> + Send + Sync>;

static HOOK: Mutex<Option<ConnectFirst>> = Mutex::new(None);

/// Sets what every download waits on before it goes out; `None` for nothing.
pub fn set_connect_first(hook: Option<ConnectFirst>) {
    *crate::lock(&HOOK) = hook;
}

/// How often a download waiting on the connection looks at its own control.
const LOOK: Duration = Duration::from_millis(200);

/// Waits for the connection before a download goes out: `Ok` at once when no
/// hook is set; `Paused` or `Cancelled` as soon as the download is asked to stop;
/// the hook's reason, as a sentence for the row, when it will not come up.
pub(crate) async fn before_download(
    control: &Control,
    redialling: impl Fn(Redial) + Send + 'static,
) -> Result<(), Error> {
    let Some(hook) = crate::lock(&HOOK).clone() else {
        return Ok(());
    };
    let asked = control.clone();
    let mut connecting =
        tokio::task::spawn_blocking(move || hook(&|| asked.wanted() == Wanted::Run, &redialling));
    loop {
        tokio::select! {
            done = &mut connecting => {
                return match done {
                    Ok(Ok(())) => Ok(()),
                    // Asked to stop while the last try ran: the stop is the answer.
                    Ok(Err(_)) if control.check().is_err() => control.check(),
                    Ok(Err(why)) => Err(Error::Internal(why)),
                    Err(join) => Err(Error::Internal(format!(
                        "the connection to dial could not be waited for: {join}"
                    ))),
                };
            }
            _ = tokio::time::sleep(LOOK) => control.check()?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The hook is the program's own, one at a time: the tests that set it take turns.
    static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn with_no_hook_a_download_goes_at_once() {
        let _turn = TURN.lock().await;
        set_connect_first(None);
        assert!(before_download(&Control::new(), |_| {}).await.is_ok());
    }

    #[tokio::test]
    async fn a_connection_that_will_not_come_up_stops_the_download_in_its_words() {
        let _turn = TURN.lock().await;
        let heard = Arc::new(AtomicUsize::new(0));
        let tries = Arc::clone(&heard);
        set_connect_first(Some(Arc::new(move |_waiting, redialling| {
            tries.fetch_add(1, Ordering::SeqCst);
            redialling(Redial {
                attempt: 1,
                of: 2,
                why: "busy".into(),
                wait: Duration::ZERO,
            });
            Err("the connection Office did not come up after 2 tries: busy".into())
        })));
        let said = std::sync::Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&said);
        let outcome = before_download(&Control::new(), move |redial| {
            crate::lock(&into).push(redial);
        })
        .await;
        set_connect_first(None);
        match outcome {
            Err(Error::Internal(why)) => assert!(why.contains("did not come up"), "{why}"),
            other => panic!("not stopped in the hook's words: {other:?}"),
        }
        assert_eq!(heard.load(Ordering::SeqCst), 1);
        assert_eq!(crate::lock(&said).len(), 1, "the failed try was not heard");
    }

    #[tokio::test]
    async fn a_pause_while_it_dials_is_heard_and_the_dialling_told() {
        let _turn = TURN.lock().await;
        let stopped = Arc::new(AtomicUsize::new(0));
        let saw_stop = Arc::clone(&stopped);
        // Dials for ever, looking whether anyone still waits, as an endless redial does.
        set_connect_first(Some(Arc::new(move |waiting, _| {
            while waiting() {
                std::thread::sleep(Duration::from_millis(20));
            }
            saw_stop.fetch_add(1, Ordering::SeqCst);
            Err("no one waits".into())
        })));
        let control = Control::new();
        let pausing = control.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            pausing.pause();
        });
        let started = std::time::Instant::now();
        let outcome =
            tokio::time::timeout(Duration::from_secs(5), before_download(&control, |_| {})).await;
        assert!(
            matches!(outcome, Ok(Err(Error::Paused))),
            "a pause while it dialled was not the answer: {outcome:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        // The dialling hears that nobody waits and ends on its own.
        for _ in 0..50 {
            if stopped.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        set_connect_first(None);
        assert_eq!(stopped.load(Ordering::SeqCst), 1, "the dialling went on");
    }
}
