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
    pub fn open_add_url(&mut self, clipboard: Option<String>) {
        self.focus_on("Add URL");
        if self.keep("Add URL", self.add_url.is_some()) {
            return;
        }
        let address = clipboard
            .as_deref()
            .and_then(crate::app::looks_downloadable)
            .unwrap_or_default();
        self.add_url = Some(AddUrl {
            address,
            ..AddUrl::default()
        });
    }

    pub fn open_file_info(&mut self, urls: Vec<String>) {
        // The keyboard comes off the list behind it.
        self.focus_on("Download details");
        self.file_info = Some(FileInfo {
            urls,
            queue: MAIN_QUEUE.into(),
            ..FileInfo::default()
        });
    }

    /// The dialog for a video the browser's panel handed over.
    pub fn open_handed(&mut self, request: dm_shell::host::Request) {
        self.focus_on("Download details");
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
        let client = dm_core::proxy::client_with(&backend.settings().proxy)
            .unwrap_or_else(|_| dm_core::client());
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
        let sign_in = dm_core::cookies::SignIn::new(&request.url, &request.cookies);
        for url in request.links.clone() {
            let (found, client, gate) = (found.clone(), client.clone(), gate.clone());
            let sign_in = sign_in.clone();
            let page = request.referrer.clone();
            backend.runtime.spawn(async move {
                let Ok(_turn) = gate.acquire().await else {
                    return;
                };
                let asked = tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    dm_core::probe::probe(
                        &client,
                        &url,
                        Some(&page).filter(|p| !p.is_empty()).map(String::as_str),
                        sign_in.as_ref(),
                    ),
                )
                .await;
                let failed = match &asked {
                    Ok(Err(e)) => Some(e.to_string()),
                    Err(_) => Some("the server did not answer in time".to_string()),
                    Ok(Ok(_)) => None,
                };
                // Said, rather than "…" for ever as though still asking.
                if let Some(why) = failed {
                    dm_core::lock(&found).insert(
                        url,
                        Found {
                            failed: Some(why),
                            ..Found::default()
                        },
                    );
                } else if let Ok(Ok(probe)) = asked {
                    let kind =
                        dm_core::category::classify(&probe.filename, probe.content_type.as_deref());
                    dm_core::lock(&found).insert(
                        url,
                        Found {
                            name: probe.filename,
                            kind: crate::app::category_word(kind),
                            size: probe.size,
                            html: dm_core::ytdlp::is_page(probe.content_type.as_deref()),
                            failed: None,
                        },
                    );
                }
            });
        }
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
        });
    }

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

    pub fn open_series(&mut self) {
        self.focus_on("Add numbered series");
        if !self.keep("Add numbered series", self.series.is_some()) {
            self.series = Some(Default::default());
        }
    }

    pub fn open_site_files(&mut self) {
        self.focus_on("Site files");
        if !self.keep("Site files", self.site_files.is_some()) {
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
            || self.confirm_remove.is_some()
            || self.confirm_outside.is_some()
            || self.confirm_cancel.is_some()
            || self.duplicate.is_some()
            || self.import_ask.is_some()
            || self.skip_offer.is_some()
    }

    /// The titles of the questions waiting for an answer.
    pub(super) fn questions(&self) -> Vec<&'static str> {
        [
            (self.confirm_exit, "Exit dm?"),
            (self.confirm_remove.is_some(), "Delete unfinished downloads"),
            (
                self.confirm_outside.is_some(),
                "Start outside the schedule?",
            ),
            (self.confirm_cancel.is_some(), "Cancel this download?"),
            (self.duplicate.is_some(), "Already in the list"),
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
            (self.confirm_remove.is_some(), "Delete unfinished downloads"),
            (
                self.confirm_outside.is_some(),
                "Start outside the schedule?",
            ),
            (self.confirm_cancel.is_some(), "Cancel this download?"),
            (self.duplicate.is_some(), "Already in the list"),
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
            (self.site_files.is_some(), "Site files"),
            (self.update_link.is_some(), "Update download link"),
            (self.move_to_queue.is_some(), "Move to queue"),
            (self.rename.is_some(), "Move/Rename"),
            (self.scheduler.is_some(), "Scheduler"),
            (self.limiter.is_some(), "Speed limiter"),
            (self.options.is_some(), "Options"),
            (self.about_open, "About dm"),
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

    pub fn open_scheduler(&mut self, settings: &Settings) {
        if self.keep("Scheduler", self.scheduler.is_some()) {
            return;
        }
        // No days kept means every day, shown as every day ticked.
        let mut schedule = settings.schedule.clone();
        if schedule.days.is_empty() {
            schedule.days = (0..=6).collect();
        }
        self.scheduler = Some((schedule, String::new()));
    }

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
        ));
    }

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
        say(self.site_files.is_some(), "Site files");
        say(self.update_link.is_some(), "Update download link");
        say(self.move_to_queue.is_some(), "Move to queue");
        say(self.rename.is_some(), "Move/Rename");
        say(self.find_open, "Find");
        say(self.scheduler.is_some(), "Scheduler");
        say(self.limiter.is_some(), "Speed limiter");
        say(self.options.is_some(), "Options");
        say(self.about_open, "About dm");
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
            || self.confirm_remove.take().is_some()
            || self.confirm_outside.take().is_some()
            || self.confirm_cancel.take().is_some()
            || self.duplicate.take().is_some()
            || self.import_ask.take().is_some()
            || self.skip_offer.take().is_some()
        {
            return true;
        }
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
            FileInfo,
            Options,
            About,
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
            (self.site_files.is_some(), "Site files", Shut::SiteFiles),
            (self.file_info.is_some(), "Download details", Shut::FileInfo),
            (self.options.is_some(), "Options", Shut::Options),
            (self.about_open, "About dm", Shut::About),
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
        let Some((_, front)) = open
            .into_iter()
            .enumerate()
            .max_by_key(|(n, (id, _))| (place(id), std::cmp::Reverse(*n)))
            .map(|(_, chosen)| chosen)
        else {
            return false;
        };
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
            Shut::SiteFiles => self.site_files = None,
            Shut::FileInfo => self.file_info = None,
            Shut::Options => self.options = None,
            Shut::About => self.about_open = false,
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
    }
}
