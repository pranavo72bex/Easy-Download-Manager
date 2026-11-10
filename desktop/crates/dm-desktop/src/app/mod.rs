// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window: a menu bar and a toolbar across the top, the category tree
//! down the left, the downloads as a sortable table, and its dialogs — Add
//! URL, Download details, progress, properties, numbered series, files on a
//! site, scheduler, speed limiter, Options.
//!
//! Every action goes through `commands::Command`: the menu bar, toolbar,
//! row menu, keyboard and control protocol all run the same command.

use crate::i18n::{t, tf};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dm_core::{Category, Entry, Id, State, MAIN_QUEUE};
use dm_shell::Settings;
use egui::RichText;

use crate::backend::Backend;
use crate::commands::{Command, Shown};
use crate::dialogs::Dialogs;
use crate::format;
use crate::icons::{self, Icon};
use crate::update::Update;

mod bars;
mod clipboard;
mod commands;
mod control;
mod drops;
mod files;
pub(crate) mod human;
mod links;
mod lists;
mod menus;
mod naming;
mod places;
mod rows;
mod scheduled;
mod search;
mod sync;
mod tree;
mod ui;
mod watch;
mod widgets;

pub use links::*;
pub use rows::*;
use watch::WATCH_EVERY;
pub use widgets::*;

/// How often the list is read back from the queue while the window shows.
const REFRESH: Duration = Duration::from_millis(250);
/// Set whenever the queue changes, cleared by the next read of the list: the
/// application's mark (`dm_shell::app::changes::CHANGED`), which says a read
/// is still owed.
pub use dm_shell::app::changes::CHANGED as QUEUE_CHANGED;

/// Whether to read the list now, and when to look again if not: at once
/// once a quarter of a second has passed since the last read, and, if the
/// queue has changed since, a quarter of a second after it (a little over,
/// so the wake is never early).
pub(crate) fn refresh_plan(
    changed: bool,
    since_read: Option<Duration>,
) -> (bool, Option<Duration>) {
    match since_read {
        None => (true, None),
        Some(since) if since >= REFRESH => (true, None),
        Some(since) if changed => (false, Some(REFRESH - since + Duration::from_millis(5))),
        Some(_) => (false, None),
    }
}

/// The smallest the window may be, on screen at 100% text.
pub const MIN_WINDOW: (f32, f32) = (720.0, 460.0);

/// How often a window with nothing moving wakes of its own accord: a change
/// to the queue wakes it at once (`Queue::on_change`), so this is only for
/// what changes with no one telling (a saved search's timer, a date).
const IDLE_REFRESH: Duration = Duration::from_secs(30);
/// While the clipboard is watched, which nothing can tell it about.
const CLIPBOARD_REFRESH: Duration = Duration::from_secs(1);
/// How often dm wakes hidden in the tray: while something moves, and idle.
const HIDDEN_REFRESH: Duration = Duration::from_secs(1);
const HIDDEN_IDLE: Duration = Duration::from_secs(30);
/// How often the clipboard is looked at, when watching it is on.
const CLIPBOARD: Duration = Duration::from_secs(1);

/// What the rows the table shows were made from.
#[derive(Debug, Clone, PartialEq)]
struct VisibleKey {
    /// The generation of the list (`DmApp::rows_gen`).
    list: u64,
    place: Place,
    filter: String,
    sort: Option<(Column, bool)>,
    /// How many sizes a dialog has told, which the rows show until they know
    /// their own.
    sizes_told: usize,
}

/// A place in the category tree: a filter over the one list
/// (`dm_shell::app::places`).
pub use dm_shell::app::places::Place;

/// A column of the download table, in its default order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    /// The file's name.
    Name,
    /// The queue the download is in.
    Queue,
    /// The size.
    Size,
    /// The status.
    Status,
    /// The time left.
    TimeLeft,
    /// The transfer rate.
    Rate,
    /// The time of the last try.
    LastTry,
    /// The description.
    Description,
    /// The page a browser handed the download over from.
    Referrer,
    /// Where the finished file is, or where it will go.
    Folder,
}

impl Column {
    /// In the order a new window shows them, reference A's: File Name, Q
    /// (narrow), Size, Status, Time left, Transfer rate, Last Try Date and
    /// Description (decisions.md, "dm works exactly like reference A").
    pub const ALL: [Column; 10] = [
        Column::Name,
        Column::Queue,
        Column::Size,
        Column::Status,
        Column::TimeLeft,
        Column::Rate,
        Column::LastTry,
        Column::Description,
        Column::Referrer,
        Column::Folder,
    ];

    /// What the settings file calls it.
    pub fn key(self) -> &'static str {
        match self {
            Column::Name => "name",
            Column::Queue => "queue",
            Column::Size => "size",
            Column::Status => "status",
            Column::TimeLeft => "time_left",
            Column::Rate => "rate",
            Column::LastTry => "last_try",
            Column::Description => "description",
            Column::Referrer => "referrer",
            Column::Folder => "folder",
        }
    }

    /// The column saved under `key`, if there is one.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|c| c.key() == key)
    }

    /// Every column in the order a person chose, the file name first and
    /// any the list does not mention after, in their usual place.
    pub fn ordered(keys: &[String]) -> Vec<Column> {
        let mut order = vec![Column::Name];
        for column in keys.iter().filter_map(|k| Column::from_key(k)) {
            if !order.contains(&column) {
                order.push(column);
            }
        }
        for column in Column::ALL {
            if !order.contains(&column) {
                order.push(column);
            }
        }
        order
    }

    /// Its heading, in the language in force.
    pub fn title(self) -> &'static str {
        t(self.english())
    }

    /// Its heading in English, which the catalogs are keyed by.
    pub fn english(self) -> &'static str {
        match self {
            Column::Name => "File Name",
            // Its heading draws the queue's mark; this is its name in the
            // columns menu and for a screen reader.
            Column::Queue => "Queue",
            Column::Size => "Size",
            Column::Status => "Status",
            Column::TimeLeft => "Time left",
            Column::Rate => "Transfer rate",
            Column::LastTry => "Last Try Date",
            Column::Description => "Description",
            Column::Referrer => "Referrer",
            Column::Folder => "Folder",
        }
    }
}

/// Whether the pointer is in `rect`.
fn at_inside(ctx: &egui::Context, rect: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.interact_pos())
        .is_some_and(|at| rect.contains(at))
}

/// A site search's name and what it found, or why it could not.
type SearchResult = (String, Result<dm_shell::lists::Crawled, String>);

/// Where each browser gets dm's extension: its store's page where it is
/// listed, otherwise the README's steps for loading it.
pub const EXTENSION_EDGE: &str =
    "https://microsoftedge.microsoft.com/addons/detail/ooafpfjpliofcdlnbdkjhonaoebcpnej";
/// Where the add-on is listed for Firefox.
pub const EXTENSION_FIREFOX: &str = "https://addons.mozilla.org/firefox/addon/dm-download-manager/";
/// Where the add-on is listed for Chrome.
pub const EXTENSION_CHROME: &str =
    "https://github.com/pranavo72bex/Easy-Download-Manager#chrome-brave-vivaldi-and-opera";

/// The file under dm's home that says the person asked not to be offered
/// the extension again.
const BROWSER_HINT_HIDDEN: &str = "browser-hint.hidden";

/// Written once the welcome's "Add dm to your browser" has been shown.
const WELCOMED: &str = "welcomed";

/// What a drop with no link in it is answered with, and what the empty list
/// says. A link dragged from a browser is taken everywhere: on macOS and
/// Windows by `drops`, on X11 and Wayland by winit as dm builds it.
pub const DROP_HINT: &str = "Drop a link, .torrent, .url, .webloc or text file of links here.";

/// The drop hint: the same everywhere, Wayland included since dm's winit
/// takes a browser's link there too.
pub fn drop_hint() -> &'static str {
    DROP_HINT
}

/// The clipboard watcher's memory: the application's (`dm_shell::app::links`).
pub use dm_shell::app::links::Clipboard;

/// The rows the table shows, shared between frames and with the queue.
pub type SharedRows = std::rc::Rc<Vec<Arc<Entry>>>;

/// The window's state: the list, the selection, the dialogs and the settings in force.
pub struct DmApp {
    /// The engine the window works through.
    pub backend: Backend,
    /// The list as last read, each row shared with the queue: a row that
    /// did not change since the read before is the same allocation.
    pub rows: Vec<Arc<Entry>>,
    refreshed: Option<Instant>,
    /// The application's memory between reads (`dm_shell::app::session`):
    /// what was told (finishes, failures, rows started elsewhere, the rows the
    /// window added itself) and the watched folder's.
    pub session: dm_shell::app::session::Session,

    /// The place in the category tree the list is filtered to.
    pub place: Place,
    /// The rows selected.
    pub selected: BTreeSet<Id>,
    /// The row a shift-click extends the selection from: the last one
    /// clicked without shift.
    pub anchor: Option<Id>,
    /// The row the arrow keys move from: the last one moved to or clicked.
    pub cursor: Option<Id>,
    /// A row to bring into view on the next frame, as the keys moved to it.
    pub scroll_to: Option<Id>,
    /// Whether the keyboard, not the pointer, was used last: the focus is
    /// shown then, with a ring.
    pub keyboard: bool,
    /// Where the focus ring was drawn last frame, if anywhere.
    pub focus_ring: Option<egui::Rect>,
    /// The column sorted by and whether the order is ascending.
    pub sort: Option<(Column, bool)>,
    /// Whether the category list is shown.
    pub show_categories: bool,
    /// Whether the toolbar is shown.
    pub show_toolbar: bool,
    /// Columns turned off under View › Columns. The file name always shows.
    pub hidden_columns: HashSet<Column>,
    /// Every column, in the order the list draws them.
    pub column_order: Vec<Column>,

    /// Whether to offer the browser extension, until one has connected or
    /// the person said not now: looked up every few seconds, since the
    /// browser's side writes it (`host::browser_seen`).
    pub browser_hint: (Option<Instant>, bool),
    /// The row fetching dm's own update, from About, until it is installed
    /// or put off: once whole and checked, the window offers to install it.
    pub update_row: Option<Id>,
    /// The window was asked to come up from inside it (the floating drop
    /// target's Show dm), for the program around it to do.
    pub show_asked: bool,
    /// The window is to go to the tray (the close question's answer).
    pub hide_asked: bool,
    /// Whether a new install's welcome has been looked into this run.
    welcome_checked: bool,

    /// Commands asked for while drawing, run once the frame is drawn.
    pub queued: Vec<Command>,
    /// The queue a Start Queue or Stop Queue drop-down chose, for the next
    /// of those commands in place of the queue the list shows.
    pub queue_chosen: Option<String>,
    /// Finished files a drag out of the list has asked to carry, for the
    /// window to hand to the system's drag: only it has the native handle.
    pub drag_out: Vec<std::path::PathBuf>,
    /// A row being dragged to another place in the queue's order.
    pub reordering: Option<Id>,
    /// Where each row was drawn last frame, in window points, so the control
    /// protocol can tell a test where to point.
    pub row_rects: HashMap<Id, egui::Rect>,
    /// What of the list's rows could be seen last frame, in window points.
    pub list_clip: Option<egui::Rect>,
    /// Where the list's up-and-down scroll bar is drawn: at the list's
    /// visible right edge, whatever is scrolled sideways.
    pub list_bar: Option<egui::Rect>,
    /// A place that bar was dragged or wheeled to, for the list to take.
    pub list_bar_moved: Option<f32>,
    /// Whether the list has been drawn since launch: it starts at its
    /// left edge, with Name in view, whatever egui remembered.
    pub list_drawn: bool,
    /// How many rows Page Up and Page Down move: a screenful, less one.
    pub list_page: usize,
    /// The dialogs that are open.
    pub dialogs: Dialogs,

    /// Problems found at start-up, said in the window.
    pub problems: Vec<String>,

    /// The settings as last applied to the window.
    pub applied: Settings,
    /// The clipboard watcher's memory.
    pub clipboard: Clipboard,
    /// What the update check has found.
    pub update: Arc<Mutex<Update>>,
    /// A weekly check is waiting for its answer, to open About on a newer dm.
    weekly_pending: bool,
    /// The speed limit to go back to when the limiter is turned on again.
    last_limit: u64,
    /// Finished downloads just taken off the list, with when, for Undo.
    pub undo_remove: dm_shell::app::backend::Undo,
    /// A word in the status bar about the last thing done.
    pub said: crate::i18n::Said,
    /// What Copy link asked to put on the clipboard, for the next frame.
    pub copied: Option<String>,
    /// Whether the look drawn last was dark, for the control protocol.
    pub dark: bool,
    /// Whether a quit was asked for (by the control port or when all downloads finished): the window closes.
    pub quit_asked: bool,
    /// The look last put in force, so a change in Options is applied once.
    theme_in_force: Option<crate::theme::Theme>,
    /// The computer's accent last put in force (`accent.rs`).
    accent_in_force: Option<Option<crate::accent::Palette>>,
    /// Whether the window's motion was last set to none (`motion.rs`).
    motion_in_force: Option<bool>,
    fonts_ready: bool,
    /// Words typed over the list: only rows whose name, address or note
    /// holds them all are shown, in whatever place is chosen.
    pub filter: String,
    /// Questions the rows raised (the server's name, a page for a file, a 404),
    /// waiting for the dialogs to be free: one is asked at a time.
    asks: dm_shell::app::questions::Questions,
    /// Names to give a file once it has finished: the address's, where a person
    /// said no to the server's while it was still coming.
    pending_renames: dm_shell::app::names::PendingRenames,
    /// What saved site searches found, by name, waiting to be added.
    search_results: Arc<std::sync::Mutex<Vec<SearchResult>>>,
    /// How many searches are out, so the window keeps drawing to take
    /// their results.
    pub searches_running: usize,
    /// When the saved searches were last looked at for one that is due.
    searches_checked: Option<Instant>,
    /// What a recheck running in the background found, when it is done.
    pub rechecked: Arc<std::sync::Mutex<Option<(usize, usize)>>>,
    /// Why the system could not open a file or folder, said when it is known.
    opener_said: Arc<std::sync::Mutex<Option<String>>>,
    /// What hanging up the dial-up connection came to when the scheduled
    /// downloads were done (Scheduler › Hang up modem when done), said when
    /// it is known.
    hung_up: Arc<std::sync::Mutex<Option<dm_shell::dialup::HungUp>>>,
    /// What had the keyboard when a dialog came in front, given it back
    /// when none is: the window behind takes no clicks meanwhile, and a
    /// control that cannot be used lets the keyboard go.
    focus_before: Option<egui::Id>,
    was_behind: bool,
    /// Whether a menu, combo box or right-click menu was open at the end
    /// of the last frame: an Escape this frame was theirs, not a dialog's.
    pub popup_was_open: bool,
    /// Handed-over downloads cancelled, by site: at two, the site is
    /// offered to the browser.
    cancelled_hand_offs: dm_shell::app::handoff::Cancels,
    /// The failed downloads the failed bar was hidden over: it stays away
    /// until another one fails.
    failed_hidden: BTreeSet<Id>,
    /// Running without a window (`--headless`).
    pub headless: bool,
    /// The window is hidden in the tray: wakes are fewer.
    pub hidden: bool,
    /// Requests from the control protocol, when it is on.
    control: Option<std::sync::mpsc::Receiver<crate::control::Request>>,
    /// The words drawn on the last frame, for the control protocol's `ui.click`.
    screen: Vec<human::ScreenText>,
    /// Pointer and key events to give the window, a step a frame.
    synthetic: std::collections::VecDeque<(Instant, human::Step)>,
    /// Painted passes so far, and the one the last step went to.
    passes: u64,
    delivered_at_pass: Option<u64>,
    delivered_at: Option<Instant>,
    /// When a control client last clicked, typed or pressed a key: what is
    /// done within moments of it is a script's, not a person's.
    scripted_at: dm_shell::app::control_api::ScriptGuard,
    /// The filter is in the top row's search box this frame.
    title_search: bool,
    /// The control port answers AI assistants only (see `main.rs`).
    mcp_only: bool,
    /// The rows the last `add` made, in order.
    pub last_added: Vec<Id>,
    /// A sentence about one row ("Trying … again with the sign-in."),
    /// cleared once that row has run and stopped, and whether it
    /// has been seen running yet.
    pub said_about: Option<(Id, crate::i18n::Said, bool)>,
    /// Sizes a dialog learned from the server before a row was added for
    /// later, shown until the row learns its own.
    pub sizes_told: HashMap<Id, u64>,
    /// Each row's place, the places' counts and the filter's matching
    /// (`dm_shell::app::places`), kept from read to read.
    places: dm_shell::app::places::Places,
    /// Counts up whenever `rows` or what places them is made again; what is
    /// worked out from the rows is kept for as long as it stands still.
    rows_gen: u64,
    /// The rows the table shows, in order, and what they were made from
    /// (`DmApp::visible_shared`): a frame with no new read of the list
    /// reuses them instead of copying and sorting every row again.
    visible_cache: std::cell::RefCell<Option<(VisibleKey, SharedRows)>>,
    /// The queues the list names, for the generation they were read at
    /// ([`DmApp::queues`]).
    queues_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<[String]>)>>,
    /// The language the places were last drawn in.
    rail_language: Option<String>,
    /// Columns were moved, shown or hidden since the list was drawn.
    pub columns_changed: bool,
    /// F10 was pressed: the first menu opens on the next drawing of the bar.
    pub open_menu_by_key: bool,
    /// A text field's right-click menu: the field and where it opened.
    field_menu: Option<(egui::Id, egui::Pos2)>,
    /// What that menu chose, given to the field on the next frame.
    field_edits: Vec<egui::Event>,
    /// The focused field's selection before a right-click moved its caret.
    field_selection: Option<(egui::Id, egui::text::CCursorRange)>,
    /// The menu bar's menus, by the ids their popups had last frame.
    menus: Vec<egui::Id>,
    /// Shift+F10 or the Menu key was pressed: the row with the keyboard's
    /// place opens its menu as it is drawn.
    pub row_menu_by_key: bool,
    /// The text size the smallest window was last set for.
    min_size_zoom: Option<f32>,
    /// Where the menu bar ends, last frame: windows per download open
    /// below it, never over it.
    pub menu_bottom: f32,
    /// Where the toolbar ends (the menu bar's bottom with no toolbar):
    /// windows a download opens on its own start below it, not over Add URL.
    pub bars_bottom: f32,
    /// Each finished file's size on disk (`snapshot::SavedSizes`).
    pub saved: dm_shell::app::snapshot::SavedSizes,
    /// The status line's sentence and when it was said: an old one is not
    /// left standing beside rows it no longer describes.
    said_at: Option<(crate::i18n::Said, Instant)>,
}

/// How long the status line keeps its last sentence.
// Long enough to read; not standing over what was done since.
const SAID_FOR: Duration = Duration::from_secs(12);
/// How long Undo is offered after a download is taken off the list.
use dm_shell::app::backend::UNDO_FOR;

impl DmApp {
    /// Creates the window's state over `backend`, reading the settings in force.
    pub fn new(backend: Backend) -> Self {
        let settings = backend.settings();
        crate::i18n::set(crate::i18n::chosen(&settings.language));
        let mut app = DmApp {
            backend,
            rows: Vec::new(),
            refreshed: None,
            session: Default::default(),
            place: Place::All,
            selected: BTreeSet::new(),
            anchor: None,
            cursor: None,
            scroll_to: None,
            keyboard: false,
            focus_ring: None,
            sort: None,
            show_categories: true,
            show_toolbar: true,
            hidden_columns: settings
                .columns_hidden
                .iter()
                .filter_map(|k| Column::from_key(k))
                .filter(|c| *c != Column::Name)
                .collect(),
            column_order: Column::ordered(&settings.column_order),
            queued: Vec::new(),
            queue_chosen: None,
            drag_out: Vec::new(),
            reordering: None,
            row_rects: HashMap::new(),
            list_clip: None,
            list_bar: None,
            list_bar_moved: None,
            list_drawn: false,
            list_page: 1,
            dialogs: Dialogs::default(),
            problems: Vec::new(),
            last_limit: dm_shell::app::commands::remembered_limit(&settings),
            applied: settings,
            clipboard: Clipboard::default(),
            browser_hint: (None, false),
            update_row: None,
            undo_remove: Default::default(),
            show_asked: false,
            hide_asked: false,
            welcome_checked: false,
            update: Arc::new(Mutex::new(Update::NotAsked)),
            weekly_pending: false,
            said: Default::default(),
            copied: None,
            dark: false,
            quit_asked: false,
            theme_in_force: None,
            motion_in_force: None,
            accent_in_force: None,
            fonts_ready: false,
            asks: Default::default(),
            pending_renames: Default::default(),
            search_results: Arc::default(),
            filter: String::new(),
            searches_running: 0,
            searches_checked: None,
            popup_was_open: false,
            rechecked: Arc::default(),
            opener_said: Arc::default(),
            hung_up: Arc::default(),
            focus_before: None,
            was_behind: false,
            cancelled_hand_offs: Default::default(),
            failed_hidden: BTreeSet::new(),
            headless: false,
            hidden: false,
            control: None,
            screen: Vec::new(),
            synthetic: Default::default(),
            passes: 0,
            delivered_at_pass: None,
            delivered_at: None,
            scripted_at: Default::default(),
            title_search: false,
            mcp_only: false,
            last_added: Vec::new(),
            said_about: None,
            sizes_told: HashMap::new(),
            rows_gen: 0,
            places: Default::default(),
            visible_cache: Default::default(),
            queues_cache: Default::default(),
            rail_language: None,
            columns_changed: false,
            min_size_zoom: None,
            open_menu_by_key: false,
            menus: Vec::new(),
            field_menu: None,
            field_edits: Vec::new(),
            field_selection: None,
            row_menu_by_key: false,
            menu_bottom: 0.0,
            bars_bottom: 0.0,
            saved: Default::default(),
            said_at: None,
        };
        app.sort = Column::from_key(&app.applied.sort_column)
            .map(|column| (column, app.applied.sort_ascending));
        app.set_clipboard_watch(app.applied.watch_clipboard);
        app.refresh();
        app
    }

    /// The engine the window works through.
    #[must_use]
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// A problem for the list above the downloads, once: one that recurs
    /// each minute is not a new line each minute.
    pub fn problem(&mut self, text: String) {
        if !self.problems.contains(&text) {
            self.problems.push(text);
        }
    }

    /// The queues, as the queue lists them, worked out from the list as
    /// last read: once a read, and with no lock on the queue. The side
    /// rail, the tree and the dialogs ask several times a frame, so the
    /// list is shared, not copied: each ask costs a count, not every name.
    pub fn queues(&self) -> std::rc::Rc<[String]> {
        let mut cache = self.queues_cache.borrow_mut();
        match &*cache {
            Some((generation, names)) if *generation == self.rows_gen => std::rc::Rc::clone(names),
            _ => {
                let names: std::rc::Rc<[String]> =
                    dm_core::queue_names(self.rows.iter().map(|r| &**r)).into();
                *cache = Some((self.rows_gen, std::rc::Rc::clone(&names)));
                names
            }
        }
    }

    /// The rows as last read.
    #[must_use]
    pub fn rows(&self) -> &[Arc<Entry>] {
        &self.rows
    }

    /// The row with this id, if it is in the list.
    #[must_use]
    pub fn row(&self, id: Id) -> Option<&Entry> {
        self.rows.iter().find(|r| r.id == id).map(|r| &**r)
    }
}

/// A problem from outside the window, in the language in force where the
/// window knows its words: a saved proxy turned off at launch is said in
/// the language the person reads. Others are shown as written.
pub fn worded_problem(problem: &str) -> String {
    let proxy = problem
        .strip_prefix("the saved proxy could not be used (")
        .and_then(|rest| rest.split_once("), so it is turned off"));
    match proxy {
        Some((why, _)) => crate::i18n::tf(
            "The proxy can't be used ({}) and is off. Fix it in Options › Advanced.",
            &[&why],
        ),
        None => problem.to_string(),
    }
}

/// A problem as it is shown on its own: begun with a capital. Problems are
/// worded to sit inside a sentence ("the cookies file … could not be
/// read"), and alone one read as cut off.
pub fn as_shown(problem: &str) -> String {
    let mut chars = problem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What a right-click on a place in the category tree can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeAction {
    /// Runs this command.
    Run(Command),
    /// Options › Folders, where categories are added, edited and deleted.
    EditCategories,
    /// The folder a category's files go to.
    OpenFolder,
}

/// The toolbar's looks, and what View › Customize toolbar calls them.
const TOOL_STYLES: [(ToolStyle, &str); 3] = [
    (ToolStyle::Both, "Icons and text"),
    (ToolStyle::Icons, "Small buttons"),
    (ToolStyle::Text, "Text only"),
];

/// Every English string the window puts through `t` that is not written in
/// the call itself but chosen at run time: choices from lists, a place's
/// menu, the countdown's verb, the names dm gives. For the tests to hold
/// each to be in every catalog, which a scan of the source cannot see.
pub fn run_time_words() -> Vec<&'static str> {
    let mut words: Vec<&'static str> = Vec::new();
    words.extend(crate::dialogs::TABS.iter().map(|(_, w)| *w));
    words.extend(crate::dialogs::THEMES.iter().map(|(_, w)| *w));
    words.extend(crate::dialogs::TEXT_SIZES.iter().map(|(w, _)| *w));
    words.extend(crate::dialogs::QUALITIES.iter().map(|(_, w)| *w));
    words.extend(crate::dialogs::DAYS);
    words.extend(TOOL_STYLES.iter().map(|(_, w)| *w));
    words.extend(dm_shell::app::commands::TOOLBAR.iter().map(|(_, w)| *w));
    words.extend(Command::ALL.iter().map(|c| c.label()));
    words.extend(Column::ALL.iter().map(|c| c.english()));
    words.extend(crate::power::WhenDone::ALL.iter().map(|w| w.verb()));
    words.extend(crate::i18n::BUILT_IN_NAMES);
    words.extend(crate::format::MONTHS);
    words.push(DROP_HINT);
    words.extend(format::REASONS.iter().map(|(_, plain)| *plain));
    words.extend(format::NEXT_STEPS.iter().map(|(_, step)| *step));
    words.extend(format::RETRY_CAUSES);
    for place in [
        Place::All,
        Place::Unfinished,
        Place::Finished,
        Place::Queue(String::new()),
    ] {
        words.extend(tree_menu(&place).into_iter().map(|(w, _)| w));
    }
    words
}

/// The right-click menu for a place in the tree.
pub fn tree_menu(place: &Place) -> Vec<(&'static str, TreeAction)> {
    match place {
        Place::All | Place::Kind(_) | Place::Custom(_) => vec![
            ("Open folder", TreeAction::OpenFolder),
            ("Add or edit categories…", TreeAction::EditCategories),
        ],
        Place::Unfinished => vec![
            ("Resume all", TreeAction::Run(Command::ResumeAll)),
            ("Stop All", TreeAction::Run(Command::StopAll)),
        ],
        Place::Finished => vec![(
            "Delete Completed",
            TreeAction::Run(Command::DeleteCompleted),
        )],
        Place::Queue(_) => vec![
            ("Start Queue", TreeAction::Run(Command::StartQueue)),
            ("Stop Queue", TreeAction::Run(Command::StopQueue)),
            ("Scheduler…", TreeAction::Run(Command::Scheduler)),
        ],
    }
}

/// When a closed schedule next opens, as a person reads it: "01:00" later
/// today, "Mon 01:00" on another day. `None` if it never does.
pub fn opens_next(schedule: &dm_core::Schedule) -> Option<String> {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let today = now.weekday().num_days_from_sunday();
    let minute = now.time().hour() * 60 + now.time().minute();
    opens_after(schedule, today, minute).map(|(days, at)| {
        let clock = format!("{:02}:{:02}", at / 60, at % 60);
        if days == 0 {
            clock
        } else {
            let day = crate::dialogs::DAYS
                .get(((today + days) % 7) as usize)
                .copied()
                .unwrap_or_default();
            format!("{} {clock}", t(day))
        }
    })
}

/// The first minute after `minute` of weekday `today` that the schedule is
/// open: how many days on, and the minute of that day.
pub fn opens_after(schedule: &dm_core::Schedule, today: u32, minute: u32) -> Option<(u32, u32)> {
    (1..=8 * 1440).map(|step| minute + step).find_map(|at| {
        let (days, at) = (at / 1440, at % 1440);
        let weekday = ((today + days) % 7) as u8;
        schedule.open_at(weekday, at).then_some((days, at))
    })
}

/// Writes what egui captured as a PNG.
fn save_png(image: &egui::ColorImage, path: &std::path::Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let [w, h] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    image::RgbaImage::from_raw(w as u32, h as u32, bytes)
        .ok_or("the screenshot had no pixels")?
        .save(path)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod superseded_tests {
    use super::*;

    /// A row whose file another, later row wrote again is
    /// gone, not Complete; the later one is not.
    #[test]
    fn a_file_written_again_by_a_later_row_is_that_rows() {
        let dir = std::env::temp_dir().join(format!("dm-superseded-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("same.bin");
        std::fs::write(&file, b"x").unwrap();
        let row = |id: Id, at: u64| {
            let mut row = rows::status_tests::paused(false);
            row.id = id;
            row.state = State::Completed;
            row.path = file.display().to_string();
            row.finished_at = Some(at);
            row
        };
        let (old, new) = (row(1, 10), row(2, 20));
        crate::table::set_superseded(&[old.clone(), new.clone()]);
        assert!(crate::table::gone(&old));
        assert!(!crate::table::gone(&new));
        // Finished in the same second: the later row's.
        let twin = row(3, 20);
        crate::table::set_superseded(&[new.clone(), twin.clone()]);
        assert!(crate::table::gone(&new) && !crate::table::gone(&twin));
        crate::table::set_superseded(std::slice::from_ref(&old));
        assert!(!crate::table::gone(&old), "alone, its file is its own");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod disk_full_tests {
    use super::*;

    /// A video page that failed says why after the sentence that was all a
    /// person had; the specific failures keep their own words.
    #[test]
    fn a_video_page_that_failed_says_why_after_the_sentence() {
        let mut row = rows::status_tests::paused(false);
        row.state = State::Failed("no stream at that quality".into());
        assert_eq!(
            status_in_english(&row),
            "Error: dm couldn't get the video from this page (no stream at that quality)"
        );
        // A failure with its own plain words is not given a second one.
        row.state = State::Failed("YouTube says: Video unavailable".into());
        assert_eq!(
            status_in_english(&row),
            "Error: this video isn't available. It may be private, removed, or not shown in your country"
        );
    }

    /// A full disk says whose, the folder the file was going to.
    #[test]
    fn a_full_disk_says_which_folder() {
        let mut row = rows::status_tests::paused(false);
        row.state = State::Failed("disk: No space left on device (os error 28)".into());
        row.began_in = "/media/usb/Downloads".into();
        assert_eq!(
            status_in_english(&row),
            "Error: the disk is full in /media/usb/Downloads"
        );
        row.save_dir = Some("/mnt/big".into());
        assert_eq!(
            status_in_english(&row),
            "Error: the disk is full in /mnt/big"
        );
        row.save_dir = None;
        row.began_in.clear();
        assert_eq!(status_in_english(&row), "Error: the disk is full");
    }
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    /// Never read yet, or a quarter of a second since: read now.
    #[test]
    fn the_list_is_read_when_it_is_due() {
        assert_eq!(refresh_plan(false, None), (true, None));
        assert_eq!(refresh_plan(false, Some(REFRESH)), (true, None));
        assert_eq!(refresh_plan(true, Some(MS(900))), (true, None));
    }

    /// Soon after a read, with nothing changed: left alone, and no extra
    /// wake asked for (the idle wait stands).
    #[test]
    fn a_recent_read_with_no_change_is_left_alone() {
        assert_eq!(refresh_plan(false, Some(MS(40))), (false, None));
    }

    /// A change made just after a read is not lost to the long idle wait:
    /// the next look is asked for a little over a quarter of a second after
    /// the read, which is when it may be taken. (Before, a resumed download
    /// showed as stopped until it finished.)
    #[test]
    fn a_change_just_after_a_read_is_looked_at_again_when_it_may_be() {
        let (now, again) = refresh_plan(true, Some(MS(13)));
        assert!(!now);
        let again = again.expect("a change must ask to be looked at");
        assert!(
            again > REFRESH - MS(13) && again <= REFRESH - MS(13) + MS(10),
            "{again:?}"
        );
        // At that moment it is due.
        assert!(refresh_plan(true, Some(MS(13) + again)).0);
    }
}
