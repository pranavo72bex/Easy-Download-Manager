// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Where a download the browser handed over goes, as the application decides
//! it (`Session::next_handed`): to a row waiting for a fresh link, dropped as
//! the same click again, added straight in, or asked about in a window the
//! presentation opens. The presentation only opens its dialogs and says what
//! could not be done.

use std::sync::Arc;
use std::time::{Duration, Instant};

use edm_core::{Entry, Id};

use super::backend::Backend;
use super::rules::{worth_keeping, Dup};
use crate::host::Request;
use crate::Settings;

/// How soon the same address handed over again is taken for the same click.
pub const HANDED_TWICE_WITHIN: Duration = Duration::from_secs(5);

/// How long a row waits for a fresh link from its page in the browser.
pub const AWAIT_LINK_FOR: Duration = Duration::from_secs(10 * 60);

/// What the presentation can ask in, for a hand-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asking {
    /// No window: everything is added straight in.
    Nobody,
    /// A window, with a dialog open that a question would wait behind.
    Busy,
    /// A window free to ask.
    Free,
}

/// Where the next hand-off went.
#[derive(Debug)]
pub enum Route {
    /// Nothing handed over, or it waits for the dialog open now.
    Nothing,
    /// The download is row `id`'s fresh link (awaited, and now ended).
    FreshLink {
        /// The row waiting.
        id: Id,
        /// Its new address.
        url: String,
    },
    /// The same address a moment after it was taken: the same click, dropped.
    Again,
    /// A page's links, for the "Download all links" dialog.
    AskLinks(Request),
    /// A page's links, to be added straight into the main queue with nobody
    /// to ask (the presentation adds them as its own).
    AddLinks(Vec<(String, edm_core::AddOptions)>),
    /// A download caught for an address already listed (row `id`): the
    /// person's remembered answer, or `None` to be asked.
    Listed {
        /// The address handed over.
        url: String,
        /// The row that has it.
        id: Id,
        /// "Remember my choice", where it was ticked.
        how: Option<Dup>,
    },
    /// Added straight in: the row, or why it was not.
    Added(Result<Id, String>),
    /// For "Download details", asked before it starts.
    Ask(Request),
}

/// The addresses taken from the browser a moment ago.
#[derive(Debug, Default)]
pub struct Recent(Vec<(String, Instant)>);

impl Recent {
    /// `url` was taken from the browser at `now`.
    pub fn taken(&mut self, url: &str, now: Instant) {
        self.0.push((url.to_string(), now));
    }

    /// Whether `url` was taken within `HANDED_TWICE_WITHIN` of `now`. One
    /// handed over and cancelled is not remembered: clicked again, it is meant.
    pub fn just_taken(&mut self, url: &str, now: Instant) -> bool {
        self.0
            .retain(|(_, at)| now.duration_since(*at) < HANDED_TWICE_WITHIN);
        self.0.iter().any(|(taken, _)| taken == url)
    }
}

/// The application's memory of hand-offs between ticks.
#[derive(Debug, Default)]
pub struct Hands {
    /// Taken a moment ago.
    pub recent: Recent,
    /// A row waiting for a fresh link from the browser, since when.
    pub awaiting_link: Option<(Id, Instant)>,
}

impl Hands {
    /// Row `id` waits for a fresh link: the next download caught in the
    /// browser, routed to EDM's window rather than its own row.
    pub fn await_link(&mut self, id: Id, now: Instant) {
        self.awaiting_link = Some((id, now));
        crate::shell::route_to_window(true);
    }

    /// Stops waiting for a fresh link.
    pub fn stop_awaiting_link(&mut self) {
        self.awaiting_link = None;
        crate::shell::route_to_window(false);
    }

    /// Takes the next download handed over from `backend` and routes it.
    /// `rows` are the list as last read; what can be done without a window
    /// (adding, the same click dropped, a fresh link's wait ended) is done.
    pub fn next(
        &mut self,
        backend: &Backend,
        settings: &Settings,
        rows: &[Arc<Entry>],
        asking: Asking,
        now: Instant,
    ) -> Route {
        // A caught download by its own setting, the panel's and menus' by
        // theirs. A part typed on the panel's "Part" line opens Download
        // details whatever they say, its ends filled in (contract row B59):
        // added at once, it was the whole video.
        let (caught, part, quiet) =
            edm_core::lock(&backend.handed)
                .front()
                .map_or((false, false, false), |r| {
                    (
                        r.caught,
                        !(r.from.trim().is_empty() && r.to.trim().is_empty()),
                        r.quiet,
                    )
                });
        // `--quiet` on the command line asks nothing (gaps row F3).
        let ask = asking != Asking::Nobody
            && !quiet
            && (part
                || if caught {
                    settings.general.ask_caught_downloads
                } else {
                    settings.general.ask_panel_downloads
                });
        if ask && asking == Asking::Busy {
            return Route::Nothing;
        }
        // A row waiting for a fresh link takes the next download caught in
        // the browser as it, rather than a row of its own.
        if let Some((id, since)) = self.awaiting_link {
            if now.duration_since(since) > AWAIT_LINK_FOR {
                self.stop_awaiting_link();
            } else {
                let next = edm_core::lock(&backend.handed)
                    .front()
                    .is_some_and(|r| r.links.is_empty());
                if next {
                    let request = edm_core::lock(&backend.handed).pop_front();
                    return match request {
                        Some(request) => {
                            self.stop_awaiting_link();
                            Route::FreshLink {
                                id,
                                url: request.url,
                            }
                        }
                        None => Route::Nothing,
                    };
                }
            }
        }
        let Some(request) = edm_core::lock(&backend.handed).pop_front() else {
            return Route::Nothing;
        };
        // The same address handed over again within a few seconds is one
        // click, not two: a double-click on a link in the browser saved
        // `setup (1).deb` and `setup (2).deb`.
        if request.links.is_empty() && self.recent.just_taken(&request.url, now) {
            return Route::Again;
        }
        // A page's links: the "Download all links" dialog, or straight into
        // the main queue with nobody to ask.
        if !request.links.is_empty() {
            if ask {
                return Route::AskLinks(request);
            }
            return Route::AddLinks(links_of(&request));
        }
        // A download caught in the browser for an address already listed is
        // not fetched a second time unasked: the person is shown the row and
        // chooses "Download again". Only with a window to ask in.
        if request.caught && asking != Asking::Nobody {
            if let Some(id) = worth_keeping(rows, &request.url) {
                self.recent.taken(&request.url, now);
                return Route::Listed {
                    url: request.url,
                    id,
                    how: Dup::from_word(&settings.duplicate_choice),
                };
            }
        }
        if ask {
            return Route::Ask(request);
        }
        let added = backend.add_with(&request.url, crate::shell::add_options(&request));
        if added.is_ok() {
            self.recent.taken(&request.url, now);
        }
        Route::Added(added)
    }
}

/// A page's links for the main queue, each with its words on the page as its
/// description (as the dialog does by default) and the name its `download`
/// attribute gives.
fn links_of(request: &Request) -> Vec<(String, edm_core::AddOptions)> {
    let with = edm_core::AddOptions {
        later: true,
        queue: edm_core::MAIN_QUEUE.into(),
        referrer: request.referrer.clone(),
        ..Default::default()
    };
    request
        .links
        .iter()
        .map(|url| {
            let own = edm_core::AddOptions {
                description: request.texts.get(url).cloned().unwrap_or_default(),
                save_name: request.names.get(url).cloned(),
                ..with.clone()
            };
            (url.clone(), own)
        })
        .collect()
}

/// How many hand-overs in a row from a site the person cancelled before
/// they are asked whether that site's downloads should stay with the
/// browser.
pub const CANCELS_BEFORE_ASKING: u32 = 2;

/// Hand-overs cancelled in Download details, by site. At
/// [`CANCELS_BEFORE_ASKING`] the person is asked whether the site's
/// downloads should stay with the browser, and the count starts again; a
/// site already left there is not counted.
#[derive(Debug, Default)]
pub struct Cancels(std::collections::HashMap<String, u32>);

impl Cancels {
    /// A hand-over from `site` was cancelled; `left` is the sites already
    /// left with the browser. Whether to ask about `site` now.
    pub fn cancelled(&mut self, site: &str, left: &[String]) -> bool {
        if left.iter().any(|h| h == site) {
            return false;
        }
        let count = self.0.entry(site.to_string()).or_insert(0);
        *count += 1;
        if *count >= CANCELS_BEFORE_ASKING {
            *count = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests;
