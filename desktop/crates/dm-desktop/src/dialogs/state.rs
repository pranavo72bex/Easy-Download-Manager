// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
impl Dialogs {
    /// Gives the keyboard to `title`'s first field when it is next drawn.
    pub fn focus_on(&mut self, title: &'static str) {
        self.focus = Some(title);
    }

    /// Whether `title`'s first field is to take the keyboard now; once.
    pub(super) fn take_focus(&mut self, title: &'static str) -> bool {
        if self.focus == Some(title) {
            self.focus = None;
            return true;
        }
        false
    }

    /// Add URL, offering a link already on the clipboard.
    pub fn open_add_url(&mut self, clipboard: Option<&str>) {
        self.focus_on("Add URL");
        if self.keep("Add URL", self.add_url.is_some()) {
            return;
        }
        let address = clipboard
            .and_then(crate::app::looks_downloadable)
            .unwrap_or_default();
        self.add_url = Some(AddUrl {
            address,
            ..AddUrl::default()
        });
    }

    /// Opens Download details for these addresses.
    pub fn open_file_info(&mut self, urls: Vec<String>) {
        // The keyboard comes off the list behind it.
        self.focus_on("Download details");
        self.file_info = Some(FileInfo {
            urls,
            queue: MAIN_QUEUE.into(),
            ..FileInfo::default()
        });
    }

    /// "Playlist" for a YouTube playlist address: read by dm itself as the
    /// dialog opens, its videos to be ticked and taken at `quality`.
    pub fn open_playlist(
        &mut self,
        url: String,
        backend: &crate::backend::Backend,
        quality: String,
    ) {
        self.focus_on("Playlist");
        let slot: PlaylistRead = Default::default();
        // Read by the application (`lookups::read_playlist_later`).
        let read = slot.clone();
        dm_shell::app::lookups::read_playlist_later(backend, url.clone(), move |got| {
            if let Ok(mut slot) = read.lock() {
                *slot = Some(got);
            }
        });
        self.playlist = Some(PlaylistDialog {
            url,
            read: Some(slot),
            quality,
            ..PlaylistDialog::default()
        });
    }

    /// "Playlist" for a playlist already read (another site's page that
    /// Download details found lists several videos).
    pub fn open_playlist_read(
        &mut self,
        url: String,
        playlist: dm_core::youtube::Playlist,
        quality: String,
    ) {
        self.focus_on("Playlist");
        let slot: PlaylistRead = std::sync::Arc::new(std::sync::Mutex::new(Some(Ok(playlist))));
        self.playlist = Some(PlaylistDialog {
            url,
            read: Some(slot),
            quality,
            ..PlaylistDialog::default()
        });
    }

    /// The dialog for a video the browser's panel handed over.
    pub fn open_handed(&mut self, request: dm_shell::host::Request) {
        self.focus_on("Download details");
        // The form for the address it was sent to, before that is moved below.
        let post = dm_shell::shell::posted(&request.url, request.post.as_ref());
        self.file_info = Some(FileInfo {
            urls: vec![request.url],
            referrer: request.referrer,
            quality: request.quality,
            sound: request.sound,
            label: request.label,
            name: request.save_name.unwrap_or_default(),
            queue: MAIN_QUEUE.into(),
            handed: true,
            cookies: request.cookies,
            cookies_once: request.cookies_once,
            post,
            headers: request.headers,
            browser: request.browser,
            caught: request.caught,
            // The panel's "Part" line: the dialog opens with the part on.
            part: !(request.from.is_empty() && request.to.is_empty()),
            part_from: if request.from.is_empty() && !request.to.is_empty() {
                "0:00".to_string()
            } else {
                request.from
            },
            part_to: request.to,
            ..FileInfo::default()
        });
    }

    /// "Download links": every link not already listed checked, and each
    /// one's server asked, six at a time, for its name, kind and size.
    pub fn open_links(
        &mut self,
        request: dm_shell::host::Request,
        backend: &crate::backend::Backend,
    ) {
        let found: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Found>>> =
            Default::default();
        let sign_in = dm_core::cookies::SignIn::new(&request.url, &request.cookies);
        // Asked by the application (`lookups::probe_links`), six at a time;
        // the window keeps what each said, its kind in its own words.
        let into = found.clone();
        dm_shell::app::lookups::probe_links(
            backend,
            request.links.clone(),
            request.referrer.clone(),
            sign_in.clone(),
            move |url, said| {
                let found = match said {
                    dm_shell::app::lookups::LinkFound::Known {
                        name,
                        kind,
                        size,
                        page,
                    } => Found {
                        name,
                        kind: crate::app::category_word(kind),
                        size,
                        html: page,
                        failed: None,
                    },
                    // Said, rather than "…" for ever as though still asking.
                    dm_shell::app::lookups::LinkFound::Failed(why) => Found {
                        failed: Some(why),
                        ..Found::default()
                    },
                };
                dm_core::lock(&into).insert(url, found);
            },
        );
        let listed: std::collections::HashSet<String> = backend
            .list()
            .iter()
            .filter(|r| r.state != State::Cancelled)
            .map(|r| bare(&r.url))
            .collect();
        self.links = Some(LinksImport {
            page: request.referrer,
            sign_in,
            cookies: request.cookies,
            items: request
                .links
                .into_iter()
                .map(|u| {
                    let new = !listed.contains(&bare(&u));
                    (u, new)
                })
                .collect(),
            listed,
            refused: Default::default(),
            types: String::new(),
            hide_html: true,
            found,
            hide_images: true,
            hide_repeats: true,
            save_to: SaveTo::ByCategory,
            category: crate::app::category_word(dm_core::Category::Archives),
            folder: String::new(),
            editing: None,
            // The names the page's `download` attributes give, as Edit would.
            names: request.names.into_iter().collect(),
            texts: request.texts,
            use_texts: true,
            queue: dm_core::MAIN_QUEUE.to_string(),
        });
    }

    /// Opens Properties for this row.
    pub fn open_properties(&mut self, row: &dm_core::Entry) {
        self.focus_on("File properties");
        if self.properties.contains_key(&row.id) {
            self.raise.push(egui::Id::unique(("properties", row.id)));
            return;
        }
        let (address, login, password) = split_login(&row.url);
        self.properties.insert(
            row.id,
            Properties {
                name: name_of(row),
                description: row.description.clone(),
                address,
                login,
                password,
                says: String::new(),
                sums: Default::default(),
                check: String::new(),
                zip: None,
                files: row.torrent_files.iter().map(|f| f.wanted).collect(),
                priorities: row.torrent_files.iter().map(|f| f.priority).collect(),
            },
        );
    }

    /// An open dialog is brought forward and kept as it is, with what was
    /// typed in it: asking for it again used to start it afresh, and
    /// Options lost every change not yet saved. True if it was open.
    pub(super) fn keep(&mut self, title: &'static str, open: bool) -> bool {
        if open {
            self.raise.push(window_id(title));
        }
        open
    }

    /// Opens the Series dialog.
    pub fn open_series(&mut self) {
        self.focus_on("Add numbered series");
        if !self.keep("Add numbered series", self.series.is_some()) {
            self.series = Some(Default::default());
        }
    }

    /// Opens the Grabber with a saved search filled in, to run or change.
    pub fn open_site_files_with(&mut self, search: &dm_shell::lists::Search) {
        self.focus_on("Grabber");
        self.site_files = Some(SiteFiles {
            page: search.page.clone(),
            types: search.types.join(" "),
            depth: search.depth,
            include: search.include.join(" "),
            exclude: search.exclude.join(" "),
            name: search.name.clone(),
            every_hours: search.every_hours,
            stop_after_minutes: search.stop_after_minutes,
            other_sites: search.other_sites,
            keep_layout: search.keep_layout,
            min_kb: search.min_kb,
            max_kb: search.max_kb,
            ..Default::default()
        });
    }

    /// Opens the Site files dialog.
    pub fn open_site_files(&mut self) {
        self.focus_on("Grabber");
        if !self.keep("Grabber", self.site_files.is_some()) {
            self.site_files = Some(Default::default());
        }
    }

    /// Whether a question waits for its answer: Remove, Exit, Start
    /// outside the schedule, Cancel this download, already listed, leave a
    /// site to the browser.
    /// Nothing else opens over one, or it is lost behind the new window
    /// and answered later by an Enter meant for something else.
    pub fn asking(&self) -> bool {
        self.confirm_exit
            || self.confirm_close
            || self.confirm_remove.is_some()
            || self.confirm_outside.is_some()
            || self.confirm_cancel.is_some()
            || self.duplicate.is_some()
            || self.name_ask.is_some()
            || self.page_ask.is_some()
            || self.not_found.is_some()
            || self.import_ask.is_some()
            || self.skip_offer.is_some()
    }

    /// The titles of the questions waiting for an answer.
    pub(super) fn questions(&self) -> Vec<&'static str> {
        [
            (self.confirm_exit, "Exit dm?"),
            (self.confirm_close, "Keep running in the tray?"),
            (self.confirm_remove.is_some(), "Delete unfinished downloads"),
            (
                self.confirm_outside.is_some(),
                "Start outside the schedule?",
            ),
            (self.confirm_cancel.is_some(), "Cancel this download?"),
            (self.duplicate.is_some(), "Already in the list"),
            (self.name_ask.is_some(), "Name sent by the server"),
            (self.page_ask.is_some(), "Web page instead of the file"),
            (self.not_found.is_some(), "The file doesn't exist"),
            (self.import_ask.is_some(), "Some are already in the list"),
            (self.skip_offer.is_some(), "Leave this site to the browser?"),
        ]
        .into_iter()
        .filter(|(open, _)| *open)
        .map(|(_, title)| title)
        .collect()
    }

    /// Whether a dialog is in front of the list, which then keeps its
    /// keys from the list. The windows a download opens on its own (its
    /// progress, its completion) are not counted.
    pub fn in_front(&self) -> bool {
        self.open_names()
            .iter()
            .any(|n| n != "Download progress" && n != "Download complete")
    }

    /// Whether a dialog that keeps the clicks to itself is open.
    pub fn modal(&self) -> bool {
        !self.modal_layers().is_empty()
    }

    /// The windows of the dialogs in front that keep the keyboard to
    /// themselves: Tab goes round their own controls and never to the menu
    /// bar or the list behind, which take no clicks while one is open.
    /// Find is left out: it finds rows in the list, which stays usable
    /// beside it.
    pub(super) fn modal_layers(&self) -> Vec<egui::LayerId> {
        let mut ids: Vec<egui::Id> = [
            (self.confirm_exit, "Exit dm?"),
            (self.confirm_close, "Keep running in the tray?"),
            (self.confirm_remove.is_some(), "Delete unfinished downloads"),
            (
                self.confirm_outside.is_some(),
                "Start outside the schedule?",
            ),
            (self.confirm_cancel.is_some(), "Cancel this download?"),
            (self.duplicate.is_some(), "Already in the list"),
            (self.name_ask.is_some(), "Name sent by the server"),
            (self.page_ask.is_some(), "Web page instead of the file"),
            (self.not_found.is_some(), "The file doesn't exist"),
            (self.import_ask.is_some(), "Some are already in the list"),
            (
                self.list_file.as_ref().is_some_and(|f| !f.save),
                "Import a list",
            ),
            (
                self.list_file.as_ref().is_some_and(|f| f.save),
                "Export the list",
            ),
            (self.skip_offer.is_some(), "Leave this site to the browser?"),
            (self.add_url.is_some(), "Add URL"),
            (self.file_info.is_some(), "Download details"),
            (self.series.is_some(), "Add numbered series"),
            (self.clip_links.is_some(), "Links on the clipboard"),
            (self.links.is_some(), "Download links"),
            (self.site_files.is_some(), "Grabber"),
            (self.update_link.is_some(), "Update download link"),
            (self.move_to_queue.is_some(), "Move to queue"),
            (self.rename.is_some(), "Move/Rename"),
            (self.scheduler.is_some(), "Scheduler"),
            (self.limiter.is_some(), "Speed limiter"),
            (self.options.is_some(), "Options"),
            (self.about_open, "About dm"),
            (self.tip.is_some(), "Tip of the day"),
            (self.welcome.is_some(), "Add dm to your browser"),
        ]
        .into_iter()
        .filter(|(open, _)| *open)
        .map(|(_, title)| window_id(title))
        .collect();
        ids.extend(
            self.properties
                .keys()
                .map(|id| egui::Id::unique(("properties", *id))),
        );
        ids.into_iter()
            .map(|id| egui::LayerId::new(egui::Order::Middle, id))
            .collect()
    }

    /// Opens the scheduler, over these settings, on `queue`'s hours.
    pub fn open_scheduler(&mut self, settings: &Settings, queue: &str) {
        if self.keep("Scheduler", self.scheduler.is_some()) {
            return;
        }
        self.scheduler = Some(SchedulerDraft::new(settings, queue));
    }

    /// Opens the speed limiter's settings, over these settings.
    pub fn open_limiter(&mut self, settings: &Settings) {
        self.focus_on("Speed limiter");
        if self.keep("Speed limiter", self.limiter.is_some()) {
            return;
        }
        self.limiter = Some((
            settings.speed_limit,
            String::new(),
            settings.limit_mb,
            settings.limit_hours,
            settings.slow_hours.clone(),
        ));
    }

    /// Opens Options, over these settings.
    pub fn open_options(&mut self, settings: &Settings) {
        if self.keep("Options", self.options.is_some()) {
            return;
        }
        self.options = Some(OptionsDialog {
            tab: Tab::General,
            draft: settings.clone(),
            opened: settings.clone(),
            says: String::new(),
            types: settings.catch.extensions.join(" "),
            skip: settings.catch.skip_hosts.join(" "),
            once: settings.catch.once_hosts.join(" "),
            bypass: settings.proxy.bypass.join(" "),
            custom: settings
                .custom_categories
                .iter()
                .map(|c| [c.name.clone(), c.extensions.join(" "), c.sites.join(" ")])
                .collect(),
            fresh: false,
            fresh_tab: 3,
            to_proxy: false,
            handlers: Handlers::default(),
            handlers_asked: false,
        });
    }

    /// The dialogs open now, by title, for the control protocol.
    pub fn open_names(&self) -> Vec<String> {
        let mut open = Vec::new();
        let mut say = |on: bool, name: &str| {
            if on {
                open.push(name.to_string());
            }
        };
        say(self.confirm_exit, "Exit dm?");
        say(self.confirm_close, "Keep running in the tray?");
        // By the title it shows (an old name whatever it said).
        let removing = if self.remove_title.is_empty() {
            "Remove from the list"
        } else {
            self.remove_title
        };
        say(self.confirm_remove.is_some(), removing);
        say(
            self.confirm_outside.is_some(),
            "Start outside the schedule?",
        );
        say(self.confirm_cancel.is_some(), "Cancel this download?");
        // Every dialog by its title (this one was never named).
        say(self.links.is_some(), "Download links");
        say(self.duplicate.is_some(), "Already in the list");
        say(self.name_ask.is_some(), "Name sent by the server");
        say(self.page_ask.is_some(), "Web page instead of the file");
        say(self.not_found.is_some(), "The file doesn't exist");
        say(self.import_ask.is_some(), "Some are already in the list");
        say(
            self.list_file.as_ref().is_some_and(|f| !f.save),
            "Import a list",
        );
        say(
            self.list_file.as_ref().is_some_and(|f| f.save),
            "Export the list",
        );
        say(self.skip_offer.is_some(), "Leave this site to the browser?");
        say(self.add_url.is_some(), "Add URL");
        say(self.file_info.is_some(), "Download details");
        say(!self.progress.is_empty(), "Download progress");
        say(!self.complete.is_empty(), "Download complete");
        say(!self.properties.is_empty(), "File properties");
        say(self.series.is_some(), "Add numbered series");
        say(self.clip_links.is_some(), "Links on the clipboard");
        say(self.site_files.is_some(), "Grabber");
        // Left out until 2026-11-08: Escape behind it left it open, the list
        // behind it took keys, and a script was told it was not there.
        say(self.playlist.is_some(), "Playlist");
        say(self.update_link.is_some(), "Update download link");
        say(self.move_to_queue.is_some(), "Move to queue");
        say(self.rename.is_some(), "Move/Rename");
        say(self.find_open, "Find");
        say(self.scheduler.is_some(), "Scheduler");
        say(self.limiter.is_some(), "Speed limiter");
        say(self.options.is_some(), "Options");
        say(self.about_open, "About dm");
        say(self.tip.is_some(), "Tip of the day");
        say(self.welcome.is_some(), "Add dm to your browser");
        open
    }

    /// Closes the dialog in front, as Escape does; true if one closed. A
    /// question being asked goes first. `back_to_front` is the windows' order
    /// on screen, which decides among the ones that stay open beside each
    /// other (Find, Download complete, the progress windows): Escape closed
    /// one behind the window in front.
    pub fn close_top(&mut self, back_to_front: &[egui::Id]) -> bool {
        if std::mem::take(&mut self.confirm_exit)
            || std::mem::take(&mut self.confirm_close)
            || self.confirm_remove.take().is_some()
            || self.confirm_outside.take().is_some()
            || self.confirm_cancel.take().is_some()
            || self.duplicate.take().is_some()
            || self.name_ask.take().is_some()
            || self.page_ask.take().is_some()
            || self.not_found.take().is_some()
            || self.import_ask.take().is_some()
            || self.skip_offer.take().is_some()
        {
            return true;
        }
        self.close_front(back_to_front, false)
    }

    /// Closes the dialog whose window has `id`, as its close button and Escape
    /// do when it is a window of its own; true if one closed.
    pub fn close_window(&mut self, id: egui::Id) -> bool {
        if id == window_id("Exit dm?") {
            return std::mem::take(&mut self.confirm_exit);
        }
        if id == window_id("Keep running in the tray?") {
            return std::mem::take(&mut self.confirm_close);
        }
        if id == window_id("Delete unfinished downloads") {
            return self.confirm_remove.take().is_some();
        }
        if id == window_id("Start outside the schedule?") {
            return self.confirm_outside.take().is_some();
        }
        if id == window_id("Cancel this download?") {
            return self.confirm_cancel.take().is_some();
        }
        if id == window_id("Already in the list") {
            return self.duplicate.take().is_some();
        }
        if id == window_id("Name sent by the server") {
            return self.name_ask.take().is_some();
        }
        if id == window_id("Web page instead of the file") {
            return self.page_ask.take().is_some();
        }
        if id == window_id("The file doesn't exist") {
            return self.not_found.take().is_some();
        }
        if id == window_id("Some are already in the list") {
            return self.import_ask.take().is_some();
        }
        if id == window_id("Leave this site to the browser?") {
            return self.skip_offer.take().is_some();
        }
        self.close_front(&[id], true)
    }

    /// Closes the window in front of the ones open beside each other; with
    /// `strict`, only one that `back_to_front` names.
    fn close_front(&mut self, back_to_front: &[egui::Id], strict: bool) -> bool {
        /* Then the window in front. Find opens over Download details, and
         * Download complete and the progress windows stand beside the
         * rest, so a fixed order closed one behind the window in front.
         * One not on screen yet counts as behind every one that is, and
         * among those the old order holds: Find first, the progress
         * windows last. */
        enum Shut {
            Find,
            Limiter,
            Scheduler,
            UpdateLink,
            MoveToQueue,
            Rename,
            ListFile,
            AddUrl,
            ClipLinks,
            Series,
            Links,
            SiteFiles,
            Playlist,
            FileInfo,
            Options,
            About,
            Tip,
            Welcome,
            Properties(Id),
            Complete,
            Progress(Id),
        }
        let list_title = match &self.list_file {
            Some(f) if f.save => "Export the list",
            _ => "Import a list",
        };
        let mut open: Vec<(egui::Id, Shut)> = [
            (self.find_open, "Find", Shut::Find),
            (self.limiter.is_some(), "Speed limiter", Shut::Limiter),
            (self.scheduler.is_some(), "Scheduler", Shut::Scheduler),
            (
                self.update_link.is_some(),
                "Update download link",
                Shut::UpdateLink,
            ),
            (
                self.move_to_queue.is_some(),
                "Move to queue",
                Shut::MoveToQueue,
            ),
            (self.rename.is_some(), "Move/Rename", Shut::Rename),
            (self.list_file.is_some(), list_title, Shut::ListFile),
            (self.add_url.is_some(), "Add URL", Shut::AddUrl),
            (
                self.clip_links.is_some(),
                "Links on the clipboard",
                Shut::ClipLinks,
            ),
            (self.series.is_some(), "Add numbered series", Shut::Series),
            (self.links.is_some(), "Download links", Shut::Links),
            (self.site_files.is_some(), "Grabber", Shut::SiteFiles),
            (self.playlist.is_some(), "Playlist", Shut::Playlist),
            (self.file_info.is_some(), "Download details", Shut::FileInfo),
            (self.options.is_some(), "Options", Shut::Options),
            (self.about_open, "About dm", Shut::About),
            (self.tip.is_some(), "Tip of the day", Shut::Tip),
            (
                self.welcome.is_some(),
                "Add dm to your browser",
                Shut::Welcome,
            ),
        ]
        .into_iter()
        .filter(|(on, _, _)| *on)
        .map(|(_, title, shut)| (window_id(title), shut))
        .collect();
        for id in self.properties.keys().rev() {
            open.push((egui::Id::unique(("properties", *id)), Shut::Properties(*id)));
        }
        if !self.complete.is_empty() {
            open.push((window_id("Download complete"), Shut::Complete));
        }
        for id in self.progress.iter().rev() {
            open.push((egui::Id::unique(("progress", *id)), Shut::Progress(*id)));
        }
        let place = |id: &egui::Id| back_to_front.iter().position(|w| w == id);
        let Some((front_id, front)) = open
            .into_iter()
            .enumerate()
            .max_by_key(|(n, (id, _))| (place(id), std::cmp::Reverse(*n)))
            .map(|(_, chosen)| chosen)
        else {
            return false;
        };
        if strict && place(&front_id).is_none() {
            return false;
        }
        match front {
            Shut::Find => self.find_open = false,
            Shut::Limiter => self.limiter = None,
            Shut::Scheduler => self.scheduler = None,
            Shut::UpdateLink => self.update_link = None,
            Shut::MoveToQueue => self.move_to_queue = None,
            Shut::Rename => self.rename = None,
            Shut::ListFile => self.list_file = None,
            Shut::AddUrl => self.add_url = None,
            Shut::ClipLinks => self.clip_links = None,
            Shut::Series => self.series = None,
            Shut::Links => self.links = None,
            Shut::Playlist => self.playlist = None,
            Shut::SiteFiles => self.site_files = None,
            Shut::FileInfo => self.file_info = None,
            Shut::Options => self.options = None,
            Shut::About => self.about_open = false,
            Shut::Tip => self.tip = None,
            Shut::Welcome => self.welcome = None,
            Shut::Properties(id) => {
                self.properties.remove(&id);
            }
            // One window for every finished file: Escape closes it, all of
            // it, as its close box does.
            Shut::Complete => self.complete.clear(),
            Shut::Progress(id) => {
                self.progress.remove(&id);
                self.progress_closed.insert(id);
            }
        }
        true
    }

    /// Closes everything about a row that is gone.
    pub fn forget(&mut self, id: Id) {
        self.progress.remove(&id);
        self.progress_closed.remove(&id);
        self.complete.remove(&id);
        self.properties.remove(&id);
        self.resumable_seen.remove(&id);
        self.conn_rates.remove(&id);
    }
}

#[cfg(test)]
mod close_window_tests {
    use super::*;

    /// A dialog's own window, closed with its close button or Escape, closes
    /// that dialog and no other.
    #[test]
    fn a_dialog_is_closed_by_its_own_window_and_no_other() {
        let mut dialogs = Dialogs {
            find_open: true,
            about_open: true,
            ..Dialogs::default()
        };
        assert!(dialogs.close_window(window_id("About dm")));
        assert!(!dialogs.about_open);
        assert!(dialogs.find_open, "Find was not the window closed");
        // A window that is not open closes nothing.
        assert!(!dialogs.close_window(window_id("About dm")));
        assert!(dialogs.find_open);
    }

    /// A question open beside another window keeps asking when that window
    /// is closed: only its own window answers it, as Cancel.
    #[test]
    fn a_question_is_answered_by_its_own_window_only() {
        let mut dialogs = Dialogs {
            confirm_exit: true,
            about_open: true,
            ..Dialogs::default()
        };
        assert!(dialogs.close_window(window_id("About dm")));
        assert!(dialogs.confirm_exit, "closing About answered the question");
        assert!(dialogs.close_window(window_id("Exit dm?")));
        assert!(!dialogs.confirm_exit);
        assert!(!dialogs.close_window(window_id("Exit dm?")));
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    /// Opens one question of each kind, by its title, and says whether it is
    /// still open.
    type Question = (&'static str, fn(&mut Dialogs), fn(&Dialogs) -> bool);

    fn questions_asked() -> Vec<Question> {
        vec![
            ("Exit dm?", |d| d.confirm_exit = true, |d| d.confirm_exit),
            (
                "Keep running in the tray?",
                |d| d.confirm_close = true,
                |d| d.confirm_close,
            ),
            (
                "Delete unfinished downloads",
                |d| d.confirm_remove = Some(vec![1]),
                |d| d.confirm_remove.is_some(),
            ),
            (
                "Start outside the schedule?",
                |d| d.confirm_outside = Some(vec![1]),
                |d| d.confirm_outside.is_some(),
            ),
            (
                "Cancel this download?",
                |d| d.confirm_cancel = Some(1),
                |d| d.confirm_cancel.is_some(),
            ),
            (
                "Already in the list",
                |d| d.duplicate = Some(("http://h/a.zip".into(), 1)),
                |d| d.duplicate.is_some(),
            ),
            (
                "Name sent by the server",
                |d| {
                    d.name_ask = Some(NameAsk {
                        id: 1,
                        address_name: "a.zip".into(),
                        server_name: "b.zip".into(),
                    })
                },
                |d| d.name_ask.is_some(),
            ),
            (
                "Web page instead of the file",
                |d| d.page_ask = Some(1),
                |d| d.page_ask.is_some(),
            ),
            (
                "The file doesn't exist",
                |d| d.not_found = Some(1),
                |d| d.not_found.is_some(),
            ),
            (
                "Some are already in the list",
                |d| {
                    d.import_ask = Some(ImportAsk {
                        text: "http://h/a.zip".into(),
                        already: 1,
                        total: 1,
                    })
                },
                |d| d.import_ask.is_some(),
            ),
            (
                "Leave this site to the browser?",
                |d| d.skip_offer = Some("h".into()),
                |d| d.skip_offer.is_some(),
            ),
        ]
    }

    /// Each question is asked alone, keeps the clicks to itself, and is
    /// answered as Cancel by its own window's close button and by Escape,
    /// before any other window.
    #[test]
    fn each_question_is_closed_by_its_own_window_and_first_by_escape() {
        let mut dialogs = Dialogs::default();
        for (title, ask, asked) in questions_asked() {
            ask(&mut dialogs);
            assert!(dialogs.asking(), "{title}");
            assert_eq!(dialogs.questions(), vec![title]);
            assert!(dialogs.modal(), "{title} does not hold the window");
            assert!(dialogs.in_front(), "{title}");
            // Another window's close button leaves it asking.
            assert!(!dialogs.close_window(window_id("About dm")), "{title}");
            assert!(asked(&dialogs), "{title}");
            assert!(dialogs.close_window(window_id(title)), "{title}");
            assert!(!asked(&dialogs), "{title}");
            assert!(!dialogs.close_window(window_id(title)), "{title}");

            // Escape answers it before the window behind it.
            ask(&mut dialogs);
            dialogs.about_open = true;
            assert!(dialogs.close_top(&[window_id("About dm")]), "{title}");
            assert!(!asked(&dialogs), "{title}");
            assert!(dialogs.about_open, "Escape closed About before {title}");
            dialogs.about_open = false;
            assert!(!dialogs.asking());
        }
        // Remove's question is named by the title it last showed.
        dialogs.confirm_remove = Some(vec![1]);
        assert_eq!(dialogs.open_names(), ["Remove from the list"]);
        dialogs.remove_title = "Delete file";
        assert_eq!(dialogs.open_names(), ["Delete file"]);
    }

    fn properties() -> Properties {
        Properties {
            name: "a.zip".into(),
            description: String::new(),
            address: "http://h/a.zip".into(),
            login: String::new(),
            password: String::new(),
            says: String::new(),
            sums: Default::default(),
            check: String::new(),
            zip: None,
            files: Vec::new(),
            priorities: Vec::new(),
        }
    }

    /// Every window that is not a question, opened alone: Escape closes it,
    /// and so does its own window's close button; a window not open closes
    /// nothing.
    #[test]
    fn every_window_is_closed_by_escape_and_by_its_own_close_button() {
        type Window = (&'static str, fn(&mut Dialogs), fn(&Dialogs) -> bool);
        let windows: Vec<Window> = vec![
            ("Find", |d| d.find_open = true, |d| d.find_open),
            (
                "Speed limiter",
                |d| d.open_limiter(&Settings::default()),
                |d| d.limiter.is_some(),
            ),
            (
                "Scheduler",
                |d| d.open_scheduler(&Settings::default(), MAIN_QUEUE),
                |d| d.scheduler.is_some(),
            ),
            (
                "Update download link",
                |d| d.update_link = Some((1, String::new(), String::new())),
                |d| d.update_link.is_some(),
            ),
            (
                "Move to queue",
                |d| d.move_to_queue = Some((vec![1], String::new(), String::new())),
                |d| d.move_to_queue.is_some(),
            ),
            (
                "Move/Rename",
                |d| d.rename = Some(MoveRename::default()),
                |d| d.rename.is_some(),
            ),
            (
                "Import a list",
                |d| d.list_file = Some(ListFile::default()),
                |d| d.list_file.is_some(),
            ),
            (
                "Export the list",
                |d| {
                    d.list_file = Some(ListFile {
                        save: true,
                        ..ListFile::default()
                    })
                },
                |d| d.list_file.is_some(),
            ),
            ("Add URL", |d| d.open_add_url(None), |d| d.add_url.is_some()),
            (
                "Links on the clipboard",
                |d| d.clip_links = Some(vec![("http://h/a.zip".into(), true)]),
                |d| d.clip_links.is_some(),
            ),
            (
                "Add numbered series",
                |d| d.open_series(),
                |d| d.series.is_some(),
            ),
            (
                "Download links",
                |d| d.links = Some(LinksImport::default()),
                |d| d.links.is_some(),
            ),
            (
                "Grabber",
                |d| d.open_site_files(),
                |d| d.site_files.is_some(),
            ),
            (
                "Download details",
                |d| d.open_file_info(vec!["http://h/a.zip".into()]),
                |d| d.file_info.is_some(),
            ),
            (
                "Options",
                |d| d.open_options(&Settings::default()),
                |d| d.options.is_some(),
            ),
            ("About dm", |d| d.about_open = true, |d| d.about_open),
            ("Tip of the day", |d| d.tip = Some(0), |d| d.tip.is_some()),
            (
                "Add dm to your browser",
                |d| d.welcome = Some(Vec::new()),
                |d| d.welcome.is_some(),
            ),
        ];
        for (title, open, is_open) in windows {
            let mut dialogs = Dialogs::default();
            open(&mut dialogs);
            assert!(is_open(&dialogs), "{title} did not open");
            assert!(dialogs.open_names().iter().any(|n| n == title), "{title}");
            assert!(dialogs.in_front(), "{title}");
            assert!(dialogs.close_top(&[]), "Escape left {title} open");
            assert!(!is_open(&dialogs), "{title}");
            assert!(!dialogs.close_top(&[]), "Escape closed something else");

            open(&mut dialogs);
            assert!(!dialogs.close_window(window_id("Exit dm?")), "{title}");
            assert!(dialogs.close_window(window_id(title)), "{title}");
            assert!(!is_open(&dialogs), "{title}");
        }
    }

    /// Properties, Download complete and the progress windows close one at a
    /// time, the one in front first; a progress window closed is kept closed.
    #[test]
    fn the_windows_beside_each_other_close_front_first() {
        let mut dialogs = Dialogs::default();
        dialogs.properties.insert(7, properties());
        dialogs.properties.insert(8, properties());
        dialogs.complete.insert(7);
        dialogs.progress.insert(9);
        assert_eq!(
            dialogs.modal_layers().len(),
            2,
            "each Properties holds the window"
        );
        assert!(dialogs
            .open_names()
            .contains(&"File properties".to_string()));
        let progress = egui::Id::unique(("progress", 9_u64));
        let props_7 = egui::Id::unique(("properties", 7_u64));
        // Behind every window on screen: not closed by its own window's id.
        assert!(!dialogs.close_window(window_id("About dm")));
        // The progress window in front of the rest.
        assert!(dialogs.close_top(&[props_7, progress]));
        assert!(dialogs.progress.is_empty());
        assert!(dialogs.progress_closed.contains(&9));
        assert!(dialogs.close_window(props_7));
        assert!(!dialogs.properties.contains_key(&7));
        // None on screen: Properties before Download complete.
        assert!(dialogs.close_top(&[]));
        assert!(dialogs.properties.is_empty());
        assert!(!dialogs.complete.is_empty());
        assert!(dialogs.close_window(window_id("Download complete")));
        assert!(dialogs.complete.is_empty());
        // The windows a download opens on its own are not in front.
        dialogs.progress.insert(9);
        dialogs.complete.insert(9);
        assert!(!dialogs.in_front());
        assert!(!dialogs.modal());
        dialogs.resumable_seen.insert(9, true);
        dialogs.forget(9);
        assert!(dialogs.progress.is_empty() && dialogs.complete.is_empty());
        assert!(!dialogs.progress_closed.contains(&9) && dialogs.resumable_seen.is_empty());
    }

    /// Asked for again while open, a dialog is brought forward with what was
    /// typed in it, not started afresh.
    #[test]
    fn a_dialog_asked_for_again_is_raised_and_kept() {
        let mut dialogs = Dialogs::default();
        dialogs.open_add_url(Some("http://h/first.zip"));
        assert_eq!(
            dialogs.add_url.as_ref().unwrap().address,
            "http://h/first.zip"
        );
        dialogs.add_url.as_mut().unwrap().address = "typed".into();
        dialogs.open_add_url(Some("http://h/second.zip"));
        assert_eq!(dialogs.add_url.as_ref().unwrap().address, "typed");
        assert!(dialogs.raise.contains(&window_id("Add URL")));
        for open in [
            Dialogs::open_series as fn(&mut Dialogs),
            Dialogs::open_site_files,
        ] {
            open(&mut dialogs);
            open(&mut dialogs);
        }
        dialogs.open_limiter(&Settings::default());
        dialogs.open_limiter(&Settings::default());
        dialogs.open_scheduler(&Settings::default(), MAIN_QUEUE);
        dialogs.open_scheduler(&Settings::default(), MAIN_QUEUE);
        dialogs.open_options(&Settings::default());
        dialogs.open_options(&Settings::default());
        for title in [
            "Add numbered series",
            "Grabber",
            "Speed limiter",
            "Scheduler",
            "Options",
        ] {
            assert!(dialogs.raise.contains(&window_id(title)), "{title}");
        }
        // The keyboard is given to the dialog last asked for, once.
        assert!(dialogs.take_focus("Speed limiter"));
        assert!(!dialogs.take_focus("Speed limiter"));
        dialogs.focus_on("Grabber");
        assert!(!dialogs.take_focus("Add URL"));
        assert!(dialogs.take_focus("Grabber"));
    }

    /// A saved Grabber search opens the Grabber with everything it keeps.
    #[test]
    fn a_saved_search_fills_the_grabber() {
        let search = dm_shell::lists::Search {
            name: "Nightly".into(),
            page: "http://h/page.html".into(),
            types: vec!["zip".into(), "pdf".into()],
            depth: 2,
            include: vec!["docs".into()],
            exclude: vec!["old".into(), "tmp".into()],
            every_hours: 3,
            ..Default::default()
        };
        let mut dialogs = Dialogs::default();
        dialogs.open_site_files_with(&search);
        let d = dialogs.site_files.as_ref().unwrap();
        assert_eq!(
            (
                d.page.as_str(),
                d.types.as_str(),
                d.depth,
                d.include.as_str()
            ),
            ("http://h/page.html", "zip pdf", 2, "docs")
        );
        assert_eq!(
            (d.exclude.as_str(), d.name.as_str(), d.every_hours),
            ("old tmp", "Nightly", 3)
        );
        assert!(dialogs.take_focus("Grabber"));
    }

    /// A playlist already read opens with it, nothing more to read.
    #[test]
    fn a_playlist_already_read_opens_with_its_videos() {
        let mut dialogs = Dialogs::default();
        let list = dm_core::youtube::Playlist {
            title: "Talks".into(),
            ..Default::default()
        };
        dialogs.open_playlist_read("http://h/list".into(), list.clone(), "720".into());
        let d = dialogs.playlist.as_ref().unwrap();
        assert_eq!(
            (d.url.as_str(), d.quality.as_str()),
            ("http://h/list", "720")
        );
        let read = d.read.as_ref().unwrap().lock().unwrap().clone();
        assert_eq!(read, Some(Ok(list)));
        assert!(dialogs.take_focus("Playlist"));
    }
}
