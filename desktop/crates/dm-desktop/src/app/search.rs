use super::*;

impl DmApp {
    /// Runs a site search in the background; what it finds that is not
    /// already in the list is added for later when it comes back.
    pub fn run_search(&mut self, search: dm_shell::lists::Search) {
        let client = self.client();
        let results = Arc::clone(&self.search_results);
        let queue = self.backend.queue.clone();
        self.searches_running += 1;
        self.backend.runtime.spawn(async move {
            let options = queue.options().await;
            let found = dm_shell::lists::crawl_with(&client, &search, Some(&options)).await;
            dm_core::lock(&results).push((search.name, found));
        });
    }

    /// Takes in what finished searches found, and starts any saved search
    /// whose time has come. Looked at once a minute, and whenever a search
    /// is out.
    pub(super) fn tick_searches(&mut self) {
        let done: Vec<_> = std::mem::take(&mut *dm_core::lock(&self.search_results));
        for (name, found) in done {
            self.searches_running = self.searches_running.saturating_sub(1);
            match found {
                Ok(found) => {
                    // New: neither in the list nor found by this search
                    // before, compared without logins, so clearing the list
                    // or a site login on the row does not bring it back.
                    let seen: std::collections::HashSet<String> = self
                        .applied
                        .site_searches
                        .iter()
                        .find(|s| s.name == name)
                        .map(|s| s.seen.iter().cloned().collect())
                        .unwrap_or_default();
                    let listed: std::collections::HashSet<String> = self
                        .rows
                        .iter()
                        .map(|r| dm_shell::lists::bare(&r.url))
                        .collect();
                    let new: Vec<String> = found
                        .files
                        .iter()
                        .filter(|f| {
                            let f = dm_shell::lists::bare(f);
                            !listed.contains(&f) && !seen.contains(&f)
                        })
                        .cloned()
                        .collect();
                    // Remembered: what was added now, and what is already in
                    // the list. A file that could not be added is not, so the
                    // next run offers it again.
                    let mut remember: Vec<String> = found
                        .files
                        .iter()
                        .map(|f| dm_shell::lists::bare(f))
                        .filter(|f| listed.contains(f))
                        .collect();
                    if new.is_empty() {
                        self.said =
                            crate::i18n::Said::naming("The search {} found nothing new.", &[&name]);
                    } else {
                        let with = dm_core::AddOptions {
                            later: true,
                            queue: dm_core::MAIN_QUEUE.into(),
                            ..Default::default()
                        };
                        let added = self.add(&new, with);
                        let now: std::collections::HashSet<String> = self
                            .rows
                            .iter()
                            .map(|r| dm_shell::lists::bare(&r.url))
                            .collect();
                        remember.extend(
                            new.iter()
                                .map(|f| dm_shell::lists::bare(f))
                                .filter(|f| now.contains(f)),
                        );
                        match added {
                            Ok(n) => {
                                self.said = crate::i18n::Said::counting_naming(
                                    "Added {} download to {}.||Added {} downloads to {}.",
                                    n as u64,
                                    &[&n, &dm_core::MAIN_QUEUE],
                                );
                            }
                            Err(e) => self.problems.push(format!(
                                "the search {name} found files that could not be added: {e}"
                            )),
                        }
                    }
                    let mut settings = self.applied.clone();
                    if let Some(search) = settings.site_searches.iter_mut().find(|s| s.name == name)
                    {
                        for f in remember {
                            if !search.seen.contains(&f) {
                                search.seen.push(f);
                            }
                        }
                        let over = search.seen.len().saturating_sub(dm_shell::lists::MOST_SEEN);
                        search.seen.drain(..over);
                        if let Err(e) = self.apply_settings(settings) {
                            self.problem(format!(
                                "what the search {name} has seen could not be saved: {e}"
                            ));
                        }
                    }
                }
                Err(e) => self.problems.push(format!("the search {name} failed: {e}")),
            }
        }

        let now = Instant::now();
        if self
            .searches_checked
            .is_some_and(|at| now.duration_since(at) < Duration::from_secs(60))
        {
            return;
        }
        self.searches_checked = Some(now);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let due: Vec<dm_shell::lists::Search> = self
            .applied
            .site_searches
            .iter()
            .filter(|s| s.due(epoch))
            .cloned()
            .collect();
        if due.is_empty() {
            return;
        }
        let mut settings = self.applied.clone();
        for search in &mut settings.site_searches {
            if due.iter().any(|d| d.name == search.name) {
                search.last_run = epoch;
            }
        }
        if let Err(e) = self.apply_settings(settings) {
            // Still run, and not again for a minute; but said, since an
            // unsaved last run means the search runs again at every launch.
            self.problem(format!(
                "when the saved searches last ran could not be saved: {e}"
            ));
        }
        for search in due {
            self.run_search(search);
        }
    }

    /// Selects the next row after the selection whose name has the text.
    pub fn find_next(&mut self) {
        let needle = self.dialogs.find_text.to_lowercase();
        if needle.is_empty() {
            return;
        }
        let rows = self.visible();
        let after = rows
            .iter()
            .position(|r| self.selected.contains(&r.id))
            .map(|i| i + 1)
            .unwrap_or(0);
        let found = rows
            .iter()
            .cycle()
            .skip(after)
            .take(rows.len())
            .find(|r| name_of(r).to_lowercase().contains(&needle));
        match found {
            Some(row) => {
                self.selected = BTreeSet::from([row.id]);
                // Brought into view, the keys going on from it.
                self.cursor = Some(row.id);
                self.anchor = Some(row.id);
                self.scroll_to = Some(row.id);
                self.said = crate::i18n::Said::new("Found {}.", &[&name_of(row)]);
            }
            None => {
                self.said =
                    crate::i18n::Said::new("Nothing is called “{}”.", &[&self.dialogs.find_text])
            }
        }
    }
}
