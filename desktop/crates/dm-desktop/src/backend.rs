//! What the window asks of the app, as plain calls.
//!
//! The web window reached the queue across Tauri's IPC, which is why a test
//! had to hold the page's command names and the handler list to each other.
//! Here the window calls the queue directly, on the app's own runtime: a
//! command the window needs and the backend lacks is a compile error.

use std::sync::{Arc, Mutex};

use dm_core::{AddOptions, Connection, Entry, Id, Queue};
use dm_shell::{lists, shell, Settings};

/// What the window needs from the operating system, kept apart so a test can
/// stand in for it: the window is driven in tests against a real queue, and
/// a test must not register a login item or post a notification.
pub trait Platform: Send + Sync {
    /// Registers or removes dm as a login item, as `wanted` says.
    fn set_login_item(&self, wanted: bool) -> Result<(), String>;
    /// Announces a finished download.
    fn notify(&self, title: &str, body: &str);
    /// The clipboard's text, for the opt-in link watcher.
    fn clipboard(&self) -> Option<String>;
    /// Shows a finished file in the file manager.
    fn reveal(&self, path: &str) -> Result<(), String>;
    /// Opens a finished file with whatever the system opens it with.
    fn open(&self, path: &str) -> Result<(), String>;
    /// Plays the system's short sound for a finished download.
    fn chime(&self);
    /// Plays the system's short sound for a failed download. The finish's
    /// sound unless a platform has a different one.
    fn alert(&self) {
        self.chime();
    }
    /// Puts the computer to sleep or shuts it down, when done.
    fn power(&self, action: crate::power::WhenDone) -> Result<(), String>;
    /// Asks for an application to open a file with. `None` when the person
    /// cancels.
    fn choose_app(&self) -> Option<std::path::PathBuf>;
    /// Opens a finished file with the application `app`.
    fn open_with(&self, path: &str, app: &std::path::Path) -> Result<(), String>;
    /// Asks for a file: one to read (`save` false) or a place to write one
    /// called `suggested`. `None` when the person cancels.
    fn choose_file(&self, save: bool, suggested: &str) -> Option<std::path::PathBuf>;
}

/// The queue, the settings in force, and the problems no row can carry.
#[derive(Clone)]
pub struct Backend {
    pub runtime: tokio::runtime::Handle,
    pub queue: Queue,
    settings: Arc<Mutex<Settings>>,
    /// What went wrong at startup — a saved proxy that could not be used, a
    /// login item that could not be written — handed to the window once.
    notices: Arc<Mutex<Vec<String>>>,
    pub platform: Arc<dyn Platform>,
    /// Videos the browser's panel handed over to be asked about: the window
    /// opens "Download details" for each before it starts.
    pub handed: Arc<Mutex<std::collections::VecDeque<dm_shell::host::Request>>>,
}

impl Backend {
    pub fn new(
        runtime: tokio::runtime::Handle,
        queue: Queue,
        settings: Settings,
        notices: Vec<String>,
        platform: Arc<dyn Platform>,
    ) -> Self {
        Backend {
            runtime,
            queue,
            settings: Arc::new(Mutex::new(settings)),
            notices: Arc::new(Mutex::new(notices)),
            platform,
            handed: Arc::default(),
        }
    }

    fn block_on<T>(&self, work: impl std::future::Future<Output = T>) -> T {
        self.runtime.block_on(work)
    }

    pub fn list(&self) -> Vec<Entry> {
        self.block_on(self.queue.list())
    }

    pub fn add(&self, text: &str) -> Result<Vec<Id>, String> {
        self.block_on(shell::add(&self.queue, text))
    }

    pub fn pause(&self, id: Id) {
        self.block_on(self.queue.pause(id));
    }

    pub fn resume(&self, id: Id) {
        self.block_on(async {
            self.queue.resume(id).await;
            shell::wake(&self.queue);
        });
    }

    pub fn retry(&self, id: Id) {
        self.block_on(async {
            if self.queue.retry(id).await {
                shell::wake(&self.queue);
            }
        });
    }

    pub fn cancel(&self, id: Id) {
        self.block_on(self.queue.cancel(id));
    }

    pub fn remove(&self, id: Id) {
        self.block_on(self.queue.remove(id));
    }

    /// One command for the lot, not one per row: pausing forty downloads by
    /// asking forty times rewrites the queue file forty times.
    pub fn pause_all(&self) -> usize {
        self.block_on(self.queue.pause_all())
    }

    pub fn resume_all(&self) -> usize {
        self.block_on(async {
            let resumed = self.queue.resume_all().await;
            if resumed > 0 {
                shell::wake(&self.queue);
            }
            resumed
        })
    }

    pub fn clear_finished(&self) -> usize {
        self.block_on(self.queue.clear_finished())
    }

    pub fn set_checksum(&self, id: Id, text: &str) -> Result<(), String> {
        let checksum = shell::checksum(text)?;
        self.block_on(self.queue.set_checksum(id, checksum));
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        dm_core::lock(&self.settings).clone()
    }

    /// Applies a change and writes it down, returning what is now in force.
    ///
    /// The login item first, since it is what can fail; then the queue; then
    /// the record of what is held, before the save that might fail — this is
    /// what the queue is actually running, so it is what the next change is
    /// compared against.
    pub fn set_settings(&self, settings: Settings) -> Result<Settings, String> {
        let settings = settings.clamped();
        let previous = self.settings();
        if previous.start_on_login != settings.start_on_login {
            self.platform
                .set_login_item(settings.start_on_login)
                .map_err(|e| format!("could not change start-on-login: {e}"))?;
        }
        self.block_on(shell::apply(&settings, Some(&previous), &self.queue))?;
        *dm_core::lock(&self.settings) = settings.clone();
        settings
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        Ok(settings)
    }

    /// What went wrong that no row can carry, each handed over once.
    pub fn problems(&self) -> Vec<String> {
        let mut all = std::mem::take(&mut *dm_core::lock(&self.notices));
        all.extend(self.queue.problems());
        all
    }

    /// Where something running apart from the window leaves a problem for
    /// the banner.
    pub fn notices(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.notices)
    }

    pub fn note(&self, problem: String) {
        dm_core::lock(&self.notices).push(problem);
    }

    /* ------------------------------------------------------------- row verbs */

    /// Adds with what the "Download details" dialog chose. Site logins
    /// are put on the address here, so every way in gets them.
    pub fn add_with(&self, url: &str, with: AddOptions) -> Result<Id, String> {
        let url = self.settings().with_login(url.trim());
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
        let starts = items.iter().any(|(_, with)| !with.later);
        let items: Vec<(String, AddOptions)> = items
            .into_iter()
            .map(|(url, with)| (settings.with_login(url.trim()), with))
            .collect();
        let ids = self.block_on(self.queue.add_each(items));
        if starts {
            self.block_on(async { shell::wake(&self.queue) });
        }
        ids.into_iter()
            .map(|id| id.ok_or_else(|| "that is not a URL dm can download".to_string()))
            .collect()
    }

    pub fn set_description(&self, id: Id, text: &str) {
        self.block_on(self.queue.set_description(id, text));
    }

    pub fn set_queue(&self, id: Id, queue: &str) {
        self.block_on(self.queue.set_queue(id, queue));
    }

    pub fn set_url(&self, id: Id, url: &str) -> Result<(), String> {
        let url = self.settings().with_login(url.trim());
        if self.block_on(self.queue.set_url(id, &url)) {
            Ok(())
        } else {
            Err(
                "a running or finished download's address cannot be changed, \
                 and it must be a link dm can download"
                    .into(),
            )
        }
    }

    pub fn redownload(&self, id: Id) {
        self.block_on(async {
            if self.queue.redownload(id).await {
                shell::wake(&self.queue);
            }
        });
    }

    pub fn shift(&self, id: Id, by: isize) {
        self.block_on(self.queue.shift(id, by));
    }

    pub fn start_queue(&self, queue: &str) -> usize {
        self.block_on(async {
            let started = self.queue.start_queue(queue).await;
            if started > 0 {
                shell::wake(&self.queue);
            }
            started
        })
    }

    pub fn stop_queue(&self, queue: &str) -> usize {
        self.block_on(self.queue.stop_queue(queue))
    }

    pub fn rename(&self, id: Id, name: &str) -> Result<(), String> {
        self.block_on(self.queue.rename(id, name))
    }

    pub fn connections(&self, id: Id) -> Vec<Connection> {
        self.block_on(self.queue.connections(id))
    }

    pub fn resumable(&self, id: Id) -> Option<bool> {
        self.block_on(self.queue.resumable(id))
    }

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
            .map(|e| lists::Listed {
                url: without_login(&e.url),
                referrer: e.referrer,
            })
            .collect();
        lists::to_list(&rows)
    }

    /// Adds what a `.dmlist` file or a plain list holds, saying how many.
    pub fn import(&self, text: &str) -> usize {
        let items = lists::from_list(text)
            .into_iter()
            .map(|row| {
                let with = AddOptions {
                    referrer: row.referrer,
                    ..AddOptions::default()
                };
                (row.url, with)
            })
            .collect();
        self.add_each(items).iter().filter(|r| r.is_ok()).count()
    }
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
mod export_tests {
    #[test]
    fn an_exported_address_carries_no_login() {
        assert_eq!(
            super::without_login("https://files.example/a.zip"),
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
