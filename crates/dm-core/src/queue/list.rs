// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The list made (its rows read back from its store) and downloads added to it.

use super::*;

impl DownloadList {
    /// A queue that downloads with `client` and `options`, `concurrency` files at a time.
    pub fn new(client: reqwest::Client, options: Options, concurrency: usize) -> Self {
        DownloadList {
            inner: Arc::new(Inner {
                slots: Mutex::new(HashMap::new()),
                order: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                options: std::sync::Mutex::new(options),
                store: std::sync::Mutex::new(None),
                saving: Mutex::new(()),
                slow_writes_ms: AtomicU64::new(0),
                written: std::sync::Mutex::new(None),
                unwritten: AtomicBool::new(false),
                trailing: AtomicBool::new(false),
                last_write_ms: std::sync::atomic::AtomicU64::new(0),
                problems: std::sync::Mutex::new(Vec::new()),
                leaving: std::sync::Mutex::new(Vec::new()),
                scheduling: AtomicBool::new(false),
                closing: AtomicBool::new(false),
                asks_same: AtomicBool::new(false),
                kick: tokio::sync::Notify::new(),
                rechecking: AtomicBool::new(false),
                concurrency: AtomicUsize::new(concurrency.max(1)),
                queue_limits: std::sync::Mutex::new(HashMap::new()),
                schedule: std::sync::Mutex::new(Schedule::default()),
                queue_schedules: std::sync::Mutex::new(Default::default()),
                quota: std::sync::Mutex::new(crate::quota::Meter::default()),
                quota_saved: std::sync::Mutex::new(None),
                recheck: std::sync::Mutex::new((None, now_millis())),
                starts: std::sync::Mutex::new(QueueStarts::default()),
                client: std::sync::Mutex::new(client),
                client_made: std::sync::Mutex::new((None, crate::only_through::generation())),
                restarts_seen: AtomicU64::new(crate::only_through::restarts()),
                saves: AtomicU64::new(0),
                saved: std::sync::Mutex::new(None),
                changed: std::sync::Mutex::new(None),
                whole_next: AtomicBool::new(false),
                held_by: std::sync::Mutex::new(None),
            }),
        }
    }

    /// Where a row's parts are staged: the folder it was first started
    /// under, so a parts folder changed in the settings afterwards strands
    /// nothing; the current setting's for a row not yet started.
    pub(super) async fn parts_dir_of(&self, id: Id) -> PathBuf {
        let recorded = self
            .inner
            .slots
            .lock()
            .await
            .get(&id)
            .map(|s| s.entry.parts_dir.clone())
            .filter(|p| !p.is_empty());
        match recorded {
            Some(dir) => PathBuf::from(dir),
            None => crate::lock(&self.inner.options)
                .parts_dir
                .join(id.to_string()),
        }
    }

    /// Where the queue is written, if anywhere: copied out, the lock let go
    /// at once (`Inner::store`).
    pub(super) fn store_path(&self) -> Option<PathBuf> {
        crate::lock(&self.inner.store).clone()
    }

    /// Whether a scheduler is already serving this queue.
    ///
    /// Only worth asking to avoid starting one that would immediately find
    /// it had nothing to do.
    pub fn scheduling(&self) -> bool {
        self.inner.scheduling.load(Ordering::SeqCst)
    }

    /// Adds a URL and returns the id it will be known by, or `None` when that
    /// is not something dm can fetch.
    ///
    /// Checked here because this is the one door. The window checked and the
    /// browser hand-off did not, so anything a browser sent became a row that
    /// could only ever fail.
    pub async fn add(&self, url: &str) -> Option<Id> {
        let url = url.trim();
        if !accepted(url) {
            return None;
        }
        let id = self.insert(url, String::new()).await;
        self.save().await;
        Some(id)
    }

    /// Adds several at once, writing the queue once rather than once each.
    ///
    /// Pasting a list is the ordinary way people add more than one, and
    /// `save` serialises every row — so twenty URLs meant twenty writes of a
    /// file that grew with each.
    pub async fn add_all(&self, urls: &[&str]) -> Vec<Id> {
        let mut ids = Vec::new();
        for url in urls {
            let url = url.trim();
            if accepted(url) {
                ids.push(self.insert(url, String::new()).await);
            }
        }
        if !ids.is_empty() {
            self.save().await;
        }
        ids
    }

    pub(super) async fn insert(&self, url: &str, referrer: String) -> Id {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let slot = Slot {
            entry: Arc::new(Entry {
                id,
                url: url.to_string(),
                state: State::Queued,
                progress: Progress {
                    downloaded: 0,
                    total: None,
                    speed: 0.0,
                    eta: None,
                    estimated: false,
                },
                filename: String::new(),
                finished_at: None,
                category: None,
                checksum: None,
                verified: false,
                disguised: None,
                mirrors: 0,
                path: String::new(),
                referrer,
                paused_by_schedule: false,
                description: String::new(),
                queue: String::new(),
                save_dir: None,
                save_name: None,
                accept_page: false,
                validator: None,
                quality: None,
                sound: None,
                span: None,
                shape: None,
                mp3: false,
                gif: false,
                delete_after: None,
                unpack: false,
                parts_dir: String::new(),
                began_in: String::new(),
                cookies: String::new(),
                cookies_once: false,
                headers: Vec::new(),
                replaces: None,
                speed_limit: 0,
                stalled: String::new(),
                fresh_link: false,
                title_hint: None,
                torrent_files: Default::default(),
                said: String::new(),
                post: None,
                posted: false,
                fail_kind: None,
                keep_no_record: false,
                accept_same: false,
            }),
            control: Control::new(),
            task: None,
            outside_schedule: false,
            retrying: None,
            swarm: None,
            own_limit: crate::limit::Limiter::unlimited(),
        };

        self.inner.slots.lock().await.insert(id, slot);
        self.inner.order.lock().await.push(id);
        id
    }

    /// Where the recheck queue's last run is kept: beside the queue file.
    pub(super) async fn recheck_file(&self) -> Option<PathBuf> {
        Some(crate::paths::recheck_at(&self.store_path()?))
    }
}
