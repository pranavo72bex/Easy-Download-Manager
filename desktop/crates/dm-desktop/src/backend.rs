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
    /// [`Platform::open`] without waiting on it: an opener's verdict takes
    /// up to a second and a half on Linux, and the window froze for it (UI
    /// round 10). A failure goes to `report`.
    fn open_later(&self, path: &str, report: crate::power::Report) {
        if let Err(e) = self.open(path) {
            report(e);
        }
    }
    /// [`Platform::reveal`] without waiting on it, as [`Platform::open_later`].
    fn reveal_later(&self, path: &str, report: crate::power::Report) {
        if let Err(e) = self.reveal(path) {
            report(e);
        }
    }
    /// Plays the system's short sound for a finished download.
    fn chime(&self);
    /// Plays the system's short sound for a failed download. The finish's
    /// sound unless a platform has a different one.
    fn alert(&self) {
        self.chime();
    }
    /// Puts the computer to sleep or shuts it down, when done. A failure to
    /// start is the answer; a refusal that comes later goes to `report`.
    fn power(
        &self,
        action: crate::power::WhenDone,
        report: crate::power::Report,
    ) -> Result<(), String>;
    /// Asks for an application to open a file with. `None` when the person
    /// cancels.
    fn choose_app(&self) -> Option<std::path::PathBuf>;
    /// Opens a finished file with the application `app`.
    fn open_with(&self, path: &str, app: &std::path::Path) -> Result<(), String>;
    /// Asks for a file: one to read (`save` false) or a place to write one
    /// called `suggested`. `None` when the person cancels.
    fn choose_file(&self, save: bool, suggested: &str) -> Option<std::path::PathBuf>;
    /// Asks for a folder, for a Browse… button beside a folder's field.
    /// `None` when the person cancels, or where there is nothing to ask with.
    fn choose_folder(&self) -> Option<std::path::PathBuf> {
        None
    }
    /// Whether this desktop has a folder chooser to ask with. Where it has
    /// none, `choose_folder` answers `None` at once, as for a Cancel, and
    /// Browse… says so instead of doing nothing.
    fn can_choose_folder(&self) -> bool {
        true
    }
    /// Whether this desktop has a file chooser to ask with, for Import and
    /// Export. Where it has none, they ask for the file's path instead of
    /// doing nothing (gap 144).
    fn can_choose_file(&self) -> bool {
        self.can_choose_folder()
    }
}

/// Whether the file chooser dm's dialogs use (the desktop portal's, else
/// zenity's) is there: the portal answering, or zenity on `path` (a `PATH`
/// value).
pub fn chooser_here(portal: bool, path: Option<&std::ffi::OsStr>) -> bool {
    portal
        || path
            .is_some_and(|path| std::env::split_paths(path).any(|dir| dir.join("zenity").is_file()))
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

    /// The rows being retried now, each with its attempt and how many
    /// there may be (0 for no end).
    pub fn retrying(&self) -> std::collections::HashMap<Id, (u32, u32)> {
        self.block_on(self.queue.retrying())
    }

    pub fn add(&self, text: &str) -> Result<shell::Added, String> {
        self.block_on(shell::add(&self.queue, text))
    }

    pub fn pause(&self, id: Id) {
        self.block_on(self.queue.pause(id));
    }

    /// Puts a sign-in saved in Options since the row was added on its
    /// address: Retry of a 401 asked again without it, while a fresh copy
    /// of the address worked (UI round 14).
    fn login_now(&self, id: Id) {
        let Some(row) = self.block_on(self.queue.get(id)) else {
            return;
        };
        if row.state == dm_core::State::Running {
            return;
        }
        let signed = self.settings().with_login(&row.url);
        if signed != row.url {
            self.block_on(self.queue.set_url(id, &signed));
        }
    }

    pub fn resume(&self, id: Id) {
        self.login_now(id);
        self.block_on(async {
            self.queue.resume(id).await;
            shell::wake(&self.queue);
        });
    }

    /// Starts one row now, outside the schedule and the download limit.
    pub fn start_outside_schedule(&self, id: Id) -> bool {
        self.block_on(async {
            let started = self.queue.start_outside_schedule(id).await;
            if started {
                shell::wake(&self.queue);
            }
            started
        })
    }

    pub fn retry(&self, id: Id) {
        self.login_now(id);
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

    /// Several rows taken out with one save of the queue (gap 58).
    pub fn remove_many(&self, ids: &[Id]) {
        self.block_on(self.queue.remove_many(ids));
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

    /// One download's own speed limit in KB/s, zero for none.
    pub fn set_own_limit(&self, id: Id, kb_per_second: u64) {
        self.block_on(self.queue.set_own_limit(id, kb_per_second));
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
    /// Whole or not at all. What the change refuses (a relative folder, a
    /// time that is not one) and a proxy that cannot be built are found
    /// first, before anything is touched; then the login item, the other
    /// thing that can fail; then the queue, putting the login item back if
    /// that fails after all. Before, the login item changed and then a
    /// refused proxy left it so, beside settings that said otherwise. Then
    /// the record of what is held, before the save that might fail — this
    /// is what the queue is actually running, so it is what the next change
    /// is compared against.
    pub fn set_settings(&self, settings: Settings) -> Result<Settings, String> {
        let previous = self.settings();
        // Judged as typed, before `clamped` quietly mends what it can: a
        // relative download folder is refused, not swapped for the default.
        if let Some(why) = settings.refusal(&previous) {
            return Err(why);
        }
        let settings = settings.clamped();
        shell::check(&settings, Some(&previous))?;
        let login_changed = previous.start_on_login != settings.start_on_login;
        if login_changed {
            self.platform
                .set_login_item(settings.start_on_login)
                .map_err(|e| format!("could not change start-on-login: {e}"))?;
        }
        let problems = match self.block_on(shell::apply(&settings, Some(&previous), &self.queue)) {
            Ok(problems) => problems,
            Err(e) => {
                if login_changed {
                    if let Err(back) = self.platform.set_login_item(previous.start_on_login) {
                        self.note(format!(
                            "start-on-login could not be put back as it was: {back}"
                        ));
                    }
                }
                return Err(e);
            }
        };
        for problem in problems {
            self.note(problem);
        }
        *dm_core::lock(&self.settings) = settings.clone();
        settings
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        Ok(settings)
    }

    /// Add URL's remembered addresses, kept and saved alone: nothing else
    /// changes, so nothing is applied to the queue — a full apply on every
    /// address added rebuilt its client and its cookies each time.
    pub fn remember_urls(&self, recent: Vec<String>) -> Result<Settings, String> {
        // Only this field changes on disk: the file is read, the list put
        // in, and written back, so nothing else saved there is overwritten.
        let (mut on_disk, _) = Settings::load();
        on_disk.recent_urls = recent.clone();
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        settings.recent_urls = recent;
        settings
            .recent_urls
            .truncate(dm_shell::settings::RECENT_URLS);
        Ok(settings.clone())
    }

    /// The folder Download details last saved a kind of file to, kept and
    /// saved alone as Add URL's addresses are; `None` forgets it, for the
    /// kind's own folder.
    pub fn remember_folder(&self, kind: &str, folder: Option<String>) -> Result<Settings, String> {
        let (mut on_disk, _) = Settings::load();
        let set = |s: &mut Settings| match &folder {
            Some(f) => {
                s.last_folders.insert(kind.to_string(), f.clone());
            }
            None => {
                s.last_folders.remove(kind);
            }
        };
        set(&mut on_disk);
        on_disk
            .clamped()
            .save()
            .map_err(|e| format!("settings could not be saved: {e}"))?;
        let mut settings = dm_core::lock(&self.settings);
        set(&mut settings);
        Ok(settings.clone())
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
        let with = admit(url, with)?;
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
        // Each refused here is answered in its place; the rest are added in
        // one go, writing the queue once.
        let admitted: Vec<Result<(String, AddOptions), String>> = items
            .into_iter()
            .map(|(url, with)| {
                let with = admit(&url, with)?;
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

    pub fn set_description(&self, id: Id, text: &str) {
        self.block_on(self.queue.set_description(id, text));
    }

    pub fn set_queue(&self, id: Id, queue: &str) {
        self.block_on(self.queue.set_queue(id, queue));
    }

    pub fn set_url(&self, id: Id, url: &str) -> Result<(), String> {
        let url = self.settings().with_login(url.trim());
        if !dm_core::queue::accepted(&url) {
            return Err("it must be a link dm can download".into());
        }
        // A download retrying between attempts is the one most likely to
        // need a new address (UI round 8): stopped, given it, and started
        // again, rather than refused as running.
        let was_running = self
            .block_on(self.queue.get(id))
            .is_some_and(|row| row.state == dm_core::State::Running);
        if was_running {
            self.pause(id);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline
                && self
                    .block_on(self.queue.get(id))
                    .is_some_and(|row| row.state == dm_core::State::Running)
            {
                std::thread::sleep(std::time::Duration::from_millis(25));
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

    /// Moves a finished file into `dir`; see `Queue::move_to`.
    pub fn move_to(&self, id: Id, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
        self.block_on(self.queue.move_to(id, dir))
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
        self.import_where(text, |_| true)
    }

    /// The addresses a list holds, in its order.
    pub fn listed(text: &str) -> Vec<String> {
        lists::from_list(text)
            .into_iter()
            .map(|row| row.url)
            .collect()
    }

    /// [`Self::import`], of only the rows whose address `keep` keeps.
    pub fn import_where(&self, text: &str, keep: impl Fn(&str) -> bool) -> usize {
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
        added.iter().filter(|r| r.is_ok()).count()
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
        let full = dm_shell::settings::full_folder(&typed, None).ok_or_else(|| {
            format!("the folder must be a full path, such as ~/Downloads, not {typed:?}")
        })?;
        with.save_dir = Some(full.display().to_string());
    }
    Ok(with)
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
    struct Nothing;

    impl super::Platform for Nothing {
        fn set_login_item(&self, _: bool) -> Result<(), String> {
            Ok(())
        }
        fn notify(&self, _: &str, _: &str) {}
        fn clipboard(&self) -> Option<String> {
            None
        }
        fn reveal(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn open(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn chime(&self) {}
        fn power(&self, _: crate::power::WhenDone, _: crate::power::Report) -> Result<(), String> {
            Ok(())
        }
        fn choose_app(&self) -> Option<std::path::PathBuf> {
            None
        }
        fn open_with(&self, _: &str, _: &std::path::Path) -> Result<(), String> {
            Ok(())
        }
        fn choose_file(&self, _: bool, _: &str) -> Option<std::path::PathBuf> {
            None
        }
    }

    fn backend(runtime: &tokio::runtime::Runtime, dir: &std::path::Path) -> super::Backend {
        let settings = dm_shell::Settings {
            download_dir: dir.join("out"),
            ..dm_shell::Settings::default()
        };
        let queue = dm_core::Queue::new(
            dm_core::client(),
            settings.options(dir.join("parts"), dm_core::Limiter::unlimited()),
            settings.concurrency,
        );
        super::Backend::new(
            runtime.handle().clone(),
            queue,
            settings,
            Vec::new(),
            std::sync::Arc::new(Nothing),
        )
    }

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
mod chooser_tests {
    /// Gap 121: with no portal and no zenity there is no chooser, and
    /// Browse… says so rather than doing nothing.
    #[test]
    fn a_chooser_is_the_portal_or_zenity_on_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert!(!super::chooser_here(false, Some(&path)));
        assert!(!super::chooser_here(false, None));
        assert!(super::chooser_here(true, None));
        std::fs::write(dir.path().join("zenity"), "").unwrap();
        assert!(super::chooser_here(false, Some(&path)));
    }
}
