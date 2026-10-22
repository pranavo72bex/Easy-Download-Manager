//! The window's dialogs: Add URL, Download details, a progress window per
//! download, Download complete, Properties, numbered series, files on a
//! site, Update download link, Move to queue, Move/Rename, Find,
//! Scheduler, Speed limiter, Options and About.
//!
//! Each is drawn from state held here and acts on the app when its button
//! is pressed; none of them reaches the queue except through the backend.

use crate::i18n::{t, tf};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use dm_core::{AddOptions, Category, Id, Schedule, State, MAIN_QUEUE};
use dm_shell::settings::SiteLogin;
use dm_shell::Settings;
use egui::{Color32, RichText};

use crate::app::{category_word, name_of, status_of, DmApp};
use crate::format;

/// The warning colour of the look in force: a fixed orange read at
/// 2.9:1 on the light list.
fn warn(ui: &egui::Ui) -> Color32 {
    crate::theme::Tokens::get(ui.ctx()).warn
}

/// Move/Rename: the finished downloads it moves, the new name where it is
/// one, and the folder they go to.
#[derive(Debug, Clone, Default)]
pub struct MoveRename {
    pub ids: Vec<Id>,
    pub name: String,
    pub folder: String,
}

#[derive(Default)]
pub struct AddUrl {
    pub address: String,
    pub authorize: bool,
    pub login: String,
    pub password: String,
    pub says: String,
}

/// "Download details": where and how, before a download starts.
#[derive(Default)]
pub struct FileInfo {
    pub urls: Vec<String>,
    /// The page a handed-over video came from, sent as Referer.
    pub referrer: String,
    /// The quality picked in the browser's panel, and what its list said.
    pub quality: Option<String>,
    pub label: Option<String>,
    pub folder: String,
    pub name: String,
    pub description: String,
    pub queue: String,
    pub says: String,
    /// Handed over by the browser, rather than typed in: cancelling it is
    /// a word against that site's downloads being handed over.
    pub handed: bool,
    /// The site's sign-in the browser handed over with it, if any.
    pub cookies: String,
    /// Those cookies are for this download alone.
    pub cookies_once: bool,
    /// Browse… was pressed on a desktop with no folder chooser: said in
    /// this opening of the dialog only (gap 133: it stayed for the next).
    pub no_chooser: bool,
    /// The name the server gives the file, once asked (gap 138), and
    /// whether a person typed in Save as since, which it then leaves alone.
    pub probed: Option<std::sync::Arc<std::sync::Mutex<Option<Probed>>>>,
    pub name_typed: bool,
}

/// What Download details learned by asking the server: the name the file
/// will have, where it gives one, and its size, where it says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probed {
    pub name: Option<String>,
    pub size: Option<u64>,
    /// Why the server would not give the file, where it said: a 404 was
    /// found only once the download had failed (UI round 10).
    pub problem: Option<String>,
    /// A web page rather than a file (a video site's watch page): dm finds
    /// the video in it when it starts, so nothing is wrong to say.
    pub page: bool,
}

/// The name an HLS stream at `url` is saved under: the engine's own naming
/// for streams (a manifest called `index.m3u8` takes its folder's name),
/// as a transport stream, and as an MP4 where `remux` puts it into one
/// (gap 156). `None` for an address that is not a stream.
pub fn stream_name(url: &str, content_type: Option<&str>, remux: bool) -> Option<String> {
    if dm_core::stream::kind_of(url, content_type) != Some(dm_core::stream::Kind::Hls) {
        return None;
    }
    // The engine names a stream from its address and what its segments
    // are; asked with one transport-stream segment, it names it as one.
    let one = "#EXTM3U\n#EXTINF:1,\nsegment.ts\n#EXT-X-ENDLIST\n";
    let named = dm_core::stream::media_playlist(one, url, url, Default::default())
        .ok()?
        .filename;
    match named.strip_suffix(".ts") {
        Some(stem) if remux => Some(format!("{stem}.mp4")),
        _ => Some(named),
    }
}

/// What dm learned of one link by asking its server.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Found {
    pub name: String,
    pub kind: String,
    pub size: Option<u64>,
    pub html: bool,
    /// Why the server could not be asked about it.
    pub failed: Option<String>,
}

/// The import dialog for a page's links: each checked or not, with its
/// name, kind and size as its server tells them.
pub struct LinksImport {
    pub page: String,
    /// The sign-in the browser handed over for the page's own server, and
    /// that server's address: given to the links on that server only.
    pub sign_in: Option<dm_core::cookies::SignIn>,
    pub cookies: String,
    pub items: Vec<(String, bool)>,
    /// Those already in dm's list (bare addresses): offered unticked and
    /// said so, rather than fetched again as `name (1).zip` without a word.
    pub listed: std::collections::HashSet<String>,
    /// Links unticked when their server refused them, once, so a person
    /// can tick one again (round 15: they were added and failed).
    pub refused: std::collections::HashSet<String>,
    pub hide_html: bool,
    pub found: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Found>>>,
}

impl LinksImport {
    /// A link that is a web page rather than a file: by what its server
    /// said, or before it answers, by the look of its address.
    pub fn is_html(&self, url: &str) -> bool {
        if let Some(found) = dm_core::lock(&self.found).get(url) {
            return found.html;
        }
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let last = path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        path.ends_with('/')
            || !last.contains('.')
            || [".html", ".htm", ".php", ".asp", ".aspx", ".jsp"]
                .iter()
                .any(|e| last.ends_with(e))
    }

    /// The links OK would add: checked, and shown.
    pub fn chosen(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|(url, on)| *on && !(self.hide_html && self.is_html(url)))
            .map(|(url, _)| url.clone())
            .collect()
    }
}

pub struct Properties {
    pub name: String,
    pub description: String,
    /// The address without its login, which has fields of its own.
    pub address: String,
    pub login: String,
    pub password: String,
    pub says: String,
}

/// An address split into itself and the user and password in it.
fn split_login(url: &str) -> (String, String, String) {
    match url::Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() => {
            let decode = |s: &str| {
                url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default()
            };
            let login = decode(parsed.username());
            let password = decode(parsed.password().unwrap_or(""));
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            (parsed.to_string(), login, password)
        }
        _ => (url.to_string(), String::new(), String::new()),
    }
}

/// The address with a user and password put back in it.
fn join_login(address: &str, login: &str, password: &str) -> String {
    match url::Url::parse(address.trim()) {
        Ok(mut parsed) if !login.is_empty() => {
            let _ = parsed.set_username(login);
            let _ = parsed.set_password(Some(password).filter(|p| !p.is_empty()));
            parsed.to_string()
        }
        _ => address.trim().to_string(),
    }
}

#[derive(Default)]
pub struct Series {
    pub template: String,
    pub from: String,
    pub to: String,
    pub width: usize,
    pub says: String,
}

#[derive(Default, Debug)]
pub enum Grab {
    #[default]
    Idle,
    Looking,
    /// The files found, each with whether it is ticked, and how many pages
    /// were read.
    /// The links, the pages read, and whether the run's time ran out.
    Found(Vec<(String, bool)>, usize, bool),
    Failed(String),
}

pub struct SiteFiles {
    pub page: String,
    pub types: String,
    /// Pages past the start page to read.
    pub depth: u8,
    /// Path filters, as typed: words separated by spaces.
    pub include: String,
    pub exclude: String,
    /// The name to save this search under, and how often it runs.
    pub name: String,
    pub every_hours: u64,
    /// Minutes a run reads pages for; 0 is no limit.
    pub stop_after_minutes: u64,
    pub says: String,
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
            include: words(&self.include),
            exclude: words(&self.exclude),
            every_hours: self.every_hours,
            stop_after_minutes: self.stop_after_minutes,
            last_run: 0,
            seen: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    General,
    FileTypes,
    SaveTo,
    Downloads,
    Connection,
    Proxy,
    SiteLogins,
}

pub const TABS: [(Tab, &str); 7] = [
    (Tab::General, "General"),
    (Tab::FileTypes, "Browser"),
    (Tab::SaveTo, "Folders"),
    (Tab::Downloads, "Downloads"),
    (Tab::Connection, "Network"),
    (Tab::Proxy, "Proxy"),
    (Tab::SiteLogins, "Sign-ins"),
];

/// Options › General's looks: the setting's word, and what the choice says.
pub const THEMES: [(&str, &str); 4] = [
    ("system", "As the computer is"),
    ("light", "Light"),
    ("dark", "Dark"),
    ("classic", "Classic"),
];

/// Options › Downloads' video qualities: the setting's word, and its name.
pub const QUALITIES: [(&str, &str); 5] = [
    ("best", "Best available"),
    ("1080", "Up to 1080p"),
    ("720", "Up to 720p"),
    ("480", "Up to 480p"),
    ("audio", "Audio only"),
];

pub struct OptionsDialog {
    pub tab: Tab,
    pub draft: Settings,
    /// The settings as they were when the dialog opened: OK writes only
    /// what differs from these, so a change made elsewhere meanwhile (View ›
    /// Language, the Scheduler, the columns) is not undone.
    pub opened: Settings,
    pub says: String,
    /// File types and skipped hosts, as typed: words separated by spaces,
    /// read into the draft on OK.
    pub types: String,
    pub skip: String,
    /// Sites whose links work once, as typed.
    pub once: String,
    /// The proxy's bypass list, as typed.
    pub bypass: String,
    /// The categories a person made, as typed: name, file types, sites.
    pub custom: Vec<[String; 3]>,
    /// A row was just added to one of the tab's lists: its first field is
    /// scrolled into view and takes the keyboard.
    pub fresh: bool,
    /// Frames left in which the dialog is brought to its top, as it opens:
    /// one was not enough, being egui's discarded sizing pass (UI round 8).
    pub fresh_tab: u8,
}

/// A list to import, some of whose addresses are already rows.
pub struct ImportAsk {
    pub text: String,
    /// How many of its addresses are already rows, and how many it holds.
    pub already: usize,
    pub total: usize,
}

/// The path of a list to import or export, typed where there is no file
/// chooser to ask with.
#[derive(Default)]
pub struct ListFile {
    /// Export (true) or Import.
    pub save: bool,
    pub path: String,
    pub says: String,
}

#[derive(Default)]
pub struct Dialogs {
    pub add_url: Option<AddUrl>,
    pub file_info: Option<FileInfo>,
    pub progress: BTreeSet<Id>,
    /// Progress windows a person closed: Resume does not open them again.
    pub progress_closed: BTreeSet<Id>,
    pub complete: BTreeSet<Id>,
    /// Whether each download could resume, as last read while it ran.
    pub resumable_seen: std::collections::HashMap<Id, bool>,
    pub properties: BTreeMap<Id, Properties>,
    pub series: Option<Series>,
    pub clip_links: Option<Vec<(String, bool)>>,
    /// The "Download all links" import dialog, for a page's links.
    pub links: Option<LinksImport>,
    pub site_files: Option<SiteFiles>,
    pub update_link: Option<(Id, String, String)>,
    /// The rows, the queue picked (as shown) and a new queue's name typed.
    pub move_to_queue: Option<(Vec<Id>, String, String)>,
    pub rename: Option<MoveRename>,
    /// Whether a dialog was in front last frame, and what had the keyboard
    /// before it opened.
    pub was_open: bool,
    pub focus_before: Option<egui::Id>,
    pub find_open: bool,
    pub find_text: String,
    pub scheduler: Option<(Schedule, String)>,
    /// The speed limit, the download limit (MB, hours), and what went wrong.
    pub limiter: Option<(u64, String, u64, u64)>,
    pub options: Option<OptionsDialog>,
    pub about_open: bool,
    /// Check for updates as soon as About is drawn.
    pub check_update: bool,
    /// Rows Delete was pressed on that are not finished: asked about first.
    pub confirm_remove: Option<Vec<Id>>,
    /// The Remove question's "Also delete the file": off each time it asks.
    pub remove_files: bool,
    /// The Remove question's title as last drawn, in English, for the
    /// control protocol: it depends on what is being removed.
    pub remove_title: &'static str,
    /// Rows the schedule or the download limit holds that a person asked to
    /// start: asked whether to start them now, outside the schedule.
    pub confirm_outside: Option<Vec<Id>>,
    /// The progress window's Cancel download, asked first: it threw away
    /// what was fetched without a word, where every other Cancel closes a
    /// dialog (UI round 2).
    pub confirm_cancel: Option<Id>,
    /// An address typed in that is already a row: the address and the row.
    pub duplicate: Option<(String, Id)>,
    /// A list being imported that holds addresses already in the list:
    /// asked once, for all of them, whether to skip them or add them again.
    pub import_ask: Option<ImportAsk>,
    /// Import or Export where the desktop has no file chooser: the file's
    /// path, typed.
    pub list_file: Option<ListFile>,
    /// A site whose handed-over downloads were cancelled twice running:
    /// asked whether to leave its downloads to the browser from now on.
    pub skip_offer: Option<String>,
    /// Windows asked for again while open, to be brought to the front.
    pub raise: Vec<egui::Id>,
    /// Exit asked for while downloads run: asked whether to stop them.
    pub confirm_exit: bool,
    /// The dialog, by English title, whose first field takes the keyboard
    /// when it is next drawn: a dialog opened to be typed in is typed in
    /// at once, without a click first.
    pub focus: Option<&'static str>,
}

impl Dialogs {
    /// Gives the keyboard to `title`'s first field when it is next drawn.
    pub fn focus_on(&mut self, title: &'static str) {
        self.focus = Some(title);
    }

    /// Whether `title`'s first field is to take the keyboard now; once.
    fn take_focus(&mut self, title: &'static str) -> bool {
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
        // The keyboard comes off the list behind it (UI round 13).
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
            label: request.label,
            name: request.save_name.unwrap_or_default(),
            queue: MAIN_QUEUE.into(),
            handed: true,
            cookies: request.cookies,
            cookies_once: request.cookies_once,
            ..FileInfo::default()
        });
    }

    /// "Download all links": every link checked, and each one's
    /// server asked, six at a time, for its name, kind and size.
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
                // Said, rather than "…" for ever as though still asking
                // (round 14).
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
            },
        );
    }

    /// An open dialog is brought forward and kept as it is, with what was
    /// typed in it: asking for it again used to start it afresh, and
    /// Options lost every change not yet saved. True if it was open.
    fn keep(&mut self, title: &'static str, open: bool) -> bool {
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

    /// Whether a dialog is in front of the list, which then keeps its
    /// keys from the list. The windows a download opens on its own (its
    /// progress, its completion) are not counted.
    /// Whether a question waits for its answer: Remove, Exit, Start
    /// outside the schedule, already listed, leave a site to the browser.
    /// Nothing else opens over one, or it is lost behind the new window
    /// and answered later by an Enter meant for something else (round 14).
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
    fn questions(&self) -> Vec<&'static str> {
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

    pub fn in_front(&self) -> bool {
        self.open_names()
            .iter()
            .any(|n| n != "Download progress" && n != "Download complete")
    }

    /// The windows of the dialogs in front that keep the keyboard to
    /// themselves: Tab goes round their own controls and never to the menu
    /// bar or the list behind, which take no clicks while one is open.
    /// Find is left out: it finds rows in the list, which stays usable
    /// beside it.
    /// Whether a dialog that keeps the clicks to itself is open.
    pub fn modal(&self) -> bool {
        !self.modal_layers().is_empty()
    }

    fn modal_layers(&self) -> Vec<egui::LayerId> {
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
        // By the title it shows (UI round 5: an old name whatever it said).
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
        // Every dialog by its title (UI round 7: this one was never named).
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
        open
    }

    /// Closes the dialog in front, as Escape does. Nothing records which
    /// was opened last, so the order is the one a person would mean: a
    /// question being asked first, then the small dialogs that open over
    /// the large ones (Find over Download details, the Speed limiter over
    /// Options), then the large ones, then the windows per download. True
    /// if one closed.
    /// Closes the dialog in front. `back_to_front` is the windows' order
    /// on screen, which decides among the ones that stay open beside each
    /// other (Find, Download complete, the progress windows): Escape closed
    /// one behind the window in front (Fluent QA, gap 71).
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

/// A dialog window: titled, not collapsible, placed in the middle.
/// A dialog, titled in the language in force but known by its English
/// title: its place and size survive a change of language, and two titles
/// that translate alike stay two windows.
/// Where the window keeps the menu bar's foot, for the dialogs.
pub fn menu_bottom_id() -> egui::Id {
    egui::Id::unique("dm-menu-bottom")
}

fn window(ctx: &egui::Context, title: &'static str) -> Dialog<'static> {
    // Below the menu bar: Options and Properties opened over it, while the
    // per-download windows kept clear of it (UI round 5).
    let mut room = ctx.content_rect();
    if let Some(bottom) = ctx.data(|d| d.get_temp::<f32>(menu_bottom_id())) {
        room.min.y = room.min.y.max(bottom).min(room.max.y - 160.0);
    }
    let tall = (room.height() - 56.0).max(160.0);
    let reached = ctx
        .memory(|m| m.area_rect(window_id(title)))
        // With some slack: a scrolled dialog stands about as tall as it may.
        .is_some_and(|was| was.height() >= tall - 8.0);
    // And once scrolled, it stays so while it is open in the same room: a
    // scrolled body is shorter than the room, and turning the scrolling off
    // again for it made the dialog flip between the two every frame, its
    // buttons moving under the pointer so that no click took (Site files on
    // Windows' fonts).
    let held_id = window_id(title).with("scrolled");
    let pass = ctx.cumulative_pass_nr();
    let held = ctx
        .data(|d| d.get_temp::<(f32, u64)>(held_id))
        .is_some_and(|(room_tall, seen)| (room_tall - tall).abs() < 1.0 && seen + 1 >= pass);
    let cramped = reached || held;
    ctx.data_mut(|d| {
        if cramped {
            d.insert_temp(held_id, (tall, pass));
        } else {
            d.remove::<(f32, u64)>(held_id);
        }
    });
    Dialog::new(t(title))
        .id(window_id(title))
        .collapsible(false)
        .resizable(false)
        // Wide enough for its title in the 20 px heading, but never wider
        // than the window: at 150% text in the smallest window 440 points
        // ran off both sides (UI round 5).
        .min_width(440.0_f32.min(room.width() - sides(ctx)).max(200.0))
        // And never taller: a dialog longer than the room scrolls, rather
        // than losing its title and buttons off the top and bottom (Files
        // on a site at 150% in the smallest window, UI round 5). Only one
        // that was that tall on the frame before: a scrolled body sizes
        // itself otherwise, and every other dialog keeps its own size.
        .max_height(tall)
        .vscroll(cramped)
        .constrain_to(room)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
}

/// A dialog laid out as WinUI's are: no title bar, but its title at the
/// top of the body on the left in the Subtitle style (20 px semibold), a
/// close button at the right for the dialogs that have one, and 12 px
/// under them. egui's own title bar centred the title in a grey band.
pub(crate) struct Dialog<'o> {
    window: egui::Window<'static>,
    title: String,
    open: Option<&'o mut bool>,
    /// The least width of its body, as `min_width` set it.
    least: f32,
    /// The width `fixed_size` gave it, frame and all.
    fixed: Option<f32>,
    /// Whether its body scrolls, under a title that stays.
    scrolls: bool,
    /// The most it may stand, as `max_height` set it.
    tallest: Option<f32>,
}

/// Where a dialog keeps, for its title and buttons, how wide its body is:
/// the least width it was given, and the body's own width last frame.
fn least_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-least")
}

fn body_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-body")
}

/// Whether a dialog's body scrolls this frame, and the room it keeps under
/// it for the buttons: where, and how tall the buttons took last frame.
fn scrolls_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-scrolls")
}

fn foot_rect_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-foot-rect")
}

fn origin_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-origin")
}

fn foot_height_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-foot-height")
}

impl<'o> Dialog<'o> {
    pub(crate) fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Dialog {
            window: egui::Window::new(title.clone()).title_bar(false),
            title,
            open: None,
            least: 0.0,
            fixed: None,
            scrolls: false,
            tallest: None,
        }
    }

    fn with(mut self, change: impl FnOnce(egui::Window<'static>) -> egui::Window<'static>) -> Self {
        self.window = change(self.window);
        self
    }

    pub(crate) fn open(mut self, open: &'o mut bool) -> Self {
        self.open = Some(open);
        self
    }

    pub(crate) fn id(self, id: egui::Id) -> Self {
        self.with(|w| w.id(id))
    }

    pub(crate) fn collapsible(self, on: bool) -> Self {
        self.with(|w| w.collapsible(on))
    }

    pub(crate) fn resizable(self, on: bool) -> Self {
        self.with(|w| w.resizable(on))
    }

    pub(crate) fn constrain(self, on: bool) -> Self {
        self.with(|w| w.constrain(on))
    }

    pub(crate) fn constrain_to(self, rect: egui::Rect) -> Self {
        self.with(|w| w.constrain_to(rect))
    }

    pub(crate) fn pivot(self, pivot: egui::Align2) -> Self {
        self.with(|w| w.pivot(pivot))
    }

    pub(crate) fn anchor(self, align: egui::Align2, offset: impl Into<egui::Vec2>) -> Self {
        let offset = offset.into();
        self.with(|w| w.anchor(align, offset))
    }

    pub(crate) fn default_pos(self, pos: impl Into<egui::Pos2>) -> Self {
        let pos = pos.into();
        self.with(|w| w.default_pos(pos))
    }

    pub(crate) fn default_width(self, width: f32) -> Self {
        self.with(|w| w.default_width(width))
    }

    pub(crate) fn default_height(self, height: f32) -> Self {
        self.with(|w| w.default_height(height))
    }

    pub(crate) fn min_width(mut self, width: f32) -> Self {
        self.least = width;
        self.with(|w| w.min_width(width))
    }

    pub(crate) fn min_height(self, height: f32) -> Self {
        self.with(|w| w.min_height(height))
    }

    pub(crate) fn max_width(self, width: f32) -> Self {
        self.with(|w| w.max_width(width))
    }

    pub(crate) fn max_height(mut self, height: f32) -> Self {
        self.tallest = Some(height);
        self.with(|w| w.max_height(height))
    }

    pub(crate) fn fixed_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        let size = size.into();
        self.fixed = Some(size.x);
        // The least width an earlier call gave it no longer applies.
        self.least = 0.0;
        self.with(|w| w.min_width(0.0).fixed_size(size))
    }

    /// The body scrolls, below the title and its close button, which stay
    /// where they can be reached; egui's own scrolled them away with it.
    pub(crate) fn vscroll(mut self, on: bool) -> Self {
        self.scrolls = on;
        self
    }

    pub(crate) fn show<R>(
        self,
        ctx: &egui::Context,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let Dialog {
            window,
            title,
            open,
            least,
            fixed,
            scrolls,
            tallest,
        } = self;
        if open.as_deref() == Some(&false) {
            return None;
        }
        let closable = open.is_some();
        let mut closed = false;
        let shown = window.show(ctx, |ui| {
            let layer = ui.layer_id();
            // A dialog of fixed size holds its body to the width inside its
            // frame: egui offered the body the width it had grown to the
            // frame before, a scroll area took all of it, and the dialog,
            // placed by its middle, ran off the window (Options at 200%).
            let least = match fixed {
                Some(width) => {
                    let inner = width - ui.spacing().window_margin.sum().x;
                    ui.set_max_width(inner);
                    inner
                }
                None => least,
            }
            .min(ui.max_rect().width());
            ui.ctx().data_mut(|d| d.insert_temp(least_id(layer), least));
            closed = head(ui, &title, closable);
            let origin = ui.max_rect().min;
            // The buttons' room kept under the scrolled part, at what they
            // took last frame; the footer draws them there.
            let kept = ui
                .ctx()
                .data(|d| d.get_temp::<f32>(foot_height_id(layer)))
                .unwrap_or(0.0);
            // From the height the dialog may have, less its title and
            // margins: what egui says is available in a dialog that sizes
            // itself is not that, and the scrolled part ran under the
            // buttons (Options at 150%).
            let spacing = ui.spacing().item_spacing.y;
            let margins = ui.spacing().window_margin.sum().y;
            let used = ui.min_rect().height() + spacing;
            let whole = tallest
                .map_or(ui.available_height(), |most| most - margins - used)
                .min(ui.available_height());
            // Where keeping the buttons in place would leave the body less
            // than a few rows, the page scrolls whole with its buttons at
            // the end, as it did (UI round 6): at 150% in the smallest
            // window Options showed a sliver of its first setting.
            let fixed_foot = scrolls && whole - kept - spacing >= 120.0;
            ui.ctx().data_mut(|d| {
                d.insert_temp(scrolls_id(layer), fixed_foot);
                d.insert_temp(origin_id(layer), origin);
            });
            if scrolls {
                let room = if fixed_foot {
                    whole - kept - spacing
                } else {
                    whole
                }
                .max(48.0);
                let inner = egui::ScrollArea::vertical()
                    .id_salt("dm-dialog-body")
                    .max_height(room)
                    .show(ui, add)
                    .inner;
                if fixed_foot && kept > 0.0 {
                    let (foot, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), kept),
                        egui::Sense::hover(),
                    );
                    // Kept from the dialog's corner, which moves with it.
                    let foot = foot.translate(-origin.to_vec2());
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(foot_rect_id(layer), foot));
                }
                inner
            } else {
                add(ui)
            }
        });
        if let (true, Some(open)) = (closed, open) {
            *open = false;
        }
        shown
    }
}

/// A dialog's title and close button; true when the button was clicked.
fn head(ui: &mut egui::Ui, title: &str, closable: bool) -> bool {
    // As wide as the dialog's body was last frame, which is how egui's own
    // title bar finds the far edge of a window that sizes itself to its
    // body; nothing on the first, when egui's own answer is the whole room
    // and a dialog took that as its width (Options at 200%).
    // Measured without the title and the buttons, which would otherwise
    // hold a dialog at any width it once had.
    let layer = ui.layer_id();
    let (least, body) = ui.ctx().data(|d| {
        (
            d.get_temp::<f32>(least_id(layer)).unwrap_or(0.0),
            d.get_temp::<f32>(body_id(layer)).unwrap_or(0.0),
        )
    });
    // A dialog with no footer has no body measured: last frame's width.
    let body = if body > 0.0 {
        body
    } else {
        ui.ctx()
            .memory(|m| m.area_rect(layer.id))
            .map_or(0.0, |r| r.width() - ui.spacing().window_margin.sum().x)
    };
    let wide = least.max(body).min(ui.max_rect().width());
    let mut closed = false;
    ui.horizontal(|ui| {
        // Painted, not a label: the window already gives a screen reader
        // its title, and a second node of the same name was a second
        // "Download details" to every search by name.
        let galley = ui.painter().layout_no_wrap(
            title.to_string(),
            egui::TextStyle::Heading.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(galley.size().x, galley.size().y.max(32.0)),
            egui::Sense::hover(),
        );
        let at = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, ui.visuals().text_color());
        if closable {
            let size = egui::vec2(32.0, 32.0);
            let gap = wide - ui.min_rect().width() - size.x - ui.spacing().item_spacing.x;
            ui.add_space(gap.max(0.0));
            closed = close_button(ui, size).clicked();
        }
    });
    ui.add_space(4.0);
    closed
}

/// A subtle button with a ×, as egui named its own: "Close window".
fn close_button(ui: &mut egui::Ui, size: egui::Vec2) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Close window")
    });
    let fill = if response.is_pointer_button_down_on() {
        t.pressed
    } else if response.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    let cross = egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0));
    let line = egui::Stroke::new(1.0, if ui.is_enabled() { t.text } else { t.disabled });
    ui.painter()
        .line_segment([cross.left_top(), cross.right_bottom()], line);
    ui.painter()
        .line_segment([cross.right_top(), cross.left_bottom()], line);
    response
}

/// What a dialog's margins take of the window's width, both sides: a
/// dialog sized to the window less this fills it and no more.
fn sides(ctx: &egui::Context) -> f32 {
    ctx.global_style().spacing.window_margin.sum().x
}

/// The id of a dialog's window, by its English title.
fn window_id(title: &'static str) -> egui::Id {
    egui::Id::unique(("dm-dialog", title))
}

/// A progress bar: a thin track with the brand fill and its words
/// beside it; the classic look keeps egui's own bar.
fn bar(ui: &mut egui::Ui, fraction: f32, width: f32, words: &str) {
    bar_beside(ui, fraction, width, words, words);
}

/// [`bar`], with room kept for `widest` beside it rather than its own
/// words: bars in a column, each with words of its own, are then one
/// length (UI round 5: "1.7 MB" and "1023.9 KB" gave two).
fn bar_beside(ui: &mut egui::Ui, fraction: f32, width: f32, words: &str, widest: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    // At most `width`, and short enough that its words fit beside it
    // clear of a scroll bar: "91%" was cut off at the edge (gap 140).
    let words_width = ui
        .painter()
        .layout_no_wrap(
            widest.to_string(),
            egui::TextStyle::Small.resolve(ui.style()),
            Color32::WHITE,
        )
        .size()
        .x;
    let room = ui.available_width()
        - words_width
        - ui.spacing().item_spacing.x
        - ui.spacing().scroll.allocated_width()
        - 8.0;
    let width = width.min(room).max(60.0);
    if !t.modern {
        // egui's bar, its track in a colour that shows on the dialog: in
        // the classic look the track was the dialog's own colour, and a
        // bar at 0% was a bare pill around its words (gap 151).
        ui.scope(|ui| {
            ui.visuals_mut().extreme_bg_color = t.border;
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(width)
                    .text(words),
            );
        });
        return;
    }
    ui.horizontal(|ui| {
        // WinUI's progress bar: a 1 px track in the strong stroke under a
        // 3 px bar in the accent, both rounded.
        let (track, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
        let line = egui::Rect::from_center_size(track.center(), egui::vec2(width, 1.0));
        ui.painter().rect_filled(line, 0.5, t.stroke);
        let mut done = egui::Rect::from_center_size(track.center(), egui::vec2(width, 3.0));
        done.set_width(width * fraction);
        ui.painter().rect_filled(done, 1.5, t.brand);
        ui.label(RichText::new(words).small().color(t.weak));
    });
}

/// A dialog's buttons as WinUI's ContentDialog has them: on a band in the
/// window's base colour under a hairline, across the whole dialog to its
/// bottom edge, below the body; the buttons at the right, the default
/// first. In a dialog whose body scrolls, the band is drawn in the room
/// the dialog keeps for it under the scrolled part, so the buttons stay in
/// reach, as a ContentDialog's do. Classic keeps a plain row.
pub(crate) fn footer<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return ui.horizontal(add);
    }
    let layer = ui.layer_id();
    let margin = ui.spacing().window_margin;
    let (scrolled, kept) = ui.ctx().data(|d| {
        let origin = d
            .get_temp::<egui::Pos2>(origin_id(layer))
            .unwrap_or_default();
        (
            d.get_temp::<bool>(scrolls_id(layer)).unwrap_or(false),
            d.get_temp::<egui::Rect>(foot_rect_id(layer))
                .map(|r| r.translate(origin.to_vec2())),
        )
    });
    if let (true, Some(foot)) = (scrolled, kept) {
        // The room kept under the scrolled body: the band at its top, the
        // row 24 below, outside the scroll area's clip.
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(foot)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(
            foot.expand2(egui::vec2(f32::from(margin.left), f32::from(margin.bottom))),
        );
        child.add_space(f32::from(margin.top));
        let shown = row_on_band(&mut child, foot.left(), foot.right(), add);
        let height = shown.response.rect.bottom() - foot.top();
        ui.ctx()
            .data_mut(|d| d.insert_temp(foot_height_id(layer), height));
        return shown;
    }
    // The body's width as laid out so far this frame, or the dialog's least
    // width: the row reaches that edge and never past it, so it cannot
    // widen the dialog it is measured against.
    let least = ui
        .ctx()
        .data(|d| d.get_temp::<f32>(least_id(layer)))
        .unwrap_or(0.0);
    let left = ui.max_rect().left();
    let body = (ui.min_rect().right() - left).max(least);
    ui.ctx().data_mut(|d| d.insert_temp(body_id(layer), body));
    let right = (left + body).min(ui.max_rect().right());
    // The band 24 below the body's last line, as a ContentDialog pads its
    // content: the row is 24 into the band, so 40 above it with the 8 the
    // layout puts after a widget. The gap goes above the row, which in a
    // layout built from the foot up means after it. 20 left the band 4
    // under the last line (Fluent QA against the Gallery).
    let upward = ui.layout().main_dir() == egui::Direction::BottomUp;
    let gap = 40.0;
    if !upward {
        ui.add_space(gap);
    }
    let shown = row_on_band(ui, left, right, add);
    if upward {
        ui.add_space(gap);
    }
    if scrolled {
        let height = shown.response.rect.height() + f32::from(margin.top);
        ui.ctx()
            .data_mut(|d| d.insert_temp(foot_height_id(layer), height));
    }
    shown
}

/// The buttons' row, set at `right`, on the band that runs from 24 above it
/// to the dialog's foot, `left` to `right` and out over the margins.
fn row_on_band<R>(
    ui: &mut egui::Ui,
    left: f32,
    right: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let t = crate::theme::Tokens::get(ui.ctx());
    let margin = ui.spacing().window_margin;
    let band = ui.painter().add(egui::Shape::Noop);
    let group = ui.scope_id().with("dm-footer-group");
    let group_width = ui.ctx().data(|d| d.get_temp::<f32>(group));
    let shown = ui.horizontal(|ui| {
        if let Some(width) = group_width {
            let free = right - ui.cursor().left() - width;
            ui.add_space(free.max(0.0).floor());
        }
        let start = ui.cursor().left();
        let inner = add(ui);
        let width = ui.min_rect().right() - start;
        ui.ctx().data_mut(|d| d.insert_temp(group, width));
        inner
    });
    let row = shown.response.rect;
    let rect = egui::Rect::from_min_max(
        egui::pos2(
            left - f32::from(margin.left),
            row.top() - f32::from(margin.top),
        ),
        egui::pos2(
            right.max(row.right()) + f32::from(margin.right),
            row.bottom() + f32::from(margin.bottom),
        ),
    )
    // Inside the dialog's hairline edge.
    .shrink2(egui::vec2(1.0, 0.0))
    .with_max_y(row.bottom() + f32::from(margin.bottom) - 1.0);
    let radius = egui::CornerRadius {
        nw: 0,
        ne: 0,
        sw: t.card_radius.saturating_sub(1),
        se: t.card_radius.saturating_sub(1),
    };
    let painter = ui.painter().with_clip_rect(rect.expand(1.0));
    painter.set(
        band,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, radius, t.frame),
            egui::Shape::hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, t.border)),
        ]),
    );
    shown
}

/// A dialog's main action: the primary button, filled with the brand
/// colour (the classic look draws it like any other).
/// A number typed into a box, as a desktop's number fields are: not
/// changed by dragging across it, with the text cursor over it. The drag
/// to change, and the resize arrow that announced it, surprised people
/// (UI round 1).
/// Its range is said on hover: 35 in Files at once became 10 without a
/// word (UI round 14).
pub(crate) struct NumberBox<'a>(egui::DragValue<'a>, Option<String>);

pub(crate) fn number_box<Num: egui::emath::Numeric>(value: &mut Num) -> NumberBox<'_> {
    NumberBox(egui::DragValue::new(value).speed(0.0), None)
}

impl NumberBox<'_> {
    pub(crate) fn range<Num: egui::emath::Numeric>(
        self,
        range: std::ops::RangeInclusive<Num>,
    ) -> Self {
        let said = tf(
            "From {} to {}",
            &[&range.start().to_f64(), &range.end().to_f64()],
        );
        NumberBox(self.0.range(range), Some(said))
    }
}

impl egui::Widget for NumberBox<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        /* Text that is not a number leaves the number as it was, which
         * egui does without a word: "abc" in a size floor left 0 there and
         * looked taken (UI round 14). The parser notes the refusal, and
         * leaving the box says so beside it until it is typed in again. */
        let refused = std::rc::Rc::new(std::cell::Cell::new(false));
        let noted = std::rc::Rc::clone(&refused);
        let parser = move |text: &str| {
            let text: String = text
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| if c == '−' { '-' } else { c })
                .collect();
            let parsed = text.parse::<f64>().ok();
            noted.set(parsed.is_none() && !text.is_empty());
            parsed
        };
        let response = ui
            .add(self.0.custom_parser(parser))
            .on_hover_cursor(egui::CursorIcon::Text);
        let said = response.id.with("not a number");
        if response.gained_focus() || response.changed() {
            ui.ctx().data_mut(|d| d.remove::<bool>(said));
        }
        if response.lost_focus() && refused.get() {
            ui.ctx().data_mut(|d| d.insert_temp(said, true));
        }
        if ui.ctx().data(|d| d.get_temp::<bool>(said)).unwrap_or(false) {
            let t = crate::theme::Tokens::get(ui.ctx());
            ui.label(
                RichText::new(crate::i18n::t("Numbers only: the one before was kept"))
                    .small()
                    .color(t.warn),
            );
        }
        // The bottom line a text field has, at rest too: the fields'
        // pass finds a number box only while it is being typed in, so at
        // rest it had none (Fluent QA, gap 71).
        let t = crate::theme::Tokens::get(ui.ctx());
        if t.modern && ui.is_enabled() {
            let focus = response.has_focus();
            let height = if focus { 2.0 } else { 1.0 };
            let rect = response.rect;
            let line = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 1.0, rect.bottom() - height),
                egui::pos2(rect.right() - 1.0, rect.bottom()),
            );
            ui.painter()
                .rect_filled(line, 0.0, if focus { t.brand } else { t.stroke });
        }
        match self.1 {
            Some(range) => response.on_hover_text(range),
            None => response,
        }
    }
}

/// A setting's words and its number box: the words wrap in the room the
/// box leaves them, and the box stays beside them, centred on them. Laid
/// out as one wrapped line, a long setting (Russian's recheck interval)
/// wrapped its words and left the box hanging after their second line
/// (UI round 10).
fn number_row(ui: &mut egui::Ui, words: &str, add: impl FnOnce(&mut egui::Ui, egui::Id)) {
    ui.horizontal(|ui| {
        let room = (ui.available_width() - 96.0).max(120.0);
        let name = ui
            // At least a box's height, centred in it: one line of words
            // sat below the box's middle.
            .allocate_ui_with_layout(
                egui::vec2(room, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                |ui| ui.add(egui::Label::new(words).wrap()),
            )
            .inner;
        add(ui, name.id);
    });
}

/// A heading over a group of settings: a little room above, in the
/// heading's weight, on the 4-point grid.
fn section(ui: &mut egui::Ui, words: &str) {
    ui.add_space(12.0);
    // The semibold face, as the window's headings are: strong() only
    // changed the colour, and the groups read as more options (round 17).
    ui.label(RichText::new(words).family(crate::theme::bold(ui.ctx())));
    ui.add_space(4.0);
}

pub(crate) fn primary(ui: &mut egui::Ui, text: &'static str) -> egui::Response {
    // The button's words in the language in force.
    primary_worded(ui, crate::i18n::t(text))
}

/// [`primary`], with words already in the language in force.
fn primary_worded(ui: &mut egui::Ui, text: &str) -> egui::Response {
    primary_sized(ui, text, 0.0)
}

/// [`primary_worded`], at least `width` wide.
fn primary_sized(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let min = egui::vec2(width, 0.0);
    if !t.modern {
        return ui.add(egui::Button::new(text).min_size(min));
    }
    // An accent button: its fill a shade lighter under the pointer and
    // lighter again pressed, as WinUI's AccentButton is.
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for (state, fill) in [
            (&mut w.inactive, t.brand),
            (&mut w.hovered, t.brand_hover),
            (&mut w.active, t.brand_pressed),
        ] {
            state.weak_bg_fill = fill;
            state.bg_fill = fill;
            state.bg_stroke = egui::Stroke::new(1.0, fill);
            state.fg_stroke = egui::Stroke::new(1.0, t.on_brand);
        }
        ui.add(egui::Button::new(RichText::new(text).color(t.on_brand)).min_size(min))
    })
    .inner
}

/// A section's opener: a thin chevron pointing at its name, turning down
/// as it opens, as an expander's does; egui's filled triangle in the
/// classic look.
fn header_chevron(ui: &mut egui::Ui, openness: f32, response: &egui::Response) {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        egui::collapsing_header::paint_default_icon(ui, openness, response);
        return;
    }
    let c = response.rect.center();
    let turn = egui::emath::Rot2::from_angle(openness * std::f32::consts::FRAC_PI_2);
    let points = [
        egui::vec2(-2.0, -4.5),
        egui::vec2(2.5, 0.0),
        egui::vec2(-2.0, 4.5),
    ]
    .map(|p| c + turn * p)
    .to_vec();
    ui.painter().line(points, egui::Stroke::new(1.0, t.weak));
}

/// A drop-down list with WinUI's chevron, in the secondary colour, where
/// egui draws a filled triangle.
pub(crate) fn combo(salt: impl std::hash::Hash + std::fmt::Debug) -> egui::ComboBox<'static> {
    egui::ComboBox::from_id_salt(salt).icon(chevron)
}

/// One choice in a drop-down's list: egui's selectable item, and the
/// picked one marked with the accent pill at its left, as WinUI's list
/// items are. It had only the subtle fill (Fluent QA, gap 71).
pub(crate) fn option(
    ui: &mut egui::Ui,
    selected: bool,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    let response = ui.selectable_label(selected, label);
    let t = crate::theme::Tokens::get(ui.ctx());
    if selected && t.modern {
        let tall = (response.rect.height() - 8.0).clamp(8.0, 16.0);
        let pill = egui::Rect::from_center_size(
            egui::pos2(response.rect.left() + 1.5, response.rect.center().y),
            egui::vec2(3.0, tall),
        );
        ui.painter().rect_filled(pill, 1.5, t.brand);
    }
    response
}

/// [`option`] for a value, as egui's `selectable_value`: a click picks
/// `value`, and the response says it changed when it did.
pub(crate) fn pick<V: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut V,
    value: V,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    let mut response = option(ui, *current == value, label);
    if response.clicked() && *current != value {
        *current = value;
        response.mark_changed();
    }
    response
}

fn chevron(ui: &egui::Ui, rect: egui::Rect, visuals: &egui::style::WidgetVisuals, _open: bool) {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        // egui's own triangle, for the classic look.
        let rect = egui::Rect::from_center_size(
            rect.center(),
            egui::vec2(rect.width() * 0.7, rect.height() * 0.45),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![rect.left_top(), rect.right_top(), rect.center_bottom()],
            visuals.fg_stroke.color,
            visuals.fg_stroke,
        ));
        return;
    }
    let c = rect.center();
    let colour = if ui.is_enabled() { t.weak } else { t.disabled };
    let glyph = egui::Rect::from_center_size(c, egui::vec2(12.0, 12.0));
    if crate::icons::glyph(
        ui.painter(),
        glyph,
        crate::icons::fluent::CHEVRON_DOWN,
        colour,
    ) {
        return;
    }
    let line = egui::Stroke::new(1.0, colour);
    ui.painter().line(
        vec![
            c + egui::vec2(-4.5, -2.0),
            c + egui::vec2(0.0, 2.5),
            c + egui::vec2(4.5, -2.0),
        ],
        line,
    );
}

/// How much of its 24 points' padding each of these tabs keeps so that
/// all of them fit in `room`: all of it where they fit, else down to 12.
/// At 150% text Options' tabs stood 19 points past the dialog's edge.
fn tabs_padding(ui: &egui::Ui, labels: &[&str], room: f32) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let words: f32 = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap((*l).to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .sum();
    let gaps = ui.spacing().item_spacing.x * labels.len().saturating_sub(1) as f32;
    let each = (room - words - gaps) / labels.len().max(1) as f32;
    each.clamp(12.0, 24.0)
}

/// One item of a row of pages, as WinUI's selector bar draws it: words in
/// the secondary colour, the picked one in the primary colour with the
/// accent pill under it, the subtle fill under the pointer. Classic keeps
/// egui's selectable label.
/// `padding` is the room beside its words, both sides together.
fn selector(ui: &mut egui::Ui, selected: bool, label: &str, padding: f32) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return ui.selectable_label(selected, label);
    }
    // Laid out in the colour it is drawn in: a colour given here wins over
    // the one given when painting, and every word came out primary.
    let words = if selected { t.text } else { t.weak };
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::TextStyle::Button.resolve(ui.style()),
        words,
    );
    let size = egui::vec2(galley.size().x + padding, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label)
    });
    let fill = if response.is_pointer_button_down_on() {
        t.pressed
    } else if response.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect.shrink2(egui::vec2(0.0, 4.0)), t.radius, fill);
    painter.galley(
        rect.center() - galley.size() / 2.0 - egui::vec2(0.0, 1.0),
        galley,
        words,
    );
    if selected {
        let pill = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - 3.5),
            egui::vec2(16.0, 3.0),
        );
        painter.rect_filled(pill, 1.5, t.brand);
    }
    response
}

/// A check box as WinUI draws one: a 20 px box outlined in the strong
/// stroke, filled with the accent and a check in the colour on it once
/// ticked; lighter under the pointer.
pub(crate) fn check(
    ui: &mut egui::Ui,
    on: &mut bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let checked = *on;
    let text = choice_words(ui, checked, text.into());
    ui.scope(|ui| {
        choice_look(ui, checked);
        ui.checkbox(on, text)
    })
    .inner
}

/// A radio button as WinUI draws one, in [`check`]'s colours.
pub(crate) fn radio<V: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut V,
    value: V,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let checked = *current == value;
    let text = choice_words(ui, checked, text.into());
    ui.scope(|ui| {
        choice_look(ui, checked);
        ui.radio_value(current, value, text)
    })
    .inner
}

/// [`radio`] for a choice that is not one value of a field: picked when
/// `checked`, its response clicked when chosen.
pub(crate) fn radio_flag(
    ui: &mut egui::Ui,
    checked: bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let text = choice_words(ui, checked, text.into());
    ui.scope(|ui| {
        choice_look(ui, checked);
        ui.radio(checked, text)
    })
    .inner
}

/// A choice's words in the text colour: egui takes them from the colour
/// the check mark is drawn in, which is the one on the accent.
fn choice_words(ui: &egui::Ui, checked: bool, text: egui::WidgetText) -> egui::WidgetText {
    let t = crate::theme::Tokens::get(ui.ctx());
    if checked && t.modern {
        text.color(t.text)
    } else {
        text
    }
}

fn choice_look(ui: &mut egui::Ui, checked: bool) {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return;
    }
    // WinUI's 20 px box; the window's own icon width is a drop-down's.
    ui.spacing_mut().icon_width = 20.0;
    // ControlAltFillColorSecondary, Tertiary and Quarternary behind an
    // empty box.
    let alt = |light: u8, dark: u8| {
        if t.dark {
            egui::Color32::from_white_alpha(dark)
        } else {
            egui::Color32::from_black_alpha(light)
        }
    };
    // Ticked but disabled, AccentFillColorDisabled: grey, not a pale
    // accent that read as ticked and live (Fluent QA, gap 71).
    let enabled = ui.is_enabled();
    let disabled = if t.dark {
        egui::Color32::from_white_alpha(0x28)
    } else {
        egui::Color32::from_black_alpha(0x37)
    };
    let accent = |live: egui::Color32| if enabled { live } else { disabled };
    let w = &mut ui.visuals_mut().widgets;
    let states = [
        (&mut w.inactive, accent(t.brand), alt(0x06, 0x00)),
        (&mut w.hovered, accent(t.brand_hover), alt(0x0f, 0x0b)),
        (&mut w.active, accent(t.brand_pressed), alt(0x18, 0x12)),
    ];
    for (state, accent, empty) in states {
        if checked {
            state.bg_fill = accent;
            state.bg_stroke = egui::Stroke::new(1.0, accent);
            state.fg_stroke = egui::Stroke::new(1.5, t.on_brand);
        } else {
            state.bg_fill = empty;
            state.bg_stroke = egui::Stroke::new(1.0, t.stroke);
        }
    }
}

/// A text field with a name a screen reader says.
fn field(ui: &mut egui::Ui, label: &str, text: &mut String, width: f32) -> egui::Response {
    let name = ui.label(label);
    // The look's focus frame, kept before the selection colours change:
    // the field draws its focused frame in the selection's stroke, which
    // `selection_seen` turns the colour of selected words — white on the
    // dialog's white, so a field focused on open had no frame at all (gap
    // 165). It is drawn again over the field in the look's own colour.
    let ring = ui.visuals().selection.stroke;
    let corner = ui.visuals().widgets.inactive.corner_radius;
    let field = ui
        .scope(|ui| {
            selection_seen(ui);
            // No wider than the room there is: at 200% text a fixed width
            // pushed Options off the window (UI round 5).
            let width = width.min(ui.available_width()).max(40.0);
            ui.add(egui::TextEdit::singleline(text).desired_width(width))
        })
        .inner
        .labelled_by(name.id);
    if field.has_focus() {
        ui.painter()
            .rect_stroke(field.rect, corner, ring, egui::StrokeKind::Inside);
    }
    field
}

/// Selected text drawn in the list's selection colours, which read on a
/// field: the looks' own selection fill is a pale grey for the tree, and a
/// link selected in Add URL could not be told from one that was not (gap
/// 160).
fn selection_seen(ui: &mut egui::Ui) {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let selection = &mut ui.visuals_mut().selection;
    selection.bg_fill = tokens.text_selection;
    selection.stroke.color = egui::Color32::WHITE;
}

/// A dialog's warning, under its fields, with its room kept while there is
/// nothing to say: appearing, it grew the dialog, which is placed by its
/// middle, and the whole dialog jumped (gap 160). Two lines, which is what
/// the dialogs' warnings take at their widths.
fn warning_line(ui: &mut egui::Ui, says: &str) {
    kept_line(ui, RichText::new(says).color(warn(ui)));
}

/// [`warning_line`]'s room, holding any words.
fn kept_line(ui: &mut egui::Ui, says: RichText) {
    let line = ui.text_style_height(&egui::TextStyle::Body);
    let room = line * 2.0 + ui.spacing().item_spacing.y;
    // As wide as what is above it: the room it could have would widen
    // a dialog that sizes itself to its contents.
    let width = ui.min_rect().width().max(160.0);
    ui.allocate_ui(egui::vec2(width, room), |ui| {
        ui.set_min_size(egui::vec2(width, room));
        if !says.text().is_empty() {
            ui.add(egui::Label::new(says).wrap());
        }
    });
}

/// Enter pressed in a field: the dialog's default button, as in any
/// dialog. A one-line field lets the keyboard go on Enter.
fn entered(field: &egui::Response) -> bool {
    field.lost_focus() && field.ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

/// Enter pressed in the dialog in front with none of its controls holding
/// the keyboard: its default button, as a field's Enter is. A button of its
/// own that has the keyboard takes Enter for itself instead.
fn enter_free(ui: &egui::Ui) -> bool {
    let ctx = ui.ctx();
    let in_front = ctx.memory(|m| m.top_modal_layer()) == Some(ui.layer_id());
    let own = ctx
        .memory(|m| m.focused())
        .and_then(|id| ctx.read_response(id))
        .is_some_and(|r| r.layer_id == ui.layer_id());
    in_front && !own && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// A number's field has all of its digits selected as it takes the
/// keyboard, so what is typed replaces them: "500" typed into the limit's
/// 0 made 0500 (gap 140).
fn select_on_focus(ui: &egui::Ui, field: &egui::Response, text: &str) {
    let key = field.id.with("dm-selected-on-focus");
    let done = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    if field.has_focus() && !done {
        let mut state = egui::TextEdit::load_state(ui.ctx(), field.id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::select_all_str(text)));
        state.store(ui.ctx(), field.id);
        ui.data_mut(|d| d.insert_temp(key, true));
    } else if !field.has_focus() && done {
        ui.data_mut(|d| d.remove::<bool>(key));
    }
}

/// Gives a field the keyboard when `now` says, and passes it on.
fn focused(field: egui::Response, now: bool) -> egui::Response {
    if now {
        field.request_focus();
    }
    field
}

/// A folder's field: its name above, the path, and Browse… beside it.
fn folder_field(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    platform: &dyn crate::backend::Platform,
) {
    let name = ui.label(label);
    ui.horizontal(|ui| {
        // Browse keeps its room beside it, however narrow the dialog.
        let wide = (ui.available_width() - 120.0).clamp(120.0, 380.0);
        ui.add(egui::TextEdit::singleline(text).desired_width(wide))
            .labelled_by(name.id);
        browse(ui, platform, text);
    });
}

/// A Browse… button beside a folder's field: the folder picked goes in it.
/// Where the desktop has no folder chooser (a Linux desktop with neither
/// the portal's nor zenity), says so beside the button and to type the
/// path: the chooser's absence looked like a Cancel, and the button did
/// nothing.
fn browse(ui: &mut egui::Ui, platform: &dyn crate::backend::Platform, folder: &mut String) {
    let missing = ui.scope_id().with("no-folder-chooser");
    if let Some(chose) = browse_button(ui, platform, folder) {
        if chose {
            ui.data_mut(|d| d.remove::<bool>(missing));
        } else {
            ui.data_mut(|d| d.insert_temp(missing, true));
        }
    }
    if ui.data(|d| d.get_temp::<bool>(missing)).unwrap_or(false) {
        ui.colored_label(
            warn(ui),
            t("No folder chooser on this desktop; type the folder's path."),
        );
    }
}

/// The Browse… button itself: `None` when not pressed, else whether the
/// desktop had a folder chooser to ask.
fn browse_button(
    ui: &mut egui::Ui,
    platform: &dyn crate::backend::Platform,
    folder: &mut String,
) -> Option<bool> {
    if !ui.button(t("Browse…")).clicked() {
        return None;
    }
    if !platform.can_choose_folder() {
        return Some(false);
    }
    if let Some(picked) = platform.choose_folder() {
        *folder = picked.display().to_string();
    }
    Some(true)
}

pub fn show(app: &mut DmApp, ctx: &egui::Context) {
    // The keyboard goes back to what opened a dialog once it closes: it
    // was left nowhere, and the next Tab started from the window's top (a
    // design review). Only where the keyboard had a place to begin with.
    let open = app.dialogs.in_front();
    if open && !app.dialogs.was_open {
        app.dialogs.focus_before = ctx.memory(|m| m.focused());
    } else if !open && app.dialogs.was_open {
        if let Some(id) = app.dialogs.focus_before.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }
    app.dialogs.was_open = open;
    // A question waiting for its answer dims the window behind it, so it
    // reads as the one thing to deal with (a design review: nothing set it
    // apart, and the list behind looked as usable as ever).
    // In the modern look every dialog that keeps the clicks to itself
    // does, as a ContentDialog lays SmokeFillColorDefault over its window.
    let tokens = crate::theme::Tokens::get(ctx);
    let smoke = if tokens.modern {
        app.dialogs.modal().then_some(0x4d)
    } else {
        app.dialogs
            .asking()
            .then_some(if tokens.dark { 110 } else { 70 })
    };
    if let Some(alpha) = smoke {
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            ctx.content_rect(),
            0.0,
            egui::Color32::from_black_alpha(alpha),
        );
    }
    for id in std::mem::take(&mut app.dialogs.raise) {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
    }
    countdown(app, ctx);
    confirm_exit(app, ctx);
    links(app, ctx);
    confirm_remove(app, ctx);
    confirm_outside(app, ctx);
    confirm_cancel(app, ctx);
    duplicate(app, ctx);
    import_ask(app, ctx);
    list_file(app, ctx);
    skip_offer(app, ctx);
    add_url(app, ctx);
    file_info(app, ctx);
    progress(app, ctx);
    complete(app, ctx);
    properties(app, ctx);
    series(app, ctx);
    clip_links(app, ctx);
    site_files(app, ctx);
    update_link(app, ctx);
    move_to_queue(app, ctx);
    rename(app, ctx);
    find(app, ctx);
    scheduler(app, ctx);
    limiter(app, ctx);
    options(app, ctx);
    about(app, ctx);
    /* Escape closes the dialog in front — unless a menu, combo box or
     * right-click menu was open: that Escape was theirs, and they have
     * already closed on it by the time this runs (menus and the table draw
     * before the dialogs), so "open now" is no guide and last frame's state
     * is the one asked. A text field with the keyboard lets it go on the
     * same Escape that closes the dialog, as dialogs generally do. */
    let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if escape && !app.popup_was_open {
        let order: Vec<egui::Id> = ctx.memory(|m| m.layer_ids().map(|layer| layer.id).collect());
        app.dialogs.close_top(&order);
    }
    app.popup_was_open = ctx.any_popup_open();
    // The topmost of the dialogs in front keeps the keyboard and the
    // clicks: egui takes the highest layer given.
    // A question is kept on top: a progress window opened by the same
    // click covered "Start outside the schedule?" (UI round 12).
    for title in app.dialogs.questions() {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, window_id(title)));
    }
    for layer in app.dialogs.modal_layers() {
        ctx.memory_mut(|m| m.set_modal_layer(layer));
    }
}

/// Asked before Remove, however it was asked for — the Delete key, the
/// toolbar, the menus: the Delete key took finished rows off at once while
/// the toolbar asked. Unfinished rows lose what was fetched; finished ones
/// only leave the list.
fn confirm_remove(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ids) = app.dialogs.confirm_remove.clone() else {
        return;
    };
    // A cancelled row has nothing left to lose: its parts went with the
    // cancel (UI round 8: it was warned about as unfinished).
    let unfinished = ids
        .iter()
        .filter_map(|id| app.row(*id))
        .any(|r| !matches!(r.state, State::Completed | State::Cancelled));
    // Whether there is anything to throw away: a row added for later and
    // never started has fetched nothing, which the warning said it would
    // lose (round 13).
    let fetched = ids.iter().filter_map(|id| app.row(*id)).any(|r| {
        !matches!(r.state, State::Completed | State::Cancelled) && r.progress.downloaded > 0
    });
    // The finished files still on disk that could go with their rows.
    let files: Vec<String> = ids
        .iter()
        .filter_map(|id| app.row(*id))
        .filter(|r| r.state == State::Completed && std::path::Path::new(&r.path).is_file())
        .map(|r| r.path.clone())
        .collect();
    let (mut yes, mut no) = (false, false);
    // Not "unfinished" of a set with finished files in it (UI round 17).
    let english = if unfinished && ids.len() == 1 {
        "Remove an unfinished download"
    } else if unfinished && files.is_empty() {
        "Remove unfinished downloads"
    } else {
        "Remove from the list"
    };
    app.dialogs.remove_title = english;
    let title = match english {
        "Remove an unfinished download" => t("Remove an unfinished download"),
        "Remove unfinished downloads" => t("Remove unfinished downloads"),
        _ => t("Remove from the list"),
    };
    Dialog::new(title)
        .id(window_id("Delete unfinished downloads"))
        .collapsible(false)
        .resizable(false)
        .min_width(440.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            // Whole sentences, one for each number, since languages do not
            // build them from the same pieces.
            // Finished files beside unfinished rows: said apart, with what
            // happens to the files; "have not fetched anything yet" was said
            // of two finished files the box would delete (UI round 16).
            let mixed = unfinished && !files.is_empty();
            ui.label(match (unfinished, ids.len()) {
                (true, _) if mixed => t(
                    "Some of these are finished and some are not. Unfinished ones lose what they fetched.",
                ),
                (true, 1) if !fetched => {
                    t("This download has not fetched anything yet. Delete it from the list?")
                }
                (true, _) if !fetched => {
                    t("These downloads have not fetched anything yet. Delete them from the list?")
                }
                (true, 1) => t(
                    "This download is not finished. Removing it throws away what has been fetched.",
                ),
                (true, _) => t(
                    "Some of these are not finished. Removing them throws away what has been fetched.",
                ),
                // What the box below says happens to the files, said here
                // too: the sentence promised the file stayed while the box
                // was deleting it (round 16).
                (false, 1) if app.dialogs.remove_files => {
                    t("Take this download off the list and delete its file from the disk?")
                }
                (false, _) if app.dialogs.remove_files => {
                    t("Take these downloads off the list and delete their files from the disk?")
                }
                // No file on the disk (cancelled, or since deleted): none to
                // stay anywhere (UI round 9).
                (false, 1) if files.is_empty() => t("Take this download off the list?"),
                (false, _) if files.is_empty() => t("Take these downloads off the list?"),
                (false, 1) => t("Take this download off the list? Its file stays where it is."),
                (false, _) => {
                    t("Take these downloads off the list? Their files stay where they are.")
                }
            });
            if mixed {
                // In its number: "the finished ones'" of one (UI round 18).
                ui.label(match (app.dialogs.remove_files, files.len()) {
                    (true, 1) => t("The finished one's file is deleted from the disk too."),
                    (true, _) => t("The finished ones' files are deleted from the disk too."),
                    (false, 1) => t("The finished one's file stays where it is."),
                    (false, _) => t("The finished ones' files stay where they are."),
                });
            }
            // Which one, by name: "this download" was all it said.
            if let [only] = ids.as_slice() {
                if let Some(row) = app.row(*only) {
                    let named = app.name_it_will_get(row, &mut app.applied.categories());
                    ui.strong(&*crate::bidi::visual(&named));
                }
            } else {
                // And how many, where it is more than one: 507 went with
                // no number said (UI round 14).
                ui.strong(tf("Downloads: {}", &[&ids.len()]));
            }
            // The buttons say what the sentence asked, in its number, and
            // Enter is the one that loses nothing (gap 149).
            let (go, keep) = match (unfinished, ids.len()) {
                (true, 1) => (t("Yes, remove"), t("Keep it")),
                (true, _) => (t("Yes, remove"), t("Keep them")),
                (false, 1) => (t("Remove it"), t("Keep it")),
                (false, _) => (t("Remove them"), t("Keep them")),
            };
            // A finished file can go with its row, if asked for; never by
            // default, and never by Enter (round 15's audit).
            let files = files.len();
            let go = if files > 0 {
                check(ui,
                    &mut app.dialogs.remove_files,
                    if files == 1 {
                        t("Also delete the file from the disk")
                    } else {
                        t("Also delete their files from the disk")
                    },
                );
                if app.dialogs.remove_files {
                    t("Yes, delete")
                } else {
                    go
                }
            } else {
                go
            };
            // The default first, on the left, as every dialog has it (UI
            // round 7: here alone it was on the right).
            footer(ui, |ui| {
                no = primary_worded(ui, keep).clicked() || enter_free(ui);
                yes = ui.button(go).clicked();
            });
        });
    if yes {
        app.dialogs.confirm_remove = None;
        let delete = std::mem::take(&mut app.dialogs.remove_files);
        if delete {
            let failed: Vec<String> = files
                .iter()
                .filter_map(|path| {
                    std::fs::remove_file(path)
                        .err()
                        .map(|e| format!("{path}: {e}"))
                })
                .collect();
            if !failed.is_empty() {
                app.said = crate::i18n::Said::new("Could not delete {}", &[&failed.join("; ")]);
            }
        }
        // The selection goes to the row after those removed, or the one
        // before where they were the last: it was lost, and the keys had
        // nowhere to go on from.
        let shown: Vec<Id> = app.visible().iter().map(|r| r.id).collect();
        let next = shown
            .iter()
            .rposition(|id| ids.contains(id))
            .and_then(|last| {
                shown
                    .get(last + 1..)
                    .and_then(|after| after.iter().find(|id| !ids.contains(id)))
                    .or_else(|| {
                        shown
                            .get(..last)
                            .and_then(|before| before.iter().rev().find(|id| !ids.contains(id)))
                    })
            })
            .copied();
        app.remove_now(&ids);
        if let Some(id) = next {
            app.selected.insert(id);
            app.cursor = Some(id);
            app.anchor = Some(id);
        }
    } else if no {
        app.dialogs.confirm_remove = None;
        app.dialogs.remove_files = false;
    }
}

/// Asked before the progress window's Cancel download throws away what
/// was fetched. Enter keeps the download.
fn confirm_cancel(app: &mut DmApp, ctx: &egui::Context) {
    let Some(id) = app.dialogs.confirm_cancel else {
        return;
    };
    // Gone, or finished (or stopped) while it was asked: nothing to cancel,
    // and the question about a finished file stood beside Download
    // complete (UI round 3).
    let fetched = app.row(id).is_some_and(|r| r.progress.downloaded > 0);
    let Some(name) = app
        .row(id)
        .filter(|r| matches!(r.state, State::Running | State::Queued | State::Paused))
        .map(|r| app.name_it_will_get(r, &mut app.applied.categories()))
    else {
        app.dialogs.confirm_cancel = None;
        return;
    };
    let (mut cancel, mut keep) = (false, false);
    window(ctx, "Cancel this download?").show(ctx, |ui| {
        ui.strong(&*crate::bidi::visual(&name));
        if fetched {
            ui.label(t("Cancelling it throws away what has been fetched."));
        }
        // Apart from the words, as in the other questions: the buttons sat
        // against the file's name (UI round 18).
        ui.add_space(8.0);
        // The default first, as in every dialog (UI round 7).
        footer(ui, |ui| {
            keep = primary_worded(ui, t("Keep downloading")).clicked() || enter_free(ui);
            cancel = ui.button(t("Yes, cancel")).clicked();
        });
    });
    if cancel {
        app.dialogs.confirm_cancel = None;
        app.backend.cancel(id);
        app.dialogs.progress.remove(&id);
        app.dialogs.progress_closed.insert(id);
        app.refresh();
    } else if keep {
        app.dialogs.confirm_cancel = None;
    }
}

/// Asked when Resume is pressed on a download the schedule or the
/// download limit holds: start it now anyway, for this one only, or let it
/// wait.
fn confirm_outside(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ids) = app.dialogs.confirm_outside.clone() else {
        return;
    };
    let (mut start, mut wait) = (false, false);
    // The name the list shows: "big.zip" was asked about while the row
    // read "big (1).zip" (UI round 11).
    let mut categories = app.applied.categories();
    let names: Vec<String> = ids
        .iter()
        .filter_map(|id| app.row(*id))
        .map(|r| app.name_it_will_get(r, &mut categories))
        .collect();
    let why = app.held_because().unwrap_or_default();
    window(ctx, "Start outside the schedule?").show(ctx, |ui| {
        ui.label(if names.len() == 1 {
            tf(
                "{} waits for the schedule or the download limit.",
                &[&names.join(", ")],
            )
        } else {
            tf(
                "{} downloads wait for the schedule or the download limit.",
                &[&names.len()],
            )
        });
        if !why.is_empty() {
            ui.label(RichText::new(why).small().weak());
        }
        ui.label(t(
            "Start it now anyway? Only what you picked starts; the rest keep waiting.",
        ));
        footer(ui, |ui| {
            start = primary(ui, "Start now").clicked() || enter_free(ui);
            wait = ui.button(t("Keep waiting")).clicked();
        });
    });
    if start {
        app.dialogs.confirm_outside = None;
        for id in &ids {
            if app.backend.start_outside_schedule(*id) && app.applied.show_progress {
                app.dialogs.progress.insert(*id);
            }
        }
        app.refresh();
    } else if wait {
        app.dialogs.confirm_outside = None;
    }
}

/// Asked when Exit is chosen while downloads run: quitting stops them, and
/// they go on from where they were the next time dm opens.
fn confirm_exit(app: &mut DmApp, ctx: &egui::Context) {
    if !app.dialogs.confirm_exit {
        return;
    }
    let (mut exit, mut stay) = (false, false);
    // Running and waiting counted apart: a queued row is not running, and
    // counting it as one told of downloads that were not (gap 157).
    let (busy, waiting) = app.running_and_waiting();
    let mut said = Vec::new();
    // Finished meanwhile: the question changes, and dm still waits for the
    // person's answer. It used to quit by itself in the first frame that
    // counted nothing running, so a person reading the question, or
    // reaching for "Keep downloading", saw dm vanish (gap 163). Only Exit
    // quits.
    let finished = busy + waiting == 0;
    if finished {
        said.push(t("Nothing is downloading any more. Exit dm?").to_string());
    }
    if busy > 0 {
        said.push(crate::i18n::tn(
            "{} download is still running.||{} downloads are still running.",
            busy as u64,
            &[&busy],
        ));
    }
    if waiting > 0 && busy > 0 {
        said.push(crate::i18n::tn(
            "{} more is waiting to start.||{} more are waiting to start.",
            waiting as u64,
            &[&waiting],
        ));
    } else if waiting > 0 {
        said.push(crate::i18n::tn(
            "{} download is waiting to start.||{} downloads are waiting to start.",
            waiting as u64,
            &[&waiting],
        ));
    }
    if !finished {
        // As plural as the count it speaks of (gap 164).
        said.push(crate::i18n::tn(
            "Exiting stops it; it goes on from where it was the next time dm opens.||Exiting stops them; each goes on from where it was the next time dm opens.",
            (busy + waiting) as u64,
            &[],
        ));
    }
    // Narrower on a screen that has not the room, as the other dialogs.
    let width = EXIT_WIDTH.min(ctx.content_rect().width() - 48.0).max(220.0);
    window(ctx, "Exit dm?").show(ctx, |ui| {
        // One width and room for three lines whatever the words say: a
        // download finishing while the question is open changes them, and
        // a window that shrank around shorter words moved its buttons —
        // Exit slid under a pointer on its way to "Keep downloading".
        ui.set_width(width);
        let lines =
            ui.text_style_height(&egui::TextStyle::Body) * 3.0 + ui.spacing().item_spacing.y * 2.0;
        ui.allocate_ui_with_layout(
            egui::vec2(width, lines),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_height(lines);
                ui.add(egui::Label::new(said.join(" ")).wrap());
            },
        );
        footer(ui, |ui| {
            // With downloads running, the highlighted answer is the one that
            // loses nothing, as Remove's is; Exit only by its own click
            // (gap 163). With nothing running, Exit is what was asked for.
            // Each button keeps its place whatever the question says (gap
            // 163); only which one is highlighted changes.
            // The second as wide as its longer words, so that the row,
            // set at the right, keeps Exit where it was either way.
            let font = egui::TextStyle::Button.resolve(ui.style());
            let words = |w: &str| {
                ui.painter()
                    .layout_no_wrap(w.to_string(), font.clone(), Color32::WHITE)
                    .size()
                    .x
            };
            let second = words(t("Stay")).max(words(t("Keep downloading")))
                + ui.spacing().button_padding.x * 2.0;
            if finished {
                exit = primary(ui, "Exit").clicked();
                stay = ui
                    .add(egui::Button::new(t("Stay")).min_size(egui::vec2(second, 0.0)))
                    .clicked();
            } else {
                exit = ui.button(t("Exit")).clicked();
                stay = primary_sized(ui, t("Keep downloading"), second).clicked();
            }
        });
    });
    if exit {
        app.dialogs.confirm_exit = false;
        app.quit_asked = true;
    } else if stay {
        app.dialogs.confirm_exit = false;
    }
}

/// The Exit question's width, held whatever its words say, where the
/// screen has room for it.
const EXIT_WIDTH: f32 = 440.0;

/// Asked after two handed-over downloads from one site were cancelled in a
/// row: whether to leave that site's downloads to the browser from now on,
/// which puts it on Options › Browser's list of sites never taken.
fn skip_offer(app: &mut DmApp, ctx: &egui::Context) {
    let Some(site) = app.dialogs.skip_offer.clone() else {
        return;
    };
    let (mut leave, mut keep) = (false, false);
    window(ctx, "Leave this site to the browser?").show(ctx, |ui| {
        ui.label(tf(
            "You cancelled two downloads handed over from {}. Leave its downloads to the browser from now on?",
            &[&site],
        ));
        footer(ui, |ui| {
            leave = primary(ui, "Leave them to the browser").clicked();
            keep = ui.button(t("Keep handing them over")).clicked();
        });
    });
    if leave {
        app.dialogs.skip_offer = None;
        let mut settings = app.applied.clone();
        if !settings.catch.skip_hosts.iter().any(|h| h == &site) {
            settings.catch.skip_hosts.push(site);
        }
        if let Err(e) = app.apply_settings(settings) {
            app.problems.push(e);
        }
    } else if keep {
        app.dialogs.skip_offer = None;
    }
}

/// An address without its login, for telling two rows apart.
pub(crate) fn bare(url: &str) -> String {
    url::Url::parse(url)
        .map(|mut u| {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.to_string()
        })
        .unwrap_or_else(|_| url.to_string())
}

/// Asked when a list being imported holds addresses already in the list,
/// as Add URL asks of one address: once, for all of them (gap 143).
fn import_ask(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ask) = app.dialogs.import_ask.as_ref() else {
        return;
    };
    let (already, total) = (ask.already, ask.total);
    let (mut skip, mut again, mut cancel) = (false, false, false);
    window(ctx, "Some are already in the list").show(ctx, |ui| {
        ui.label(crate::i18n::tn(
            "{} of the {} addresses in this list is already in dm's list.||{} of the {} addresses in this list are already in dm's list.",
            already as u64,
            &[&already, &total],
        ));
        footer(ui, |ui| {
            skip = primary(ui, "Skip them").clicked() || enter_free(ui);
            again = ui.button(t("Add them again")).clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if skip || again {
        if let Some(ask) = app.dialogs.import_ask.take() {
            app.import_text(&ask.text, skip);
        }
    } else if cancel {
        app.dialogs.import_ask = None;
    }
}

/// Import or Export's file, typed where the desktop has no file chooser:
/// both did nothing there, without a word (gap 144).
fn list_file(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("List file");
    let Some(d) = app.dialogs.list_file.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    let dialog = if d.save {
        window(ctx, "Export the list")
    } else {
        window(ctx, "Import a list")
    };
    dialog.open(&mut open).show(ctx, |ui| {
        ui.label(t("No file chooser on this desktop; type the file's path."));
        let path = focused(field(ui, t("File's path"), &mut d.path, 420.0), focus);
        ok |= entered(&path);
        if !d.says.is_empty() {
            ui.colored_label(warn(ui), &d.says);
        }
        footer(ui, |ui| {
            ok |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if cancel || !open {
        app.dialogs.list_file = None;
        return;
    }
    if !ok {
        return;
    }
    let typed = d.path.trim();
    // The home folder as the settings find it, on Windows too, which has
    // no HOME (CI, 0.2.0).
    let path = dm_shell::settings::full_path(typed).unwrap_or_else(|| typed.into());
    if typed.is_empty() || !path.is_absolute() {
        d.says = tf(
            "Type the file's full path, such as {}.",
            &[&default_list_path(&app.applied).display()],
        );
        return;
    }
    let save = d.save;
    let done = if save {
        app.export_to(&path)
    } else {
        app.import_from(&path)
    };
    // Kept open with what went wrong, and the path as typed: closed, the
    // path was lost and further typing went to the list (UI round 12).
    match done {
        Ok(()) => app.dialogs.list_file = None,
        Err(why) => {
            if let Some(d) = app.dialogs.list_file.as_mut() {
                d.says = why;
            }
            // The path field keeps the keyboard, to be put right (UI
            // round 13: typing went nowhere).
            app.dialogs.focus_on("List file");
        }
    }
}

/// Sites as the rules match them, by host name: an address pasted whole,
/// scheme and path and all, was kept as it was and matched nothing (UI
/// round 14). A bare name, or one with a leading dot, is kept.
pub fn sites(words: &[String]) -> Vec<String> {
    words
        .iter()
        .map(|w| {
            let host = if w.contains("://") {
                url::Url::parse(w)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string))
            } else {
                // "example.com/path" or "example.com:8080": the name alone.
                w.split(['/', ':']).next().map(str::to_string)
            };
            host.filter(|h| !h.is_empty())
                .unwrap_or_else(|| w.clone())
                .to_ascii_lowercase()
        })
        .collect()
}

/// The `file://` address of a `.torrent` on this computer named by `text`,
/// its path (`~` for the home folder) or that address itself, if the file
/// is there.
pub fn local_torrent(text: &str) -> Option<String> {
    let text = text.trim();
    let path = match dm_core::torrent::local_file(text) {
        Some(path) => path,
        None if !text.contains("://") => dm_shell::settings::full_path(text)?,
        None => return None,
    };
    if !dm_core::is_torrent(&path.to_string_lossy()) || !path.is_file() {
        return None;
    }
    dm_core::torrent::file_url(&path)
}

/// Where Export offers to write the list when its path is typed.
pub fn default_list_path(settings: &Settings) -> std::path::PathBuf {
    settings.download_dir.join("downloads.dmlist")
}

/// The row already holding `address`, if one does and was not cancelled.
pub(crate) fn already_listed(app: &DmApp, address: &str) -> Option<Id> {
    let wanted = bare(address);
    app.rows()
        .iter()
        .find(|r| r.state != State::Cancelled && bare(&r.url) == wanted)
        .map(|r| r.id)
}

/// Asked when an address typed in is already a row: download it again,
/// show the row, or leave it.
fn duplicate(app: &mut DmApp, ctx: &egui::Context) {
    let Some((address, id)) = app.dialogs.duplicate.clone() else {
        return;
    };
    let status = app.row(id).map(crate::app::status_of).unwrap_or_default();
    let (mut again, mut show, mut cancel) = (false, false, false);
    window(ctx, "Already in the list").show(ctx, |ui| {
        ui.label(RichText::new(format::shown_url(&address)).weak());
        ui.label(tf("This address is already in the list ({}).", &[&status]));
        footer(ui, |ui| {
            // Enter presses it, as in every other dialog (gap 154).
            // Enter shows the row, which changes nothing: an Enter pressed
            // for Download details, a moment before this asked, fetched the
            // file a second time (round 15).
            show = primary(ui, "Show it").clicked() || enter_free(ui);
            again = ui.button(t("Download again")).clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if again {
        app.dialogs.duplicate = None;
        app.dialogs.open_file_info(vec![address]);
    } else if show {
        app.dialogs.duplicate = None;
        app.selected.clear();
        app.selected.insert(id);
    } else if cancel {
        app.dialogs.duplicate = None;
    }
}

fn add_url(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Add URL");
    let recent = app.applied.recent_urls.clone();
    let Some(d) = app.dialogs.add_url.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel, mut forget) = (false, false, false);
    window(ctx, "Add URL").open(&mut open).show(ctx, |ui| {
        // As wide as the window allows: at 200% text a fixed 420 ran the
        // dialog past the window's edge (UI round 1).
        let wide = (ctx.content_rect().width() - 160.0).clamp(200.0, 420.0);
        let address = focused(field(ui, t("Address"), &mut d.address, wide), focus);
        // What was said of the last address goes when it is changed: the
        // error stayed under a good one (speed check 2, 2026-10-19).
        if address.changed() {
            d.says.clear();
        }
        // The link it was opened with is selected, so a link pasted or
        // typed replaces it rather than joining it (gap 142).
        select_on_focus(ui, &address, &d.address);
        ok |= entered(&address);
        // Addresses added here before, to pick again, and a way to forget
        // them.
        if !recent.is_empty() {
            ui.horizontal(|ui| {
                let label = ui.label(t("Recent"));
                // As wide as the address beside it allows: a fixed 320 with
                // Clear history beside it ran past a narrow window.
                combo("add-url-recent")
                    .selected_text("")
                    .width((wide - 160.0).max(120.0))
                    .show_ui(ui, |ui| {
                        for url in &recent {
                            if ui.selectable_label(false, format::shown_url(url)).clicked() {
                                d.address = url.clone();
                            }
                        }
                    })
                    .response
                    .labelled_by(label.id);
                forget = ui.button(t("Clear history")).clicked();
            });
        }
        check(ui, &mut d.authorize, t("Needs a sign-in"));
        ui.add_enabled_ui(d.authorize, |ui| {
            ui.horizontal(|ui| {
                ok |= entered(&field(ui, t("Login"), &mut d.login, 140.0));
                let name = ui.label(t("Password"));
                let password = ui
                    .add(
                        egui::TextEdit::singleline(&mut d.password)
                            .password(true)
                            .desired_width(140.0),
                    )
                    .labelled_by(name.id);
                ok |= entered(&password);
            });
        });
        warning_line(ui, &d.says);
        footer(ui, |ui| {
            ok |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if forget {
        match app.backend.remember_urls(Vec::new()) {
            Ok(now) => app.applied.recent_urls = now.recent_urls,
            Err(e) => app.said = e.into(),
        }
        return;
    }
    if cancel || !open {
        app.dialogs.add_url = None;
        return;
    }
    if !ok {
        return;
    }
    let mut address = d.address.trim().to_string();
    // A .torrent on this computer, by its path or its file:// address: it
    // could only be dragged in, and Add URL called it no link (UI round
    // 17).
    if let Some(local) = local_torrent(&address) {
        address = local;
    }
    if crate::app::looks_downloadable(&address).is_none() && local_torrent(&address).is_none() {
        // Nothing typed is asked for, not called a bad link.
        d.says = if address.is_empty() {
            t("Type or paste the address of what to download.").into()
        } else {
            t("That is not a link dm can download. Paste a web address, such as https://example.com/file.zip.").into()
        };
        // The address keeps the keyboard, to be put right: after the
        // warning, typing went nowhere (round 13).
        app.dialogs.focus_on("Add URL");
        return;
    }
    if crate::app::two_addresses(&address) {
        d.says = t("That is two addresses run together. Add one at a time, or several with Program › Add links from clipboard.").into();
        // The address keeps the keyboard, to be put right: after the
        // warning, typing went nowhere (round 13).
        app.dialogs.focus_on("Add URL");
        return;
    }
    if d.authorize && !d.login.is_empty() {
        if let Ok(mut url) = url::Url::parse(&address) {
            let _ = url.set_username(&d.login);
            let _ = url.set_password(Some(&d.password));
            address = url.to_string();
        }
    }
    app.dialogs.add_url = None;
    // Remembered for Add URL's list, without any login.
    let remembered = bare(&address);
    if app.applied.recent_urls.first() != Some(&remembered) {
        let mut recent = app.applied.recent_urls.clone();
        recent.retain(|u| *u != remembered);
        recent.insert(0, remembered);
        match app.backend.remember_urls(recent) {
            Ok(now) => app.applied.recent_urls = now.recent_urls,
            Err(e) => app.said = e.into(),
        }
    }
    // Already a row: asked, rather than a second row nobody meant.
    if let Some(id) = already_listed(app, &address) {
        app.dialogs.duplicate = Some((address, id));
        return;
    }
    app.dialogs.open_file_info(vec![address]);
}

fn file_info(app: &mut DmApp, ctx: &egui::Context) {
    if app.dialogs.file_info.is_none() {
        return;
    }
    let focus = app.dialogs.take_focus("Download details");
    let Some(d) = app.dialogs.file_info.as_mut() else {
        return;
    };
    let queues = app.backend.queues();
    let default_dir = app.applied.download_dir.display().to_string();
    // Each kind's folder, and each category a person made, to pick from:
    // a file can go to another kind's folder without typing its path.
    let places: Vec<(String, String)> = {
        // Each kind's own folder, whether or not finished files are sorted
        // into them: picked here, it is asked for (with sorting off every
        // kind named the download folder itself, round 17).
        let mut categories = app.applied.categories();
        categories.enabled = true;
        let root = &app.applied.download_dir;
        let mut places: Vec<(String, String)> = dm_core::Category::ALL
            .iter()
            .map(|kind| {
                let word = format::titled(&category_word(*kind));
                (
                    crate::i18n::name(&word),
                    categories.destination(root, *kind).display().to_string(),
                )
            })
            .collect();
        places.extend(app.applied.custom_categories.iter().map(|custom| {
            let name = custom.name.trim().to_string();
            (name.clone(), root.join(&name).display().to_string())
        }));
        places
    };
    // The folder an empty field means: sorted into kinds' folders only
    // where Options › Folders sorts them.
    let default_dir = if app.applied.categorize {
        tf("{} (sorted by kind)", &[&default_dir])
    } else {
        default_dir
    };
    let platform = std::sync::Arc::clone(&app.backend.platform);
    // The server is asked what it calls the file, once, as the dialog
    // opens; Save as had only the address's last part to show.
    if d.probed.is_none() && d.urls.len() == 1 && d.name.is_empty() {
        let slot: std::sync::Arc<std::sync::Mutex<Option<Probed>>> = Default::default();
        let remux = app.applied.remux_ts;
        d.probed = Some(slot.clone());
        let url = d.urls.first().cloned().unwrap_or_default();
        // Not a torrent's: what the server says of a .torrent is the
        // metainfo's few kilobytes and its file name, which Download details
        // showed as the download's (torrent check, 2026-10-19).
        if (url.starts_with("http://") || url.starts_with("https://")) && !dm_core::is_torrent(&url)
        {
            let client = dm_core::proxy::client_with(&app.backend.settings().proxy)
                .unwrap_or_else(|_| dm_core::client());
            let sign_in = dm_core::cookies::SignIn::new(&url, &d.cookies);
            let referrer = d.referrer.clone();
            let ctx = ctx.clone();
            app.backend.runtime.spawn(async move {
                let asked = tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    dm_core::probe::probe(
                        &client,
                        &url,
                        Some(&referrer)
                            .filter(|r| !r.is_empty())
                            .map(String::as_str),
                        sign_in.as_ref(),
                    ),
                )
                .await;
                // A web page's name ("watch") and size are no file's: left
                // unsaid. A stream's are its file's, not its playlist's.
                if let Ok(Ok(probe)) = asked {
                    let kind = probe.content_type.as_deref();
                    let found = if let Some(name) = stream_name(&url, kind, remux) {
                        Probed {
                            name: Some(name),
                            ..Probed::default()
                        }
                    } else if dm_core::ytdlp::is_page(kind) {
                        Probed {
                            page: !dm_core::download::names_a_document(&url),
                            ..Probed::default()
                        }
                    } else {
                        Probed {
                            name: Some(probe.filename).filter(|n| !n.is_empty()),
                            size: probe.size,
                            ..Probed::default()
                        }
                    };
                    *dm_core::lock(&slot) = Some(found);
                    ctx.request_repaint();
                } else if let Ok(Err(e)) = asked {
                    // A watch page that turned dm's quick look away (a 429
                    // bot check, a 403) is the video helper's to read when
                    // the download starts, as the engine does: no
                    // "Not available" and the bot page's text (newcomer
                    // check, 2026-10-21).
                    let refused_page = matches!(
                        e,
                        dm_core::Error::Refused {
                            status: 403 | 429,
                            ..
                        }
                    ) && !dm_core::download::names_a_document(&url);
                    *dm_core::lock(&slot) = Some(if refused_page {
                        Probed {
                            page: true,
                            ..Probed::default()
                        }
                    } else {
                        Probed {
                            problem: Some(e.to_string()),
                            ..Probed::default()
                        }
                    });
                    ctx.request_repaint();
                }
            });
        } else {
            // Nothing to ask: known at once, so nothing waits on it.
            *dm_core::lock(&slot) = Some(Probed::default());
        }
    }
    let probed = d.probed.as_ref().and_then(|p| dm_core::lock(p).clone());
    let server_name = probed.as_ref().and_then(|p| p.name.clone());
    let problem = probed.as_ref().and_then(|p| p.problem.clone());
    let page = probed.as_ref().is_some_and(|p| p.page);
    let size = probed.and_then(|p| p.size);
    let remux = app.applied.remux_ts;
    if let Some(name) = &server_name {
        if !d.name_typed && d.name.is_empty() {
            d.name = name.clone();
        }
    }
    // The name it will really get where its folder has one of that name
    // already: Save as said `big.zip` and the file came as `big (1).zip`
    // (UI round 10).
    let renamed = (d.urls.len() == 1 && !d.name.trim().is_empty())
        .then(|| {
            let name = d.name.trim();
            let url = d.urls.first().map(String::as_str).unwrap_or_default();
            let dir = if !d.folder.trim().is_empty() {
                std::path::PathBuf::from(d.folder.trim())
            } else {
                let mut categories = app.applied.categories();
                if categories.enabled {
                    categories.site = dm_core::category::host_of(url);
                    let kind = dm_core::category::by_name(name).unwrap_or(dm_core::Category::Other);
                    categories.place(&app.applied.download_dir, kind, name)
                } else {
                    app.applied.download_dir.clone()
                }
            };
            // Every download of that name still waiting to be placed there
            // takes a free name first (UI round 12: two rows read
            // "big (1).zip" and one landed as "big (2).zip").
            let waiting = app
                .rows
                .iter()
                .filter(|r| {
                    !r.state.settled()
                        && r.path.is_empty()
                        && name_of(r) == name
                        && r.save_dir.as_deref().is_none_or(|s| {
                            s.trim().is_empty() || std::path::Path::new(s) == dir
                        })
                })
                .count();
            let free = dm_core::download::nth_free_name(&dir, name, waiting);
            if free == name {
                None
            } else if dm_core::download::free_name(&dir, name) == name {
                // Free on disk: it is the list that has the name.
                Some(t("Another download in the list has this name; this one will be given a number.").to_string())
            } else {
                Some(tf(
                    "A file of that name is in the folder already; this one will be {}.",
                    &[&crate::bidi::visual(&free)],
                ))
            }
        })
        .flatten();
    let mut open = true;
    let mut chosen: Option<bool> = None; // Some(later)
    let mut cancel = false;
    // No wider than the window: a warning in it wraps rather than pushing
    // Start now off the edge (gap 133).
    let room = (ctx.content_rect().width() - sides(ctx)).clamp(360.0, 600.0);
    window(ctx, "Download details")
        .max_width(room)
        .open(&mut open)
        .show(ctx, |ui| {
            let first = d.urls.first().cloned().unwrap_or_default();
            ui.label(RichText::new(format::shown_url(&first)).weak());
            if d.urls.len() > 1 {
                ui.label(tf("and {} more", &[&(d.urls.len() - 1)]));
            }
            ui.separator();
            egui::Grid::new("file-info")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let name = ui.label(t("Category"));
                    combo("details-category")
                        .selected_text(
                            places
                                .iter()
                                .find(|(_, path)| *path == d.folder.trim())
                                .map(|(name, _)| name.clone())
                                .unwrap_or_else(|| t("As its type says").to_string()),
                        )
                        .show_ui(ui, |ui| {
                            if option(ui, d.folder.trim().is_empty(), t("As its type says"))
                                .clicked()
                            {
                                d.folder.clear();
                            }
                            for (label, path) in &places {
                                if option(ui, d.folder.trim() == path, label).clicked() {
                                    d.folder = path.clone();
                                }
                            }
                        })
                        .response
                        .labelled_by(name.id);
                    ui.end_row();
                    let name = ui.label(t("Save to folder"));
                    ui.horizontal(|ui| {
                        let folder = ui
                            .add(
                                egui::TextEdit::singleline(&mut d.folder)
                                    .hint_text(&default_dir)
                                    .desired_width(280.0),
                            )
                            .labelled_by(name.id);
                        if entered(&folder) {
                            chosen = Some(false);
                        }
                        if let Some(chose) = browse_button(ui, &*platform, &mut d.folder) {
                            d.no_chooser = !chose;
                        }
                    });
                    ui.end_row();
                    if d.urls.len() == 1 {
                        let name = ui.label(t("Save as"));
                        let save_as = ui
                            .add(
                                egui::TextEdit::singleline(&mut d.name)
                                    .hint_text(if d.quality.is_some() || page {
                                        // A video picked from a player page is
                                        // named from its title: the page's own
                                        // name ("player2.html") was shown, and
                                        // the file came as something else (UI
                                        // round 8).
                                        t("The video's own title").to_string()
                                    } else if dm_core::is_torrent(&first) {
                                        t("The torrent's own name").to_string()
                                    } else {
                                        stream_name(&first, None, remux)
                                            .unwrap_or_else(|| format::provisional(&first))
                                    })
                                    .desired_width(360.0),
                            )
                            .labelled_by(name.id);
                        if save_as.changed() {
                            d.name_typed = true;
                        }
                        if entered(&save_as) {
                            chosen = Some(false);
                        }
                        ui.end_row();
                    }
                    // The size, once the server has said it (gap 160): its
                    // line is there from the start, so the buttons below do
                    // not move when the answer comes.
                    if d.urls.len() == 1 {
                        ui.label(t("Size"));
                        // A video page's size is learned once it starts:
                        // said here, in the row that is always there, so
                        // the dialog does not grow as the answer comes and
                        // move Start now from under the pointer (newcomer
                        // check, 2026-10-21).
                        match size {
                            Some(size) => ui.label(format::bytes(size)),
                            None if page => {
                                ui.label(RichText::new(t("Known once the video starts")).weak())
                            }
                            None => ui.label(RichText::new("—").weak()),
                        };
                        ui.end_row();
                    }
                    let name = ui.label(t("Note"));
                    let note = ui
                        .add(egui::TextEdit::singleline(&mut d.description).desired_width(360.0))
                        .labelled_by(name.id);
                    if entered(&note) {
                        chosen = Some(false);
                    }
                    ui.end_row();
                    if let Some(label) = &d.label {
                        ui.label(t("Quality"));
                        ui.label(label);
                        ui.end_row();
                    }
                });
            // Below the fields and wrapped to the dialog's width: beside
            // Browse… it widened the dialog past a 720 px window.
            if d.no_chooser {
                ui.add(
                    egui::Label::new(
                        RichText::new(t(
                            "No folder chooser on this desktop; type the folder's path.",
                        ))
                        .color(warn(ui)),
                    )
                    .wrap(),
                );
            }
            if let Some(said) = &renamed {
                ui.add(egui::Label::new(RichText::new(said).color(warn(ui))).wrap());
            }
            if let Some(why) = &problem {
                let plain = format::reason(why).map(t).unwrap_or(why.as_str());
                ui.add(
                    egui::Label::new(
                        RichText::new(tf("Not available: {}.", &[&plain])).color(warn(ui)),
                    )
                    .wrap(),
                )
                .on_hover_text(why);
            }
            if !d.says.is_empty() {
                ui.add(egui::Label::new(RichText::new(&d.says).color(warn(ui))).wrap());
            }
            footer(ui, |ui| {
                if focused(primary(ui, "Start now"), focus).clicked() || enter_free(ui) {
                    chosen = Some(false);
                }
                // Read as one phrase: "Add for later to Main queue ▾"; the
                // queue's name used to sit between the button and an "into"
                // after it (gap 168).
                if ui.button(t("Add for later")).clicked() {
                    chosen = Some(true);
                }
                // "to" by itself is many words in other languages: the key
                // says which one.
                let to = ui.label(t("queue:to").trim_start_matches("queue:"));
                combo("details-queue")
                    .selected_text(crate::i18n::name(&d.queue))
                    .show_ui(ui, |ui| {
                        for q in &queues {
                            pick(ui, &mut d.queue, q.clone(), crate::i18n::name(q));
                        }
                    })
                    .response
                    .labelled_by(to.id);
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    if cancel || !open {
        let site = app
            .dialogs
            .file_info
            .as_ref()
            .filter(|d| d.handed)
            .and_then(|d| d.urls.first())
            .map(|url| dm_core::category::host_of(url))
            .filter(|host| !host.is_empty());
        app.dialogs.file_info = None;
        if let Some(site) = site {
            app.hand_off_cancelled(&site);
        }
        return;
    }
    let Some(later) = chosen else {
        return;
    };
    let with = AddOptions {
        description: d.description.trim().to_string(),
        queue: if later {
            d.queue.clone()
        } else {
            String::new()
        },
        save_dir: Some(d.folder.trim().to_string()).filter(|f| !f.is_empty()),
        // The server's own name, untouched, is what dm names the file
        // anyway: left for it to place as it always does.
        save_name: Some(d.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .filter(|n| d.name_typed || server_name.as_deref() != Some(n.as_str())),
        later,
        referrer: d.referrer.clone(),
        quality: d.quality.clone(),
        cookies: d.cookies.clone(),
        cookies_once: d.cookies_once,
    };
    let urls = d.urls.clone();
    // The size the dialog showed stays on the row until the row learns its
    // own: a row added for later had none to show (gap 167).
    let sizes: HashMap<String, u64> = urls
        .first()
        .zip(size)
        .filter(|_| urls.len() == 1)
        .map(|(url, size)| (url.clone(), size))
        .into_iter()
        .collect();
    let handed = d.handed;
    match app.add_sized(&urls, with, &sizes) {
        Ok(_) => {
            app.dialogs.file_info = None;
            if handed {
                for url in &urls {
                    app.taken_from_browser(url);
                }
            }
            // Start now while the schedule or the download limit holds
            // downloads: asked, as Resume asks, whether to start it outside
            // the schedule; it only waited, without a word (gap 139).
            // Asked of the schedule itself: the list, not yet refreshed,
            // had no row waiting on it, so nothing asked (UI round 12).
            let shut = app.applied.schedule.enabled && !app.applied.schedule.open();
            let spent = app.backend.queue.allowance_spent().is_some();
            if !later && !app.headless && (shut || spent) {
                app.dialogs.confirm_outside = Some(app.last_added.clone());
            }
        }
        Err(e) => {
            if let Some(d) = app.dialogs.file_info.as_mut() {
                d.says = e;
            }
        }
    }
}

fn progress(app: &mut DmApp, ctx: &egui::Context) {
    let mut ids: Vec<Id> = app.dialogs.progress.iter().copied().collect();
    ids.sort_unstable();
    for (nth, id) in ids.into_iter().enumerate() {
        let Some(mut row) = app.row(id).cloned() else {
            app.dialogs.progress.remove(&id);
            continue;
        };
        // The size a dialog learned, until the row learns its own, as the
        // list shows it (UI round 12: "File size unknown" beside it).
        if row.progress.total.is_none() {
            row.progress.total = app.sizes_told.get(&id).copied();
        }
        let connections = app.backend.connections(id);
        // While its parts are joined the plan they are read from is gone:
        // the last answer stands, and nothing reads as unknown or unstarted
        // with every byte in (speed check 2, 2026-10-19).
        let joining = row.state == State::Running && crate::app::fetched_all(&row);
        let resumable = match app.backend.resumable(id) {
            Some(answer) => {
                app.dialogs.resumable_seen.insert(id, answer);
                Some(answer)
            }
            None if joining => app.dialogs.resumable_seen.get(&id).copied(),
            None => None,
        };
        let mut open = true;
        let (mut pause, mut resume, mut cancel) = (false, false, false);
        // No wider or taller than the screen leaves room for: a long
        // status (a server's refusal, with where to sign in) wraps, and a
        // window too tall scrolls rather than running off the bottom.
        let screen = ctx.content_rect();
        let room = (screen.width() - 16.0 - sides(ctx)).clamp(300.0, 560.0);
        // Below the menu bar and the toolbar, which it opened over and hid
        // (gap 168; UI rounds 10 and 11: kept on screen, a tall window was
        // pushed back up over Add URL).
        let mut below_menu = screen;
        below_menu.min.y = app
            .menu_bottom
            .max(app.bars_bottom)
            .max(screen.top())
            .min(screen.bottom() - 120.0);
        let step = 28.0 * (nth % 8) as f32;
        // Behind any dialog that keeps the clicks too: behind Options its
        // Stop download paused a download (UI round 12).
        let asking = app.dialogs.asking() || app.dialogs.modal();
        // The name it will get, as the list says it: two catches of one
        // link were both "big.zip" until they had begun (UI round 10).
        let mut categories = app.applied.categories();
        let named = app.name_it_will_get(&row, &mut categories);
        Dialog::new(tf(
            "Download progress — {}",
            &[&crate::bidi::visual(&named)],
        ))
        .id(egui::Id::unique(("progress", id)))
        .collapsible(false)
        .resizable(false)
        // Below the menu bar, which it opened over and hid (gap 168).
        .constrain_to(below_menu)
        // Each a step down and across from the one before, as windows
        // cascade: three started together opened exactly over each other.
        // Below the toolbar too: over it, the window covered Add URL and
        // took the click meant for it (UI round 10).
        .default_pos(below_menu.left_top() + egui::Vec2::new(24.0 + step, 8.0 + step))
        // One width: free to grow, it widened a few points as its numbers
        // did (UI round 13). A long status wraps inside it.
        .default_width(room.min(480.0))
        .min_width(room.min(480.0))
        .max_width(room.min(480.0))
        .open(&mut open)
        .show(ctx, |ui| {
            // Under a question, it takes no clicks: Stop download stopped
            // a download behind the Exit question (UI round 11).
            if asking {
                ui.disable();
            }
            // One height whatever it holds: grown and shrunk as
            // connections came and went, the window jumped about
            // the screen, kept on it by being moved (gap 140).
            // As tall as a tall window allows, so sixteen connections are
            // seen without scrolling in one (UI round 8: held at 400 points
            // in a 900-point window).
            // No taller than the room under the toolbar leaves, less the
            // title, the buttons and its step down, so a window does not
            // have to be pushed up over the toolbar to fit.
            let tall = (below_menu.height() - 130.0 - step)
                .min((screen.height() * 0.7).max(400.0))
                .max(120.0);
            // As tall as what it holds, up to that: held at the room on the
            // screen whatever it held, it filled 84% of the window's height
            // with three connections and a band of nothing (speed check,
            // 2026-10-19). Its height changes only when Connections is
            // opened or shut, which a person does.
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(tall)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(format::shown_url(&row.url)).small().weak())
                            .truncate(),
                    )
                    .on_hover_text(&row.url);
                    let p = &row.progress;
                    // Rows as tall as their text, not as a control: six
                    // lines of words took a third of its height (speed
                    // check, 2026-10-19).
                    ui.spacing_mut().interact_size.y = ui.text_style_height(&egui::TextStyle::Body);
                    egui::Grid::new(("progress-grid", id))
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .max_col_width(room - 110.0)
                        .show(ui, |ui| {
                            ui.label(t("Status"));
                            let status = ui.add(egui::Label::new(status_of(&row)).wrap());
                            if let Some(raw) = crate::app::failure_as_said(&row) {
                                status.on_hover_text(raw);
                            }
                            ui.end_row();
                            ui.label(t("File size"));
                            ui.label(
                                p.total
                                    .map(format::bytes)
                                    .unwrap_or_else(|| t("unknown").into()),
                            );
                            ui.end_row();
                            ui.label(t("Downloaded"));
                            // Digits of one width: these change every second.
                            // The percentage is Status's, above: said
                            // twice it read as two numbers (UI round 8).
                            let fetched = format::bytes(p.downloaded);
                            ui.label(crate::table::digits(ui, fetched));
                            ui.end_row();
                            ui.label(t("Speed"));
                            let rate = if row.state == State::Running && !joining {
                                format::rate(p.speed)
                            } else {
                                String::new()
                            };
                            ui.label(crate::table::digits(ui, rate));
                            ui.end_row();
                            // Only while it runs: a stopped download has
                            // no time left to tell (gap 157).
                            ui.label(t("Time left"));
                            // The span alone: under "Time left" it read
                            // "Time left: 9s left" (round 14).
                            let left = if row.state == State::Running {
                                p.eta.map(format::span).unwrap_or_default()
                            } else {
                                String::new()
                            };
                            ui.label(crate::table::digits(ui, left));
                            ui.end_row();
                            ui.label(t("Can resume"));
                            // What the server said when the download was planned, not
                            // a guess from the size: a known size says nothing about
                            // whether a server serves ranges.
                            ui.label(match resumable {
                                Some(true) => t("Yes"),
                                Some(false) => t("No — a stop starts it again from the beginning"),
                                // A torrent goes on from the pieces it has;
                                // there is no server to ask (UI round 12),
                                // whether or not it failed (torrent check).
                                // Not one that failed before it had any:
                                // a .torrent that could not be read has no
                                // pieces to go on from (torrent check, pass 2).
                                None if dm_core::is_torrent(&row.url)
                                    && matches!(row.state, State::Failed(_))
                                    && p.downloaded == 0 =>
                                {
                                    t("No — nothing of it was fetched")
                                }
                                None if dm_core::is_torrent(&row.url) => {
                                    t("Yes, from the pieces it already has")
                                }
                                // Failed before the server said: a 401
                                // is an answer, but not to this.
                                // The server answered, with a refusal: a
                                // 404 read "Not yet known" under its own
                                // reason (UI round 17).
                                None if matches!(&row.state, State::Failed(why) if why.contains("the server answered")) => {
                                    t("No — the server refused it")
                                }
                                None if matches!(row.state, State::Failed(_)) => {
                                    t("Not yet known: it failed before the server said")
                                }
                                None => t("Unknown until the server answers"),
                            });
                            ui.end_row();
                        });
                    let fraction = p.fraction().unwrap_or(0.0) as f32;
                    // No words of its own: Status above says the percentage
                    // (UI round 9: it was said twice).
                    bar(ui, fraction, 420.0, "");
                    // A torrent's pieces come from its peers, not from
                    // connections dm plans: the section said "Nothing fetched
                    // yet." while megabytes arrived (torrent check).
                    let torrent = dm_core::is_torrent(&row.url);
                    if !torrent {
                        // Shut until asked for: what most want is the bar,
                        // the bytes, the speed and the time left above.
                        egui::CollapsingHeader::new(t("Connections"))
                            .icon(header_chevron)
                            .default_open(false)
                            .id_salt(("connections", id))
                            .show(ui, |ui| {
                                if connections.is_empty() && joining {
                                    ui.label(
                                        RichText::new(t("Every piece is in; putting them together."))
                                            .weak(),
                                    );
                                } else if connections.is_empty() {
                                    ui.label(RichText::new(t("Nothing fetched yet.")).weak());
                                }
                                // The file is cut into more pieces than connections,
                                // and each connection takes the next piece when it is
                                // done: the pieces under way are the connections at
                                // work, and the rest is a count.
                                let span_of =
                                    |c: &dm_core::Connection| (c.end - c.start).saturating_add(1);
                                // Each held to its range and adding up to the
                                // Downloaded above (gap 166).
                                let held = format::pieces_shown_open(
                                    &connections
                                        .iter()
                                        .map(|c| (span_of(c), c.done))
                                        .collect::<Vec<_>>(),
                                    &connections.iter().map(|c| c.started).collect::<Vec<_>>(),
                                    p.downloaded,
                                );
                                let finished = connections
                                    .iter()
                                    .zip(&held)
                                    .filter(|(c, &done)| done >= span_of(c))
                                    .count();
                                if !connections.is_empty() {
                                    ui.label(
                                        RichText::new(crate::i18n::tn(
                                            "{} of {} piece done||{} of {} pieces done",
                                            connections.len() as u64,
                                            &[&finished, &connections.len()],
                                        ))
                                        .weak(),
                                    );
                                }
                                // Every piece of the plan: those under way with
                                // their bars, the rest said to be waiting or
                                // done, so the list does not grow and shrink as
                                // connections start (gap 160).
                                // The longest words any piece's bar could have,
                                // so every bar is one length.
                                let widest = connections
                                    .iter()
                                    .map(|c| {
                                        let most = format::bytes(span_of(c));
                                        tf("{} of {}", &[&most, &most])
                                    })
                                    .max_by_key(|w| w.chars().count())
                                    .unwrap_or_default();
                                for (n, (c, &done)) in connections.iter().zip(&held).enumerate() {
                                    let span = span_of(c);
                                    ui.horizontal(|ui| {
                                        // Digits of one width, so the bars start
                                        // in one line (UI round 8: 1's was 2 px
                                        // left of the others).
                                        ui.label(crate::table::digits(ui, format!("{:>3}", n + 1)));
                                        if done >= span {
                                            ui.label(
                                                RichText::new(tf(
                                                    "done, {}",
                                                    &[&format::bytes(span)],
                                                ))
                                                .weak(),
                                            );
                                        // Waiting only where no request for it is
                                        // open yet (UI round 8).
                                        } else if done == 0
                                            && !c.started
                                            && row.state == State::Running
                                        {
                                            ui.label(
                                                RichText::new(tf(
                                                    "waiting, {}",
                                                    &[&format::bytes(span)],
                                                ))
                                                .weak(),
                                            );
                                        } else if row.progress.total.is_none() || span >= 1 << 60 {
                                            // Of a length nobody said: what has
                                            // come, not a bar against the range's
                                            // open end, which read "of 16777216.00
                                            // TB" (speed check, 2026-10-19).
                                            ui.label(crate::table::digits(
                                                ui,
                                                tf("{} so far", &[&format::bytes(done)]),
                                            ));
                                        } else {
                                            bar_beside(
                                                ui,
                                                done as f32 / span.max(1) as f32,
                                                260.0,
                                                &tf(
                                                    "{} of {}",
                                                    &[&format::bytes(done), &format::bytes(span)],
                                                ),
                                                &widest,
                                            );
                                        }
                                    });
                                }
                            });
                    }
                });
            // The buttons below what scrolls, always in view; a plain row,
            // which keeps the progress window compact.
            ui.horizontal(|ui| {
                if matches!(row.state, State::Running | State::Queued) {
                    pause = ui.button(t("Stop download")).clicked();
                }
                // A failed download is tried again, and the button says so.
                if matches!(row.state, State::Failed(_)) {
                    resume = ui.button(t("Retry")).clicked();
                }
                // Held by the schedule, it is not resumed but started
                // outside it, as the row's own button says (UI round 10).
                if row.state == State::Paused {
                    let word = if row.paused_by_schedule {
                        "Start now"
                    } else {
                        "Resume"
                    };
                    resume = ui.button(t(word)).clicked();
                }
                if !row.state.settled() {
                    cancel = ui.button(t("Cancel download")).clicked();
                }
            });
        });
        if pause {
            app.backend.pause(id);
        }
        if resume {
            match row.state {
                State::Failed(_) => app.backend.retry(id),
                // Held by the schedule: Resume would only put it back to be
                // held again. Asked whether to start it outside the schedule.
                State::Paused if row.paused_by_schedule => {
                    app.dialogs.confirm_outside = Some(vec![id]);
                }
                _ => app.backend.resume(id),
            }
        }
        if cancel {
            app.dialogs.confirm_cancel = Some(id);
        }
        // Its × too takes no click while it is behind.
        if !open && !asking {
            app.dialogs.progress.remove(&id);
            app.dialogs.progress_closed.insert(id);
        }
        if pause || resume || cancel {
            app.refresh();
        }
    }
}

/// One "Download complete" window for everything that finished, each file
/// with its own Open and Open folder: a window per file stacked them, five
/// for a five-file series, and a click on one half-hidden behind another
/// opened the wrong file.
fn complete(app: &mut DmApp, ctx: &egui::Context) {
    let mut rows: Vec<dm_core::Entry> = app
        .dialogs
        .complete
        .iter()
        .filter_map(|id| app.row(*id).cloned())
        .collect();
    // The newest first, where the eye lands: the file just finished sat
    // below the fold, under the older ones (round 17).
    rows.sort_by_key(|r| std::cmp::Reverse(r.finished_at));
    app.dialogs
        .complete
        .retain(|id| rows.iter().any(|r| r.id == *id));
    if rows.is_empty() {
        return;
    }
    // Not over a question waiting for its answer: it opened over Exit, its
    // Open folder under the pointer, and took the click (UI round 10). The
    // files wait in the list and the window opens once it is answered.
    // Nor over a dialog that keeps the clicks: behind Scheduler its Close
    // closed it (UI round 12), and over it, it hid the Scheduler's days
    // (UI round 13). It opens once the dialog is done.
    if app.dialogs.asking() || app.dialogs.modal() {
        return;
    }
    let mut done: Vec<Id> = Vec::new();
    let mut close = false;
    let mut never_again = false;
    let mut with: Vec<String> = Vec::new();
    let mut shut = true;
    // In the corner above the status bar, out of the way of the rows a
    // person is clicking while a batch finishes, rather than over them in
    // the middle; resizable, and no wider than the window.
    let screen = ctx.content_rect();
    let width = (screen.width() - 32.0).clamp(280.0, 440.0);
    Dialog::new(t("Download complete"))
        .id(window_id("Download complete"))
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .default_pos(screen.right_bottom() - egui::vec2(16.0, 40.0))
        .default_width(width)
        .max_width(width)
        .min_height(160.0)
        .default_height(360.0_f32.min(screen.height() - 120.0))
        // Never most of the window: at 150% text it filled it and ran off
        // its bottom (round 16); the list inside scrolls instead.
        .max_height((screen.height() * 0.6).max(160.0))
        .open(&mut shut)
        .show(ctx, |ui| {
            if rows.len() > 1 {
                ui.label(tf("{} downloads finished.", &[&rows.len()]));
                ui.separator();
            }
            // Every file reachable: the list scrolls in what height the window
            // has, its buttons below it always in view.
            // As tall as the window may be, less its words and buttons: from
            // what was left of a window growing into its size, thirteen
            // files showed one at a time (UI round 6).
            let list = (screen.height() * 0.6 - 140.0).max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("complete-list")
                .auto_shrink([false, true])
                .max_height(list)
                .min_scrolled_height(list)
                .show(ui, |ui| {
                    // Several in two lines each, the name with its size and
                    // the buttons with the folder: in four, thirteen files
                    // showed under three at a time (UI round 6).
                    let several = rows.len() > 1;
                    for row in &rows {
                        let name = name_of(row);
                        let size = crate::table::digits(
                            ui,
                            format::bytes(row.progress.total.unwrap_or(row.progress.downloaded)),
                        );
                        let folder = |ui: &mut egui::Ui| {
                            let drawn = ui.add(
                                egui::Label::new(
                                    RichText::new(&*crate::bidi::visual(&row.path))
                                        .small()
                                        .weak(),
                                )
                                .truncate(),
                            );
                            crate::bidi::said_as(&drawn, &row.path);
                            drawn.on_hover_text(&row.path);
                        };
                        // A long name is cut short, all of it on hover, so the
                        // buttons stay on screen.
                        ui.horizontal(|ui| {
                            if several {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(size.clone().weak());
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                let drawn = ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&*crate::bidi::visual(&name))
                                                            .strong(),
                                                    )
                                                    .truncate(),
                                                );
                                                crate::bidi::said_as(&drawn, &name);
                                                drawn.on_hover_text(&name);
                                            },
                                        );
                                    },
                                );
                            } else {
                                let drawn = ui.add(
                                    egui::Label::new(
                                        RichText::new(&*crate::bidi::visual(&name)).strong(),
                                    )
                                    .truncate(),
                                );
                                crate::bidi::said_as(&drawn, &name);
                                drawn.on_hover_text(&name);
                            }
                        });
                        if !several {
                            ui.label(size);
                            folder(ui);
                        }
                        ui.horizontal(|ui| {
                            // Named for its file where there are several, so a
                            // screen reader says which Open this is.
                            let named = |ui: &mut egui::Ui, word: &str, said: String| {
                                let button = ui.button(word);
                                if rows.len() > 1 {
                                    button.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::Role::Button, true, &said)
                                    });
                                }
                                button.clicked()
                            };
                            if named(ui, t("Open"), tf("Open {}", &[&name])) {
                                app.open_file(&row.path);
                                done.push(row.id);
                            }
                            if named(ui, t("Open with…"), tf("Open {} with…", &[&name])) {
                                with.push(row.path.clone());
                                done.push(row.id);
                            }
                            if named(ui, t("Open folder"), tf("Open the folder of {}", &[&name])) {
                                app.reveal_file(&row.path);
                                done.push(row.id);
                            }
                            if several {
                                folder(ui);
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
            footer(ui, |ui| {
                close = ui.button(t("Close")).clicked();
                // As the window's own choice, not only Options': a
                // "Don't show this dialog again" (QA pass, 2026-10-22).
                ui.checkbox(&mut never_again, t("Don't show this window again"));
            });
        });
    if never_again {
        let mut wanted = app.applied.clone();
        wanted.show_complete = false;
        if let Err(why) = app.apply_settings(wanted) {
            app.said = why.into();
        } else {
            app.said = crate::i18n::Said::new(
                "Download complete will not open again; turn it back on in Options › General.",
                &[],
            );
        }
        app.dialogs.complete.clear();
    }
    if close || !shut {
        app.dialogs.complete.clear();
    }
    for id in done {
        app.dialogs.complete.remove(&id);
    }
    for path in with {
        app.open_with_chosen(&path);
    }
}

fn properties(app: &mut DmApp, ctx: &egui::Context) {
    let ids: Vec<Id> = app.dialogs.properties.keys().copied().collect();
    let mut nth = 0.0_f32;
    for id in ids {
        let Some(row) = app.row(id).cloned() else {
            app.dialogs.properties.remove(&id);
            continue;
        };
        // What a row not yet placed will have, as the list shows it: its
        // size, kind and folder were dashes here (UI round 11).
        // And the size a dialog learned before it started, as the list
        // shows (UI round 12: a dash beside the list's 300.0 KB).
        let size = crate::table::shown_size(&row)
            .or_else(|| app.sizes_told.get(&id).copied())
            .map(format::bytes)
            .unwrap_or_default();
        // As the tree places it: a person's own category, the kind, or
        // Other for a kind not yet known (UI round 13: "—" for a row the
        // tree had under Other).
        let kind = match (app.custom_of(&row), crate::app::kind_of(&row)) {
            (Some(own), _) => own,
            (None, kind) => crate::i18n::name(&format::titled(&category_word(
                kind.unwrap_or(dm_core::Category::Other),
            ))),
        };
        // And the name the list shows: a waiting copy read "slow.bin"
        // beside the list's "slow (2).bin".
        let shown_name = app.name_it_will_get(&row, &mut app.applied.categories());
        let saved_to = if row.path.is_empty() {
            let mut categories = app.applied.categories();
            let folder = row
                .save_dir
                .clone()
                .filter(|d| !d.trim().is_empty())
                .or_else(|| app.planned_folder(&row, &mut categories));
            folder
                .map(|f| {
                    std::path::Path::new(&f)
                        .join(app.name_it_will_get(&row, &mut categories))
                        .display()
                        .to_string()
                })
                .unwrap_or_default()
        } else {
            row.path.clone()
        };
        let (mut ok, mut cancel, mut open_file) = (false, false, false);
        let focus = app.dialogs.take_focus("File properties");
        let Some(d) = app.dialogs.properties.get_mut(&id) else {
            continue;
        };
        // A server that wants a sign-in: the keyboard goes to Login.
        let wants_login =
            matches!(&row.state, State::Failed(why) if crate::app::needs_sign_in(why));
        // No wider than the window leaves room for, however long a name,
        // path or referrer: they wrap, rather than pushing the buttons off
        // the screen.
        let room = (ctx.content_rect().width() - 16.0 - sides(ctx)).clamp(320.0, 620.0);
        // A close box in its title bar, as every other window has.
        let mut shut = true;
        // Below the menu bar, as the progress window keeps (UI round 5).
        let below = {
            let mut below = ctx.content_rect();
            below.min.y = app.menu_bottom.max(below.top()).min(below.bottom() - 120.0);
            below
        };
        // Taller than that room, its body scrolls: in a 720×460 window its
        // buttons were under the window's foot.
        let window = egui::Id::unique(("properties", id));
        let step = nth;
        nth += 1.0;
        let tall = (below.height() - 16.0).max(160.0);
        let cramped = ctx
            .memory(|m| m.area_rect(window))
            .is_some_and(|was| was.height() >= tall - 8.0);
        Dialog::new(t("File properties"))
            .id(window)
            .collapsible(false)
            .resizable(false)
            .open(&mut shut)
            .max_width(room)
            .max_height(tall)
            .vscroll(cramped)
            .constrain_to(below)
            // In the middle, as every other dialog opens, each further one
            // a step down and across; it opened in the top left corner.
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(below.center() + egui::vec2(24.0, 24.0) * step)
            .show(ctx, |ui| {
                ui.set_max_width(room);
                // The values' column: what is left beside the headings.
                let value = (room - 130.0).max(160.0);
                // Nothing to say is a plain dash: an empty wrapped label
                // stood a whole line tall and left gaps down the dialog.
                let said = |ui: &mut egui::Ui, text: &str| {
                    if text.is_empty() {
                        ui.label(RichText::new("—").weak());
                        return;
                    }
                    ui.scope(|ui| {
                        ui.set_max_width(value);
                        let drawn = ui.add(
                            egui::Label::new(&*crate::bidi::visual(text))
                                .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                        crate::bidi::said_as(&drawn, text);
                    });
                };
                // The fields scroll inside a window too short for them, so
                // the buttons below stay on the screen.
                let tall = (ctx.content_rect().height() - 150.0).max(120.0);
                egui::ScrollArea::vertical()
                    .id_salt(("props-scroll", id))
                    .max_height(tall)
                    .show(ui, |ui| {
                        egui::Grid::new(("props", id))
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .max_col_width(value)
                            .show(ui, |ui| {
                                ui.label(t("File name"));
                                said(ui, &shown_name);
                                ui.end_row();
                                ui.label(t("Status"));
                                // Held to the values' column like the rest:
                                // a long engine error, unwrapped, stretched
                                // every row of the grid (gap 153).
                                let status = status_of(&row);
                                match crate::app::failure_as_said(&row) {
                                    Some(raw) => {
                                        ui.scope(|ui| said(ui, &status))
                                            .response
                                            .on_hover_text(raw);
                                    }
                                    None => said(ui, &status),
                                }
                                ui.end_row();
                                ui.label(t("Size"));
                                said(ui, &size);
                                ui.end_row();
                                ui.label(t("Category"));
                                // In the window's language, as the category tree
                                // names it.
                                said(ui, &kind);
                                ui.end_row();
                                // Not yet, for one not yet placed (UI round 15).
                                ui.label(if row.path.is_empty() {
                                    t("Will be saved to")
                                } else {
                                    t("Saved to")
                                });
                                said(ui, &saved_to);
                                ui.end_row();
                                ui.label(t("Referrer"));
                                said(ui, &row.referrer);
                                ui.end_row();
                                let name = ui.label(t("Address"));
                                // Also while it waits to try again: a wrong
                                // address is what it most needs then (UI
                                // round 8).
                                let editable =
                                    !matches!(row.state, State::Running | State::Completed)
                                        || crate::app::retrying(row.id).is_some();
                                let address = ui
                                    .add_enabled(
                                        editable,
                                        egui::TextEdit::singleline(&mut d.address)
                                            .desired_width(value.min(360.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&focused(address, focus && editable && !wants_login));
                                ui.end_row();
                                let name = ui.label(t("Login"));
                                let login = ui
                                    .add_enabled(
                                        editable,
                                        egui::TextEdit::singleline(&mut d.login)
                                            .desired_width(value.min(200.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&focused(login, focus && editable && wants_login));
                                ui.end_row();
                                let name = ui.label(t("Password"));
                                let password = ui
                                    .add_enabled(
                                        editable,
                                        egui::TextEdit::singleline(&mut d.password)
                                            .password(true)
                                            .desired_width(value.min(200.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&password);
                                ui.end_row();
                                let name = ui.label(t("Note"));
                                let note = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut d.description)
                                            .desired_width(value.min(360.0)),
                                    )
                                    .labelled_by(name.id);
                                ok |= entered(&focused(note, focus && !editable));
                                ui.end_row();
                            });
                    });
                if !d.says.is_empty() {
                    ui.colored_label(warn(ui), &d.says);
                }
                footer(ui, |ui| {
                    // Properties runs a finished file from here.
                    let can_open = row.state == State::Completed && !crate::table::gone(&row);
                    // The default first, as every dialog has it.
                    ok |= primary(ui, "OK").clicked() || enter_free(ui);
                    open_file = ui
                        .add_enabled(can_open, egui::Button::new(t("Open")))
                        .clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        let cancel = cancel || !shut;
        if open_file {
            if let Err(e) = app.backend.platform.open(&row.path) {
                d.says = e;
            }
        }
        let (description, address) = (
            d.description.clone(),
            join_login(&d.address, &d.login, &d.password),
        );
        if cancel {
            app.dialogs.properties.remove(&id);
        }
        if ok {
            if description != row.description {
                app.backend.set_description(id, &description);
            }
            let changed = address.trim() != row.url;
            let result = if changed {
                app.backend.set_url(id, &address)
            } else {
                Ok(())
            };
            match result {
                Ok(()) => {
                    app.dialogs.properties.remove(&id);
                    // A sign-in given to a download that failed for want
                    // of one is what it was waiting for: tried again with it.
                    if changed && wants_login {
                        app.backend.retry(id);
                        app.said = crate::i18n::Said::new(
                            "Trying {} again with the sign-in.",
                            &[&name_of(&row)],
                        );
                        app.said_about = Some((id, app.said.clone(), false));
                    }
                }
                Err(e) => {
                    if let Some(d) = app.dialogs.properties.get_mut(&id) {
                        d.says = e;
                    }
                }
            }
            app.refresh();
        }
    }
}

fn series(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Add numbered series");
    let Some(d) = app.dialogs.series.as_mut() else {
        return;
    };
    let mut open = true;
    let mut add: Option<bool> = None;
    let before = (d.template.clone(), d.from.clone(), d.to.clone(), d.width);
    let mut cancel = false;
    window(ctx, "Add numbered series")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(t(
                "Put * where the number or letter changes, as in example.com/photo*.jpg",
            ));
            let mut enter = entered(&focused(
                field(ui, t("Address with *"), &mut d.template, 420.0),
                focus,
            ));
            ui.horizontal(|ui| {
                enter |= entered(&field(ui, t("From"), &mut d.from, 60.0));
                enter |= entered(&field(ui, t("To"), &mut d.to, 60.0));
                let name = ui.label(t("Wildcard size"));
                ui.add(number_box(&mut d.width).range(0..=9))
                    .labelled_by(name.id);
            });
            // One line, its room kept whether or not it says anything, so
            // the dialog does not jump as it starts to; and nothing said
            // about the range until both its ends are typed (gap 165).
            let typed = !d.template.trim().is_empty()
                && !d.from.trim().is_empty()
                && !d.to.trim().is_empty();
            // What a press of Start said stands only until the address or
            // the range is changed: kept, it hid the count and the live
            // error of what was typed since.
            if (&d.template, &d.from, &d.to, d.width) != (&before.0, &before.1, &before.2, before.3)
            {
                d.says.clear();
            }
            let mut warn = d.says.clone();
            let mut count = String::new();
            let mut listed = Vec::new();
            match dm_shell::lists::series(&d.template, &d.from, &d.to, d.width) {
                Ok(urls) => {
                    listed = urls.clone();
                    if let Some(first) = urls.first() {
                        count = crate::i18n::tn(
                            "{} address: {}||{} addresses, the first {}",
                            urls.len() as u64,
                            &[&urls.len(), first],
                        );
                    }
                }
                Err(e) if typed && warn.is_empty() => warn = series_error(e).to_string(),
                Err(_) => {}
            }
            if warn.is_empty() {
                kept_line(ui, RichText::new(count));
            } else {
                warning_line(ui, &warn);
            }
            // Every address the series makes, so a wrong padding or end is
            // seen before anything is fetched; its room is kept whether or
            // not there is a list yet, and only the rows in view are drawn.
            let row = ui.text_style_height(&egui::TextStyle::Small) + 2.0;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if listed.is_empty() {
                    ui.label(
                        RichText::new(t("Each address the series makes is listed here."))
                            .small()
                            .weak(),
                    );
                }
                egui::ScrollArea::vertical()
                    .id_salt("series-addresses")
                    .min_scrolled_height(row * 6.0)
                    .max_height(row * 6.0)
                    .auto_shrink([false, false])
                    .show_rows(ui, row, listed.len(), |ui, rows| {
                        for url in listed.get(rows).unwrap_or_default() {
                            ui.label(RichText::new(url).small().monospace());
                        }
                    });
            });
            footer(ui, |ui| {
                if primary(ui, "Start now").clicked() || enter || enter_free(ui) {
                    add = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    add = Some(true);
                }
                if ui.button(t("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    if !open || cancel {
        app.dialogs.series = None;
        return;
    }
    let Some(later) = add else {
        return;
    };
    match dm_shell::lists::series(&d.template, &d.from, &d.to, d.width) {
        Ok(urls) => {
            let with = AddOptions {
                later,
                queue: if later {
                    MAIN_QUEUE.into()
                } else {
                    String::new()
                },
                ..AddOptions::default()
            };
            match app.add(&urls, with) {
                Ok(n) => {
                    app.said = crate::i18n::Said::counting(
                        "Added {} download.||Added {} downloads.",
                        n as u64,
                        &[&n],
                    );
                    app.dialogs.series = None;
                }
                Err(e) => d_says(&mut app.dialogs.series, e),
            }
        }
        Err(e) => d.says = series_error(e).to_string(),
    }
}

/// Why a series cannot be made, in the language in force.
fn series_error(e: dm_shell::lists::SeriesError) -> &'static str {
    use dm_shell::lists::SeriesError;
    match e {
        SeriesError::Star => t("The address needs exactly one * to fill in."),
        SeriesError::Backwards => t("The range runs backwards."),
        SeriesError::TooMany => t("Ten thousand addresses is more than a batch."),
        SeriesError::Ends => t("From and To must both be numbers, or both letters of one case."),
    }
}

fn d_says(series: &mut Option<Series>, e: String) {
    if let Some(d) = series.as_mut() {
        d.says = e;
    }
}

/// A list of links to tick before adding: the clipboard's, or a site's.
/// The addresses already in dm's list, bare (without a login), for a pick
/// list to offer unticked and say so.
pub(crate) fn listed_addresses(app: &DmApp) -> std::collections::HashSet<String> {
    app.rows()
        .iter()
        .filter(|r| r.state != State::Cancelled)
        .map(|r| bare(&r.url))
        .collect()
}

fn pick_list(
    ui: &mut egui::Ui,
    links: &mut [(String, bool)],
    listed: &std::collections::HashSet<String>,
) {
    ui.horizontal(|ui| {
        if ui.button(t("Check all")).clicked() {
            links.iter_mut().for_each(|l| l.1 = true);
        }
        if ui.button(t("Uncheck all")).clicked() {
            links.iter_mut().for_each(|l| l.1 = false);
        }
    });
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .show(ui, |ui| {
            for (link, on) in links.iter_mut() {
                let shown = format::shown_url(link);
                if listed.contains(&bare(link)) {
                    check(ui, on, tf("{} (already in the list)", &[&shown]));
                } else {
                    check(ui, on, shown);
                }
            }
        });
}

/// The "Download all links" dialog.
fn links(app: &mut DmApp, ctx: &egui::Context) {
    let Some(d) = app.dialogs.links.as_mut() else {
        return;
    };
    let mut open = true;
    // Some(later): Start now, or Add for later.
    let mut chosen: Option<bool> = None;
    let mut cancel = false;
    // Titled for either list the browser hands over: a page's every link,
    // or the links in what was selected on it.
    window(ctx, "Download links")
        .open(&mut open)
        .show(ctx, |ui| {
            // The links listed, and the pages left out of the list said
            // so: "4 links" over three lines read as one lost (UI round 8).
            let hidden = if d.hide_html {
                d.items.iter().filter(|(u, _)| d.is_html(u)).count()
            } else {
                0
            };
            ui.label(tf(
                "{} links on {}",
                &[&(d.items.len() - hidden), &format::shown_url(&d.page)],
            ));
            if hidden > 0 {
                ui.label(
                    RichText::new(crate::i18n::tn(
                        "{} web page hidden||{} web pages hidden",
                        hidden as u64,
                        &[&hidden],
                    ))
                    .weak(),
                );
            }
            ui.horizontal(|ui| {
                check(ui, &mut d.hide_html, t("Hide HTML files"));
                if ui.button(t("Check all")).clicked() {
                    d.items.iter_mut().for_each(|l| l.1 = true);
                }
                if ui.button(t("Uncheck all")).clicked() {
                    d.items.iter_mut().for_each(|l| l.1 = false);
                }
            });
            let found = dm_core::lock(&d.found).clone();
            for (url, on) in d.items.iter_mut() {
                if found.get(url.as_str()).is_some_and(|f| f.failed.is_some())
                    && d.refused.insert(url.clone())
                {
                    *on = false;
                }
            }
            let hide = d.hide_html;
            let html: Vec<bool> = d.items.iter().map(|(u, _)| d.is_html(u)).collect();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    egui::Grid::new("links")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong(t("File name"));
                            ui.strong(t("Type"));
                            ui.strong(t("Size"));
                            ui.end_row();
                            for ((url, on), is_html) in d.items.iter_mut().zip(html) {
                                if hide && is_html {
                                    continue;
                                }
                                let what = found.get(url.as_str());
                                let name = what
                                    .map(|f| f.name.clone())
                                    .filter(|n| !n.is_empty())
                                    .unwrap_or_else(|| format::provisional(url));
                                let name = if d.listed.contains(&bare(url)) {
                                    tf("{} (already in the list)", &[&name])
                                } else {
                                    name
                                };
                                // The name is the checkbox's, so a screen reader says it.
                                check(ui, on, &name).on_hover_text(format::shown_url(url));
                                match what {
                                    Some(Found {
                                        failed: Some(why), ..
                                    }) => {
                                        let plain =
                                            format::reason(why).map(t).unwrap_or(why.as_str());
                                        ui.colored_label(warn(ui), t("Error"))
                                            .on_hover_text(format::titled(plain));
                                    }
                                    // The kind as the tree names it, in the
                                    // language in force.
                                    Some(f) => {
                                        ui.label(crate::i18n::name(&format::titled(&f.kind)));
                                    }
                                    None => {
                                        ui.label("…");
                                    }
                                }
                                let size = what
                                    .and_then(|f| f.size)
                                    .map(format::bytes)
                                    .unwrap_or_default();
                                ui.label(crate::table::digits(ui, size));
                                ui.end_row();
                            }
                        });
                });
            footer(ui, |ui| {
                if primary(ui, "Start now").clicked() || enter_free(ui) {
                    chosen = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    chosen = Some(true);
                }
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    if let Some(later) = chosen {
        let chosen = d.chosen();
        let with = AddOptions {
            later,
            queue: if later {
                MAIN_QUEUE.into()
            } else {
                String::new()
            },
            referrer: d.page.clone(),
            ..AddOptions::default()
        };
        // The sizes the servers already told, kept on the rows until each
        // learns its own.
        let sizes: std::collections::HashMap<String, u64> = dm_core::lock(&d.found)
            .iter()
            .filter_map(|(url, found)| found.size.map(|size| (url.clone(), size)))
            .collect();
        // The page's sign-in goes with the links on its own server, and with
        // no others.
        let (signed, plain): (Vec<String>, Vec<String>) = chosen.into_iter().partition(|link| {
            d.sign_in
                .as_ref()
                .is_some_and(|s| s.for_url(link).is_some())
        });
        let signed_with = AddOptions {
            cookies: d.cookies.clone(),
            ..with.clone()
        };
        app.dialogs.links = None;
        let added = match (
            app.add_sized(&signed, signed_with, &sizes),
            app.add_sized(&plain, with, &sizes),
        ) {
            (Ok(a), Ok(b)) => Ok(a + b),
            (Err(e), _) | (_, Err(e)) => Err(e),
        };
        match added {
            Ok(n) if later => {
                app.said = crate::i18n::Said::counting_naming(
                    "Added {} download to {}.||Added {} downloads to {}.",
                    n as u64,
                    &[&n, &MAIN_QUEUE],
                )
            }
            Ok(n) => {
                app.said = crate::i18n::Said::counting(
                    "Started {} download.||Started {} downloads.",
                    n as u64,
                    &[&n],
                )
            }
            Err(e) => app.said = e.into(),
        }
        return;
    }
    if cancel || !open {
        app.dialogs.links = None;
    }
}

fn clip_links(app: &mut DmApp, ctx: &egui::Context) {
    let listed = listed_addresses(app);
    let Some(links) = app.dialogs.clip_links.as_mut() else {
        return;
    };
    let mut open = true;
    // Some(later): Start now, or Add for later.
    let mut add: Option<bool> = None;
    let mut cancel = false;
    window(ctx, "Links on the clipboard")
        .open(&mut open)
        .show(ctx, |ui| {
            // How many were found, and how many are ticked (gap 168).
            let ticked = links.iter().filter(|l| l.1).count();
            ui.label(crate::i18n::tn(
                "Found {} link; {} ticked.||Found {} links; {} ticked.",
                links.len() as u64,
                &[&links.len(), &ticked],
            ));
            pick_list(ui, links, &listed);
            footer(ui, |ui| {
                if primary(ui, "Start now").clicked() || enter_free(ui) {
                    add = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    add = Some(true);
                }
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    if cancel {
        open = false;
    }
    if let Some(later) = add {
        let chosen: Vec<String> = links.iter().filter(|l| l.1).map(|l| l.0.clone()).collect();
        let with = AddOptions {
            later,
            queue: if later {
                MAIN_QUEUE.into()
            } else {
                String::new()
            },
            ..AddOptions::default()
        };
        match app.add(&chosen, with) {
            Ok(n) if later => {
                app.said = crate::i18n::Said::counting_naming(
                    "Added {} download to {}.||Added {} downloads to {}.",
                    n as u64,
                    &[&n, &MAIN_QUEUE],
                )
            }
            Ok(n) => {
                app.said = crate::i18n::Said::counting(
                    "Started {} download.||Started {} downloads.",
                    n as u64,
                    &[&n],
                )
            }
            Err(e) => app.said = e.into(),
        }
        open = false;
    }
    if !open {
        app.dialogs.clip_links = None;
    }
}

fn site_files(app: &mut DmApp, ctx: &egui::Context) {
    // Built before the dialog is borrowed: the client reads the settings.
    let client = app.client();
    let listed = listed_addresses(app);
    let focus = app.dialogs.take_focus("Site files");
    let Some(d) = app.dialogs.site_files.as_mut() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let mut add: Option<Vec<String>> = None;
    let mut look = false;
    let mut save = false;
    let mut run_saved: Option<dm_shell::lists::Search> = None;
    let mut delete_saved: Option<String> = None;
    let saved = app.applied.site_searches.clone();
    window(ctx, "Site files")
        .open(&mut open)
        .show(ctx, |ui| {
            look |= entered(&focused(
                field(ui, t("Start page"), &mut d.page, 420.0),
                focus,
            ));
            field(ui, t("File types"), &mut d.types, 420.0);
            ui.horizontal(|ui| {
                let name = ui.label(t("Pages deep (0 for the start page only)"));
                ui.add(number_box(&mut d.depth).range(0..=dm_shell::lists::MAX_DEPTH))
                    .labelled_by(name.id);
            });
            ui.horizontal(|ui| {
                let name = ui.label(t("Stop reading pages after, minutes (0 for no limit)"));
                ui.add(
                    number_box(&mut d.stop_after_minutes).range(0..=dm_shell::lists::MOST_MINUTES),
                )
                .labelled_by(name.id);
            });
            field(ui, t("Only paths containing"), &mut d.include, 420.0);
            field(ui, t("Skip paths containing"), &mut d.exclude, 420.0);
            // The dialog's action straight after what it acts on: below the
            // saving lines it was out of sight at 960×660 (UI round 12).
            look |= ui.button(t("Find files")).clicked() || enter_free(ui);
            ui.separator();
            // Saving and the saved list come before the results, which can
            // run long enough to push anything below them off the screen.
            // The name on a line of its own, then the timer and the button:
            // wrapped as one line, its words broke as "Run every," / "hours"
            // (UI round 12). The words are not wrapped, so the line breaks
            // between its parts if anywhere; in the smallest window Save
            // search goes below rather than off the edge.
            ui.horizontal_wrapped(|ui| {
                field(ui, t("Save this search as"), &mut d.name, 160.0);
            });
            ui.horizontal_wrapped(|ui| {
                let name = ui.add(egui::Label::new(t("Run every, hours (0 only by hand)")).extend());
                ui.add(number_box(&mut d.every_hours).range(0..=24 * 31))
                    .labelled_by(name.id);
                save = ui.button(t("Save search")).clicked();
            });
            if !d.says.is_empty() {
                ui.colored_label(warn(ui), &d.says);
            }
            if !saved.is_empty() {
                ui.label(t("Saved searches"));
                for search in &saved {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} — {}", search.name, search.page));
                        if ui.button(t("Run")).clicked() {
                            run_saved = Some(search.clone());
                        }
                        if ui.button(t("Delete search")).clicked() {
                            delete_saved = Some(search.name.clone());
                        }
                    });
                }
            }
            ui.separator();
            let mut state = dm_core::lock(&d.state);
            match &mut *state {
                Grab::Idle => {}
                Grab::Looking => {
                    ui.label(t("Reading the page…"));
                }
                Grab::Failed(e) => {
                    ui.colored_label(warn(ui), e.as_str());
                }
                Grab::Found(links, _, _) if links.is_empty() => {
                    ui.label(t("No files of those types are linked from that page."));
                }
                Grab::Found(links, pages, stopped) => {
                    // Counted with a colon, which needs no plural: "on 1
                    // pages" (UI round 12).
                    ui.label(tf(
                        "Files found: {} · pages read: {}",
                        &[&links.len(), pages],
                    ));
                    if *stopped {
                        ui.label(
                            RichText::new(t(
                                "Its time ran out with pages still to read; these are the files found until then.",
                            ))
                            .weak(),
                        );
                    }
                    pick_list(ui, links, &listed);
                    if ui.button(t("Add for later")).clicked() {
                        add = Some(links.iter().filter(|l| l.1).map(|l| l.0.clone()).collect());
                    }
                }
            }
            // A Close, as every other dialog has (UI round 12).
            footer(ui, |ui| {
                if ui.button(t("Close")).clicked() {
                    close = true;
                }
            });
        });
    if close {
        open = false;
    }
    if look {
        let search = d.search();
        let state = Arc::clone(&d.state);
        *dm_core::lock(&state) = Grab::Looking;
        let ctx = ctx.clone();
        // Those already in the list offered unticked, as the links dialog
        // offers them: all were ticked, and Add for later queued them again
        // without asking (UI round 15).
        let listed = listed.clone();
        let queue = app.backend.queue.clone();
        app.backend.runtime.spawn(async move {
            let options = queue.options().await;
            let found = dm_shell::lists::crawl_with(&client, &search, Some(&options)).await;
            *dm_core::lock(&state) = match found {
                Ok(found) => Grab::Found(
                    found
                        .files
                        .into_iter()
                        .map(|l| {
                            let new = !listed.contains(&bare(&l));
                            (l, new)
                        })
                        .collect(),
                    found.pages,
                    found.stopped,
                ),
                Err(e) => Grab::Failed(e),
            };
            ctx.request_repaint();
        });
    }
    if save {
        let search = d.search();
        if search.name.is_empty() {
            d.says = t("A saved search needs a name.").to_string();
        } else if search.page.is_empty() {
            d.says = t("A saved search needs a start page.").to_string();
        } else {
            d.says.clear();
            let mut settings = app.applied.clone();
            // Saved again under its name: what it found before is kept.
            let mut search = search;
            // Its first run on the timer one interval from now: run at once,
            // a search just saved queued its files with nothing said in the
            // dialog (UI round 15). Find files runs it now.
            search.last_run = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if let Some(before) = settings
                .site_searches
                .iter()
                .find(|s| s.name == search.name)
            {
                search.seen = before.seen.clone();
                search.last_run = before.last_run;
            }
            settings.site_searches.retain(|s| s.name != search.name);
            settings.site_searches.push(search);
            if let Err(e) = app.apply_settings(settings) {
                if let Some(d) = app.dialogs.site_files.as_mut() {
                    d.says = e;
                }
            }
        }
    }
    if let Some(search) = run_saved {
        app.run_search(search);
    }
    if let Some(name) = delete_saved {
        let mut settings = app.applied.clone();
        settings.site_searches.retain(|s| s.name != name);
        if let Err(e) = app.apply_settings(settings) {
            match app.dialogs.site_files.as_mut() {
                Some(d) => d.says = e,
                None => app.problem(e),
            }
        }
    }
    if let Some(chosen) = add {
        let with = AddOptions {
            later: true,
            queue: MAIN_QUEUE.into(),
            ..AddOptions::default()
        };
        if let Ok(n) = app.add(&chosen, with) {
            app.said = crate::i18n::Said::counting_naming(
                "Added {} download to {}.||Added {} downloads to {}.",
                n as u64,
                &[&n, &MAIN_QUEUE],
            );
        }
        open = false;
    }
    if !open {
        app.dialogs.site_files = None;
    }
}

fn update_link(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Update download link");
    // Which download, and the address it has now: the dialog named neither
    // (UI round 13).
    let Some(row) = app
        .dialogs
        .update_link
        .as_ref()
        .and_then(|(id, _, _)| app.row(*id).cloned())
    else {
        app.dialogs.update_link = None;
        return;
    };
    let named = app.name_it_will_get(&row, &mut app.applied.categories());
    let Some((id, address, says)) = app.dialogs.update_link.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    window(ctx, "Update download link")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.strong(&*crate::bidi::visual(&named));
            ui.add(
                egui::Label::new(RichText::new(format::shown_url(&row.url)).small().weak())
                    .truncate(),
            )
            .on_hover_text(format::shown_url(&row.url));
            ui.label(t("The download's link has expired? Paste a fresh one:"));
            ok = entered(&focused(field(ui, t("New address"), address, 420.0), focus));
            if !says.is_empty() {
                ui.colored_label(warn(ui), says.as_str());
            }
            // Cancel beside OK, as every dialog has (UI round 4).
            footer(ui, |ui| {
                ok |= primary(ui, "OK").clicked() || enter_free(ui);
                if ui.button(t("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    open &= !cancel;
    if ok && open {
        match app.backend.set_url(*id, address) {
            Ok(()) => open = false,
            Err(e) => *says = e,
        }
    }
    if !open {
        app.dialogs.update_link = None;
    }
}

fn move_to_queue(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Move to queue");
    let Some((ids, queue, new)) = app.dialogs.move_to_queue.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    let known = app.backend.queues();
    // The queue is held as shown, so the field reads in the language in
    // force, and taken back to dm's own name on OK: typing "Principal" in
    // Spanish is the Main queue, not a new one beside it. The pick and a
    // new name are kept apart: one field for both started with "Main
    // queue" in it, and typing "Night" made "Main queueNight" (UI round 15).
    window(ctx, "Move to queue")
        .open(&mut open)
        .show(ctx, |ui| {
            for q in &known {
                let shown = crate::i18n::name(q);
                let picked = new.trim().is_empty() && *queue == shown;
                if radio_flag(ui, picked, shown.clone()).clicked() {
                    *queue = shown;
                    new.clear();
                }
            }
            ok = entered(&focused(field(ui, t("Or a new queue"), new, 240.0), focus));
            footer(ui, |ui| {
                ok |= primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    open &= !cancel;
    let chosen = if new.trim().is_empty() {
        queue.trim().to_string()
    } else {
        new.trim().to_string()
    };
    if ok && open && !chosen.is_empty() {
        let named = crate::i18n::queue_named(&chosen);
        for id in ids.iter() {
            app.backend.set_queue(*id, &named);
        }
        open = false;
    }
    if !open {
        app.dialogs.move_to_queue = None;
    }
}

fn rename(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Move/Rename");
    let platform = std::sync::Arc::clone(&app.backend.platform);
    let Some(d) = app.dialogs.rename.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    window(ctx, "Move/Rename").open(&mut open).show(ctx, |ui| {
        // A new name for one; for several, only where they go.
        if d.ids.len() == 1 {
            ok = entered(&focused(
                field(ui, t("New name"), &mut d.name, 320.0),
                focus,
            ));
        } else {
            ui.label(crate::i18n::tn(
                "Moves {} finished file.||Moves {} finished files.",
                d.ids.len() as u64,
                &[&d.ids.len()],
            ));
        }
        let name = ui.label(t("Folder"));
        ui.horizontal(|ui| {
            let folder = ui
                .add(egui::TextEdit::singleline(&mut d.folder).desired_width(280.0))
                .labelled_by(name.id);
            ok |= entered(&folder);
            browse_button(ui, &*platform, &mut d.folder);
        });
        footer(ui, |ui| {
            ok |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    open &= !cancel;
    if ok {
        let d = d.clone();
        let folder = std::path::PathBuf::from(d.folder.trim());
        let mut said: Option<crate::i18n::Said> = None;
        for id in &d.ids {
            let Some(row) = app.row(*id).cloned() else {
                continue;
            };
            // Moved first where the folder changed, then renamed in it.
            let here = std::path::Path::new(&row.path)
                .parent()
                .map(|p| p.to_path_buf());
            if !d.folder.trim().is_empty() && here.as_deref() != Some(folder.as_path()) {
                if let Err(e) = app.backend.move_to(*id, &folder) {
                    said = Some(e.into());
                    continue;
                }
            }
            if d.ids.len() == 1 {
                let asked = d.name.trim().to_string();
                match app.backend.rename(*id, &asked) {
                    Ok(()) => {
                        app.refresh();
                        // A name already taken in that folder is given a
                        // number, as a download's is; said, rather than a
                        // different name appearing without a word (round 12).
                        if let Some(given) = app.row(*id).map(|r| r.filename.clone()) {
                            if given != asked {
                                said = Some(crate::i18n::Said::new(
                                    "{} is taken in that folder, so it is called {}.",
                                    &[&asked, &given],
                                ));
                            }
                        }
                    }
                    Err(e) => said = Some(e.into()),
                }
            }
        }
        app.refresh();
        if let Some(said) = said {
            app.said = said;
        }
        open = false;
    }
    if !open {
        app.dialogs.rename = None;
    }
}

fn find(app: &mut DmApp, ctx: &egui::Context) {
    if !app.dialogs.find_open {
        return;
    }
    let focus = app.dialogs.take_focus("Find");
    let mut open = true;
    let mut next = false;
    // Never over the row it found: moved above or below that row where
    // it would cover it (gap 136), else in the middle.
    let screen = ctx.content_rect();
    let found = app.cursor.and_then(|id| app.row_rects.get(&id)).copied();
    let size = ctx
        .memory(|m| m.area_rect(window_id("Find")))
        .map(|r| r.size())
        .unwrap_or(egui::vec2(440.0, 170.0));
    let mut place = window(ctx, "Find");
    if let Some(row) = found {
        let middle = egui::Rect::from_center_size(screen.center(), size);
        if middle.expand(4.0).intersects(row) {
            let below = row.bottom() + 12.0;
            let top = if below + size.y <= screen.bottom() - 8.0 {
                below
            } else {
                (row.top() - 12.0 - size.y).max(screen.top())
            };
            place = place.anchor(egui::Align2::CENTER_TOP, [0.0, top - screen.top()]);
        }
    }
    let mut close = false;
    place.open(&mut open).show(ctx, |ui| {
        let r = focused(
            field(ui, t("Find what"), &mut app.dialogs.find_text, 260.0),
            focus,
        );
        // Enter finds the next and the field keeps the keyboard, so the
        // next Enter finds the one after and typing goes on into it.
        let typed = entered(&r);
        if typed {
            r.request_focus();
        }
        // A Close beside it, as every other dialog has (UI round 11).
        // A plain row: a band made the small window tall enough to cover
        // the row it had found.
        ui.horizontal(|ui| {
            next = ui.button(t("Find next")).clicked() || typed;
            if ui.button(t("Close")).clicked() {
                close = true;
            }
        });
    });
    if close {
        open = false;
    }
    if next {
        app.find_next();
    }
    app.dialogs.find_open = open;
}

/// The Scheduler's days, Sunday first, as `Schedule::days` numbers them.
pub const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn scheduler(app: &mut DmApp, ctx: &egui::Context) {
    let Some((schedule, says)) = app.dialogs.scheduler.as_mut() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    window(ctx, "Scheduler").open(&mut open).show(ctx, |ui| {
        // Plainly, for every queue: "When Main queue and every other
        // download may run." read oddly (UI round 12).
        ui.label(t("The hours when downloads may run, in every queue."));
        check(
            ui,
            &mut schedule.enabled,
            t("Only download between set hours"),
        );
        ui.add_enabled_ui(schedule.enabled, |ui| {
            ui.horizontal(|ui| {
                apply |= entered(&field(ui, t("Start at"), &mut schedule.from, 60.0));
                apply |= entered(&field(ui, t("Stop at"), &mut schedule.to, 60.0));
            });
            ui.horizontal(|ui| {
                for (day, word) in DAYS.iter().enumerate() {
                    let day = day as u8;
                    let mut on = schedule.days.contains(&day);
                    if check(ui, &mut on, t(word)).changed() {
                        if on {
                            schedule.days.push(day);
                            schedule.days.sort_unstable();
                        } else {
                            schedule.days.retain(|d| *d != day);
                        }
                    }
                }
            });
            // The days are shown as they are kept, all ticked for every
            // day; the hint said no tick meant every day beside seven ticks.
            ui.label(
                RichText::new(t(
                    "HH:MM, local time. A window ending before it starts runs overnight.",
                ))
                .small()
                .weak(),
            );
        });
        if !says.is_empty() {
            ui.colored_label(warn(ui), says.as_str());
        }
        footer(ui, |ui| {
            apply |= primary(ui, "OK").clicked() || enter_free(ui);
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    open &= !cancel;
    if apply && schedule.enabled && schedule.days.is_empty() {
        *says = t("Tick at least one day.").to_string();
    } else if apply
        && schedule.enabled
        && !(dm_core::schedule::is_time(&schedule.from) && dm_core::schedule::is_time(&schedule.to))
    {
        // Refused here, where it was typed: saved, "25:00" or "1am" turned
        // the schedule off without a word.
        *says = tf(
            "\"{}\" to \"{}\": times are HH:MM in 24 hours, such as 01:00 or 23:30.",
            &[&schedule.from, &schedule.to],
        );
    } else if apply {
        let mut wanted = app.applied.clone();
        wanted.schedule = schedule.clone();
        match app.apply_settings(wanted) {
            Ok(()) => open = false,
            Err(e) => {
                if let Some((_, says)) = app.dialogs.scheduler.as_mut() {
                    *says = e;
                }
            }
        }
    }
    if !open {
        app.dialogs.scheduler = None;
    }
}

fn limiter(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Speed limiter");
    let Some((limit, says, mb, hours)) = app.dialogs.limiter.as_mut() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    window(ctx, "Speed limiter")
        .open(&mut open)
        .show(ctx, |ui| {
            // The fields in one column, the buttons apart below them: each
            // field sat where its label happened to end (UI round 11).
            egui::Grid::new("limiter")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let name = ui.label(t("Maximum, KB/s across every download (0 for none)"));
                    // The limit takes the keyboard as the dialog opens, and
                    // Enter in it applies, as in any other dialog's field.
                    let field = focused(
                        ui.add(number_box(limit).range(0..=1024 * 1024))
                            .labelled_by(name.id),
                        focus,
                    );
                    select_on_focus(ui, &field, &limit.to_string());
                    apply |= entered(&field);
                    ui.end_row();
                    // Download limits, for a link with a fair-use
                    // allowance: downloads are held once it is used and go
                    // on after.
                    let name = ui.label(t("Download at most this many MB (0 for no limit)"));
                    let mb_field = ui
                        .add(number_box(mb).range(0..=1024 * 1024 * 1024))
                        .labelled_by(name.id);
                    select_on_focus(ui, &mb_field, &mb.to_string());
                    apply |= entered(&mb_field);
                    ui.end_row();
                    let name = ui.label(t("in each period of this many hours"));
                    let hours_field = ui
                        .add(number_box(hours).range(1..=24 * 31))
                        .labelled_by(name.id);
                    select_on_focus(ui, &hours_field, &hours.to_string());
                    apply |= entered(&hours_field);
                    ui.end_row();
                });
            // The modern footer draws its own line.
            if !crate::theme::Tokens::get(ui.ctx()).modern {
                ui.separator();
            }
            if !says.is_empty() {
                ui.colored_label(warn(ui), says.as_str());
            }
            footer(ui, |ui| {
                apply |= primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    open &= !cancel;
    if apply && open {
        let mut wanted = app.applied.clone();
        wanted.speed_limit = *limit;
        wanted.limit_mb = *mb;
        wanted.limit_hours = *hours;
        match app.apply_settings(wanted) {
            Ok(()) => open = false,
            Err(e) => {
                if let Some((_, says, _, _)) = app.dialogs.limiter.as_mut() {
                    *says = e;
                }
            }
        }
    }
    if !open {
        app.dialogs.limiter = None;
    }
}

/// The settings in force now, with only the settings Options changed taken
/// from its draft: each top-level setting the draft holds differently from
/// when the dialog opened. Whatever else changed meanwhile stands.
pub fn changed_only(
    opened: &Settings,
    draft: &Settings,
    now: &Settings,
) -> Result<Settings, String> {
    let object = |s: &Settings| match serde_json::to_value(s) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        _ => Err("the settings could not be read to save them".to_string()),
    };
    let (opened, draft, mut merged) = (object(opened)?, object(draft)?, object(now)?);
    for (key, value) in draft {
        if opened.get(&key) != Some(&value) {
            merged.insert(key, value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(merged))
        .map_err(|e| format!("the settings could not be saved: {e}"))
}

fn options(app: &mut DmApp, ctx: &egui::Context) {
    if app.dialogs.options.is_none() {
        return;
    }
    let queues = app.backend.queues();
    let platform = std::sync::Arc::clone(&app.backend.platform);
    // One size for every tab, so the dialog and its buttons stay where they
    // are when another tab is picked; a tab longer than that scrolls.
    // Sized from the first frame: a window that grew into its size over a
    // few frames moved under the pointer, centred as it is.
    // No taller than the window either: at 150% in the smallest window its
    // OK sat below the bottom edge (UI round 4).
    // Below the menu bar, as every dialog keeps (UI round 5).
    let menu_bottom = app.menu_bottom.max(0.0);
    let room_below = ctx.content_rect().height() - menu_bottom - 16.0;
    let tall = room_below.clamp(240.0, 680.0);
    let Some(d) = app.dialogs.options.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    // No wider than the window: at 200% text in a 960-point window the
    // tabs, the title and OK were off its edges (UI round 3).
    let wide = (ctx.content_rect().width() - sides(ctx)).clamp(320.0, 660.0);
    // The settings of the tab picked, drawn in whichever layout fits.
    let settings = |ui: &mut egui::Ui, d: &mut OptionsDialog| {
        let s = &mut d.draft;
        let fresh = std::mem::take(&mut d.fresh);
        // The newest row's first field, when one was just added.
        let first = |field: egui::Response, last: bool| {
            if fresh && last {
                field.request_focus();
                field.scroll_to_me(None);
            }
        };
        match d.tab {
            Tab::General => {
                ui.horizontal_wrapped(|ui| {
                    let now = crate::i18n::chosen(&s.language);
                    // Named by its label, so a screen reader says what it is;
                    // the name before the choice, as every number and choice
                    // in Options has it (UI round 5: some had it after).
                    let name = ui.label(t("Language"));
                    combo("language")
                        .selected_text(crate::i18n::name_of(now))
                        .show_ui(ui, |ui| {
                            if let Some(code) = crate::i18n::pick(ui, now) {
                                s.language = code.to_string();
                            }
                        })
                        .response
                        .labelled_by(name.id);
                });
                // In groups under headings: one list of a dozen boxes was
                // hard to find a setting in (a design review).
                section(ui, t("Starting and updating"));
                check(ui, &mut s.start_on_login, t("Start dm when you log in, so a browser hand-off always has an app to land in"));
                check(ui, &mut s.check_updates, t("Look for a new version about once a week (asks GitHub; nothing about you is sent)"));
                section(ui, t("Windows and sounds"));
                check(
                    ui,
                    &mut s.show_progress,
                    t("Show a progress window when a download starts"),
                );
                check(
                    ui,
                    &mut s.show_complete,
                    t("Show a window when a download finishes"),
                );
                check(
                    ui,
                    &mut s.sound_on_complete,
                    t("Play a sound when a download finishes"),
                );
                check(
                    ui,
                    &mut s.sound_on_failure,
                    t("Play a sound when a download fails"),
                );
                section(ui, t("Adding and keeping files"));
                check(
                    ui,
                    &mut s.watch_clipboard,
                    t("Watch the clipboard for links and offer to download them"),
                );
                check(
                    ui,
                    &mut s.ask_panel_downloads,
                    t("Ask where to save a video from the browser's panel"),
                );
                check(
                    ui,
                    &mut s.server_dates,
                    t("Give a finished file the date its server says it last changed"),
                );
                section(ui, t("The list"));
                // Each choice of a group under its name, as WinUI's radio
                // buttons stand.
                ui.label(t("Double-clicking a download"));
                ui.vertical(|ui| {
                    for (word, name) in [
                        ("properties", t("shows its properties")),
                        ("open", t("opens the file")),
                        ("folder", t("opens its folder")),
                    ] {
                        radio(ui, &mut s.double_click, word.to_string(), name);
                    }
                });
                ui.label(t("Look"));
                ui.vertical(|ui| {
                    for (word, name) in THEMES {
                        radio(ui, &mut s.theme, word.to_string(), t(name));
                    }
                });
                // Settings to carry to another computer, never with a
                // password; read back in, they wait here for OK.
                section(ui, t("Settings file"));
                ui.horizontal_wrapped(|ui| {
                    // Without a file chooser, the download folder's
                    // dm-settings.json, said where: the words asked for a
                    // path with nowhere to type one (round 17).
                    let fallback = s.download_dir.join("dm-settings.json");
                    if ui.button(t("Save settings to a file…")).clicked() {
                        let path = if platform.can_choose_file() {
                            platform.choose_file(true, "dm-settings.json")
                        } else {
                            Some(fallback.clone())
                        };
                        d.says = if let Some(path) = path {
                            match std::fs::write(&path, s.exported()) {
                                Ok(()) => {
                                    tf("Saved to {}, without any password.", &[&path.display()])
                                }
                                Err(e) => tf("{} could not be written: {}", &[&path.display(), &e]),
                            }
                        } else {
                            String::new()
                        };
                    }
                    if ui.button(t("Load settings from a file…")).clicked() {
                        let path = if platform.can_choose_file() {
                            platform.choose_file(false, "dm-settings.json")
                        } else {
                            Some(fallback.clone())
                        };
                        if let Some(path) = path {
                            d.says = match std::fs::read_to_string(&path)
                                .map_err(|e| e.to_string())
                                .and_then(|text| s.imported(&text))
                            {
                                Ok(read) => {
                                    *s = read;
                                    t("Loaded. Look them over, then OK to use them.").into()
                                }
                                Err(e) => tf("{} could not be read: {}", &[&path.display(), &e]),
                            };
                        }
                    }
                });
            }
            Tab::FileTypes => {
                // The extension, for someone who said not now to the bar
                // over the list, or has a second browser.
                ui.horizontal_wrapped(|ui| {
                    ui.label(t("Add dm to your browser:"));
                    for (name, page) in [
                        ("Microsoft Edge", crate::app::EXTENSION_EDGE),
                        ("Firefox", crate::app::EXTENSION_FIREFOX),
                        (t("Chrome and others"), crate::app::EXTENSION_CHROME),
                    ] {
                        if ui.button(name).clicked() {
                            platform.open_later(page, Box::new(|_| {}));
                        }
                    }
                });
                ui.add_space(8.0);
                let c = &mut s.catch;
                check(
                    ui,
                    &mut c.enabled,
                    t("Take downloads of these types from the browser"),
                );
                ui.add_enabled_ui(c.enabled, |ui| {
                    let name = ui.label(t("File types"));
                    ui.add(egui::TextEdit::multiline(&mut d.types).desired_rows(3).desired_width(ui.available_width().min(480.0)))
                        .labelled_by(name.id);
                    ui.label(RichText::new(t("Without dots, separated by spaces. Empty takes every type.")).small().weak());
                    number_row(ui, t("Leave files smaller than, KB (0 for none)"), |ui, name| {
                        ui.add(number_box(&mut c.min_size_kb).range(0..=1024 * 1024)).labelled_by(name);
                    });
                    field(ui, t("Never take downloads from these sites"), &mut d.skip, 480.0);
                    ui.label(RichText::new(t("Sites such as example.com or *.example.com, separated by spaces; each covers its subdomains.")).small().weak());
                    // A link that works once was spent by dm asking about
                    // it before taking it (gap 49).
                    field(ui, t("Sites whose links work only once"), &mut d.once, 480.0);
                    ui.label(RichText::new(t("dm takes their downloads without first asking the server whether it can fetch them, which would use the link up. A link that needs the browser's sign-in then fails in dm rather than finishing in the browser.")).small().weak());
                });
                // The keys a click on a link can be held with.
                let keys = [
                    ("alt", t("Alt")),
                    ("shift", t("key:Shift")),
                    ("ctrl", t("Ctrl or Cmd")),
                ];
                let word = |key: &str| {
                    keys.iter()
                        .find(|(k, _)| *k == key)
                        .map(|(_, w)| w.trim_start_matches("key:").to_string())
                        .unwrap_or_else(|| t("None").to_string())
                };
                ui.horizontal_wrapped(|ui| {
                    let name = ui.label(t("Hold while clicking to leave it with the browser"));
                    combo("keep-key")
                        .selected_text(word(&c.keep_key))
                        .show_ui(ui, |ui| {
                            for (key, _) in &keys {
                                pick(ui, &mut c.keep_key, key.to_string(), word(key));
                            }
                        })
                        .response
                        .labelled_by(name.id);
                });
                ui.horizontal_wrapped(|ui| {
                    let name = ui.label(t(
                        "Hold while clicking to send it to dm whatever its type or size",
                    ));
                    combo("take-key")
                        .selected_text(word(&c.take_key))
                        .show_ui(ui, |ui| {
                            pick(ui, &mut c.take_key, String::new(), word(""));
                            for (key, _) in &keys {
                                pick(ui, &mut c.take_key, key.to_string(), word(key));
                            }
                        })
                        .response
                        .labelled_by(name.id);
                });
                if !c.take_key.is_empty() && c.take_key == c.keep_key {
                    ui.colored_label(warn(ui), t("The two keys are the same; holding it leaves the download with the browser."));
                }
                check(
                    ui,
                    &mut c.video_panel,
                    t("Show \"Download video\" over videos in the browser"),
                );
                ui.label(RichText::new(t("Read by the browser extension the next time it talks to dm: a download it catches, or its right-click menu. The right-click menu hands anything over regardless.")).small().weak());
            }
            Tab::SaveTo => {
                let mut dir = s.download_dir.display().to_string();
                let before = dir.clone();
                folder_field(ui, t("Download folder"), &mut dir, &*platform);
                if dir != before {
                    s.download_dir = dir.into();
                }
                folder_field(
                    ui,
                    t("Folder for unfinished downloads (empty for dm's own)"),
                    &mut s.parts_dir,
                    &*platform,
                );
                ui.label(
                    RichText::new(t("A download already begun keeps the folder it began in."))
                        .small()
                        .weak(),
                );
                check(
                    ui,
                    &mut s.categorize,
                    t("Sort finished files into folders by kind"),
                );
                // Why what follows is greyed: "Add a category" did nothing,
                // with no word (UI round 18).
                if !s.categorize {
                    ui.label(
                        RichText::new(t(
                            "Turn on sorting to set the kinds' folders and your own categories.",
                        ))
                        .small()
                        .weak(),
                    );
                }
                ui.add_enabled_ui(s.categorize, |ui| {
                    // Two kinds a row: seven rows pushed the categories
                    // below out of sight. One where two do not fit: at 200%
                    // text they pushed Options off the window (UI round 5).
                    let per_row = if ui.available_width() >= 600.0 { 2 } else { 1 };
                    egui::Grid::new("folders").num_columns(2 * per_row).spacing([12.0, 6.0]).show(ui, |ui| {
                        for (i, c) in Category::ALL.iter().enumerate() {
                            // In the window's language, as the places
                            // name them; the folder beside it is a name on
                            // disk and stays as it is.
                            let name = ui.label(crate::i18n::name(&format::titled(&category_word(*c))));
                            if let Some(folder) = s.category_folders.get_mut(i) {
                                // Sized to what it holds, and given that
                                // size: a field in a grid took the width its
                                // column had, and the first column's were
                                // cut to two letters.
                                let font = egui::TextStyle::Body.resolve(ui.style());
                                let wide = ui
                                    .painter()
                                    .layout_no_wrap(folder.clone(), font, egui::Color32::PLACEHOLDER)
                                    .size()
                                    .x;
                                let size = egui::vec2(
                                    (wide + 28.0).clamp(140.0, 280.0),
                                    ui.spacing().interact_size.y,
                                );
                                ui.add_sized(size, egui::TextEdit::singleline(folder))
                                    .labelled_by(name.id);
                            }
                            if i % per_row == per_row - 1 {
                                ui.end_row();
                            }
                        }
                    });
                    ui.separator();
                    ui.label(t("Your categories, looked at first: a file of these types, from these sites (any when empty), goes in a folder of the category's name."));
                    let mut remove = None;
                    let count = d.custom.len();
                    for (i, [name, types, sites]) in d.custom.iter_mut().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            first(field(ui, t("Category"), name, 110.0), i + 1 == count);
                            field(ui, t("Types"), types, 150.0);
                            field(ui, t("Sites"), sites, 150.0);
                            if ui.button(t("Delete category")).clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        d.custom.remove(i);
                    }
                    if ui.button(t("Add a category")).clicked() {
                        d.custom.push(Default::default());
                        d.fresh = true;
                    }
                });
            }
            Tab::Downloads => {
                egui::Grid::new("downloads")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        let name = ui.label(t("Connections per file"));
                        ui.add(number_box(&mut s.connections).range(1..=32))
                            .labelled_by(name.id);
                        ui.end_row();
                        let name = ui.label(t("Files at once"));
                        ui.add(number_box(&mut s.concurrency).range(1..=10))
                            .labelled_by(name.id);
                        ui.end_row();
                        let choices = QUALITIES;
                        let current = choices
                            .iter()
                            .find(|(v, _)| *v == s.stream_quality)
                            .map(|(_, l)| *l)
                            .unwrap_or("Best available");
                        ui.label(t("Video quality"));
                        combo("quality")
                            .selected_text(t(current))
                            .show_ui(ui, |ui| {
                                for (value, label) in choices {
                                    pick(ui, &mut s.stream_quality, value.to_string(), t(label));
                                }
                            });
                        ui.end_row();
                    });
                check(
                    ui,
                    &mut s.start_queue_on_launch,
                    t("Start the Main queue when dm opens"),
                );
                check(
                    ui,
                    &mut s.subtitles,
                    t("Embed subtitles in videos from player pages"),
                );
                check(
                    ui,
                    &mut s.remux_ts,
                    t("Save streams that arrive as .ts as MP4"),
                );
                ui.add_space(4.0);
                ui.label(t(
                    "Queues that run fewer files at once than the number above:",
                ));
                let mut remove = None;
                // One limit a queue: two for Main queue were both kept and
                // the last won without a word (UI round 12). Each offers
                // only the queues no other limit has.
                let limited: Vec<String> = s.queue_limits.iter().map(|l| l.queue.clone()).collect();
                for (i, limit) in s.queue_limits.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        // Chosen from the queues there are, shown in the
                        // language in force and kept by dm's own name: a
                        // typed name had to be the English one to match.
                        let name = ui.label(t("Queue"));
                        combo(("queue limit", i))
                            .selected_text(crate::i18n::name(&limit.queue))
                            .show_ui(ui, |ui| {
                                let mut offered: Vec<String> = queues
                                    .iter()
                                    .filter(|q| {
                                        **q == limit.queue
                                            || !limited
                                                .iter()
                                                .enumerate()
                                                .any(|(j, l)| j != i && l == *q)
                                    })
                                    .cloned()
                                    .collect();
                                if !offered.contains(&limit.queue) {
                                    offered.push(limit.queue.clone());
                                }
                                for q in offered {
                                    let shown = crate::i18n::name(&q);
                                    pick(ui, &mut limit.queue, q, shown);
                                }
                            })
                            .response
                            .labelled_by(name.id);
                        let name = ui.label(t("Files at once"));
                        ui.add(number_box(&mut limit.files).range(1..=10))
                            .labelled_by(name.id);
                        if ui.button(t("Delete queue limit")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    s.queue_limits.remove(i);
                }
                let unlimited = queues
                    .iter()
                    .find(|q| !s.queue_limits.iter().any(|l| &l.queue == *q))
                    .cloned();
                if ui
                    .add_enabled(
                        unlimited.is_some(),
                        egui::Button::new(t("Add a queue limit")),
                    )
                    .clicked()
                {
                    if let Some(queue) = unlimited {
                        s.queue_limits
                            .push(dm_shell::settings::QueueLimit { queue, files: 1 });
                    }
                }
                number_row(ui, t("Ask the Recheck queue's servers for changes every, hours (0 only when asked)"), |ui, name| {
                    ui.add(number_box(&mut s.recheck_hours).range(0..=24 * 31)).labelled_by(name);
                });
                ui.add_space(8.0);
                ui.label(t("When a download finishes, run (a virus checker, a script); the file's path is added at the end:"));
                field(ui, t("Command"), &mut s.on_complete, 320.0);
            }
            Tab::Connection => {
                number_row(ui, t("Speed limit, KB/s (0 for none)"), |ui, name| {
                    ui.add(number_box(&mut s.speed_limit).range(0..=1024 * 1024))
                        .labelled_by(name);
                });
                number_row(
                    ui,
                    t("Attempts per connection (0 to keep trying)"),
                    |ui, name| {
                        ui.add(number_box(&mut s.attempts).range(0..=100))
                            .labelled_by(name);
                    },
                );
                number_row(
                    ui,
                    t("Seconds between attempts (0 lets dm choose)"),
                    |ui, name| {
                        ui.add(number_box(&mut s.retry_wait).range(0..=3600))
                            .labelled_by(name);
                    },
                );
                number_row(
                    ui,
                    t("Give up on a connection that sends nothing for, seconds"),
                    |ui, name| {
                        ui.add(number_box(&mut s.read_timeout).range(5..=600))
                            .labelled_by(name);
                    },
                );
                ui.add_space(8.0);
                ui.label(t(
                    "Sites that take fewer connections at once than the setting:",
                ));
                let mut remove = None;
                let count = s.host_limits.len();
                for (i, limit) in s.host_limits.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        first(field(ui, t("Site"), &mut limit.host, 200.0), i + 1 == count);
                        let name = ui.label(t("Connections"));
                        ui.add(number_box(&mut limit.connections).range(1..=32))
                            .labelled_by(name.id);
                        if ui.button(t("Delete site limit")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    s.host_limits.remove(i);
                }
                if ui.button(t("Add a site limit")).clicked() {
                    s.host_limits.push(dm_core::HostLimit {
                        host: String::new(),
                        connections: 1,
                    });
                    d.fresh = true;
                }
                ui.add_space(8.0);
                ui.label(t("For downloads that need you signed in: a cookies.txt exported from your browser."));
                let cookies_before = s.cookies_file.clone();
                ui.horizontal_wrapped(|ui| {
                    field(ui, t("Cookies file"), &mut s.cookies_file, 260.0);
                    if ui.button(t("Choose…")).clicked() {
                        if let Some(file) = rfd::FileDialog::new()
                            .add_filter("cookies.txt", &["txt"])
                            .pick_file()
                        {
                            s.cookies_file = file.display().to_string();
                        }
                    }
                    if !s.cookies_file.is_empty() && ui.button(t("Clear")).clicked() {
                        s.cookies_file.clear();
                    }
                });
                // What was said about the file goes with it: cleared, the
                // red "could not be read" stayed on every tab (UI round 16).
                if s.cookies_file != cookies_before
                    && !cookies_before.is_empty()
                    && d.says.contains(&cookies_before)
                {
                    d.says.clear();
                }
            }
            Tab::Proxy => {
                use dm_core::proxy::Kind;
                let p = &mut s.proxy;
                check(ui, &mut p.enabled, t("Connect through a proxy"));
                ui.add_enabled_ui(p.enabled, |ui| {
                    let kind_name = |kind: Kind| match kind {
                        Kind::System => t("The computer's proxy settings").to_string(),
                        other => format!("{other:?}").to_uppercase(),
                    };
                    ui.horizontal(|ui| {
                        let name = ui.label(t("Proxy type"));
                        combo("proxy-kind")
                            .selected_text(kind_name(p.kind))
                            .show_ui(ui, |ui| {
                                for kind in [Kind::System, Kind::Http, Kind::Https, Kind::Socks4, Kind::Socks5] {
                                    let was = p.kind;
                                    if pick(ui, &mut p.kind, kind, kind_name(kind)).changed() {
                                        // The usual port of the kind picked,
                                        // where the port was the last kind's
                                        // usual one: SOCKS5 kept 8080 (UI
                                        // round 14). A port typed in stays.
                                        let usual = |k: Kind| match k {
                                            Kind::Socks4 | Kind::Socks5 => Some(1080),
                                            Kind::Http | Kind::Https => Some(8080),
                                            Kind::System => None,
                                        };
                                        if usual(was).is_some_and(|port| port == p.port) || p.port == 0 {
                                            if let Some(port) = usual(kind) {
                                                p.port = port;
                                            }
                                        }
                                    }
                                }
                            })
                            .response
                            .labelled_by(name.id);
                    });
                    ui.add_enabled_ui(p.kind != Kind::System, |ui| {
                        field(ui, t("Host"), &mut p.host, 260.0);
                        number_row(ui, t("Port"), |ui, name| {
                            ui.add(number_box(&mut p.port).range(1..=65535)).labelled_by(name);
                        });
                        field(ui, t("Username, if it needs one"), &mut p.username, 200.0);
                        let name = ui.label(t("Password"));
                        ui.add(egui::TextEdit::singleline(&mut p.password).password(true).desired_width(ui.available_width().min(200.0))).labelled_by(name.id);
                        field(ui, t("Not for these sites (fetched directly)"), &mut d.bypass, 260.0);
                    });
                    ui.label(RichText::new(t("Every download of dm's own, yt-dlp and a torrent's SOCKS5 connections go through it; FTP through SOCKS or HTTP CONNECT.")).small().weak());
                });
            }
            Tab::SiteLogins => {
                ui.label(t(
                    "A user and password used for every download from a host.",
                ));
                ui.label(
                    RichText::new(t(
                        "Sent as a Basic login, or as a Digest where the server asks for one.",
                    ))
                    .small()
                    .weak(),
                );
                let mut remove = None;
                let count = s.site_logins.len();
                for (i, login) in s.site_logins.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        first(field(ui, t("Host"), &mut login.host, 150.0), i + 1 == count);
                        field(ui, t("User"), &mut login.username, 110.0);
                        let name = ui.label(t("Password"));
                        ui.add(
                            egui::TextEdit::singleline(&mut login.password)
                                .password(true)
                                .desired_width(ui.available_width().min(110.0)),
                        )
                        .labelled_by(name.id);
                        if ui.button(t("Delete")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    s.site_logins.remove(i);
                }
                if ui.button(t("Add a login")).clicked() {
                    s.site_logins.push(SiteLogin::default());
                    d.fresh = true;
                }
                ui.label(
                    RichText::new(t(
                        "Kept in dm's settings file on this computer, readable by your account.",
                    ))
                    .small()
                    .weak(),
                );
            }
        }
    };
    // Too short for the tabs, a few rows of settings and the buttons (200%
    // text in a 460-point window, 300% in 800): the dialog scrolls as one
    // page, its buttons at the end, rather than drawing them over the
    // settings (UI round 6).
    let short = room_below < 270.0;
    let mut frame = window(ctx, "Options").open(&mut open);
    frame = if short {
        frame
            .vscroll(true)
            .min_width(wide)
            .max_width(wide)
            .max_height((room_below - 56.0).max(120.0))
    } else {
        // Its body scrolls itself, between the tabs and the buttons.
        frame
            .vscroll(false)
            .fixed_size(egui::vec2(wide, tall - 60.0))
    };
    frame.show(ctx, |ui| {
        // At its top as it opens, in a window where the whole dialog
        // scrolls: asked of the tab, the request went to the tabs' own
        // sideways strip and the page stayed where it was left (UI round 8).
        // Its very top, title and all: the cursor is under the title now.
        if d.fresh_tab > 0 {
            let top = egui::Rect::from_min_size(ui.min_rect().min, egui::vec2(1.0, 1.0));
            ui.scroll_to_rect(top, Some(egui::Align::TOP));
        }
        // The tabs scroll sideways where they do not all fit, with a bar
        // that shows it: one shown only under the pointer left Sign-ins cut
        // off at 150% text with no sign of more (UI round 10). Wrapped on
        // to more rows, they left the page no room at 200%.
        let strip = egui::ScrollArea::horizontal().id_salt("options-tabs");
        let labels: Vec<&str> = TABS.iter().map(|(_, label)| t(label)).collect();
        // A few points kept spare: fitted exactly, the strip still drew
        // its scroll bar at 150% under tabs that did not move (QA loop 2).
        let padding = tabs_padding(ui, &labels, ui.available_width() - 4.0);
        ui.scope(|ui| {
            ui.spacing_mut().scroll = crate::theme::steady_scroll();
            strip.show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in TABS {
                        let picked = selector(ui, d.tab == tab, t(label), padding);
                        if picked.clicked() {
                            // At its top, not where it was left: Folders
                            // opened with its first field scrolled out of
                            // sight (UI round 18).
                            if d.tab != tab {
                                d.fresh_tab = 2;
                            }
                            d.tab = tab;
                        }
                        // The tab in force in sight as the dialog opens: the
                        // strip kept the place it was scrolled to, General
                        // out of view (UI round 6).
                        // To the top: in a short window the dialog itself
                        // scrolls, and reopened where it was left, its tabs
                        // out of view (UI round 7).
                        if d.tab == tab && d.fresh_tab > 0 {
                            picked.scroll_to_me(Some(egui::Align::TOP));
                        }
                    }
                })
            });
        });
        let fresh = d.fresh_tab > 0;
        d.fresh_tab = d.fresh_tab.saturating_sub(1);
        ui.separator();
        if short {
            settings(ui, d);
            if !crate::theme::Tokens::get(ui.ctx()).modern {
                ui.separator();
            }
            if !d.says.is_empty() {
                ui.add(egui::Label::new(RichText::new(&d.says).color(warn(ui))).wrap());
            }
            footer(ui, |ui| {
                ok = primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
            return;
        }
        // The buttons first, from the window's foot up, with a warning
        // over them only when there is one; the tab's body takes exactly
        // the room left. A fixed 80 kept for them left two settings showing
        // at 720×460 (UI round 4); a guess left a band under the body, and
        // a measured one lagged a frame behind a warning (UI round 5).
        let top = ui.cursor().top();
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            footer(ui, |ui| {
                ok = primary(ui, "OK").clicked() || enter_free(ui);
                cancel = ui.button(t("Cancel")).clicked();
            });
            // The modern footer draws its own line.
            if !crate::theme::Tokens::get(ui.ctx()).modern {
                ui.separator();
            }
            if !d.says.is_empty() {
                ui.add(egui::Label::new(RichText::new(&d.says).color(warn(ui))).wrap());
            }
            // What is left between the tabs and the buttons.
            let body = (ui.cursor().bottom() - ui.spacing().item_spacing.y - top).max(24.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                // A bar that is always drawn where a tab is longer than the
                // room, rather than one that shows only under the pointer:
                // at 960 px General's last line was cut off with no sign of
                // more (gap 148).
                ui.spacing_mut().scroll = crate::theme::steady_scroll();
                let mut area = egui::ScrollArea::vertical()
                    // Each tab its own place: one shared position opened
                    // Browser or Downloads part-way down (UI round 2).
                    .id_salt(("options-tab", d.tab))
                    .max_height(body)
                    .min_scrolled_height(body)
                    .auto_shrink([false, false]);
                // At its top as the dialog opens, wherever it was left.
                if fresh {
                    area = area.vertical_scroll_offset(0.0);
                }
                area.show(ui, |ui| settings(ui, d));
            });
        });
    });
    if cancel || !open {
        app.dialogs.options = None;
        return;
    }
    if ok {
        // Split on ';' too: "xlsx;" was kept as a type (UI round 14).
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                .map(|w| w.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|w| !w.is_empty())
                .collect()
        };
        let mut wanted = d.draft.clone();
        wanted.catch.extensions = words(&d.types);
        wanted.catch.skip_hosts = sites(&words(&d.skip));
        wanted.catch.once_hosts = sites(&words(&d.once));
        wanted.proxy.bypass = words(&d.bypass);
        // A sign-in's host as the rules read it: an address pasted whole
        // matched nothing (UI round 15).
        for login in &mut wanted.site_logins {
            if let Some(host) = sites(&[login.host.trim().to_string()]).pop() {
                login.host = host;
            }
        }
        wanted.custom_categories = d
            .custom
            .iter()
            .filter(|[name, ..]| !name.trim().is_empty())
            .map(|[name, types, only]| dm_core::Custom {
                name: name.trim().to_string(),
                extensions: words(types),
                sites: sites(&words(only)),
            })
            .collect();
        // A category's name is its folder: one that is not one folder
        // would be saved and then sort nothing, without a word.
        // Only those this edit made or renamed: one an older dm saved is
        // left out as the settings load, and must not fail every OK after.
        // Held to every computer's rules, since settings travel; one saved
        // before that works here keeps working. The built-in kinds' folders
        // (Options › Folders) likewise, where this edit renamed one; empty
        // leaves that kind unsorted.
        // A proxy turned on with nowhere to connect to was taken, and every
        // download then went out directly without a word (gap 141).
        {
            use dm_core::proxy::Kind;
            let p = &wanted.proxy;
            if p.enabled && p.kind != Kind::System {
                let host = p.host.trim();
                let why = if host.is_empty() {
                    Some(t("A proxy needs a host: type its name or address, or untick \"Connect through a proxy\".").to_string())
                } else if host.contains("://")
                    || host
                        .chars()
                        .any(|c| c.is_whitespace() || "/@?#".contains(c))
                {
                    Some(tf(
                        "\"{}\" is not a proxy host: type only its name or address, such as proxy.example.com, with the port in its own field.",
                        &[&host],
                    ))
                } else if p.port == 0 {
                    Some(t("A proxy needs a port from 1 to 65535.").to_string())
                } else {
                    None
                };
                if let Some(why) = why {
                    d.says = why;
                    d.tab = Tab::Proxy;
                    return;
                }
            }
        }
        let new_custom = wanted
            .custom_categories
            .iter()
            .map(|c| c.name.clone())
            .filter(|name| !d.opened.custom_categories.iter().any(|o| o.name == *name));
        let renamed_kinds = wanted
            .category_folders
            .iter()
            .enumerate()
            .filter(|(i, name)| {
                !name.trim().is_empty()
                    && d.opened.category_folders.get(*i).map(|o| o.trim()) != Some(name.trim())
            })
            .map(|(_, name)| name.trim().to_string());
        if let Some(bad) = new_custom
            .chain(renamed_kinds)
            .find(|name| !portable_folder(name))
        {
            d.says = tf(
                "\"{}\" cannot be a category: its name is also its folder, and that is not a folder name every computer can use (no / \\ : * ? \" < > |, no name like CON, nothing ending in a dot or space)",
                &[&bad],
            );
            d.tab = Tab::SaveTo;
            return;
        }
        let applied = changed_only(&d.opened, &wanted, &app.applied)
            .and_then(|wanted| app.apply_settings(wanted));
        match applied {
            Ok(()) => app.dialogs.options = None,
            Err(e) => {
                // Left open with what was typed, and why it was not taken:
                // reopening it on what is in force threw every edit away to
                // say what was wrong with one. OK compares against what it
                // opened on, so another try changes only what was changed.
                if let Some(d) = app.dialogs.options.as_mut() {
                    d.says = e;
                }
            }
        }
    }
}

/// The countdown before the computer is turned off: time to say no.
fn countdown(app: &mut DmApp, ctx: &egui::Context) {
    let Some((action, at)) = app.countdown else {
        return;
    };
    let left = at
        .saturating_duration_since(std::time::Instant::now())
        .as_secs();
    // Escape cancels it, taken before any window behind it sees the key:
    // it closed Download complete instead, and the countdown ran out (UI
    // round 13). It is drawn first of the dialogs for that.
    let mut cancel = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let mut now = false;
    // The keyboard on Cancel as it appears, so Enter or Space stops it.
    let shown_id = egui::Id::unique("countdown-shown");
    let first = ctx.data(|d| d.get_temp::<std::time::Instant>(shown_id)) != Some(at);
    window(ctx, "Downloads are done").show(ctx, |ui| {
        ui.label(tf("dm will {} in {} seconds.", &[&t(action.verb()), &left]));
        footer(ui, |ui| {
            now = ui.button(t("Now")).clicked();
            let stop = ui.button(t("Cancel"));
            if first {
                stop.request_focus();
            }
            cancel |= stop.clicked();
        });
    });
    ctx.data_mut(|d| d.insert_temp(shown_id, at));
    if cancel {
        app.countdown = None;
    } else if now {
        app.countdown = Some((action, std::time::Instant::now()));
        app.countdown_tick();
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}

fn about(app: &mut DmApp, ctx: &egui::Context) {
    if !app.dialogs.about_open {
        return;
    }
    let mut open = true;
    window(ctx, "About dm").open(&mut open).show(ctx, |ui| {
        ui.heading("dm");
        ui.label(tf("Version {}", &[&env!("CARGO_PKG_VERSION")]));
        ui.label(t("A free download manager, written in Rust."));
        ui.separator();
        if ui.button(t("Check for updates")).clicked()
            || std::mem::take(&mut app.dialogs.check_update)
        {
            let client = app.client();
            crate::update::check(
                &app.backend.runtime,
                client,
                Arc::clone(&app.update),
                ctx.clone(),
            );
        }
        let state = dm_core::lock(&app.update).clone();
        ui.label(RichText::new(state.shown()).weak());
        // What changed in it, on the release's own page.
        if matches!(state, crate::update::Update::Newer(..)) {
            if let Some(notes) = dm_core::lock(&crate::update::LATEST_NOTES).clone() {
                if ui.button(t("What's new")).clicked() {
                    if let Err(e) = app.backend.platform.open(&notes) {
                        app.said = e.into();
                    }
                }
            }
        }
        if let crate::update::Update::Newer(_, Some(installer)) = state {
            if ui.button(t("Download the update")).clicked() {
                app.said = match app.backend.add(&installer) {
                    Ok(_) => crate::i18n::Said::new(
                        "The new installer is downloading. Open it when it has finished.",
                        &[],
                    ),
                    Err(e) => e.into(),
                };
            }
        }
    });
    app.dialogs.about_open = open;
}

/// A folder name every computer can make: what a category made or renamed
/// in Options is held to, since settings travel between computers.
fn portable_folder(name: &str) -> bool {
    dm_core::category::portable_folder(name).is_ok()
}

#[cfg(test)]
mod changed_only_tests {
    use super::*;

    #[test]
    fn options_writes_only_what_it_changed() {
        let opened = Settings::default();
        // Options changed the connections.
        let mut draft = opened.clone();
        draft.connections = 4;
        // Meanwhile View › Language and the Scheduler changed others.
        let mut now = opened.clone();
        now.language = "de".into();
        now.schedule.enabled = true;
        let wanted = changed_only(&opened, &draft, &now).unwrap();
        assert_eq!(wanted.connections, 4, "Options' own change");
        assert_eq!(wanted.language, "de", "the language picked meanwhile");
        assert!(wanted.schedule.enabled, "the schedule set meanwhile");
    }
}

#[cfg(test)]
mod sites_tests {
    /// UI round 14: a site pasted as an address is kept by its host name,
    /// which is what the rules match.
    #[test]
    fn sites_are_kept_by_their_host_name() {
        let given: Vec<String> = [
            "https://ads.example.com/path",
            "tracker.example:8080",
            "example.org/files",
            "plain.example",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            super::sites(&given),
            [
                "ads.example.com",
                "tracker.example",
                "example.org",
                "plain.example"
            ]
        );
    }
}

#[cfg(test)]
mod stream_name_tests {
    use super::*;

    /// Gap 156: a stream is suggested under the name it will be saved as,
    /// the engine's own: its folder's name, as an MP4 where it is remuxed.
    #[test]
    fn a_stream_is_suggested_under_the_name_it_is_saved_as() {
        let url = "http://127.0.0.1:8000/hls/index.m3u8";
        assert_eq!(stream_name(url, None, true).as_deref(), Some("hls.mp4"));
        assert_eq!(stream_name(url, None, false).as_deref(), Some("hls.ts"));
        assert_eq!(
            stream_name("http://h/live/lecture.m3u8?t=1", None, true).as_deref(),
            Some("lecture.mp4")
        );
        assert_eq!(
            stream_name("http://h/x", Some("application/vnd.apple.mpegurl"), true).as_deref(),
            Some("x.mp4")
        );
        assert_eq!(stream_name("http://h/file.zip", None, true), None);
    }
}
