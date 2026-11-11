// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's wake-ups, from any thread: at most one of each kind waits
//! to be read (gaps row 181). A wake says only "look again", so a second
//! of the same kind before the first is read adds nothing; the channel
//! they went through had no bound, and a burst of queue changes while the
//! window was busy queued one each.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dm_desktop_base::boot::Wake;

/// Whether a wake of each kind waits: `Soon`'s, then `Now`'s.
type Waiting = Arc<[AtomicBool; 2]>;

fn slot(wake: Wake) -> usize {
    match wake {
        Wake::Soon => 0,
        Wake::Now => 1,
    }
}

/// Where wakes are sent from.
#[derive(Clone)]
pub struct Wakes {
    sender: async_channel::Sender<Wake>,
    waiting: Waiting,
}

/// Where the window reads them.
pub struct Woken {
    receiver: async_channel::Receiver<Wake>,
    waiting: Waiting,
}

/// A new pair.
pub fn pair() -> (Wakes, Woken) {
    // Room for one of each kind: never full when a send is made.
    let (sender, receiver) = async_channel::bounded(2);
    let waiting: Waiting = Arc::new([AtomicBool::new(false), AtomicBool::new(false)]);
    (
        Wakes {
            sender,
            waiting: Arc::clone(&waiting),
        },
        Woken { receiver, waiting },
    )
}

impl Wakes {
    /// Wakes the window, unless a wake of this kind already waits.
    pub fn send(&self, wake: Wake) {
        if !self.waiting[slot(wake)].swap(true, Ordering::AcqRel) {
            let _ = self.sender.try_send(wake);
        }
    }
}

impl Woken {
    /// The next wake; an error once every sender is gone.
    pub async fn recv(&self) -> Result<Wake, async_channel::RecvError> {
        let wake = self.receiver.recv().await?;
        self.read(wake);
        Ok(wake)
    }

    /// A wake already waiting, if any.
    pub fn try_recv(&self) -> Result<Wake, async_channel::TryRecvError> {
        let wake = self.receiver.try_recv()?;
        self.read(wake);
        Ok(wake)
    }

    /// `wake` is read: the next of its kind is sent.
    fn read(&self, wake: Wake) {
        self.waiting[slot(wake)].store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thousand wakes of each kind, none read: one of each waits.
    #[test]
    fn at_most_one_wake_of_each_kind_waits() {
        let (wakes, woken) = pair();
        for _ in 0..1000 {
            wakes.send(Wake::Soon);
            wakes.send(Wake::Now);
        }
        assert_eq!(woken.try_recv(), Ok(Wake::Soon));
        assert_eq!(woken.try_recv(), Ok(Wake::Now));
        assert!(woken.try_recv().is_err());
        // Once read, the next is sent.
        wakes.send(Wake::Now);
        assert_eq!(woken.try_recv(), Ok(Wake::Now));
    }
}
