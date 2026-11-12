// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What Add URL's OK does with what was typed, for every window (B01,
//! B02): says what is wrong with it, or sends it on (Download details, the
//! question for an address already listed, the Playlist dialog), and the
//! addresses to remember for its Recent list. What each window draws, and
//! where the keyboard goes, is its own.

use std::sync::Arc;

use dm_core::{Entry, Id};
use dm_shell::app::rules::{Dup, GoOn};
use dm_shell::Settings;

use crate::i18n::t;

/// Where Add URL sends what was typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// It stays, saying this; the address keeps the keyboard, to be put
    /// right.
    Says(&'static str),
    /// Download details for these addresses (several pasted one a line: a
    /// batch, in one).
    Details(Vec<String>),
    /// The row `id` already holds the address: asked about first.
    Duplicate {
        /// The address.
        address: String,
        /// The row holding it.
        id: Id,
    },
    /// As [`Next::Duplicate`], answered by the person's remembered choice.
    Answered {
        /// The address.
        address: String,
        /// The row holding it.
        id: Id,
        /// The choice.
        how: Dup,
    },
    /// A YouTube playlist: its videos listed to be ticked.
    Playlist(String),
}

/// What OK decided, and the Recent list to remember where it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    /// Where it goes.
    pub next: Next,
    /// The Recent list to save, the new address first, where it changes.
    pub remember: Option<Vec<String>>,
}

/// The `file://` address of a `.torrent` on this computer named by `text`,
/// its path (`~` for the home folder) or that address itself, if the file
/// is there.
pub fn local_torrent(text: &str) -> Option<String> {
    let text = text.trim();
    let path = match dm_core::local_file(text) {
        Some(path) => path,
        None if !text.contains("://") => dm_shell::settings::full_path(text)?,
        None => return None,
    };
    if !dm_core::is_torrent(&path.to_string_lossy()) || !path.is_file() {
        return None;
    }
    dm_core::file_url(&path)
}

/// Whether an address's query carries something secret: a token, a
/// signature, a key, a password or a session. Such an address is not
/// remembered for Add URL's list of recent ones (the list is kept in the
/// settings file in the clear, and a signed link has expired by the next
/// time), and one already remembered is not offered.
pub fn query_carries_a_secret(address: &str) -> bool {
    const SECRET: [&str; 8] = [
        "token", "sig", "key", "auth", "pass", "pwd", "secret", "session",
    ];
    let Ok(url) = url::Url::parse(address) else {
        return false;
    };
    url.query_pairs().any(|(name, _)| {
        let name = name.to_ascii_lowercase();
        SECRET.iter().any(|word| name.contains(word)) || name.contains("credential")
    })
}

/// The Recent list Add URL offers: the settings', less any carrying a
/// secret.
pub fn recent(settings: &Settings) -> Vec<String> {
    settings
        .recent_urls
        .iter()
        .filter(|url| !query_carries_a_secret(url))
        .cloned()
        .collect()
}

/// What OK does with `typed` (and a sign-in, where `authorize`), against
/// the settings in force and the rows listed.
pub fn decide(
    typed: &str,
    authorize: bool,
    login: &str,
    password: &str,
    settings: &Settings,
    rows: &[Arc<Entry>],
) -> Decided {
    let says = |words| Decided {
        next: Next::Says(words),
        remember: None,
    };
    let mut address = typed.trim().to_string();
    // A .torrent on this computer, by its path or its file:// address: it
    // could only be dragged in, and Add URL called it no link.
    if let Some(local) = local_torrent(&address) {
        address = local;
    }
    if dm_shell::app::links::looks_downloadable(&address).is_none()
        && local_torrent(&address).is_none()
    {
        // Nothing typed is asked for, not called a bad link.
        return says(if address.is_empty() {
            t("Type or paste the address of what to download.")
        } else {
            t("That isn't a link dm can download. Paste a web address like https://example.com/file.zip.")
        });
    }
    // Several links pasted one after another (a list copied from a page or
    // a chat, a line each): a batch, in one Download details. Run together
    // with nothing between them, they are still asked about, since where
    // one ends is a guess.
    if dm_shell::app::links::two_addresses(&address) {
        let found = dm_shell::app::links::links_in(&address);
        if found.len() > 1 && found.len() == address.split_whitespace().count() {
            return Decided {
                next: Next::Details(found),
                remember: None,
            };
        }
        return says(t(
            "That looks like two links. Add one at a time, or use Add links from clipboard.",
        ));
    }
    if authorize && !login.is_empty() {
        if let Ok(mut url) = url::Url::parse(&address) {
            let _ = url.set_username(login);
            let _ = url.set_password(Some(password));
            address = url.to_string();
        }
    }
    // Remembered for Add URL's list, without any login.
    let remembered = dm_shell::lists::bare(&address);
    let remember = (!query_carries_a_secret(&remembered)
        && settings.recent_urls.first() != Some(&remembered))
    .then(|| {
        let mut recent = settings.recent_urls.clone();
        recent.retain(|u| *u != remembered);
        recent.insert(0, remembered);
        recent
    });
    // Already a row: asked, rather than a second row nobody meant; answered
    // at once where the person ticked "Remember my choice".
    let next = if let Some(id) = dm_shell::app::rules::already_listed(rows, &address) {
        match Dup::from_word(&settings.duplicate_choice) {
            Some(how) => Next::Answered { address, id, how },
            None => Next::Duplicate { address, id },
        }
    } else if dm_core::youtube::playlist_id(&address).is_some() {
        // A playlist is its videos, each a download: listed to be ticked.
        Next::Playlist(address)
    } else {
        Next::Details(vec![address])
    };
    Decided { next, remember }
}

/// What an answer to "Already in the list" does, asked or remembered.
#[derive(Debug, Clone)]
pub enum Carried {
    /// The row is chosen; a stopped or failed one goes on (`go_on`:
    /// resumed, or tried again), so the person who clicked the link again
    /// is not left to find the button.
    Show {
        /// The row.
        id: Id,
        /// How it goes on, if it does.
        go_on: Option<GoOn>,
    },
    /// Download details for the address: a numbered copy.
    Details(String),
    /// The address, added at once over the row's finished file.
    Replace {
        /// The address.
        address: String,
        /// The options that replace the file.
        with: Box<dm_core::AddOptions>,
    },
}

/// What answering `how` for `address`, already the row `id`, does.
pub fn carry(rows: &[Arc<Entry>], address: String, id: Id, how: Dup) -> Carried {
    let row = rows.iter().find(|r| r.id == id);
    match how {
        Dup::Show => Carried::Show {
            id,
            // Held or not: the person asked for this file again.
            go_on: row
                .map(|r| dm_shell::app::rules::go_on(&r.state, false))
                .filter(|g| matches!(g, GoOn::Resume | GoOn::Retry)),
        },
        Dup::Again => Carried::Details(address),
        Dup::Replace => match row.and_then(|r| dm_shell::app::rules::replacing(r)) {
            Some(with) => Carried::Replace {
                address,
                with: Box::new(with),
            },
            // Nothing finished to replace: a numbered copy, as before.
            None => Carried::Details(address),
        },
    }
}

/// What "Already in the list" offers for the row `id`: whether its first
/// button goes on with it ("Resume it" rather than "Show it"), and whether
/// it can replace the row's finished file.
pub fn duplicate_offers(rows: &[Arc<Entry>], id: Id) -> (bool, bool) {
    let row = rows.iter().find(|r| r.id == id);
    let go_on = row.is_some_and(|r| {
        matches!(
            dm_shell::app::rules::go_on(&r.state, false),
            GoOn::Resume | GoOn::Retry
        )
    });
    let replace = row
        .and_then(|r| dm_shell::app::rules::replacing(r))
        .is_some();
    (go_on, replace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_typed_is_asked_for_and_a_bad_link_is_said_to_be_one() {
        let settings = Settings::default();
        assert_eq!(
            decide("  ", false, "", "", &settings, &[]).next,
            Next::Says("Type or paste the address of what to download.")
        );
        assert!(matches!(
            decide("not a link", false, "", "", &settings, &[]).next,
            Next::Says(s) if s.starts_with("That isn't a link dm can download")
        ));
    }

    #[test]
    fn an_address_goes_to_download_details_and_is_remembered_without_its_secret() {
        let settings = Settings::default();
        let plain = decide("https://e.com/a.zip", false, "", "", &settings, &[]);
        assert_eq!(
            plain.next,
            Next::Details(vec!["https://e.com/a.zip".into()])
        );
        assert_eq!(plain.remember, Some(vec!["https://e.com/a.zip".into()]));
        let signed = decide(
            "https://e.com/a.zip?token=abc",
            false,
            "",
            "",
            &settings,
            &[],
        );
        assert_eq!(signed.remember, None, "a signed link is not remembered");
        let login = decide("https://e.com/a.zip", true, "me", "pw", &settings, &[]);
        assert_eq!(
            login.next,
            // Built here, as a sign-in in an address is never written out.
            Next::Details(vec![format!("https://me:{}@e.com/a.zip", "pw")])
        );
        assert_eq!(
            login.remember,
            Some(vec!["https://e.com/a.zip".into()]),
            "without its login"
        );
    }

    #[test]
    fn several_links_a_line_each_are_a_batch_and_run_together_are_asked_about() {
        let settings = Settings::default();
        assert_eq!(
            decide(
                "https://e.com/a.zip\nhttps://e.com/b.zip",
                false,
                "",
                "",
                &settings,
                &[]
            )
            .next,
            Next::Details(vec![
                "https://e.com/a.zip".into(),
                "https://e.com/b.zip".into()
            ])
        );
        assert!(matches!(
            decide("https://e.com/a.ziphttps://e.com/b.zip", false, "", "", &settings, &[]).next,
            Next::Says(s) if s.starts_with("That looks like two links")
        ));
    }

    #[test]
    fn a_secret_in_a_query_keeps_an_address_out_of_recent() {
        assert!(query_carries_a_secret(
            "https://cdn.example/f.zip?token=abc"
        ));
        assert!(query_carries_a_secret("https://example.com/get?Password=x"));
        assert!(!query_carries_a_secret(
            "https://example.com/get?id=7&name=report"
        ));
        let settings = Settings {
            recent_urls: vec!["https://e.com/a?sig=1".into(), "https://e.com/b".into()],
            ..Settings::default()
        };
        assert_eq!(recent(&settings), vec!["https://e.com/b".to_string()]);
    }
}
