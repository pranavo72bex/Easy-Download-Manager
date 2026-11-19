// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Downloads added: from an address, from outside, in a batch, a hand-off given up, and a site
//! search's finds.

use super::*;

impl Downloads {
    /// Adds `urls` with the options Download details chose, as the person's
    /// own; the rows are read again.
    pub fn add(
        &mut self,
        urls: Vec<String>,
        with: dm_core::AddOptions,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let later = with.later;
        let items = urls.into_iter().map(|u| (u, with.clone())).collect();
        let added = self.session.add(&self.backend, items);
        if let Some(id) = added.rows.first().map(|(id, _, _)| *id) {
            // The new row is the one chosen, as the person expects of a row
            // just added.
            self.list.selected.clear();
            self.list.selected.insert(id);
        }
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => dm_desktop_base::i18n::tf("One of the links was not added: {}", &[&e]),
                n => dm_desktop_base::i18n::tf("{} of the links were not added: {}", &[&n, &e]),
            });
        }
        self.read();
        // Its progress window opens as it starts, where that is asked for
        // (B63), for one download at a time.
        if let [(id, _, false)] = added.rows.as_slice() {
            if self.backend.settings().general.show_progress {
                self.show_progress(*id, cx);
            }
        }
        // Start now while the schedule or the download limit holds
        // downloads: asked, as Resume asks, whether to start it outside the
        // schedule (asked of the schedule itself: the list, just read, may
        // hold no row waiting on it yet).
        let settings = self.backend.settings();
        let shut = settings.schedule.enabled && !settings.schedule.open();
        if !later && (shut || self.backend.allowance_spent().is_some()) {
            let ids = added.rows.iter().map(|(id, _, _)| *id).collect();
            self.ask_outside(ids, cx);
        }
        cx.notify();
        added.outcome()
    }

    /// "Start outside the schedule?" for the rows `ids` the schedule or the
    /// download limit holds: Start now starts them this once; Keep waiting
    /// leaves them.
    pub fn ask_outside(&mut self, ids: Vec<dm_core::Id>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let settings = self.backend.settings();
        let mut categories = settings.categories();
        // The name the list shows: "big.zip" was asked about while the row
        // read "big (1).zip".
        let names: Vec<String> = ids
            .iter()
            .filter_map(|id| self.rows.iter().find(|r| r.id == *id))
            .map(|row| {
                dm_shell::app::names::name_it_will_get_after(
                    &self.rows,
                    row,
                    &settings.folders.download_dir,
                    &mut categories,
                    None,
                )
            })
            .collect();
        let why = dm_desktop_base::held::held_because(&settings, self.backend.allowance_spent());
        let ask = dm_desktop_base::asks::outside(&names, why);
        let asked = crate::dialogs::question::Asked {
            title: ask.title.into(),
            lines: std::iter::once(ask.sentence)
                .chain(ask.why)
                .chain(std::iter::once(ask.question.to_string()))
                .map(Into::into)
                .collect(),
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            // Keep waiting beside Start now, the default, rightmost.
            buttons: vec![ask.start.into(), ask.wait.into()],
            default: 0,
            escape: 1,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, move |view, cx| {
                // Off the window's thread, as the toolbar's Resume is.
                view.later_then_read(
                    move |backend| {
                        for id in &ids {
                            backend.start_outside_schedule(*id);
                        }
                    },
                    cx,
                );
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Adds downloads with options of their own (Download links' each has
    /// its name, its description, its sign-in), as the person's own; how
    /// many went in.
    pub fn add_items(
        &mut self,
        items: Vec<(String, dm_core::AddOptions)>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        self.add_items_sized(items, &Default::default(), cx)
    }

    /// [`Self::add_items`], with the sizes a dialog already learned for some of the links,
    /// shown on their rows until each learns its own.
    pub fn add_items_sized(
        &mut self,
        items: Vec<(String, dm_core::AddOptions)>,
        sizes: &std::collections::HashMap<String, u64>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let added = self.session.add(&self.backend, items);
        for (id, url, _) in &added.rows {
            if let Some(size) = sizes.get(url) {
                self.known.sizes_told.insert(*id, *size);
            }
        }
        if let Some((failed, e)) = added.partly() {
            // The rest went in; the ones that did not are said, not lost.
            self.say(match failed {
                1 => dm_desktop_base::i18n::tf("One of the links was not added: {}", &[&e]),
                n => dm_desktop_base::i18n::tf("{} of the links were not added: {}", &[&n, &e]),
            });
        }
        self.read();
        cx.notify();
        added.outcome()
    }

    /// A handed-over download from `site` was cancelled: the second asks
    /// whether to leave that site's downloads to the browser (counted by the
    /// application, `handoff::Cancels`).
    pub fn hand_off_cancelled(&mut self, site: &str, cx: &mut Context<Self>) {
        let settings = self.backend.settings();
        if !self
            .questions
            .cancelled_hand_offs
            .cancelled(site, &settings.browser.catch.skip_hosts)
        {
            return;
        }
        let asked = crate::dialogs::question::Asked {
            title: dm_desktop_base::i18n::t("Leave this site to the browser?").into(),
            lines: vec![dm_desktop_base::i18n::tf(
                "You cancelled two downloads from {}. Leave its downloads to the browser?",
                &[&site],
            )
            .into()],
            named: None,
            detail: None,
            toggle: None,
            ticked: false,
            buttons: vec![
                dm_desktop_base::i18n::t("Leave them to the browser").into(),
                dm_desktop_base::i18n::t("Keep handing them over").into(),
            ],
            default: 0,
            escape: 1,
        };
        let site = site.to_string();
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, _, cx| {
            if button != 0 {
                return;
            }
            let _ = view.update(cx, |view, _| {
                let mut wanted = view.backend.settings();
                if !wanted.browser.catch.skip_hosts.contains(&site) {
                    wanted.browser.catch.skip_hosts.push(site);
                }
                if let Err(e) = view.backend.set_settings(wanted) {
                    view.status.problems.push(e);
                }
            });
        });
        self.sheet(asked, None, None, answer, cx);
    }

    /// Adds what a search found. With `keep_layout` each file goes in a
    /// folder under the download folder that mirrors its address, one add
    /// for each folder; otherwise all of them in one.
    pub fn add_found(
        &mut self,
        urls: Vec<String>,
        with: dm_core::AddOptions,
        keep_layout: bool,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        if !keep_layout {
            let items = urls.into_iter().map(|u| (u, with.clone())).collect();
            return self.add_items(items, cx);
        }
        let root = self.backend.settings().folders.download_dir;
        let mut items = Vec::new();
        for (folder, group) in dm_shell::app::search::by_site_folder(&urls, &root) {
            let mut here = with.clone();
            if folder.is_some() {
                here.save_dir = folder;
            }
            items.extend(group.into_iter().map(|u| (u, here.clone())));
        }
        self.add_items(items, cx)
    }

    /// Runs a saved search now, in the background; what it finds is taken
    /// in on a later wake (`take_searches`).
    pub fn run_search(&mut self, search: dm_shell::lists::Search) {
        let mut search = search;
        // The login Options › Sign-ins keeps for the site, where it keeps one.
        search.page = self.backend.settings().with_login(&search.page);
        let results = self.threads.search_results.clone();
        let name = search.name.clone();
        dm_shell::app::search::crawl_later(&self.backend, search, move |found| {
            dm_core::lock(&results).push((name, found));
            dm_shell::app::changes::changed();
        });
    }

    /// Takes in what finished saved searches found (their new files added
    /// for later, said; what they have seen kept), and starts any saved
    /// search whose time has come, looked at once a minute.
    pub(crate) fn take_searches(&mut self, cx: &mut Context<Self>) {
        use dm_desktop_base::i18n::{tf, tn};
        let done: Vec<_> = std::mem::take(&mut *dm_core::lock(&self.threads.search_results));
        for (name, found) in done {
            let settings = self.backend.settings();
            match found {
                Ok(found) => {
                    let saved = settings.site_searches.iter().find(|s| s.name == name);
                    let seen: Vec<String> = saved.map(|s| s.seen.clone()).unwrap_or_default();
                    let keep_layout = saved.is_some_and(|s| s.keep_layout);
                    // New: neither in the list nor found by this search
                    // before (`dm_shell::app::search`).
                    let finds = dm_shell::app::search::sort_finds(&found.files, &self.rows, &seen);
                    let new = finds.new;
                    let mut remember = finds.listed;
                    if new.is_empty() {
                        self.say(tf("The search {} found nothing new.", &[&name]));
                    } else {
                        let with = dm_core::AddOptions {
                            later: true,
                            queue: dm_core::MAIN_QUEUE.into(),
                            ..Default::default()
                        };
                        let added = self.add_found(new.clone(), with, keep_layout, cx);
                        let now = dm_shell::app::search::listed(&self.rows);
                        remember.extend(
                            new.iter()
                                .map(|f| dm_shell::lists::bare(f))
                                .filter(|f| now.contains(f)),
                        );
                        match added {
                            Ok(n) => self.say(tn(
                                "Added {} download to {}.||Added {} downloads to {}.",
                                n as u64,
                                &[&n, &dm_desktop_base::i18n::name(dm_core::MAIN_QUEUE)],
                            )),
                            Err(e) => self.status.problems.push(tf(
                                "the search {} found files that could not be added: {}",
                                &[&name, &e],
                            )),
                        }
                    }
                    let settings = self.backend.settings();
                    if let Some(wanted) =
                        dm_shell::app::search::seen_saved(&settings, &name, remember)
                    {
                        if let Err(e) = self.backend.set_settings(wanted) {
                            self.status.problems.push(tf(
                                "what the search {} has seen could not be saved: {}",
                                &[&name, &e],
                            ));
                        }
                    }
                }
                Err(e) => self
                    .status
                    .problems
                    .push(tf("the search {} failed: {}", &[&name, &e])),
            }
        }
        let now = std::time::Instant::now();
        if self
            .threads
            .searches_checked
            .is_some_and(|at| now.duration_since(at) < SEARCHES_EVERY)
        {
            return;
        }
        self.threads.searches_checked = Some(now);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let settings = self.backend.settings();
        let Some((due, wanted)) = dm_shell::app::search::due_now(&settings, epoch) else {
            return;
        };
        if let Err(e) = self.backend.set_settings(wanted) {
            // Still run, and not again for a minute; but said, since an
            // unsaved last run means the search runs again at every launch.
            self.status.problems.push(dm_desktop_base::i18n::tf(
                "when the saved searches last ran could not be saved: {}",
                &[&e],
            ));
        }
        for search in due {
            self.run_search(search);
        }
    }
}
