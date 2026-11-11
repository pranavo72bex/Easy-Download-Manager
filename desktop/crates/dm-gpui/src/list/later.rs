// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Work a handler hands off the window's thread (gaps row 183): Stop on
//! ten thousand chosen rows, a file moved to another disk, a list
//! exported. It is done after the work asked for before it
//! (`Backend::later`), and what came of it is said here, on the window's
//! thread, in the words and order it was said in when the window waited:
//! the window goes on drawing and answering meanwhile.

use dm_shell::app::backend::Backend;
use gpui::Context;
use serde_json::Value;

use super::Downloads;

/// A control port answer, and where it goes.
type Reply = (
    std::sync::mpsc::Sender<Result<Value, String>>,
    Result<Value, String>,
);

/// Work handed off and not answered yet, and the `command.run` answers
/// held for it.
#[derive(Default)]
pub(crate) struct Pending {
    count: usize,
    replies: Vec<Reply>,
}

impl Downloads {
    /// Does `work` off the window's thread, after the work asked for before
    /// it; then `then` with its answer, here, and the window is drawn
    /// again.
    pub(crate) fn later<R: Send + 'static>(
        &mut self,
        work: impl FnOnce(&Backend) -> R + Send + 'static,
        then: impl FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.errands.count += 1;
        let answer = self.backend.later(work);
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.errands.count -= 1;
                if let Some(answer) = answer {
                    then(this, answer, cx);
                }
                this.release_replies();
                cx.notify();
            });
        })
        .detach();
    }

    /// The control port's answer to `method`, sent now, or held where it
    /// is `command.run` and work it handed off is not done: a script that
    /// runs Stop and then reads the list finds every row stopped, and the
    /// answer's `said` is what the status bar says once it is, as when the
    /// window waited for it. Other asks (`ui.texts`) are answered at once
    /// meanwhile, on their own connections.
    pub(super) fn reply(&mut self, method: &str, reply: Reply) {
        if method == "command.run" && self.errands.count > 0 {
            self.errands.replies.push(reply);
            return;
        }
        let (to, answer) = reply;
        let _ = to.send(answer);
    }

    /// Sends the held answers once nothing handed off is left.
    fn release_replies(&mut self) {
        if self.errands.count > 0 {
            return;
        }
        let said = self.said.as_ref().map(|(said, _)| said.clone());
        for (to, mut answer) in self.errands.replies.drain(..) {
            if let (Ok(Value::Object(ran)), Some(said)) = (&mut answer, &said) {
                if ran.contains_key("said") {
                    ran.insert("said".into(), Value::String(said.clone()));
                }
            }
            let _ = to.send(answer);
        }
    }
}
