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
        let id = self.add_unsaved(url, with).await;
        if id.is_some() {
            self.kick();
            self.save().await;
        }
        id
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
                slot.entry.speed_limit = with.speed_limit;
                own_limit = Some(slot.own_limit.clone());
            }
            slot.entry.description = with.description;
            slot.entry.queue = with.queue;
            slot.entry.save_dir = with.save_dir.filter(|d| !d.trim().is_empty());
            slot.entry.save_name = with.save_name.filter(|n| !n.trim().is_empty());
            slot.entry.quality = with.quality.filter(|q| !q.trim().is_empty());
            slot.entry.sound = with
                .sound
                .map(|s| s.trim().to_string())
                .filter(|s| s.starts_with("https://") || s.starts_with("http://"));
            slot.entry.span = with.span;
            slot.entry.cookies = with.cookies.trim().to_string();
            slot.entry.cookies_once = with.cookies_once && !slot.entry.cookies.is_empty();
            slot.entry.replaces = with.replaces.filter(|p| !p.trim().is_empty());
            if with.checksum.is_some() {
                slot.entry.checksum = with.checksum;
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

    pub async fn set_description(&self, id: Id, text: &str) -> bool {
        let text = text.to_string();
        self.edit(id, |slot| {
            slot.entry.description = text;
            true
        })
        .await
    }

    /// Puts a row in a named queue, or takes it out with an empty name.
    pub async fn set_queue(&self, id: Id, queue: &str) -> bool {
        let queue = queue.trim().to_string();
        self.edit(id, |slot| {
            slot.entry.queue = queue;
            true
        })
        .await
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
                slot.entry.cookies.clear();
            }
            slot.entry.url = url;
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
                    slot.entry.progress = Progress {
                        downloaded: 0,
                        total: None,
                        speed: 0.0,
                        eta: None,
                    };
                    slot.entry.path = replacing.clone().unwrap_or_default();
                    slot.entry.replaces = replacing;
                    slot.entry.verified = false;
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
