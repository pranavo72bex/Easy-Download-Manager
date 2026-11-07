// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::family::{same_family, same_path};
use super::*;

/* --------------------------------------------------------------------- row verbs
 *
 * What the File and Downloads menus do to a row, beyond pause and resume:
 * a description, named queues, a folder and name chosen before the start,
 * a fresh address for an expired link, a download started again from
 * nothing, the order rows wait in, and how each connection is getting on.
 */
impl Queue {
    /// Adds a download with what the "Download details" dialog chose.
    pub async fn add_with(&self, url: &str, with: AddOptions) -> Option<Id> {
        if with.fresh_link_ok {
            if let Some(id) = self.adopt_fresh_link(url, &with).await {
                return Some(id);
            }
        }
        let id = self.add_unsaved(url, with).await;
        if id.is_some() {
            self.kick();
            self.save().await;
        }
        id
    }

    /// A fresh address from the browser for a file whose row waits for one
    /// (`Entry::fresh_link`): the row takes the new address and goes on from
    /// the parts it has, instead of the same file being added as another
    /// row. It must be the same file: the same name, the very same size
    /// (a name alone is not enough: another build of a program has the
    /// name), and a server of the same family as the old address. Nothing is
    /// asked of the network while no row waits. `None` where nothing matched;
    /// the caller adds the address as it would have.
    pub async fn adopt_fresh_link(&self, url: &str, with: &AddOptions) -> Option<Id> {
        let url = url.trim();
        if !accepted(url) {
            return None;
        }
        let all_waiting: Vec<(Id, String, String, Option<u64>)> = {
            let slots = self.inner.slots.lock().await;
            slots
                .values()
                .filter(|s| s.entry.fresh_link && matches!(s.entry.state, State::Failed(_)))
                .map(|s| {
                    (
                        s.entry.id,
                        s.entry.url.clone(),
                        s.entry.filename.clone(),
                        s.entry.progress.total,
                    )
                })
                .filter(|(_, old, ..)| same_family(old, url))
                .collect()
        };
        // A row that failed at its very first request never learned a name or a size, so those
        // cannot be compared. Its signed address and the fresh one differ in the query alone:
        // the same server family and the very same path is the same file.
        if let Some((id, ..)) = all_waiting.iter().find(|(_, old, name, total)| {
            (name.is_empty() || total.is_none()) && same_path(old, url)
        }) {
            let id = *id;
            if self.set_url(id, url).await {
                self.take_hand_over(id, with).await;
                return self.retry(id).await.then_some(id);
            }
            return None;
        }
        let waiting: Vec<(Id, String, String, u64)> = all_waiting
            .into_iter()
            .filter_map(|(id, old, name, total)| Some((id, old, name, total?)))
            .filter(|(_, _, name, _)| !name.is_empty())
            .collect();
        if waiting.is_empty() {
            return None;
        }
        let client = self.inner.client.lock().await.clone();
        let sign_in = crate::cookies::SignIn::with_headers(url, &with.cookies, &with.headers);
        let referrer = Some(with.referrer.trim()).filter(|r| !r.is_empty());
        let probed = crate::probe::probe(&client, url, referrer, sign_in.as_ref())
            .await
            .ok()?;
        let (id, ..) = waiting.into_iter().find(|(_, _, name, total)| {
            name.eq_ignore_ascii_case(&probed.filename) && probed.size == Some(*total)
        })?;
        if !self.set_url(id, url).await {
            return None;
        }
        self.take_hand_over(id, with).await;
        self.retry(id).await.then_some(id)
    }

    /// What the fresh hand-over brings to the row that takes it: the page, the sign-in and the
    /// request headers it came with; and the row no longer waits for one.
    async fn take_hand_over(&self, id: Id, with: &AddOptions) {
        self.edit(id, |slot| {
            if !with.referrer.trim().is_empty() {
                slot.entry_mut().referrer = with.referrer.trim().to_string();
            }
            if !with.cookies.is_empty() {
                slot.entry_mut().cookies.clone_from(&with.cookies);
            }
            if !with.headers.is_empty() {
                slot.entry_mut().headers = crate::cookies::forwardable(&with.headers);
            }
            slot.entry_mut().fresh_link = false;
            true
        })
        .await;
    }

    /// Adds every one of `items`, each with its own options, and writes the
    /// queue once. One at a time it was written, and synced to disk, once a
    /// link: two thousand links from a page were two thousand rewrites of a
    /// growing file, with the window waiting.
    pub async fn add_each(&self, items: Vec<(String, AddOptions)>) -> Vec<Option<Id>> {
        let mut ids = Vec::with_capacity(items.len());
        for (url, with) in items {
            ids.push(self.add_unsaved(&url, with).await);
        }
        if ids.iter().any(Option::is_some) {
            self.kick();
            self.save().await;
        }
        ids
    }

    pub(super) async fn add_unsaved(&self, url: &str, with: AddOptions) -> Option<Id> {
        let url = url.trim();
        if !accepted(url) {
            return None;
        }
        let id = self.insert(url, with.referrer.trim().to_string()).await;
        let mut own_limit = None;
        if let Some(slot) = self.inner.slots.lock().await.get_mut(&id) {
            if with.speed_limit > 0 {
                slot.entry_mut().speed_limit = with.speed_limit;
                own_limit = Some(slot.own_limit.clone());
            }
            slot.entry_mut().description = with.description;
            slot.entry_mut().queue = with.queue;
            slot.entry_mut().save_dir = with.save_dir.filter(|d| !d.trim().is_empty());
            slot.entry_mut().save_name = with.save_name.filter(|n| !n.trim().is_empty());
            slot.entry_mut().title_hint = with.title_hint.filter(|t| !t.trim().is_empty());
            slot.entry_mut().quality = with.quality.filter(|q| !q.trim().is_empty());
            slot.entry_mut().sound = with
                .sound
                .map(|s| s.trim().to_string())
                .filter(|s| s.starts_with("https://") || s.starts_with("http://"));
            slot.entry_mut().span = with.span;
            // A shape only for a part, which is cut from MP4 streams.
            slot.entry_mut().shape = with.shape.filter(|_| with.span.is_some());
            slot.entry_mut().mp3 = with.mp3;
            slot.entry_mut().gif = with.gif && with.span.is_some();
            slot.entry_mut().delete_after = with.delete_after.filter(|s| *s > 0);
            slot.entry_mut().cookies = with.cookies.trim().to_string();
            slot.entry_mut().headers = crate::cookies::forwardable(&with.headers);
            slot.entry_mut().cookies_once = with.cookies_once
                && !(slot.entry.cookies.is_empty() && slot.entry.headers.is_empty());
            slot.entry_mut().replaces = with.replaces.filter(|p| !p.trim().is_empty());
            // A form's answer: the form for its own address only.
            if let Some(post) = with.post.filter(|p| p.url == url) {
                slot.entry_mut().posted = true;
                slot.entry_mut().post = Some(post);
            }
            if with.checksum.is_some() {
                slot.entry_mut().checksum = with.checksum;
            }
            if with.later {
                slot.hold(false);
            }
        }
        if let Some(limiter) = own_limit {
            limiter
                .set_rate(with.speed_limit.saturating_mul(1024))
                .await;
        }
        Some(id)
    }

    /// Changes something about a row and writes the queue, if the row exists.
    pub(super) async fn edit(&self, id: Id, change: impl FnOnce(&mut Slot) -> bool) -> bool {
        let changed = match self.inner.slots.lock().await.get_mut(&id) {
            Some(slot) => change(slot),
            None => false,
        };
        if changed {
            self.save().await;
        }
        changed
    }

    /// Sets the row's description; false if there is no such row.
    pub async fn set_description(&self, id: Id, text: &str) -> bool {
        let text = text.to_string();
        self.edit(id, |slot| {
            slot.entry_mut().description = text;
            true
        })
        .await
    }

    /// "Delete file after": its file goes to the Trash, and the row with
    /// it, `seconds` after it finished (at once, for one finished longer
    /// ago); `None` keeps it.
    pub async fn set_delete_after(&self, id: Id, seconds: Option<u64>) -> bool {
        let seconds = seconds.filter(|s| *s > 0);
        self.edit(id, |slot| {
            slot.entry_mut().delete_after = seconds;
            true
        })
        .await
    }

    /// Puts a row in a named queue, or takes it out with an empty name.
    pub async fn set_queue(&self, id: Id, queue: &str) -> bool {
        let queue = queue.trim().to_string();
        self.edit(id, |slot| {
            slot.entry_mut().queue = queue;
            true
        })
        .await
    }

    /// Which of a torrent's files are fetched, one tick a file in its
    /// order. Only while it is not running or finished, and never none: a
    /// person stops it, chooses, and starts it again.
    pub async fn set_torrent_files(&self, id: Id, wanted: &[bool]) -> Result<(), String> {
        if !wanted.iter().any(|w| *w) {
            return Err("Tick at least one file.".into());
        }
        let mut why = None;
        let wanted = wanted.to_vec();
        let done = self
            .edit(id, |slot| {
                if matches!(slot.entry.state, State::Running | State::Completed) {
                    why = Some("Stop the download to change which files are fetched.");
                    return false;
                }
                if slot.entry.torrent_files.len() != wanted.len() {
                    why = Some("The torrent's files are not known yet.");
                    return false;
                }
                for (file, on) in slot.entry_mut().torrent_files.iter_mut().zip(&wanted) {
                    file.wanted = *on;
                }
                true
            })
            .await;
        match (done, why) {
            (true, _) => Ok(()),
            (false, Some(why)) => Err(why.into()),
            (false, None) => Err("There is no such download.".into()),
        }
    }

    /// Sets when each of a torrent's files is fetched (gaps row 149): only
    /// while it is not running, as the choice of files is, and one priority
    /// for each file listed.
    pub async fn set_torrent_priorities(
        &self,
        id: Id,
        priorities: &[crate::torrent::Priority],
    ) -> Result<(), String> {
        let mut why = None;
        let priorities = priorities.to_vec();
        let done = self
            .edit(id, |slot| {
                if matches!(slot.entry.state, State::Running | State::Completed) {
                    why = Some("Stop the download to change which files are fetched.");
                    return false;
                }
                if slot.entry.torrent_files.len() != priorities.len() {
                    why = Some("The torrent's files are not known yet.");
                    return false;
                }
                for (file, p) in slot.entry_mut().torrent_files.iter_mut().zip(&priorities) {
                    file.priority = *p;
                }
                true
            })
            .await;
        match (done, why) {
            (true, _) => Ok(()),
            (false, Some(why)) => Err(why.into()),
            (false, None) => Err("There is no such download.".into()),
        }
    }

    /// "Update download link": a new link for a download whose
    /// old one expired. Only while it is not running, and only to something
    /// dm can fetch. The parts stay; the plan recorded beside them, with the
    /// file's validator, decides whether they belong to the new link's file.
    pub async fn set_url(&self, id: Id, url: &str) -> bool {
        let url = url.trim().to_string();
        if !accepted(&url) {
            return false;
        }
        self.edit(id, |slot| {
            if slot.entry.state == State::Running || slot.entry.state == State::Completed {
                return false;
            }
            // A sign-in is for the server it came from: an address on
            // another one does not get it.
            let same_server = crate::cookies::SignIn::new(&slot.entry.url, "x=1")
                .and_then(|s| s.for_url(&url).map(|_| ()))
                .is_some();
            if !same_server {
                slot.entry_mut().cookies.clear();
                slot.entry_mut().headers.clear();
            }
            slot.entry_mut().url = url;
            true
        })
        .await
    }

    /// "Redownload": the same address fetched again from nothing.
    pub async fn redownload(&self, id: Id) -> bool {
        self.fetch_again(id, None).await
    }

    /// Fetched again from nothing; with `replacing`, the finished file the
    /// row keeps pointing at until the new version is whole and takes its
    /// place.
    pub(super) async fn fetch_again(&self, id: Id, replacing: Option<String>) -> bool {
        let reset = {
            let mut slots = self.inner.slots.lock().await;
            match slots.get_mut(&id) {
                Some(slot) => {
                    slot.control.cancel();
                    slot.requeue();
                    slot.entry_mut().progress = Progress {
                        downloaded: 0,
                        total: None,
                        speed: 0.0,
                        eta: None,
                    };
                    slot.entry_mut().path = replacing.clone().unwrap_or_default();
                    slot.entry_mut().replaces = replacing;
                    slot.entry_mut().verified = false;
                    true
                }
                None => false,
            }
        };
        if reset {
            self.discard_parts(id).await;
            self.kick();
            self.save().await;
        }
        reset
    }

    /// Moves a row up (negative) or down the order the queue is served in.
    pub async fn shift(&self, id: Id, by: isize) -> bool {
        let moved = {
            let mut order = self.inner.order.lock().await;
            match order.iter().position(|x| *x == id) {
                Some(at) => {
                    let to = (at as isize + by).clamp(0, order.len() as isize - 1) as usize;
                    let row = order.remove(at);
                    order.insert(to, row);
                    to != at
                }
                None => false,
            }
        };
        if moved {
            self.save().await;
        }
        moved
    }
}
