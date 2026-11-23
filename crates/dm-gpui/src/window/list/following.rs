// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Playlists, channels' uploads and feeds followed (gaps row W10): kept from
//! the Playlist dialog's "Check again every", read again on the minute's
//! look of the application's tick (`dm_desktop_base::session`, which dm with
//! no window runs too), what each gained added for later to its queue, and
//! its row in the side panel's Grabber projects asking Check now, Stop
//! following or Close. What is new, and what is kept, is the application's
//! (`dm_shell::app::follow`).

use super::*;
use dm_core::youtube::Playlist;
use dm_desktop_base::i18n::{t, tf, tn};
use dm_shell::app::follow::{self, Follow};

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
        // The next frame draws the settings as now, not those the last read kept: the side
        // panel lists it at once (B75).
        self.known.drawn_fresh = false;
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
        self.known.drawn_fresh = false;
        cx.notify();
    }

    /// Reads `follow` now, in the background, with the login Options ›
    /// Sign-ins keeps for its site; what it gained is taken in at a later
    /// tick (`Session::check_follow`, carried as `Effect::Followed`).
    pub fn check_follow(&mut self, follow: &Follow) {
        self.session.check_follow(&self.backend, follow);
    }

    /// What a reading of the one at `address` brought, as the application's
    /// tick hands it on (`Effect::Followed`): the items it did not list before
    /// added for later to its queue, said; what it has seen kept.
    pub(crate) fn take_followed(
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
        let mut lines: Vec<gpui::SharedString> = vec![
            follow.address.clone().into(),
            tf(
                "Checked again every {} hours; what is new goes to {}.",
                &[&follow.every_hours, &queue],
            )
            .into(),
        ];
        // A torrent feed's word filter, where it has one (gaps row C10).
        let words = follow.words.trim();
        if !words.is_empty() {
            lines.push(
                tf(
                    "Only items whose titles have the words {} are added.",
                    &[&words],
                )
                .into(),
            );
        }
        let asked = crate::dialogs::question::Asked {
            title: t("Followed").into(),
            lines,
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
