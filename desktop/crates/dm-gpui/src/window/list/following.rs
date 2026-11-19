// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Playlists, channels' uploads and feeds followed (gaps row W10): kept from
//! the Playlist dialog's "Check again every", read again on the minute's
//! look as the saved searches are, what each gained added for later to its
//! queue, and its row in the side panel's Grabber projects asking Check
//! now, Stop following or Close. What is new, and what is kept, is the
//! application's (`dm_shell::app::follow`).

use super::*;
use dm_core::youtube::Playlist;
use dm_desktop_base::i18n::{t, tf, tn};
use dm_shell::app::follow::{self, Follow};

/// How often the followed ones are looked at for one that is due.
const FOLLOWS_EVERY: Duration = Duration::from_secs(60);

impl Downloads {
    /// Follows `follow` from now on, kept in the settings, said.
    pub fn follow(&mut self, follow: Follow, cx: &mut Context<Self>) {
        let words = tf(
            "Following {}, checked again every {} hours.",
            &[&follow.name(), &follow.every_hours],
        );
        let wanted = follow::followed(&self.backend.settings(), follow);
        match self.backend.set_settings(wanted) {
            Ok(_) => self.say(words),
            Err(e) => self
                .status
                .problems
                .push(tf("what is followed could not be saved: {}", &[&e])),
        }
        cx.notify();
    }

    /// Stops following the one at `address`.
    pub fn unfollow(&mut self, address: &str, cx: &mut Context<Self>) {
        let wanted = follow::unfollowed(&self.backend.settings(), address);
        if let Err(e) = self.backend.set_settings(wanted) {
            self.status
                .problems
                .push(tf("what is followed could not be saved: {}", &[&e]));
        }
        cx.notify();
    }

    /// Reads `follow` now, in the background, with the login Options ›
    /// Sign-ins keeps for its site; what it gained is taken in on a later
    /// wake (`take_follows`).
    pub fn check_follow(&mut self, follow: &Follow) {
        let results = self.threads.follow_results.clone();
        let address = follow.address.clone();
        let reading = self.backend.settings().with_login(&address);
        follow::read_later(&self.backend, reading, move |read| {
            dm_core::lock(&results).push((address, read));
            dm_shell::app::changes::changed();
        });
    }

    /// Takes in what finished readings brought, and starts reading any
    /// followed one whose time has come, looked at once a minute.
    pub(crate) fn take_follows(&mut self, cx: &mut Context<Self>) {
        let done: Vec<_> = std::mem::take(&mut *dm_core::lock(&self.threads.follow_results));
        for (address, read) in done {
            self.take_followed(&address, read, cx);
        }
        let now = Instant::now();
        if self
            .threads
            .follows_checked
            .is_some_and(|at| now.duration_since(at) < FOLLOWS_EVERY)
        {
            return;
        }
        self.threads.follows_checked = Some(now);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let Some((due, wanted)) = follow::due_now(&self.backend.settings(), epoch) else {
            return;
        };
        if let Err(e) = self.backend.set_settings(wanted) {
            // Still read, and not again for a minute; but said, since an
            // unsaved last reading reads it again at every launch.
            self.status
                .problems
                .push(tf("what is followed could not be saved: {}", &[&e]));
        }
        for one in &due {
            self.check_follow(one);
        }
    }

    /// What a reading of the one at `address` brought: the items it did not
    /// list before added for later to its queue, said; what it has seen kept.
    fn take_followed(
        &mut self,
        address: &str,
        read: Result<Playlist, String>,
        cx: &mut Context<Self>,
    ) {
        let settings = self.backend.settings();
        // No longer followed: what it brought is let go.
        let Some(kept) = settings.follows.iter().find(|f| f.address == address) else {
            return;
        };
        let name = kept.name().to_string();
        let list = match read {
            Ok(list) => list,
            Err(e) => {
                self.status
                    .problems
                    .push(tf("the followed {} could not be read: {}", &[&name, &e]));
                return;
            }
        };
        let new = follow::new_items(kept, &list, &self.rows);
        if new.is_empty() {
            self.say(tf("{} has nothing new.", &[&name]));
        } else {
            let queue = dm_desktop_base::i18n::name(kept.queue_or_main());
            match self.add_items(new, cx) {
                Ok(n) => self.say(tn(
                    "Added {} download to {}.||Added {} downloads to {}.",
                    n as u64,
                    &[&n, &queue],
                )),
                Err(e) => self.status.problems.push(tf(
                    "the items {} gained could not be added: {}",
                    &[&name, &e],
                )),
            }
        }
        let settings = self.backend.settings();
        if let Some(wanted) = follow::seen_saved(&settings, address, &list, &self.rows) {
            if let Err(e) = self.backend.set_settings(wanted) {
                self.status
                    .problems
                    .push(tf("what is followed could not be saved: {}", &[&e]));
            }
        }
    }

    /// A followed one's row in the side panel, clicked: what it is, and
    /// Check now, Stop following or Close.
    pub fn follow_asked(&mut self, follow: Follow, cx: &mut Context<Self>) {
        let queue = dm_desktop_base::i18n::name(follow.queue_or_main());
        let asked = crate::dialogs::question::Asked {
            title: t("Followed").into(),
            lines: vec![
                follow.address.clone().into(),
                tf(
                    "Checked again every {} hours; what is new goes to {}.",
                    &[&follow.every_hours, &queue],
                )
                .into(),
            ],
            named: Some(follow.name().to_string().into()),
            detail: None,
            toggle: None,
            ticked: false,
            buttons: vec![
                t("Check now").into(),
                t("Stop following").into(),
                t("Close").into(),
            ],
            default: 0,
            escape: 2,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            let _ = view.update(cx, move |view, cx| {
                match button {
                    0 => view.check_follow(&follow),
                    1 => view.unfollow(&follow.address, cx),
                    _ => {}
                }
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }
}
