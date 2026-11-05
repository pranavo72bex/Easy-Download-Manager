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

mod about;
mod add;
pub mod apart;
mod asks;
mod complete;
mod confirm;
mod controls;
mod file_info;
mod frame;
mod links;
mod options;
mod playlist;
mod progress;
mod properties;
mod row_dialogs;
mod series;
mod site_files;
mod speed_schedule;
mod state;
mod zip_list;

use about::*;
pub use add::*;
pub use asks::NameAsk;
use asks::*;
use complete::*;
use confirm::*;
pub(crate) use controls::*;
use file_info::*;
pub use frame::*;
use links::*;
pub use options::*;
use progress::*;
use properties::*;
use row_dialogs::*;
pub(crate) use series::*;
use site_files::*;
pub use speed_schedule::*;

/// The warning colour of the look in force, which the looks' tests hold to
/// 4.5:1 on every surface.
fn warn(ui: &egui::Ui) -> Color32 {
    crate::theme::Tokens::get(ui.ctx()).warn
}

/// Move/Rename: the finished downloads it moves, the new name where it is
/// one, and the folder they go to.
#[derive(Debug, Clone, Default)]
pub struct MoveRename {
    /// The finished downloads to move.
    pub ids: Vec<Id>,
    /// The new name, where one download is moved.
    pub name: String,
    /// The folder they go to.
    pub folder: String,
    /// For several files: how to name them. Empty keeps their names;
    /// `{n}` counts from 1 in the order they are listed, `{name}` is the
    /// old name without its ending, and the ending is kept.
    pub pattern: String,
}

/// The Add URL dialog: an address, and a login if it needs one.
#[derive(Default)]
pub struct AddUrl {
    /// The address typed.
    pub address: String,
    /// Whether the address needs a login (Authorization ticked).
    pub authorize: bool,
    /// The user name for that login.
    pub login: String,
    /// The password for that login.
    pub password: String,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
}

/// "Download details": where and how, before a download starts.
#[derive(Default)]
pub struct FileInfo {
    /// The addresses to download.
    pub urls: Vec<String>,
    /// The page a handed-over video came from, sent as Referer.
    pub referrer: String,
    /// The quality picked in the browser's panel, and what its list said.
    pub quality: Option<String>,
    /// A part of the video rather than the whole: from one second to
    /// another, as typed in the dialog's Part fields (`part_from`,
    /// `part_to`) and read when it starts.
    pub span: Option<dm_core::Span>,
    /// "Delete file after": seconds after it finishes; `None` keeps it.
    pub delete_after: Option<u64>,
    /// The part made vertical or square; `None` as it is.
    pub shape: Option<dm_core::shape::Shape>,
    /// The part saved as a GIF.
    pub gif: bool,
    /// "Only a part of the video", and its ends as typed ("4:10").
    pub part: bool,
    /// The start of the part, as typed.
    pub part_from: String,
    /// The end of the part, as typed.
    pub part_to: String,
    /// A video page's qualities with their sizes, read as the dialog opens
    /// (`dm_core::videopage::list_choices`): `None` until asked; an empty list
    /// where the page gave none.
    pub choices: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<dm_core::videopage::Choice>>>>>,
    /// A ZIP's files, read from its end as the dialog opens
    /// (`dm_core::zip_contents`): `None` until asked; an empty list where the
    /// server or the file gave none.
    pub zip: Option<ZipSlot>,
    /// A YouTube playlist page: the Playlist dialog takes over.
    pub playlist: std::sync::Arc<std::sync::Mutex<Option<dm_core::youtube::Playlist>>>,
    /// The browser a handed-over download came from, where the host could
    /// tell ("Microsoft Edge").
    pub browser: Option<String>,
    /// A download the browser caught (not one asked for from the panel or
    /// a menu): offered "Always save here without asking".
    pub caught: bool,
    /// Whether "Always save here without asking" is ticked.
    pub always_here: bool,
    /// A separate sound track for the first address, fetched and joined with it.
    pub sound: Option<String>,
    /// The quality's name, shown beside the file when the choice came from a video's list.
    pub label: Option<String>,
    /// The folder to save into, as typed.
    pub folder: String,
    /// The name to save as, as typed.
    pub name: String,
    /// The description typed for the download.
    pub description: String,
    /// The queue to put the download in, as shown.
    pub queue: String,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
    /// Handed over by the browser, rather than typed in: cancelling it is
    /// a word against that site's downloads being handed over.
    pub handed: bool,
    /// The site's sign-in the browser handed over with it, if any.
    pub cookies: String,
    /// Those cookies are for this download alone.
    pub cookies_once: bool,
    /// The request headers the browser sent for it.
    pub headers: Vec<(String, String)>,
    /// Browse… was pressed on a desktop with no folder chooser: said in
    /// this opening of the dialog only (it stayed for the next).
    pub no_chooser: bool,
    /// The name the server gives the file, once asked, and
    /// whether a person typed in Save as since, which it then leaves alone.
    pub probed: Option<std::sync::Arc<std::sync::Mutex<Option<Probed>>>>,
    /// Whether a person typed in Save as.
    pub name_typed: bool,
    /// The folder last used for this kind of file has been offered (once
    /// the name says the kind), so clearing it is not undone.
    pub folder_offered: bool,
    /// A file of its name is in the folder already, and the person chose
    /// to replace it once this one is whole rather than take a number.
    pub replace: bool,
}

/// Where the name Download details shows is taken already.
enum Clash {
    /// By another download in the list: this one takes a number.
    Listed,
    /// By a file in the folder: this one takes `free`, or replaces `taken`
    /// once it is whole.
    OnDisk {
        free: String,
        taken: std::path::PathBuf,
    },
}

/// What Download details learned by asking the server: the name the file
/// will have, where it gives one, and its size, where it says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probed {
    /// The name the file will have.
    pub name: Option<String>,
    /// The file's size in bytes.
    pub size: Option<u64>,
    /// Why the server would not give the file, where it said: a 404 was
    /// found only once the download had failed.
    pub problem: Option<String>,
    /// A web page rather than a file (a video site's watch page): dm finds
    /// the video in it when it starts, so nothing is wrong to say.
    pub page: bool,
}

/// The name an HLS stream at `url` is saved under: the engine's own naming
/// for streams (a manifest called `index.m3u8` takes its folder's name),
/// as a transport stream, and as an MP4 where `remux` puts it into one.
/// `None` for an address that is not a stream.
pub fn stream_name(url: &str, content_type: Option<&str>, remux: bool) -> Option<String> {
    if dm_core::kind_of(url, content_type) != Some(dm_core::StreamKind::Hls) {
        return None;
    }
    // The engine names a stream from its address and what its segments
    // are; asked with one transport-stream segment, it names it as one.
    let one = "#EXTM3U\n#EXTINF:1,\nsegment.ts\n#EXT-X-ENDLIST\n";
    let named = dm_core::media_playlist(one, url, url, Default::default())
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
    pub listed: std::collections::HashSet<String>,
    /// Links unticked when their server refused them, once, so a person
    /// can tick one again (they were added and failed).
    pub refused: std::collections::HashSet<String>,
    /// Whether web pages are left out of the list.
    pub hide_html: bool,
    /// File types to list alone, as typed ("zip pdf"); empty lists all.
    pub types: String,
    /// What each link's server said, by address.
    pub found: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Found>>>,
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
    pub names: std::collections::HashMap<String, String>,
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

    /// A link that is a picture: by what its server said, or before it
    /// answers, by its address.
    pub fn is_image(&self, url: &str) -> bool {
        let name = dm_core::lock(&self.found)
            .get(url)
            .map(|f| f.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                let path = url.split(['?', '#']).next().unwrap_or(url);
                path.rsplit('/').next().unwrap_or_default().to_string()
            });
        dm_core::classify(&name, None) == dm_core::Category::Pictures
    }

    /// Which rows are listed, in order: what the filters leave, and of an
    /// address the page links to more than once, its first.
    pub fn visible(&self) -> Vec<bool> {
        let mut seen = std::collections::HashSet::new();
        self.items
            .iter()
            .map(|(url, _)| {
                let first = seen.insert(bare(url));
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
        let wanted: Vec<String> = self
            .types
            .split([' ', ',', ';'])
            .map(|t| t.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if wanted.is_empty() {
            return true;
        }
        // By the name its server gave, or before it answers, its address.
        let name = dm_core::lock(&self.found)
            .get(url)
            .map(|f| f.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                let path = url.split(['?', '#']).next().unwrap_or(url);
                path.rsplit('/').next().unwrap_or_default().to_string()
            });
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        wanted.contains(&ext)
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
                category_folder(&self.category, root, categories)
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
            .unwrap_or_else(|| crate::format::provisional(url))
    }
}

/// A category's folder, by the category's word (a kind) or a person's own
/// category's name.
pub fn category_folder(
    word: &str,
    root: &std::path::Path,
    categories: &dm_core::Categories,
) -> std::path::PathBuf {
    match dm_core::Category::ALL
        .iter()
        .find(|c| crate::app::category_word(**c) == word)
    {
        Some(kind) => categories.destination(root, *kind),
        None => root.join(word),
    }
}

/// The Properties dialog of one download: its name, address, login and checks.
pub struct Properties {
    /// The name, as edited.
    pub name: String,
    /// The description, as edited.
    pub description: String,
    /// The address without its login, which has fields of its own.
    pub address: String,
    /// The login's user name, as edited.
    pub login: String,
    /// The login's password, as edited.
    pub password: String,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
    /// Digests of the finished file, worked out in the background, by the
    /// algorithm's name; `None` while one is still being worked out.
    pub sums: Sums,
    /// A digest the person was given, to check the file against.
    pub check: String,
    /// A finished ZIP's files, read from the file in the background.
    pub zip: Option<ZipSlot>,
    /// A torrent's files ticked to be fetched, as edited; empty until its
    /// files are known.
    pub files: Vec<bool>,
}

/// The files of a ZIP, as a background reading leaves them: `None` while it
/// is under way, an empty list where there was nothing to list.
pub type ZipSlot = Arc<Mutex<Option<Vec<dm_core::ZipEntry>>>>;

/// What the background hashing of a finished file has found so far.
pub type Sums = Arc<Mutex<HashMap<&'static str, Option<Result<String, String>>>>>;

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

/// A playlist being read for the dialog: `None` until it is; the reading's
/// own words where it could not be.
pub type PlaylistRead =
    std::sync::Arc<std::sync::Mutex<Option<Result<dm_core::youtube::Playlist, String>>>>;

/// "Playlist": a YouTube playlist read by dm, its videos ticked to download.
#[derive(Default)]
pub struct PlaylistDialog {
    /// The playlist's address.
    pub url: String,
    /// The reading of the playlist, once started.
    pub read: Option<PlaylistRead>,
    /// Which videos are ticked, one for each.
    pub picked: Vec<bool>,
    /// The quality to take them at (`QUALITIES`' words).
    pub quality: String,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
}

/// The Series dialog: an address with a changing number, and the range to count over.
#[derive(Default)]
pub struct Series {
    /// The address with the place of the number marked.
    pub template: String,
    /// The first number.
    pub from: String,
    /// The last number.
    pub to: String,
    /// How many digits the number is padded to.
    pub width: usize,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
}

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
        }
    }
}

/// The pages of Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    /// The General page.
    General,
    /// The File types page.
    FileTypes,
    /// The Save to page.
    SaveTo,
    /// The Downloads page.
    Downloads,
    /// The expert settings, in one page: the connection's and the proxy's.
    Advanced,
    /// The Sign-ins page.
    SiteLogins,
}

/// The settings each tab of Options edits, by the names they are saved under:
/// what its Restore defaults puts back. Sign-ins hold a person's own logins,
/// which are not a default to go back to.
pub fn tab_settings(tab: Tab) -> &'static [&'static str] {
    match tab {
        Tab::General => &[
            "language",
            "theme",
            "start_on_login",
            "allow_command_line",
            "allow_mcp",
            "check_updates",
            "sound_on_complete",
            "sound_on_failure",
            "mica",
            "show_progress",
            "show_complete",
            "watch_clipboard",
            "ask_panel_downloads",
            "ask_caught_downloads",
            "offer_new_link",
            "server_dates",
            "double_click",
            "on_close",
        ],
        Tab::FileTypes => &["catch"],
        Tab::SaveTo => &[
            "categorize",
            "category_folders",
            "download_dir",
            "parts_dir",
        ],
        Tab::Downloads => &[
            "concurrency",
            "connections",
            "fetch_tools",
            "on_complete",
            "queue_limits",
            "queue_starts",
            "recheck_hours",
            "record_minutes",
            "remux_ts",
            "start_queue_on_launch",
            "stream_quality",
            "subtitles",
            "watch_folder",
        ],
        Tab::Advanced => &[
            "attempts",
            "cookies_file",
            "host_limits",
            "proxy",
            "read_timeout",
            "retry_wait",
            "site_rules",
            "speed_limit",
        ],
        Tab::SiteLogins => &[],
    }
}

impl OptionsDialog {
    /// Puts the tab's settings back to what a new install has, in the draft:
    /// OK keeps them, Cancel leaves everything as it was. The words typed for
    /// the lists are read again from what they now hold.
    pub fn restore_defaults(&mut self) {
        let (Ok(serde_json::Value::Object(mut now)), Ok(serde_json::Value::Object(fresh))) = (
            serde_json::to_value(&self.draft),
            serde_json::to_value(Settings::default()),
        ) else {
            return;
        };
        for key in tab_settings(self.tab) {
            if let Some(value) = fresh.get(*key) {
                now.insert((*key).to_string(), value.clone());
            }
        }
        if let Ok(draft) = serde_json::from_value(serde_json::Value::Object(now)) {
            self.draft = draft;
        }
        let s = &self.draft;
        self.types = s.catch.extensions.join(" ");
        self.skip = s.catch.skip_hosts.join(" ");
        self.once = s.catch.once_hosts.join(" ");
        self.bypass = s.proxy.bypass.join(" ");
        self.custom = s
            .custom_categories
            .iter()
            .map(|c| [c.name.clone(), c.extensions.join(" "), c.sites.join(" ")])
            .collect();
    }
}

/// Every page of Options, with the name it shows.
pub const TABS: [(Tab, &str); 6] = [
    (Tab::General, "General"),
    (Tab::FileTypes, "Browser"),
    (Tab::SaveTo, "Folders"),
    (Tab::Downloads, "Downloads"),
    (Tab::Advanced, "Advanced"),
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

/// The Options dialog: a draft of the settings, edited until OK.
pub struct OptionsDialog {
    /// The page open.
    pub tab: Tab,
    /// The settings as they are being edited.
    pub draft: Settings,
    /// The settings as they were when the dialog opened: OK writes only
    /// what differs from these, so a change made elsewhere meanwhile (View ›
    /// Language, the Scheduler, the columns) is not undone.
    pub opened: Settings,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
    /// File types and skipped hosts, as typed: words separated by spaces,
    /// read into the draft on OK.
    pub types: String,
    /// The hosts to skip, as typed.
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
    /// one was not enough, being egui's discarded sizing pass.
    pub fresh_tab: u8,
    /// Advanced is to open at its proxy section, where a proxy that is
    /// wrong was said (OK on a host that cannot be one).
    pub to_proxy: bool,
    /// Who opens magnet links and `.torrent` files, read beside the window
    /// as File types opens (`reg.exe` takes a moment each); `None` until read.
    pub handlers: Handlers,
    /// Whether that reading was started.
    pub handlers_asked: bool,
}

/// Who opens magnet links and `.torrent` files, once read.
pub type Handlers = Arc<Mutex<Option<(dm_shell::register::Owner, dm_shell::register::Owner)>>>;

/// A list to import, some of whose addresses are already rows.
pub struct ImportAsk {
    /// The addresses of the list, as read.
    pub text: String,
    /// How many of its addresses are already rows, and how many it holds.
    pub already: usize,
    /// How many addresses the list holds.
    pub total: usize,
}

/// The path of a list to import or export, typed where there is no file
/// chooser to ask with.
#[derive(Default)]
pub struct ListFile {
    /// Export (true) or Import.
    pub save: bool,
    /// The path typed.
    pub path: String,
    /// What went wrong, said in the dialog; empty when nothing did.
    pub says: String,
}

/// Every dialog dm can have open, with what it is showing.
#[derive(Default)]
pub struct Dialogs {
    /// The Add URL dialog.
    pub add_url: Option<AddUrl>,
    /// The Download details dialog.
    pub file_info: Option<FileInfo>,
    /// The progress windows open, by download.
    pub progress: BTreeSet<Id>,
    /// Progress windows a person closed: Resume does not open them again.
    pub progress_closed: BTreeSet<Id>,
    /// The Download complete windows open, by download.
    pub complete: BTreeSet<Id>,
    /// "Don't show this window again" ticked in the Download complete
    /// window: kept ticked while it is open and saved when it closes.
    pub complete_never: bool,
    /// Whether each download could resume, as last read while it ran.
    pub resumable_seen: std::collections::HashMap<Id, bool>,
    /// What each progress window has seen of its connections' speeds, to work
    /// out each one's rate (`ConnRates`).
    pub conn_rates: std::collections::HashMap<Id, ConnRates>,
    /// The KB/s typed for a download's own limit in its progress window,
    /// kept while the limit is off so ticking it again brings it back.
    pub own_limits: std::collections::HashMap<Id, u64>,
    /// The Properties dialogs open, by download.
    pub properties: BTreeMap<Id, Properties>,
    /// The Series dialog.
    pub series: Option<Series>,
    /// "Playlist": a YouTube playlist's videos, ticked to download.
    pub playlist: Option<PlaylistDialog>,
    /// The addresses found on the clipboard, each with whether it is ticked.
    pub clip_links: Option<Vec<(String, bool)>>,
    /// The "Download links" import dialog, for a page's links.
    pub links: Option<LinksImport>,
    /// The Site files dialog.
    pub site_files: Option<SiteFiles>,
    /// The download whose address is being replaced, the new address typed and what went wrong.
    pub update_link: Option<(Id, String, String)>,
    /// The rows, the queue picked (as shown) and a new queue's name typed.
    pub move_to_queue: Option<(Vec<Id>, String, String)>,
    /// The Move/Rename dialog.
    pub rename: Option<MoveRename>,
    /// Whether a dialog was in front last frame, and what had the keyboard
    /// before it opened.
    pub was_open: bool,
    /// What had the keyboard before a dialog opened.
    pub focus_before: Option<egui::Id>,
    /// Whether the search box is open.
    pub find_open: bool,
    /// What is typed in the search box.
    pub find_text: String,
    /// The Scheduler being edited: every queue's hours.
    pub scheduler: Option<SchedulerDraft>,
    /// The speed limit, what went wrong, the download limit (MB, hours) and
    /// the slow hours.
    pub limiter: Option<(u64, String, u64, u64, dm_shell::settings::SlowHours)>,
    /// The Options dialog.
    pub options: Option<OptionsDialog>,
    /// Whether About is open.
    pub about_open: bool,
    /// A new install's "Add dm to your browser", with the browsers found.
    pub welcome: Option<Vec<dm_shell::register::Installed>>,
    /// The welcome's download folder, as it is being edited.
    pub welcome_dir: String,
    /// Check for updates as soon as About is drawn.
    pub check_update: bool,
    /// Rows Remove was asked for, by any of its ways: asked about first.
    pub confirm_remove: Option<Vec<Id>>,
    /// The Remove question's "Also delete the file": off each time it asks.
    pub remove_files: bool,
    /// The question is Delete file's: the file goes to the Recycle Bin or
    /// the Trash, and the row with it.
    pub deleting_files: bool,
    /// The Remove question's title as last drawn, in English, for the
    /// control protocol: it depends on what is being removed.
    pub remove_title: &'static str,
    /// Rows the schedule or the download limit holds that a person asked to
    /// start: asked whether to start them now, outside the schedule.
    pub confirm_outside: Option<Vec<Id>>,
    /// The progress window's Cancel download, asked first: it threw away
    /// what was fetched without a word, where every other Cancel closes a
    /// dialog.
    pub confirm_cancel: Option<Id>,
    /// An address typed in that is already a row: the address and the row.
    pub duplicate: Option<(String, Id)>,
    /// The server sent another name than the address's: asked which to save
    /// the file under (reference A asks).
    pub name_ask: Option<NameAsk>,
    /// A web page answered for a file: asked whether to save it as an HTML page.
    pub page_ask: Option<Id>,
    /// A server said the file is not there: told once, with the row it was.
    pub not_found: Option<Id>,
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
    /// The window's close button pressed with no answer kept yet: asked
    /// whether dm keeps running in the tray.
    pub confirm_close: bool,
    /// The dialog, by English title, whose first field takes the keyboard
    /// when it is next drawn: a dialog opened to be typed in is typed in
    /// at once, without a click first.
    pub focus: Option<&'static str>,
}

/// Draws every dialog that is open.
pub fn show(app: &mut DmApp, ctx: &egui::Context) {
    apart::begin_frame(ctx);
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
    let smoke = if tokens.high_contrast {
        // Nothing dims in High Contrast: a grey laid over the window is a
        // contrast the person turned the theme on to be rid of.
        None
    } else if tokens.modern {
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
        if apart::separate(ctx) {
            // A window of its own comes forward by being given the keyboard.
            apart::raise(id);
        } else {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
        }
    }
    app.promote_asks();
    countdown(app, ctx);
    confirm_exit(app, ctx);
    confirm_close(app, ctx);
    links(app, ctx);
    confirm_remove(app, ctx);
    confirm_outside(app, ctx);
    confirm_cancel(app, ctx);
    duplicate(app, ctx);
    name_ask(app, ctx);
    page_ask(app, ctx);
    not_found(app, ctx);
    import_ask(app, ctx);
    list_file(app, ctx);
    skip_offer(app, ctx);
    add_url(app, ctx);
    file_info(app, ctx);
    progress(app, ctx);
    complete(app, ctx);
    properties(app, ctx);
    series(app, ctx);
    playlist::playlist(app, ctx);
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
    welcome(app, ctx);
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
    // The windows of dialogs that were closed with their own close button or
    // Escape, which the dialogs' windows asked for while they were drawn.
    for id in apart::take_closing() {
        app.dialogs.close_window(id);
    }
    app.popup_was_open = ctx.any_popup_open();
    // The topmost of the dialogs in front keeps the keyboard and the
    // clicks: egui takes the highest layer given.
    // A question is kept on top: a progress window opened by the same
    // click covered "Start outside the schedule?".
    if apart::separate(ctx) {
        hold_the_window(app, ctx);
        return;
    }
    for title in app.dialogs.questions() {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, window_id(title)));
    }
    for layer in app.dialogs.modal_layers() {
        ctx.memory_mut(|m| m.set_modal_layer(layer));
    }
}

/// While a dialog that keeps the clicks to itself is open in a window of its
/// own, the main window takes none, nor the keyboard: a layer over all of it
/// is the one egui lets through, and a click on it brings the dialog forward,
/// as a click on the window that owns a dialog does.
fn hold_the_window(app: &DmApp, ctx: &egui::Context) {
    if !app.dialogs.modal() {
        return;
    }
    let room = ctx.content_rect();
    let held = egui::Area::new(egui::Id::unique("dm-dialogs-hold-the-window"))
        .order(egui::Order::Foreground)
        .fixed_pos(room.min)
        .show(ctx, |ui| {
            ui.allocate_exact_size(room.size(), egui::Sense::click_and_drag())
                .1
        });
    ctx.memory_mut(|m| m.set_modal_layer(held.response.layer_id));
    if held.inner.is_pointer_button_down_on() {
        if let Some((window, _)) = apart::windows().last() {
            ctx.send_viewport_cmd_to(*window, egui::ViewportCommand::Focus);
        }
    }
}

#[cfg(test)]
mod stream_name_tests {
    use super::*;

    /// A stream is suggested under the name it will be saved as,
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
