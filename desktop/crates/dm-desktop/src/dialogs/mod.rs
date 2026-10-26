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
mod complete;
mod confirm;
mod controls;
mod file_info;
mod frame;
mod links;
mod options;
mod progress;
mod properties;
mod row_dialogs;
mod series;
mod site_files;
mod speed_schedule;
mod state;

use about::*;
pub use add::*;
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
    pub ids: Vec<Id>,
    pub name: String,
    pub folder: String,
    /// For several files: how to name them. Empty keeps their names;
    /// `{n}` counts from 1 in the order they are listed, `{name}` is the
    /// old name without its ending, and the ending is kept.
    pub pattern: String,
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
    /// A separate sound track for the first address, fetched and joined with it.
    pub sound: Option<String>,
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
    /// this opening of the dialog only (it stayed for the next).
    pub no_chooser: bool,
    /// The name the server gives the file, once asked, and
    /// whether a person typed in Save as since, which it then leaves alone.
    pub probed: Option<std::sync::Arc<std::sync::Mutex<Option<Probed>>>>,
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
    pub name: Option<String>,
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
    /// can tick one again (they were added and failed).
    pub refused: std::collections::HashSet<String>,
    pub hide_html: bool,
    /// File types to list alone, as typed ("zip pdf"); empty lists all.
    pub types: String,
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

    /// Whether the filters leave `url` in the list.
    pub fn shown(&self, url: &str) -> bool {
        if self.hide_html && self.is_html(url) {
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

    /// The links OK would add: checked, and shown.
    pub fn chosen(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|(url, on)| *on && self.shown(url))
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
    /// Digests of the finished file, worked out in the background, by the
    /// algorithm's name; `None` while one is still being worked out.
    pub sums: Sums,
    /// A digest the person was given, to check the file against.
    pub check: String,
}

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
    /// The links found, each with whether it is ticked; how many pages were
    /// read; and whether the run's time ran out.
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
    /// one was not enough, being egui's discarded sizing pass.
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
    /// "Don't show this window again" ticked in the Download complete
    /// window: kept ticked while it is open and saved when it closes.
    pub complete_never: bool,
    /// Whether each download could resume, as last read while it ran.
    pub resumable_seen: std::collections::HashMap<Id, bool>,
    /// The KB/s typed for a download's own limit in its progress window,
    /// kept while the limit is off so ticking it again brings it back.
    pub own_limits: std::collections::HashMap<Id, u64>,
    pub properties: BTreeMap<Id, Properties>,
    pub series: Option<Series>,
    pub clip_links: Option<Vec<(String, bool)>>,
    /// The "Download links" import dialog, for a page's links.
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
    /// The speed limit, what went wrong, and the download limit (MB, hours).
    pub limiter: Option<(u64, String, u64, u64)>,
    pub options: Option<OptionsDialog>,
    pub about_open: bool,
    /// A new install's "Add dm to your browser", with the browsers found.
    pub welcome: Option<Vec<dm_shell::register::Installed>>,
    /// Check for updates as soon as About is drawn.
    pub check_update: bool,
    /// Rows Remove was asked for, by any of its ways: asked about first.
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
    /// dialog.
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
    app.popup_was_open = ctx.any_popup_open();
    // The topmost of the dialogs in front keeps the keyboard and the
    // clicks: egui takes the highest layer given.
    // A question is kept on top: a progress window opened by the same
    // click covered "Start outside the schedule?".
    for title in app.dialogs.questions() {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, window_id(title)));
    }
    for layer in app.dialogs.modal_layers() {
        ctx.memory_mut(|m| m.set_modal_layer(layer));
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
