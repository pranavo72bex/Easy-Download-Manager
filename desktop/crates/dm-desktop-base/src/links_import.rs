// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Download links", for every window: a page's links handed over by the
//! browser, each one's server asked for its name, kind and size, filtered
//! (web pages, pictures, repeats, types), ticked, given a folder, and
//! added. What each window draws is its own.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use dm_core::AddOptions;

use crate::i18n::{t, tn};

/// What dm learned of one link by asking its server.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Found {
    /// The name the server gave the link.
    pub name: String,
    /// The kind of file it is, as its server says.
    pub kind: String,
    /// The size in bytes, where the server said.
    pub size: Option<u64>,
    /// Whether it is a web page rather than a file.
    pub html: bool,
    /// Why the server could not be asked about it.
    pub failed: Option<String>,
}

/// Where the links ticked in "Download links" are saved, as reference A
/// offers: each to its category's folder, all to one category's, or all to
/// one folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveTo {
    /// Each file to the folder of its category.
    #[default]
    ByCategory,
    /// Every file to one category's folder (`LinksImport::category`).
    OneCategory,
    /// Every file to one folder (`LinksImport::folder`).
    OneFolder,
}

/// The import dialog for a page's links: each checked or not, with its
/// name, kind and size as its server tells them.
#[derive(Default)]
pub struct LinksImport {
    /// The page the links came from.
    pub page: String,
    /// The sign-in the browser handed over for the page's own server, and
    /// that server's address: given to the links on that server only.
    pub sign_in: Option<dm_core::cookies::SignIn>,
    /// The cookies the browser handed over, given to the links on the page's own server.
    pub cookies: String,
    /// Each link's address, and whether it is ticked.
    pub items: Vec<(String, bool)>,
    /// Those already in dm's list (bare addresses): offered unticked and
    /// said so, rather than fetched again as `name (1).zip` without a word.
    pub listed: HashSet<String>,
    /// Links unticked when their server refused them, once, so a person
    /// can tick one again (they were added and failed).
    pub refused: HashSet<String>,
    /// Whether web pages are left out of the list.
    pub hide_html: bool,
    /// File types to list alone, as typed ("zip pdf"); empty lists all.
    pub types: String,
    /// What each link's server said, by address.
    pub found: Arc<Mutex<HashMap<String, Found>>>,
    /// Whether pictures are left out of the list (reference A's "Hide images
    /// located on this web page", on as there).
    pub hide_images: bool,
    /// Whether an address the page links to more than once is listed once.
    pub hide_repeats: bool,
    /// Where the links ticked are saved.
    pub save_to: SaveTo,
    /// The category every file goes to, for `SaveTo::OneCategory`: a kind's
    /// word or a person's own category's name.
    pub category: String,
    /// The folder every file goes to, for `SaveTo::OneFolder`, as typed.
    pub folder: String,
    /// The row whose name and address are being edited, by its place.
    pub editing: Option<usize>,
    /// Names given with Edit, by address.
    pub names: HashMap<String, String>,
    /// Each link's words on the page, by address, as the browser sent them.
    pub texts: BTreeMap<String, String>,
    /// Whether those words become the downloads' descriptions (reference A's
    /// "Use link texts as descriptions", on as there).
    pub use_texts: bool,
    /// The queue "Add for later" puts them in; the Main queue at first.
    pub queue: String,
}

impl LinksImport {
    /// "Download links" for the page's links the browser handed over:
    /// every link not already listed ticked, and each one's server asked
    /// (`lookups::probe_links`, six at a time), `told` called as each says
    /// something, so the window can draw it.
    pub fn open(
        request: dm_shell::host::Request,
        backend: &dm_shell::app::backend::Backend,
        told: impl Fn() + Send + Sync + 'static,
    ) -> LinksImport {
        let found: Arc<Mutex<HashMap<String, Found>>> = Default::default();
        let sign_in = dm_core::cookies::SignIn::new(&request.url, &request.cookies);
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
                        kind: dm_shell::app::names::category_word(kind),
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
                told();
            },
        );
        let listed: HashSet<String> = backend
            .list()
            .iter()
            .filter(|r| r.state != dm_core::State::Cancelled)
            .map(|r| dm_shell::lists::bare(&r.url))
            .collect();
        LinksImport {
            page: request.referrer,
            sign_in,
            cookies: request.cookies,
            items: request
                .links
                .into_iter()
                .map(|u| {
                    let new = !listed.contains(&dm_shell::lists::bare(&u));
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
            category: dm_shell::app::names::category_word(dm_core::Category::Archives),
            folder: String::new(),
            editing: None,
            // The names the page's `download` attributes give, as Edit would.
            names: request.names.into_iter().collect(),
            texts: request.texts,
            use_texts: true,
            queue: dm_core::MAIN_QUEUE.to_string(),
        }
    }

    /// A link that is a web page rather than a file: by what its server
    /// said, or before it answers, by the look of its address.
    pub fn is_html(&self, url: &str) -> bool {
        if let Some(found) = dm_core::lock(&self.found).get(url) {
            return found.html;
        }
        dm_shell::app::links::looks_like_page(url)
    }

    /// A link that is a picture: by what its server said, or before it
    /// answers, by its address.
    pub fn is_image(&self, url: &str) -> bool {
        let name = dm_core::lock(&self.found)
            .get(url)
            .map(|f| f.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| dm_shell::app::links::name_in_address(url).to_string());
        dm_shell::app::links::is_picture(&name)
    }

    /// Which rows are listed, in order: what the filters leave, and of an
    /// address the page links to more than once, its first.
    pub fn visible(&self) -> Vec<bool> {
        let mut seen = HashSet::new();
        self.items
            .iter()
            .map(|(url, _)| {
                let first = seen.insert(dm_shell::lists::bare(url));
                self.shown(url) && (first || !self.hide_repeats)
            })
            .collect()
    }

    /// Whether the filters leave `url` in the list.
    pub fn shown(&self, url: &str) -> bool {
        if self.hide_html && self.is_html(url) {
            return false;
        }
        if self.hide_images && self.is_image(url) {
            return false;
        }
        let wanted = dm_shell::app::links::wanted_types(&self.types);
        if wanted.is_empty() {
            return true;
        }
        // By the name its server gave, or before it answers, its address.
        let name = dm_core::lock(&self.found)
            .get(url)
            .map(|f| f.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| dm_shell::app::links::name_in_address(url).to_string());
        dm_shell::app::links::of_wanted_type(&name, &wanted)
    }

    /// The links OK would add: checked, and listed.
    pub fn chosen(&self) -> Vec<String> {
        self.items
            .iter()
            .zip(self.visible())
            .filter(|((_, on), listed)| *on && *listed)
            .map(|((url, _), _)| url.clone())
            .collect()
    }

    /// The folder the links go to where the choice names one: `None` is each
    /// to the folder of its category, as dm sorts a download.
    pub fn save_dir(
        &self,
        root: &std::path::Path,
        categories: &dm_core::Categories,
    ) -> Option<String> {
        match self.save_to {
            SaveTo::ByCategory => None,
            SaveTo::OneFolder => {
                let folder = self.folder.trim();
                (!folder.is_empty()).then(|| folder.to_string())
            }
            SaveTo::OneCategory => Some(
                dm_shell::app::names::category_folder(&self.category, root, categories)
                    .display()
                    .to_string(),
            ),
        }
    }

    /// The folder `url` will be saved to, as the Save to column says it.
    pub fn folder_of(
        &self,
        url: &str,
        root: &std::path::Path,
        categories: &dm_core::Categories,
    ) -> String {
        if let Some(dir) = self.save_dir(root, categories) {
            return dir;
        }
        let name = self.name_of(url);
        categories
            .destination(root, dm_core::classify(&name, None))
            .display()
            .to_string()
    }

    /// The name `url` will be saved under: the one given with Edit, else its
    /// server's, else its address's.
    pub fn name_of(&self, url: &str) -> String {
        if let Some(name) = self.names.get(url).filter(|n| !n.trim().is_empty()) {
            return name.trim().to_string();
        }
        dm_core::lock(&self.found)
            .get(url)
            .map(|f| f.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| dm_shell::app::names::provisional(url))
    }

    /// Unticks, once, each link whose server refused it, so a person can
    /// tick one again (they were added and failed). Called as the dialog is
    /// drawn.
    pub fn untick_refused(&mut self) {
        let found = dm_core::lock(&self.found).clone();
        for (url, on) in self.items.iter_mut() {
            if found.get(url.as_str()).is_some_and(|f| f.failed.is_some())
                && self.refused.insert(url.clone())
            {
                *on = false;
            }
        }
    }

    /// What Start now (`later` false) or Add for later adds: each link with
    /// what is its own (the page's sign-in for the links on its own server
    /// and no others, a name given with Edit, its words on the page as its
    /// description where that is asked for), and the sizes the servers
    /// told, kept on the rows until each learns its own. Every file to one
    /// folder with no folder typed is said.
    #[allow(clippy::type_complexity)]
    pub fn adds(
        &self,
        later: bool,
        root: &std::path::Path,
        categories: &dm_core::Categories,
    ) -> Result<(Vec<(String, AddOptions)>, HashMap<String, u64>), &'static str> {
        if self.save_to == SaveTo::OneFolder && self.folder.trim().is_empty() {
            return Err(t("Choose the folder every file goes to."));
        }
        let queue = self.queue_or_main();
        let with = AddOptions {
            later,
            queue: if later { queue } else { String::new() },
            referrer: self.page.clone(),
            save_dir: self.save_dir(root, categories),
            ..AddOptions::default()
        };
        let sizes: HashMap<String, u64> = dm_core::lock(&self.found)
            .iter()
            .filter_map(|(url, found)| found.size.map(|size| (url.clone(), size)))
            .collect();
        let items = self
            .chosen()
            .into_iter()
            .map(|url| {
                let signed = self
                    .sign_in
                    .as_ref()
                    .is_some_and(|s| s.for_url(&url).is_some());
                let own = AddOptions {
                    cookies: if signed {
                        self.cookies.clone()
                    } else {
                        String::new()
                    },
                    save_name: self
                        .names
                        .get(&url)
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty()),
                    description: if self.use_texts {
                        self.texts.get(&url).cloned().unwrap_or_default()
                    } else {
                        String::new()
                    },
                    ..with.clone()
                };
                (url, own)
            })
            .collect();
        Ok((items, sizes))
    }

    /// The queue Add for later puts them in: the one chosen, else the Main
    /// queue.
    pub fn queue_or_main(&self) -> String {
        if self.queue.trim().is_empty() {
            dm_core::MAIN_QUEUE.to_string()
        } else {
            self.queue.clone()
        }
    }
}

/// What is said once `n` links went in: added to `queue` for later, or
/// started.
pub fn added_words(n: usize, later: bool, queue: &str) -> String {
    if later {
        tn(
            "Added {} download to {}.||Added {} downloads to {}.",
            n as u64,
            &[&n, &crate::i18n::name(queue)],
        )
    } else {
        tn(
            "Started {} download.||Started {} downloads.",
            n as u64,
            &[&n],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(urls: &[&str]) -> LinksImport {
        LinksImport {
            items: urls.iter().map(|u| (u.to_string(), true)).collect(),
            hide_html: true,
            hide_images: true,
            hide_repeats: true,
            use_texts: true,
            queue: dm_core::MAIN_QUEUE.into(),
            ..LinksImport::default()
        }
    }

    #[test]
    fn pages_pictures_and_repeats_are_left_out_and_types_filter() {
        let mut d = links(&[
            "https://e.com/a.zip",
            "https://e.com/page.html",
            "https://e.com/pic.png",
            "https://e.com/a.zip",
            "https://e.com/b.pdf",
        ]);
        assert_eq!(d.visible(), vec![true, false, false, false, true]);
        d.types = "pdf".into();
        assert_eq!(d.chosen(), vec!["https://e.com/b.pdf".to_string()]);
    }

    #[test]
    fn what_is_added_carries_the_page_and_says_how_many() {
        let mut d = links(&["https://e.com/a.zip"]);
        d.page = "https://e.com/".into();
        d.texts
            .insert("https://e.com/a.zip".into(), "The archive".into());
        let root = std::path::Path::new("/tmp");
        let (items, _) = d.adds(true, root, &dm_core::Categories::default()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1.referrer, "https://e.com/");
        assert_eq!(items[0].1.description, "The archive");
        assert_eq!(items[0].1.queue, dm_core::MAIN_QUEUE);
        d.save_to = SaveTo::OneFolder;
        assert_eq!(
            d.adds(false, root, &dm_core::Categories::default())
                .unwrap_err(),
            "Choose the folder every file goes to."
        );
        assert_eq!(added_words(2, false, ""), "Started 2 downloads.");
    }
}
