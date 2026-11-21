// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Work a window's handler hands off its own thread (gaps row 183): a file
//! moved to another disk, Stop on ten thousand rows, a list exported. Each
//! is done on one thread of its own, one after another in the order asked,
//! so two commands given in a row still happen in that order, as they did
//! when the window waited for each; and its answer comes back as a future
//! the window awaits, to say what came of it on its own thread.

use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll};

type Job = Box<dyn FnOnce() + Send>;

/// The thread errands are done on, started with the first.
#[derive(Default)]
pub struct Errands {
    to: std::sync::Mutex<Option<mpsc::Sender<Job>>>,
}

impl Errands {
    /// Does `work` on the errands' thread after those asked before it; its
    /// answer, or `None` where it panicked (said on standard error, as a
    /// panic is) or the thread could not be started.
    pub fn run<R: Send + 'static>(&self, work: impl FnOnce() -> R + Send + 'static) -> Answer<R> {
        let (tell, answer) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move || {
            // A panic loses this answer only: the thread goes on to the next.
            if let Ok(done) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
                let _ = tell.send(done);
            }
        });
        let mut to = dm_core::lock(&self.to);
        let job = match to.as_ref() {
            Some(sender) => match sender.send(job) {
                Ok(()) => return Answer(answer),
                // The thread is gone (it cannot end while a sender is
                // kept, but a new one costs nothing): started again.
                Err(mpsc::SendError(job)) => job,
            },
            None => job,
        };
        let (sender, jobs) = mpsc::channel::<Job>();
        let started = std::thread::Builder::new()
            .name("dm-errands".into())
            .spawn(move || {
                for job in jobs {
                    job();
                }
            });
        if started.is_ok() && sender.send(job).is_ok() {
            *to = Some(sender);
        }
        Answer(answer)
    }
}

/// An errand's answer, awaited on any executor (the window's own).
pub struct Answer<R>(pub(crate) tokio::sync::oneshot::Receiver<R>);

/// Where an answer given later is told (`answer_later`).
pub struct Teller<R>(tokio::sync::oneshot::Sender<R>);

impl<R> Teller<R> {
    /// Gives the answer; nothing where it is no longer awaited.
    pub fn tell(self, answer: R) {
        let _ = self.0.send(answer);
    }
}

/// An answer something else gives later (a system panel, when it is
/// closed), and where it is told.
pub fn answer_later<R>() -> (Teller<R>, Answer<R>) {
    let (tell, answer) = tokio::sync::oneshot::channel();
    (Teller(tell), Answer(answer))
}

impl<R> Future for Answer<R> {
    type Output = Option<R>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<R>> {
        Pin::new(&mut self.0).poll(cx).map(Result::ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Errands are done in the order asked, off the asking thread, and a
    /// panic loses its own answer alone.
    #[test]
    fn errands_run_in_order_off_the_thread_and_survive_a_panic() {
        let errands = Errands::default();
        let here = std::thread::current().id();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let answers: Vec<_> = (0..50)
            .map(|i| {
                let seen = seen.clone();
                errands.run(move || {
                    std::thread::sleep(std::time::Duration::from_micros(200));
                    seen.lock().unwrap().push(i);
                    (i, std::thread::current().id())
                })
            })
            .collect();
        let broken = errands.run(|| -> u32 { panic!("an errand that breaks") });
        let after = errands.run(|| 7);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        for (i, answer) in answers.into_iter().enumerate() {
            let (n, thread) = runtime.block_on(answer).unwrap();
            assert_eq!(n, i);
            assert_ne!(thread, here);
        }
        assert_eq!(*seen.lock().unwrap(), (0..50).collect::<Vec<_>>());
        assert_eq!(runtime.block_on(broken), None);
        assert_eq!(runtime.block_on(after), Some(7));
    }
}
