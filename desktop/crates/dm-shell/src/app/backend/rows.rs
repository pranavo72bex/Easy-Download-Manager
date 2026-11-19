// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The row verbs: adding, changing and moving downloads, and lists in and out.

use dm_core::{AddOptions, Connection, Id};

use super::Backend;
use crate::{lists, shell};

/// How long a running row given a new address (`Backend::set_url`) is waited for to stop.
const STOP_FOR_ADDRESS_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// How often that wait looks at whether the row has stopped.
const STOP_FOR_ADDRESS_LOOK: std::time::Duration = std::time::Duration::from_millis(25);

impl Backend {
    /* ------------------------------------------------------------- row verbs */

    /// Adds with what the "Download details" dialog chose. Site logins
    /// are put on the address here, so every way in gets them.
    pub fn add_with(&self, url: &str, with: AddOptions) -> Result<Id, String> {
        let settings = self.settings();
        let with = settings.with_site_rule(url, admit(url, with)?);
        held_to_downloads(&with, &settings)?;
        let url = settings.with_login(url.trim());
        let later = with.later;
        let id = self
            .block_on(self.queue.add_with(&url, with))
            .ok_or_else(|| "that is not a URL dm can download".to_string())?;
        if !later {
            self.block_on(async { shell::wake(&self.queue) });
        }
        Ok(id)
    }

    /// Adds many at once, each with its options and a site's saved sign-in,
    /// writing the queue once. One result per address, in order.
    pub fn add_each(&self, items: Vec<(String, AddOptions)>) -> Vec<Result<Id, String>> {
        let settings = self.settings();
        // Each refused here is answered in its place; the rest are added in
        // one go, writing the queue once.
        let admitted: Vec<Result<(String, AddOptions), String>> = items
            .into_iter()
            .map(|(url, with)| {
                let with = settings.with_site_rule(&url, admit(&url, with)?);
                held_to_downloads(&with, &settings)?;
                Ok((settings.with_login(url.trim()), with))
            })
            .collect();
        let starts = admitted
            .iter()
            .any(|item| item.as_ref().is_ok_and(|(_, with)| !with.later));
        let ids = self.block_on(
            self.queue.add_each(
                admitted
                    .iter()
                    .filter_map(|item| item.as_ref().ok().cloned())
                    .collect(),
            ),
        );
        if starts {
            self.block_on(async { shell::wake(&self.queue) });
        }
        let mut ids = ids.into_iter();
        admitted
            .into_iter()
            .map(|item| {
                item?;
                ids.next()
                    .flatten()
                    .ok_or_else(|| "that is not a URL dm can download".to_string())
            })
            .collect()
    }

    /// Sets a download's description.
    pub fn set_description(&self, id: Id, text: &str) {
        self.block_on(self.queue.set_description(id, text));
    }

    /// The request headers a download's requests carry, from its next start on.
    pub fn set_headers(&self, id: Id, headers: Vec<(String, String)>) {
        self.block_on(self.queue.set_headers(id, headers));
    }

    /// "Delete file after" for a row; `None` keeps its file.
    pub fn set_delete_after(&self, id: Id, seconds: Option<u64>) {
        self.block_on(self.queue.set_delete_after(id, seconds));
    }

    /// "Unpack when done" for this download alone (gaps row W14).
    pub fn set_unpack(&self, id: Id, on: bool) {
        self.block_on(self.queue.set_unpack(id, on));
    }

    /// What a finished download says of itself beside its state: one of `dm_core::NOTES`,
    /// in English (what "Unpack when done" made of its archive).
    pub fn set_said(&self, id: Id, said: &str) {
        self.block_on(self.queue.set_said(id, said));
    }

    /// Puts a download in the queue of this name.
    pub fn set_queue(&self, id: Id, queue: &str) {
        self.block_on(self.queue.set_queue(id, queue));
    }

    /// Gives a download a new address.
    pub fn set_url(&self, id: Id, url: &str) -> Result<(), String> {
        let url = self.settings().with_login(url.trim());
        if !dm_core::accepted(&url) {
            return Err("it must be a link dm can download".into());
        }
        // A download retrying between attempts is the one most likely to
        // need a new address: stopped, given it, and started
        // again, rather than refused as running.
        let was_running = self
            .block_on(self.queue.get(id))
            .is_some_and(|row| row.state == dm_core::State::Running);
        if was_running {
            self.pause(id);
            let deadline = std::time::Instant::now() + STOP_FOR_ADDRESS_WITHIN;
            while std::time::Instant::now() < deadline
                && self
                    .block_on(self.queue.get(id))
                    .is_some_and(|row| row.state == dm_core::State::Running)
            {
                std::thread::sleep(STOP_FOR_ADDRESS_LOOK);
            }
        }
        let changed = self.block_on(self.queue.set_url(id, &url));
        if was_running {
            self.resume(id);
        }
        if changed {
            Ok(())
        } else {
            Err(
                "a running or finished download's address cannot be changed, \
                 and it must be a link dm can download"
                    .into(),
            )
        }
    }

    /// Downloads a file again from the start.
    pub fn redownload(&self, id: Id) {
        self.block_on(async {
            if self.queue.redownload(id).await {
                shell::wake(&self.queue);
            }
        });
    }

    /// Moves a download up or down its queue by this many places.
    pub fn shift(&self, id: Id, by: isize) {
        self.block_on(self.queue.shift(id, by));
    }

    /// Starts a queue and gives how many downloads it started.
    pub fn start_queue(&self, queue: &str) -> usize {
        self.block_on(async {
            let started = self.queue.start_queue(queue).await;
            if started > 0 {
                shell::wake(&self.queue);
            }
            started
        })
    }

    /// Stops a queue and gives how many downloads it stopped.
    pub fn stop_queue(&self, queue: &str) -> usize {
        self.block_on(self.queue.stop_queue(queue))
    }

    /// Renames a finished download's file.
    pub fn rename(&self, id: Id, name: &str) -> Result<(), String> {
        self.block_on(self.queue.rename(id, name))
    }

    /// Moves a finished file into `dir`; see `DownloadList::move_to`.
    /// A folder typed as a relative path is refused, as a new download's is
    /// (`admit`): it was taken against wherever dm was started, and the file
    /// went there.
    pub fn move_to(&self, id: Id, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let dir = full_folder(&dir.to_string_lossy())?;
        if let Some(why) =
            crate::app::control_api::folder_refused(&dir, &self.settings().folders.download_dir)
        {
            return Err(why);
        }
        self.block_on(self.queue.move_to(id, &dir))
    }

    /// Move/Rename for a download that failed or is stopped: given the name and the folder
    /// asked for (a folder checked as `move_to` checks it), and tried again where it failed or
    /// stalled (`DownloadList::place_unfinished`).
    pub fn place_unfinished(
        &self,
        id: Id,
        dir: Option<&std::path::Path>,
        name: Option<String>,
    ) -> Result<(), String> {
        let dir = match dir {
            Some(dir) => {
                let dir = full_folder(&dir.to_string_lossy())?;
                let root = self.settings().folders.download_dir;
                if let Some(why) = crate::app::control_api::folder_refused(&dir, &root) {
                    return Err(why);
                }
                Some(dir)
            }
            None => None,
        };
        self.login_now(id);
        self.block_on(async {
            self.queue.place_unfinished(id, dir, name).await?;
            shell::wake(&self.queue);
            Ok(())
        })
    }

    /// A playable file of what a download has fetched so far.
    pub fn preview(&self, id: Id) -> Result<std::path::PathBuf, String> {
        self.block_on(self.queue.preview(id))
    }

    /// A download's connections, each with its state.
    #[must_use]
    pub fn connections(&self, id: Id) -> Vec<Connection> {
        self.block_on(self.queue.connections(id))
    }

    /// Whether a download can resume, if that is known yet.
    #[must_use]
    pub fn resumable(&self, id: Id) -> Option<bool> {
        self.block_on(self.queue.resumable(id))
    }

    /// The names of the queues.
    #[must_use]
    pub fn queues(&self) -> Vec<String> {
        self.block_on(self.queue.queues())
    }

    /// Every download, as a `.dmlist` file.
    pub fn export(&self) -> String {
        let rows: Vec<lists::Listed> = self
            .list()
            .into_iter()
            // Without a login in the address: a list file is shared and
            // kept, and a password is not something to leave in it. Options
            // › Sign-ins puts the login back on import.
            // What a person gave the row goes with it: a list that came
            // back without its queues, notes, names and digests was a list
            // of addresses, and the rest had to be done again by hand.
            .map(|e| lists::Listed {
                url: without_login(&e.url),
                referrer: e.referrer,
                queue: e.queue,
                note: e.description,
                name: e.save_name.unwrap_or_default(),
                checksum: e
                    .checksum
                    .map(|c| format!("{}:{}", c.algorithm.name(), c.expected))
                    .unwrap_or_default(),
            })
            .collect();
        lists::to_list(&rows)
    }

    /// Adds what a `.dmlist` file or a plain list holds, saying how many.
    pub fn import(&self, text: &str) -> usize {
        self.import_where(text, |_| true).len()
    }

    /// The addresses a list holds, in its order.
    pub fn listed(text: &str) -> Vec<String> {
        lists::from_list(text)
            .into_iter()
            .map(|row| row.url)
            .collect()
    }

    /// [`Self::import`], of only the rows whose address `keep` keeps; the
    /// rows added.
    pub fn import_where(&self, text: &str, keep: impl Fn(&str) -> bool) -> Vec<Id> {
        let mut rows = lists::from_list(text);
        rows.retain(|row| keep(&row.url));
        let items = rows
            .iter()
            .map(|row| {
                let with = AddOptions {
                    referrer: row.referrer.clone(),
                    queue: row.queue.clone(),
                    description: row.note.clone(),
                    save_name: (!row.name.is_empty()).then(|| row.name.clone()),
                    // Held until its digest is on it: started at once, it
                    // could be fetched and finished before the check was
                    // there to be made.
                    later: !row.checksum.is_empty(),
                    ..AddOptions::default()
                };
                (row.url.clone(), with)
            })
            .collect();
        let added = self.add_each(items);
        let mut held = false;
        for (row, id) in rows.iter().zip(&added) {
            let Ok(id) = id else { continue };
            if row.checksum.is_empty() {
                continue;
            }
            // A digest that does not read is left off rather than failing
            // the row: the download is still the one listed.
            let _ = self.set_checksum(*id, &row.checksum);
            self.block_on(self.queue.resume(*id));
            held = true;
        }
        if held {
            self.block_on(async { shell::wake(&self.queue) });
        }
        added.into_iter().filter_map(Result::ok).collect()
    }
}

/// A download as the queue may keep it, or why not: an address with no
/// site in it, or a folder that is not a full path. `~` is the home folder;
/// a relative folder is refused, because a row keeps it and Open and Open
/// folder would read it later from wherever dm was started — `/` from the
/// Dock — and `~/x` made a folder called `~`.
fn admit(url: &str, mut with: AddOptions) -> Result<AddOptions, String> {
    if let Some(why) = shell::unusable(url) {
        return Err(why);
    }
    if let Some(typed) = with.save_dir.take().filter(|f| !f.trim().is_empty()) {
        with.save_dir = Some(full_folder(&typed)?.display().to_string());
    }
    Ok(with)
}

/// A chosen folder held to the download folder while a script drives the window
/// (`control_api::folder_refused`).
fn held_to_downloads(with: &AddOptions, settings: &crate::Settings) -> Result<(), String> {
    match with.save_dir.as_deref() {
        Some(folder) => crate::app::control_api::folder_refused(
            std::path::Path::new(folder),
            &settings.folders.download_dir,
        )
        .map_or(Ok(()), Err),
        None => Ok(()),
    }
}

/// A folder typed for a row, as a full path (`~` is the home folder), or why not.
fn full_folder(typed: &str) -> Result<std::path::PathBuf, String> {
    crate::settings::full_folder(typed, None).ok_or_else(|| {
        format!("the folder must be a full path, such as ~/Downloads, not {typed:?}")
    })
}

/// An address with no user or password in it.
fn without_login(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() || parsed.password().is_some() => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod admit_tests {
    use super::*;

    #[test]
    fn a_download_is_refused_with_no_site_or_a_relative_folder() {
        let no_host = admit("http://", AddOptions::default()).unwrap_err();
        assert!(no_host.contains("names no site"), "{no_host}");
        let relative = AddOptions {
            save_dir: Some("out".into()),
            ..AddOptions::default()
        };
        assert!(admit("https://files.example/a.zip", relative).is_err());
        let home = AddOptions {
            save_dir: Some("~/x".into()),
            ..AddOptions::default()
        };
        let admitted = admit("https://files.example/a.zip", home).unwrap();
        let saved = admitted.save_dir.unwrap();
        assert!(!saved.contains('~') && std::path::Path::new(&saved).is_absolute());
        assert!(admit(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            AddOptions::default()
        )
        .is_ok());
    }
}

#[cfg(test)]
mod export_tests {
    use crate::app::backend::testing::backend;

    #[test]
    fn an_exported_list_brings_back_queue_note_name_and_digest() {
        /* Only the address and the page went out, so a list moved to
         * another computer came back without the rest. Held for later so
         * nothing is fetched; nothing listens on the port anyway. */
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let from = backend(&runtime, dir.path());
        let digest = "a".repeat(64);
        let id = from
            .add_with(
                "http://127.0.0.1:9/a.zip",
                dm_core::AddOptions {
                    referrer: "http://127.0.0.1:9/page".into(),
                    queue: "Nightly".into(),
                    description: "the big one".into(),
                    save_name: Some("mine.zip".into()),
                    later: true,
                    ..dm_core::AddOptions::default()
                },
            )
            .unwrap();
        from.set_checksum(id, &format!("sha256:{digest}")).unwrap();

        let to = backend(&runtime, dir.path());
        assert_eq!(to.import(&from.export()), 1);
        let row = to.list().pop().unwrap();
        assert_eq!(row.referrer, "http://127.0.0.1:9/page");
        assert_eq!(row.queue, "Nightly");
        assert_eq!(row.description, "the big one");
        assert_eq!(row.save_name.as_deref(), Some("mine.zip"));
        assert_eq!(row.checksum.map(|c| c.expected), Some(digest));
    }

    #[test]
    fn an_exported_address_carries_no_login() {
        assert_eq!(
            super::without_login(concat!("https://me:", "p%40ss", "@files.example/a.zip")),
            "https://files.example/a.zip"
        );
        assert_eq!(
            super::without_login("https://files.example/a.zip"),
            "https://files.example/a.zip"
        );
        assert_eq!(
            super::without_login("magnet:?xt=urn:btih:abc"),
            "magnet:?xt=urn:btih:abc"
        );
    }
}

#[cfg(test)]
mod set_url_tests {
    use crate::app::backend::testing::backend;
    use std::time::Duration;

    /// A download running when it is given a new address is stopped, given
    /// it and started again; one that is no link is refused.
    #[test]
    fn a_running_download_given_a_new_address_is_stopped_changed_and_started_again() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(&runtime, dir.path());
        // A server that takes each request and never answers it.
        let addr = runtime.block_on(async {
            let app = axum::Router::new().fallback(|| async {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                "never"
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            addr
        });
        let id = backend
            .add_with(
                &format!("http://{addr}/old.bin"),
                dm_core::AddOptions::default(),
            )
            .unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while backend.row(id).unwrap().state != dm_core::State::Running {
            assert!(std::time::Instant::now() < until, "never ran");
            std::thread::sleep(Duration::from_millis(10));
        }
        let new = format!("http://{addr}/new.bin");
        backend.set_url(id, &new).unwrap();
        let row = backend.row(id).unwrap();
        assert_eq!(row.url, new);
        assert!(
            matches!(row.state, dm_core::State::Running | dm_core::State::Queued),
            "started again: {:?}",
            row.state
        );
        assert!(backend.set_url(id, "not a link").is_err());
        backend.cancel(id);
    }
}
