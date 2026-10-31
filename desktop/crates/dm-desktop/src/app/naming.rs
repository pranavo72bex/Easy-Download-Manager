use super::*;

impl DmApp {
    /// Where a row not saved anywhere yet will go, for the Folder column
    /// to show until it is: the download folder, or the kind's folder under
    /// it where Options › Folders sorts by kind — as the queue places it.
    /// Nothing where that waits on the server: a name that does not say its
    /// kind (`download.php`, `.bin`) is placed by the type the server gives.
    /// Only shown, never written into the row: what a command
    /// reads of a row is what the person chose.
    pub fn planned_folder(
        &self,
        row: &Entry,
        categories: &mut dm_core::Categories,
    ) -> Option<String> {
        // Not for one finished or cancelled; a failed one still goes there
        // once retried, and lost its Folder.
        if !row.path.is_empty()
            || row.save_dir.is_some()
            || matches!(row.state, State::Completed | State::Cancelled)
        {
            return None;
        }
        // The folder it began in, once it has begun.
        let root = if row.began_in.is_empty() {
            self.applied.download_dir.clone()
        } else {
            std::path::PathBuf::from(&row.began_in)
        };
        if !categories.enabled {
            return Some(root.display().to_string());
        }
        let name = if row.filename.is_empty() {
            format::provisional(&row.url)
        } else {
            row.filename.clone()
        };
        categories.site = dm_core::category::host_of(&row.url);
        let kind = match kind_of(row).or_else(|| dm_core::category::by_name(&name)) {
            Some(kind) => kind,
            None if categories.custom_for(&name, &categories.site).is_some() => {
                dm_core::Category::Other
            }
            None => return None,
        };
        Some(categories.place(&root, kind, &name).display().to_string())
    }

    /// The name a row's file will have: for one not yet placed, with the
    /// ` (1)` it will get where its folder already has that name — a file
    /// added again said `big.bin` until it finished as `big (1).bin`.
    /// The folder is asked at most every two seconds per row.
    /// For every row not yet placed, how many rows before it are bound for the
    /// same folder under the same name: what `name_it_will_get_after` is told.
    /// One pass, in the order the rows were added.
    pub fn earlier_of_the_same_name(&self) -> HashMap<Id, usize> {
        let mut order: Vec<&Entry> = self.rows.iter().map(|r| &**r).collect();
        order.sort_by_key(|r| r.id);
        let mut seen: HashMap<(Option<String>, String), usize> = HashMap::new();
        let mut before = HashMap::new();
        for r in order {
            if r.state.settled() || !r.path.is_empty() {
                continue;
            }
            let count = seen.entry((r.save_dir.clone(), name_of(r))).or_insert(0);
            before.insert(r.id, *count);
            *count += 1;
        }
        before
    }

    pub fn name_it_will_get(&self, row: &Entry, categories: &mut dm_core::Categories) -> String {
        self.name_it_will_get_after(row, categories, None)
    }

    /// [`Self::name_it_will_get`], where how many earlier rows of the same name
    /// bound for the same folder take a free name first is already known
    /// (`earlier_of_the_same_name`), as a list of every row's name needs it
    /// once for all and not by a search of every row for each.
    pub fn name_it_will_get_after(
        &self,
        row: &Entry,
        categories: &mut dm_core::Categories,
        known_before: Option<usize>,
    ) -> String {
        let name = name_of(row);
        if !row.path.is_empty() || matches!(row.state, State::Completed | State::Cancelled) {
            return name;
        }
        let folder = row
            .save_dir
            .clone()
            .filter(|d| !d.trim().is_empty())
            .or_else(|| self.planned_folder(row, categories))
            .unwrap_or_else(|| self.applied.download_dir.display().to_string());
        let key = (row.id, std::path::Path::new(&folder).join(&name));
        thread_local! {
            static FREE: std::cell::RefCell<HashMap<(Id, std::path::PathBuf), (Instant, String)>> =
                std::cell::RefCell::new(HashMap::new());
        }
        FREE.with(|free| {
            let mut free = free.borrow_mut();
            if let Some((at, said)) = free.get(&key) {
                if at.elapsed() < Duration::from_secs(2) {
                    return said.clone();
                }
            }
            if free.len() > 2000 {
                free.clear();
            }
            // Each earlier row of the same name bound for the same folder,
            // and not yet placed, takes a free name first: three added for
            // later all read "slow (1).bin".
            let before = known_before.unwrap_or_else(|| {
                self.rows
                    .iter()
                    .filter(|r| {
                        r.id < row.id
                            && !r.state.settled()
                            && r.path.is_empty()
                            && r.save_dir == row.save_dir
                            && name_of(r) == name
                    })
                    .count()
            });
            let said =
                dm_core::download::nth_free_name(std::path::Path::new(&folder), &name, before);
            free.insert(key, (Instant::now(), said.clone()));
            said
        })
    }
}
