// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Grabber (Tasks › Grabber, B60, B61), for every window: a site's
//! pages searched for files, as typed; its templates; a search saved, to
//! run by itself; and what a run found. What each window draws is its own.

use std::sync::{Arc, Mutex};

use dm_shell::Settings;

use crate::i18n::t;

/// How the search of a site for files is going.
#[derive(Default, Debug)]
pub enum Grab {
    /// Not started.
    #[default]
    Idle,
    /// Pages are being read.
    Looking,
    /// The links found, each with whether it is ticked; how many pages were
    /// read; and whether the run's time ran out.
    Found(Vec<(String, bool)>, usize, bool),
    /// The search failed, with the reason.
    Failed(String),
}

/// The Site files dialog: a page to search for files, and how far to follow its links.
pub struct SiteFiles {
    /// The page to start from, as typed.
    pub page: String,
    /// The file types to list, as typed.
    pub types: String,
    /// Pages past the start page to read.
    pub depth: u8,
    /// Path filters, as typed: words separated by spaces.
    pub include: String,
    /// Paths to leave out, as typed.
    pub exclude: String,
    /// The name to save this search under, and how often it runs.
    pub name: String,
    /// How often a saved search runs, in hours.
    pub every_hours: u64,
    /// Minutes a run reads pages for; 0 is no limit.
    pub stop_after_minutes: u64,
    /// Whether pages on other sites are followed too.
    pub other_sites: bool,
    /// Whether files are saved in folders that mirror the site's.
    pub keep_layout: bool,
    /// Files smaller than this (KB) are left out; 0 is no floor.
    pub min_kb: u64,
    /// Files larger than this (KB) are left out; 0 is no ceiling.
    pub max_kb: u64,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
    /// How the running search is going.
    pub state: Arc<Mutex<Grab>>,
}

impl Default for SiteFiles {
    fn default() -> Self {
        SiteFiles {
            page: String::new(),
            types: dm_shell::lists::DEFAULT_TYPES.into(),
            depth: 0,
            include: String::new(),
            exclude: String::new(),
            name: String::new(),
            every_hours: 0,
            stop_after_minutes: 0,
            other_sites: false,
            keep_layout: false,
            min_kb: 0,
            max_kb: 0,
            says: String::new(),
            state: Arc::default(),
        }
    }
}

impl SiteFiles {
    /// What the dialog describes, as a search.
    pub fn search(&self) -> dm_shell::lists::Search {
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| c == ',' || c.is_whitespace())
                .filter(|w| !w.is_empty())
                .map(|w| w.trim_start_matches('.').to_string())
                .collect()
        };
        dm_shell::lists::Search {
            name: self.name.trim().to_string(),
            page: self.page.trim().to_string(),
            types: words(&self.types),
            depth: self.depth.min(dm_shell::lists::MAX_DEPTH),
            other_sites: self.other_sites,
            keep_layout: self.keep_layout,
            include: words(&self.include),
            exclude: words(&self.exclude),
            every_hours: self.every_hours,
            stop_after_minutes: self.stop_after_minutes,
            last_run: 0,
            seen: Vec::new(),
            min_kb: self.min_kb,
            max_kb: self.max_kb,
        }
    }
}

/// Reference A's project templates: the kinds of file a search is usually
/// for, filling in the types (their words in the language in force, then
/// the types).
pub fn templates() -> [(&'static str, &'static str); 5] {
    [
        (t("Pictures"), "jpg jpeg png gif webp bmp tif tiff svg"),
        (
            t("Documents"),
            "pdf doc docx xls xlsx ppt pptx odt ods odp rtf txt epub",
        ),
        (
            t("Videos and music"),
            "mp4 mkv avi mov wmv webm flv m4v mp3 m4a aac wav flac ogg opus",
        ),
        (
            t("Archives and programs"),
            "zip rar 7z gz tar bz2 xz exe msi iso dmg apk",
        ),
        (t("Every file"), ""),
    ]
}

/// The settings with `search` saved under its name (what it found before
/// kept; its first run on the timer one interval from now, `now` in seconds
/// since 1970), or why it cannot be saved.
pub fn saved(
    settings: &Settings,
    search: dm_shell::lists::Search,
    now: u64,
) -> Result<Settings, &'static str> {
    if search.name.is_empty() {
        return Err(t("A saved search needs a name."));
    }
    if search.page.is_empty() {
        return Err(t("A saved search needs a start page."));
    }
    let mut settings = settings.clone();
    let mut search = search;
    search.last_run = now;
    if let Some(before) = settings
        .site_searches
        .iter()
        .find(|s| s.name == search.name)
    {
        search.seen.clone_from(&before.seen);
        search.last_run = before.last_run;
    }
    settings.site_searches.retain(|s| s.name != search.name);
    settings.site_searches.push(search);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_is_saved_only_with_a_name_and_a_page_and_keeps_what_it_saw() {
        let settings = Settings::default();
        let mut d = SiteFiles {
            page: "https://e.com/files".into(),
            ..SiteFiles::default()
        };
        assert_eq!(
            saved(&settings, d.search(), 1).unwrap_err(),
            "A saved search needs a name."
        );
        d.name = "Nightly".into();
        let once = saved(&settings, d.search(), 100).unwrap();
        assert_eq!(once.site_searches.len(), 1);
        assert_eq!(once.site_searches[0].last_run, 100);
        let mut again = once.clone();
        again.site_searches[0].seen = vec!["https://e.com/a.zip".into()];
        let twice = saved(&again, d.search(), 200).unwrap();
        assert_eq!(twice.site_searches.len(), 1, "saved again under its name");
        assert_eq!(
            twice.site_searches[0].seen,
            vec!["https://e.com/a.zip".to_string()]
        );
        assert_eq!(twice.site_searches[0].last_run, 100);
    }
}
