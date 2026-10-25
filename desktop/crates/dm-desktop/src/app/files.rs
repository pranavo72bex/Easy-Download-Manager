use super::*;

impl DmApp {
    /// Opens `path` with an application the person picks: from the row
    /// menu and from Download complete.
    pub fn open_with_chosen(&mut self, path: &str) {
        // Nothing to pick an application with: said, rather than a button
        // that did nothing.
        if !self.backend.platform.can_choose_file() {
            self.said = crate::i18n::Said::new(
                "No file chooser on this desktop to pick an application with; Open uses the file's usual one.",
                &[],
            );
        } else if let Some(app) = self.backend.platform.choose_app() {
            if let Err(e) = self.backend.platform.open_with(path, &app) {
                self.said = e.into();
            }
        }
    }

    /// Opens `path` with the system's opener, off the window's thread.
    pub fn open_file(&self, path: &str) {
        let said = Arc::clone(&self.opener_said);
        let report: crate::power::Report = Box::new(move |why| *dm_core::lock(&said) = Some(why));
        self.backend.platform.open_later(path, report);
    }

    /// Shows `path` in the file manager, off the window's thread.
    /// Extracts a finished ZIP into a new folder beside it, off the
    /// window's thread; the answer arrives in the status line the way an
    /// opener's does. Nothing in it is opened or run.
    pub fn extract_zip(&mut self, path: &str, url: &str, into: Option<std::path::PathBuf>) {
        let said = Arc::clone(&self.opener_said);
        let (path, url) = (std::path::PathBuf::from(path), url.to_string());
        self.said = crate::i18n::t("Extracting…").into();
        let spawned = std::thread::Builder::new()
            .name("dm-extract".into())
            .spawn(move || {
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let origin = (!url.is_empty()).then_some(url.as_str());
                let limits = dm_core::extract::Limits::default();
                let answer = match dm_core::extract::extract_in(
                    &path,
                    into.as_deref(),
                    &limits,
                    &cancel,
                    origin,
                    &|_, _| {},
                ) {
                    Ok(done) => format!(
                        "Extracted {} {} to {}",
                        done.files,
                        if done.files == 1 { "file" } else { "files" },
                        done.folder.display()
                    ),
                    Err(e) => format!("Not extracted: {e}"),
                };
                *dm_core::lock(&said) = Some(answer);
            });
        if let Err(e) = spawned {
            self.said = format!("Not extracted: {e}").into();
        }
    }

    /// Uploads a finished file to Gofile and puts the link to it on the
    /// clipboard. Only on request: nothing is sent otherwise.
    pub fn share_on_gofile(&mut self, path: &str) {
        let done = Arc::clone(&self.gofile_done);
        let path = std::path::PathBuf::from(path);
        let client = self.client();
        self.said = crate::i18n::t("Uploading to Gofile…").into();
        let spawned = std::thread::Builder::new()
            .name("dm-gofile".into())
            .spawn(move || {
                let answer = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())
                    .and_then(|rt| {
                        rt.block_on(dm_core::gofile::upload(&client, &path))
                            .map(|up| up.page)
                    });
                *dm_core::lock(&done) = Some(answer);
            });
        if let Err(e) = spawned {
            self.said = format!("Not shared: {e}").into();
        }
    }

    pub fn reveal_file(&self, path: &str) {
        let said = Arc::clone(&self.opener_said);
        let report: crate::power::Report = Box::new(move |why| *dm_core::lock(&said) = Some(why));
        self.backend.platform.reveal_later(path, report);
    }

    /// Whether the schedule is closed with downloads waiting on it, and when
    /// it next opens.
    pub fn schedule_holds(&self) -> Option<Option<String>> {
        let schedule = &self.applied.schedule;
        if !schedule.enabled || schedule.open() {
            return None;
        }
        let waiting = self.rows.iter().any(|r| {
            r.state == State::Queued || (r.state == State::Paused && r.paused_by_schedule)
        });
        waiting.then(|| opens_next(schedule))
    }
}
