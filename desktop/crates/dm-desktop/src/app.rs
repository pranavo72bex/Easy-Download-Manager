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
use egui::{Color32, RichText};

use crate::backend::Backend;
use crate::commands::Command;
use crate::dialogs::Dialogs;
use crate::format;
use crate::icons::{self, Icon};
use crate::update::Update;

/// How often the list is read back from the queue while the window shows.
const REFRESH: Duration = Duration::from_millis(250);
/// How often the clipboard is looked at, when watching it is on.
const CLIPBOARD: Duration = Duration::from_secs(1);

/// A place in the category tree: a filter over the one list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    All,
    Kind(Category),
    /// A category a person made, by name.
    Custom(String),
    Unfinished,
    Finished,
    Queue(String),
}

/// A column of the download table, in its default order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    Name,
    Queue,
    Size,
    Status,
    TimeLeft,
    Rate,
    LastTry,
    Description,
    /// The page a browser handed the download over from.
    Referrer,
    /// Where the finished file is, or where it will go.
    Folder,
}

impl Column {
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
            Column::Name => "Name",
            Column::Queue => "Queue",
            Column::Size => "Size",
            Column::Status => "Status",
            Column::TimeLeft => "Time left",
            Column::Rate => "Speed",
            Column::LastTry => "Date",
            Column::Description => "Note",
            Column::Referrer => "Referrer",
            Column::Folder => "Folder",
        }
    }
}

/// A state's own word, as the wire spells it.
pub fn kind(state: &State) -> &'static str {
    match state {
        State::Queued => "queued",
        State::Running => "running",
        State::Paused => "paused",
        State::Completed => "completed",
        State::Failed(_) => "failed",
        State::Cancelled => "cancelled",
    }
}

pub fn category_word(c: Category) -> String {
    serde_json::to_value(c)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The name a row is shown by: the file's, or a guess at it until the
/// server says.
pub fn name_of(row: &Entry) -> String {
    if row.filename.is_empty() {
        format::provisional(&row.url)
    } else {
        row.filename.clone()
    }
}

/// What the Status column says.
/// A site search's name and what it found, or why it could not.
type SearchResult = (String, Result<dm_shell::lists::Crawled, String>);

pub fn status_of(row: &Entry) -> String {
    status_in(row, t)
}

/// The folder a row's file is in, once finished, or the one it was asked
/// to go to; empty where that is the download folder's own choice.
pub fn folder_of(row: &Entry) -> String {
    if !row.path.is_empty() {
        return std::path::Path::new(&row.path)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
    }
    row.save_dir.clone().unwrap_or_default()
}

/// The same in English, whatever the window's language: what the control
/// protocol and MCP report, so a script or an agent reads the same words on
/// every machine.
pub fn status_in_english(row: &Entry) -> String {
    status_in(row, |english| english)
}

fn status_in(row: &Entry, words: fn(&'static str) -> &'static str) -> String {
    match &row.state {
        State::Running => match row.progress.fraction() {
            Some(f) => format!("{:.2}%", f * 100.0),
            // A length the server did not say: what has arrived, and how
            // fast, rather than "Starting…" for the whole run.
            None if row.progress.downloaded > 0 => {
                if row.progress.speed > 1.0 {
                    format!(
                        "{} · {}",
                        format::bytes(row.progress.downloaded),
                        format::rate(row.progress.speed)
                    )
                } else {
                    format::bytes(row.progress.downloaded)
                }
            }
            None => words("Starting…").into(),
        },
        State::Queued => words("Queued").into(),
        // Held by the schedule or the download limit, not by a person: it
        // goes on by itself, which "Stopped" would not say.
        State::Paused if row.paused_by_schedule => words("Waiting").into(),
        // How far it got, so a stopped row is not mistaken for one that
        // never started.
        State::Paused => match row.progress.fraction() {
            Some(f) if f > 0.0 => format!("{} · {:.0}%", words("Stopped"), f * 100.0),
            _ => words("Stopped").into(),
        },
        State::Completed => words("Complete").into(),
        // A server that wants a sign-in says where to give one.
        State::Failed(reason) if needs_sign_in(reason) => format!(
            "{} — {}",
            crate::i18n::fill(words("Error: {}"), &[reason]),
            words("it needs a sign-in: Properties › Login")
        ),
        State::Failed(reason) => crate::i18n::fill(words("Error: {}"), &[reason]),
        State::Cancelled => words("Cancelled").into(),
    }
}

/// Whether a failure is a server refusing without a sign-in (401, 403).
pub fn needs_sign_in(reason: &str) -> bool {
    [" 401 ", " 403 "]
        .iter()
        .any(|code| format!(" {reason} ").contains(code))
}

/// What the window offers to queue on its own: the first word of some text,
/// if it is a link. The queue is the authority on what it accepts.
pub fn looks_downloadable(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?;
    let lower = first.to_ascii_lowercase();
    (lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("magnet:?"))
    .then(|| first.to_string())
}

/// Links inside some text: a dropped `.webloc` or `.url` file, a list, the
/// clipboard.
pub fn links_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || "<>\"'=".contains(c)) {
        if looks_downloadable(word).is_some() && !found.iter().any(|f| f == word) {
            found.push(word.to_string());
        }
    }
    found
}

/// How much of a dropped file is looked at for links.
const DROP_LOOK: u64 = 64 * 1024;

/// The kinds of file a drop is read for links: a browser's link file, a
/// list, a page. Anything else — a PDF, an ISO, a photo — is not a list of
/// links, and the addresses that happen to be inside one are not what the
/// person dropping it wanted queued.
const DROP_KINDS: [&str; 6] = ["webloc", "url", "txt", "text", "html", "htm"];

/// Links in a dropped browser item: the address itself where a drag source
/// hands one over as the path, else the first 64 KB of a text-like file.
/// Read from the path, and only that much: the whole file used to be read,
/// on the window's thread, to look at the start of it, and a dropped 4 GB
/// image held the window until it had. The file's bytes as the drop hands
/// them over are the fallback where the path cannot be opened.
pub fn dropped_links(file: &dyn egui::DroppedFile) -> Vec<String> {
    use std::io::Read;
    let path = file.path();
    let text = path.to_string_lossy();
    if text.contains("://") || text.starts_with("magnet:?") {
        return links_in(&text);
    }
    let kind = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if kind.is_some_and(|kind| !DROP_KINDS.contains(&kind.as_str())) {
        return Vec::new();
    }
    let mut head = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|f| f.take(DROP_LOOK).read_to_end(&mut head))
        .is_ok();
    if !read {
        head = file.bytes().unwrap_or_default();
        head.truncate(DROP_LOOK as usize);
    }
    // A file with no extension is read only if it looks like text.
    if head.contains(&0) {
        return Vec::new();
    }
    links_in(&String::from_utf8_lossy(&head))
}

/// The clipboard watcher's memory: offered once, never again once dismissed,
/// and nothing offered for what was copied before it was turned on.
#[derive(Default)]
pub struct Clipboard {
    pub on: bool,
    pub last: Option<String>,
    pub offered: Option<String>,
    pub dismissed: HashSet<String>,
    pub polled: Option<Instant>,
}

pub struct DmApp {
    pub backend: Backend,
    pub rows: Vec<Entry>,
    refreshed: Option<Instant>,
    /// What each row was at the last refresh, to notice a finish.
    last_state: HashMap<Id, State>,

    pub place: Place,
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
    pub sort: Option<(Column, bool)>,
    pub show_categories: bool,
    pub show_toolbar: bool,
    /// Columns turned off under View › Columns. The file name always shows.
    pub hidden_columns: HashSet<Column>,
    /// Every column, in the order the list draws them.
    pub column_order: Vec<Column>,

    /// Commands asked for while drawing, run once the frame is drawn.
    pub queued: Vec<Command>,
    /// Finished files a drag out of the list has asked to carry, for the
    /// window to hand to the system's drag: only it has the native handle.
    pub drag_out: Vec<std::path::PathBuf>,
    /// Where each row was drawn last frame, in window points, so the control
    /// protocol can tell a test where to point.
    pub row_rects: HashMap<Id, egui::Rect>,
    pub dialogs: Dialogs,

    pub problems: Vec<String>,
    /// Completions already announced, so a notification fires once as each
    /// finishes — and never for the history there when the window opened.
    notified: HashSet<Id>,
    notify_ready: bool,

    pub applied: Settings,
    pub clipboard: Clipboard,
    pub update: Arc<Mutex<Update>>,
    /// A weekly check is waiting for its answer, to open About on a newer dm.
    weekly_pending: bool,
    /// The speed limit to go back to when the limiter is turned on again.
    last_limit: u64,
    /// A word in the status bar about the last thing done.
    pub said: crate::i18n::Said,
    /// Whether the look drawn last was dark, for the control protocol.
    pub dark: bool,
    pub quit_asked: bool,
    /// The look last put in force, so a change in Options is applied once.
    theme_in_force: Option<crate::theme::Theme>,
    fonts_ready: bool,
    /// What to do when the queue runs dry, for this session only.
    pub when_done: crate::power::WhenDone,
    /// Rows whose failure has been sounded, so one failure is one sound.
    failed_told: HashSet<Id>,
    /// What saved site searches found, by name, waiting to be added.
    search_results: Arc<std::sync::Mutex<Vec<SearchResult>>>,
    /// How many searches are out, so the window keeps drawing to take
    /// their results.
    pub searches_running: usize,
    /// When the saved searches were last looked at for one that is due.
    searches_checked: Option<Instant>,
    /// What a recheck running in the background found, when it is done.
    pub rechecked: Arc<std::sync::Mutex<Option<(usize, usize)>>>,
    /// Whether a menu, combo box or right-click menu was open at the end
    /// of the last frame: an Escape this frame was theirs, not a dialog's.
    pub popup_was_open: bool,
    /// Handed-over downloads cancelled, by site, since one was accepted:
    /// at two, the site is offered to the browser.
    cancelled_hand_offs: std::collections::HashMap<String, u32>,
    /// What "When done" has seen since the queue last ran dry.
    dry: DryWatch,
    /// A sleep or shut down waiting out its countdown.
    pub countdown: Option<(crate::power::WhenDone, Instant)>,
    /// Running without a window (`--headless`).
    pub headless: bool,
    /// Requests from the control protocol, when it is on.
    control: Option<std::sync::mpsc::Receiver<crate::control::Request>>,
    /// Rows the window added itself (Add URL, a series, an import): they
    /// open their progress window, if at all, as they are added. Any other
    /// row that appears running — a browser's hand-off, a second launch's
    /// link — opens one when it is first seen.
    own: HashSet<Id>,
    /// The rows the last `add` made, in order.
    pub last_added: Vec<Id>,
    /// A sentence about one row ("Trying … again with the sign-in."),
    /// cleared once that row has run and stopped (gap 140), and whether it
    /// has been seen running yet.
    pub said_about: Option<(Id, crate::i18n::Said, bool)>,
    /// Sizes a dialog learned from the server before a row was added for
    /// later, shown until the row learns its own.
    pub sizes_told: HashMap<Id, u64>,
    /// The status line's sentence and when it was said: an old one is not
    /// left standing beside rows it no longer describes.
    said_at: Option<(crate::i18n::Said, Instant)>,
}

/// How long the status line keeps its last sentence.
const SAID_FOR: Duration = Duration::from_secs(30);

impl DmApp {
    pub fn new(backend: Backend) -> Self {
        let settings = backend.settings();
        crate::i18n::set(crate::i18n::chosen(&settings.language));
        let mut app = DmApp {
            backend,
            rows: Vec::new(),
            refreshed: None,
            last_state: HashMap::new(),
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
            drag_out: Vec::new(),
            row_rects: HashMap::new(),
            dialogs: Dialogs::default(),
            problems: Vec::new(),
            notified: HashSet::new(),
            notify_ready: false,
            // The limit in force, else the one last turned off — kept in
            // the settings, so it survives a restart — else 512 KB/s.
            last_limit: [settings.speed_limit, settings.last_speed_limit]
                .into_iter()
                .find(|kb| *kb > 0)
                .unwrap_or(512),
            applied: settings,
            clipboard: Clipboard::default(),
            update: Arc::new(Mutex::new(Update::NotAsked)),
            weekly_pending: false,
            said: Default::default(),
            dark: false,
            quit_asked: false,
            theme_in_force: None,
            fonts_ready: false,
            when_done: Default::default(),
            failed_told: HashSet::new(),
            search_results: Arc::default(),
            searches_running: 0,
            searches_checked: None,
            popup_was_open: false,
            rechecked: Arc::default(),
            cancelled_hand_offs: std::collections::HashMap::new(),
            dry: DryWatch::default(),
            countdown: None,
            headless: false,
            control: None,
            own: HashSet::new(),
            last_added: Vec::new(),
            said_about: None,
            sizes_told: HashMap::new(),
            said_at: None,
        };
        app.sort = Column::from_key(&app.applied.sort_column)
            .map(|column| (column, app.applied.sort_ascending));
        app.set_clipboard_watch(app.applied.watch_clipboard);
        app.refresh();
        app
    }

    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// A client built the way the settings say: through the proxy, with
    /// the timeout. For the window's own fetches, which are not downloads.
    pub fn client(&self) -> dm_core::reqwest::Client {
        dm_core::proxy::client_with_timeout(&self.applied.proxy, self.applied.read_timeout())
            .unwrap_or_else(|_| dm_core::client())
    }

    /// A handed-over download from `site` was cancelled in Download
    /// details. Twice running, and the person is asked whether that site's
    /// downloads should stay with the browser; a site already left there,
    /// or one asked about, is not asked about again.
    pub fn hand_off_cancelled(&mut self, site: &str) {
        if self.applied.catch.skip_hosts.iter().any(|h| h == site) {
            return;
        }
        let count = self
            .cancelled_hand_offs
            .entry(site.to_string())
            .or_insert(0);
        *count += 1;
        if *count >= 2 {
            *count = 0;
            self.dialogs.skip_offer = Some(site.to_string());
        }
    }

    /// Runs a site search in the background; what it finds that is not
    /// already in the list is added for later when it comes back.
    pub fn run_search(&mut self, search: dm_shell::lists::Search) {
        let client = self.client();
        let results = Arc::clone(&self.search_results);
        self.searches_running += 1;
        self.backend.runtime.spawn(async move {
            let found = dm_shell::lists::crawl(&client, &search).await;
            dm_core::lock(&results).push((search.name, found));
        });
    }

    /// Takes in what finished searches found, and starts any saved search
    /// whose time has come. Looked at once a minute, and whenever a search
    /// is out.
    fn tick_searches(&mut self) {
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
                                self.said = crate::i18n::Said::naming(
                                    "Added {} download(s) to {}.",
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

    /// A problem for the list above the downloads, once: one that recurs
    /// each minute is not a new line each minute.
    pub fn problem(&mut self, text: String) {
        if !self.problems.contains(&text) {
            self.problems.push(text);
        }
    }

    pub fn rows(&self) -> &[Entry] {
        &self.rows
    }

    pub fn row(&self, id: Id) -> Option<&Entry> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// Reads the list back from the queue and does what follows from it:
    /// announcing what finished, collecting problems.
    pub fn refresh(&mut self) {
        self.rows = self.backend.list();
        self.refreshed = Some(Instant::now());
        let ids: HashSet<Id> = self.rows.iter().map(|r| r.id).collect();
        self.selected.retain(|id| ids.contains(id));
        self.forget_said_about();

        let mut finished = Vec::new();
        let mut failed = Vec::new();
        for row in &self.rows {
            // Once per failure: a row retried and back in line is told
            // again when it fails again.
            if !matches!(row.state, State::Failed(_)) {
                self.failed_told.remove(&row.id);
            }
            if matches!(row.state, State::Failed(_))
                && self.failed_told.insert(row.id)
                && self.notify_ready
            {
                failed.push(row.id);
            }
            if row.state == State::Completed && self.notified.insert(row.id) && self.notify_ready {
                self.backend
                    .platform
                    .notify(t("Download finished"), &name_of(row));
                finished.push(row.id);
                // The command a person set to run on each finished file.
                let command = self.applied.on_complete.trim();
                if !command.is_empty() && !row.path.is_empty() {
                    if let Err(why) =
                        dm_shell::shell::run_after(command, std::path::Path::new(&row.path))
                    {
                        self.problems.push(why);
                    }
                }
            }
        }
        if self.applied.sound_on_complete && !finished.is_empty() {
            self.backend.platform.chime();
        }
        if self.applied.sound_on_failure && !failed.is_empty() {
            self.backend.platform.alert();
        }
        self.when_queue_dry();
        if self.applied.show_complete {
            for id in finished {
                // In place of its progress window.
                self.dialogs.progress.remove(&id);
                self.dialogs.complete.insert(id);
            }
        }
        // A download that started without the window starting it — handed
        // over by a browser, or by a second launch — opens its progress
        // window as one started here does, where that is asked for. Not
        // the rows there when the window opened, and not one added for
        // later, which has not started.
        if self.applied.show_progress && self.notify_ready && !self.headless {
            for row in &self.rows {
                if !self.last_state.contains_key(&row.id)
                    && !self.own.contains(&row.id)
                    && matches!(row.state, State::Running | State::Queued)
                {
                    self.dialogs.progress.insert(row.id);
                }
            }
        }
        self.own.retain(|id| ids.contains(id));
        // A size told by a dialog stands only until the row has its own.
        let rows = &self.rows;
        self.sizes_told.retain(|id, _| {
            rows.iter()
                .any(|r| r.id == *id && r.progress.total.is_none() && r.progress.downloaded == 0)
        });
        self.last_state = self.rows.iter().map(|r| (r.id, r.state.clone())).collect();
        // After the first pass every completion already on disk is seeded.
        self.notify_ready = true;
        self.problems.extend(self.backend.problems());
    }

    /// "When done": once everything that was downloading has settled,
    /// quit, or start the countdown to sleep or shut down. Once only: the
    /// choice goes back to doing nothing.
    ///
    /// Settled means nothing is left that dm will finish by itself: nothing
    /// Running or Queued, and nothing the schedule or the download limit is
    /// holding (Paused with `paused_by_schedule`) — those start again when
    /// the window reopens or the allowance rolls over, so quitting or
    /// sleeping under them leaves a download half done that nothing resumes.
    ///
    /// A row a person stopped is theirs: it is left alone, not waited for,
    /// and does not keep the choice from firing once the rest has finished.
    /// A Failed row is settled too — nothing retries it without a person,
    /// and its parts stay for Retry whatever the computer does next. But a
    /// stretch that ended with nothing finished or failed (a person pressed
    /// Stop all) is not "done": the choice stays armed for the next one.
    fn when_queue_dry(&mut self) {
        if !self.dry.look(&self.rows, &self.last_state) {
            return;
        }
        use crate::power::WhenDone;
        match std::mem::take(&mut self.when_done) {
            WhenDone::Nothing => {}
            WhenDone::Quit => self.quit_asked = true,
            action => {
                let at = Instant::now() + Duration::from_secs(crate::power::COUNTDOWN_SECS);
                self.countdown = Some((action, at));
            }
        }
    }

    /// A video the browser's panel handed over: "Download details",
    /// one at a time. With no window to ask in, it is simply added.
    pub fn take_handed(&mut self) {
        let ask = !self.headless && self.applied.ask_panel_downloads;
        if ask && (self.dialogs.file_info.is_some() || self.dialogs.links.is_some()) {
            return;
        }
        let Some(request) = dm_core::lock(&self.backend.handed).pop_front() else {
            return;
        };
        // A page's links: the "Download all links" dialog, or straight
        // into the main queue with nobody to ask.
        if !request.links.is_empty() {
            if ask {
                let backend = self.backend.clone();
                self.dialogs.open_links(request, &backend);
            } else {
                let with = dm_core::AddOptions {
                    later: true,
                    queue: dm_core::MAIN_QUEUE.into(),
                    referrer: request.referrer.clone(),
                    ..Default::default()
                };
                if let Err(e) = self.add(&request.links, with) {
                    self.said = e.into();
                }
            }
            return;
        }
        if !ask {
            if let Err(e) = self
                .backend
                .add_with(&request.url, dm_shell::shell::add_options(&request))
            {
                self.said = e.into();
            }
        } else {
            self.dialogs.open_handed(request);
        }
    }

    /// Runs a countdown's action once its time is up.
    pub fn countdown_tick(&mut self) {
        if let Some((action, at)) = self.countdown {
            if Instant::now() >= at {
                self.countdown = None;
                // In the list of problems as well as the status line: whoever
                // set it is likely away, and a status line is overwritten.
                // A refusal that comes back later is left for the list too,
                // as anything running apart from the window does.
                let notices = self.backend.notices();
                let report: crate::power::Report =
                    Box::new(move |why| dm_core::lock(&notices).push(why));
                if let Err(e) = self.backend.platform.power(action, report) {
                    self.problem(e.clone());
                    self.said = e.into();
                }
            }
        }
    }

    /// What runs whether or not the window shows: keeping the list current
    /// and watching the clipboard.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if self.refreshed.is_none_or(|at| at.elapsed() >= REFRESH) {
            self.refresh();
        }
        self.take_handed();
        self.countdown_tick();
        if let Some((checked, changed)) = dm_core::lock(&self.rechecked).take() {
            self.said = crate::i18n::Said::new(
                "Checked {} finished file(s) with their servers; {} had changed and are being downloaded again.",
                &[&checked, &changed],
            );
        }
        // Here, not in the drawing: saved searches run on their timer while
        // dm is in the tray or headless, when nothing is drawn.
        self.tick_searches();
        if self.clipboard.on
            && self
                .clipboard
                .polled
                .is_none_or(|at| at.elapsed() >= CLIPBOARD)
        {
            self.check_clipboard();
        }
        self.answer_control(ctx);
        self.weekly_update(ctx);
        ctx.request_repaint_after(REFRESH);
    }

    /// The weekly update check: about once a week, where turned on,
    /// a look for a newer dm, and About opened to offer it when there is one.
    /// Nothing is installed without the person.
    fn weekly_update(&mut self, ctx: &egui::Context) {
        // Due again a week after the last, however long dm has been running
        // in the tray: the time of the last check is what decides.
        if self.applied.check_updates {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if crate::update::due(self.applied.update_checked, now) {
                self.weekly_pending = true;
                crate::update::check(
                    &self.backend.runtime,
                    self.client(),
                    Arc::clone(&self.update),
                    ctx.clone(),
                );
                let mut wanted = self.applied.clone();
                wanted.update_checked = now;
                if let Err(e) = self.apply_settings(wanted) {
                    self.said = e.into();
                    // Not saved, but not asked again every frame either.
                    self.applied.update_checked = now;
                }
            }
        }
        if self.weekly_pending && matches!(*dm_core::lock(&self.update), Update::Newer(..)) {
            self.weekly_pending = false;
            self.dialogs.about_open = true;
        }
    }

    /* --------------------------------------------------- control protocol */

    /// Takes requests from the control protocol from now on.
    pub fn attach_control(&mut self, requests: std::sync::mpsc::Receiver<crate::control::Request>) {
        self.control = Some(requests);
    }

    fn answer_control(&mut self, ctx: &egui::Context) {
        let mut asked = Vec::new();
        if let Some(requests) = &self.control {
            while let Ok(request) = requests.try_recv() {
                asked.push(request);
            }
        }
        for request in asked {
            if request.method == "ui.screenshot" && self.headless {
                let _ = request.reply.send(Err(
                    "dm is running headless: there is no window to picture".into(),
                ));
                continue;
            }
            if request.method == "ui.screenshot" {
                let name = request
                    .params
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("dm")
                    .to_string();
                let name = dm_core::probe::sanitize(&name);
                let name = if name.is_empty() { "dm".into() } else { name };
                let path = dm_shell::home()
                    .join("screenshots")
                    .join(format!("{name}.png"));
                // Answered when the integration has read the frame's pixels.
                let reply = request.reply;
                ctx.request_screenshot(move |image| {
                    let _ = reply.send(
                        save_png(&image, &path)
                            .map(|()| serde_json::json!({ "path": path.display().to_string() })),
                    );
                });
                ctx.request_repaint();
                continue;
            }
            let answer = self.control_call(&request.method, &request.params);
            let _ = request.reply.send(answer);
        }
    }

    /// Answers one control request on the window's own thread.
    pub fn control_call(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use serde_json::json;
        match method {
            "commands.list" => Ok(json!(Command::ALL
                .iter()
                .map(|c| json!({ "id": c.id(), "label": c.label(), "enabled": self.enabled(*c) }))
                .collect::<Vec<_>>())),
            "command.run" => {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let command =
                    Command::from_id(id).ok_or_else(|| format!("no command is called {id}"))?;
                // Opening a downloaded file runs it, on Windows, if it is a
                // program: a person's click in the window, never a script's
                // or an AI client's that a web page may have talked into it.
                if matches!(command, Command::OpenFile | Command::OpenWith) {
                    return Err(format!(
                        "{id} is done in the window by a person, not over this connection"
                    ));
                }
                if !self.enabled(command) {
                    return Err(format!("{id} has nothing to act on"));
                }
                // A script cannot answer "Exit while downloading?": asked
                // over this connection, dm quits.
                if command == Command::Quit {
                    self.quit_asked = true;
                    return Ok(json!({ "ran": id, "said": self.said.english() }));
                }
                self.run(command);
                Ok(json!({ "ran": id, "said": self.said.english() }))
            }
            "ui.select" => {
                let ids: Vec<Id> = params
                    .get("ids")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                    .unwrap_or_default();
                self.selected = ids
                    .into_iter()
                    .filter(|id| self.row(*id).is_some())
                    .collect();
                Ok(json!({ "selected": self.selected }))
            }
            "ui.place" => {
                let place = params.get("place").and_then(|v| v.as_str()).unwrap_or("");
                self.place = match place {
                    "all" => Place::All,
                    "unfinished" => Place::Unfinished,
                    "finished" => Place::Finished,
                    other => {
                        if let Some(name) = other.strip_prefix("queue:") {
                            // A place nothing is filed under showed an empty
                            // list as if it were a queue.
                            if !self.backend.queues().iter().any(|q| q == name) {
                                return Err(format!("no queue is called {name}"));
                            }
                            Place::Queue(name.to_string())
                        } else if let Some(name) = other.strip_prefix("custom:") {
                            if !self
                                .applied
                                .custom_categories
                                .iter()
                                .any(|c| c.name == name)
                            {
                                return Err(format!("no category is called {name}"));
                            }
                            Place::Custom(name.to_string())
                        } else if let Some(word) = other.strip_prefix("kind:") {
                            let kind = Category::ALL
                                .into_iter()
                                .find(|c| category_word(*c) == word)
                                .ok_or_else(|| format!("no category is called {word}"))?;
                            Place::Kind(kind)
                        } else {
                            return Err(format!("no place is called {other}"));
                        }
                    }
                };
                Ok(json!({ "place": place }))
            }
            "ui.inspect" => Ok(json!({
                "place": format!("{:?}", self.place),
                "selected": self.selected,
                "sort": self.sort.map(|(c, up)| json!({ "column": c.english(), "ascending": up })),
                "categories": self.show_categories,
                "toolbar": self.show_toolbar,
                "dark": self.dark,
                "said": self.said.english(),
                "problems": self.problems,
                "dialogs": self.dialogs.open_names(),
                "rows": self.visible().iter().map(|r| json!({
                    "id": r.id,
                    "name": name_of(r),
                    "status": status_in_english(r),
                    "queue": r.queue,
                    "description": r.description,
                    "url": format::shown_url(&r.url),
                    "rect": self.row_rects.get(&r.id).map(|b| json!([b.min.x, b.min.y, b.max.x, b.max.y])),
                })).collect::<Vec<_>>(),
            })),
            "queue.list" => Ok(json!(self
                .rows
                .iter()
                .map(|r| {
                    let mut value = serde_json::to_value(r).unwrap_or_default();
                    value["url"] = json!(format::shown_url(&r.url));
                    // A site's sign-in, handed over by the browser, is the
                    // row's to send to that server and nobody's to read: not
                    // a script's, and not an AI client's through MCP. Said
                    // only as whether there is one.
                    if let Some(row) = value.as_object_mut() {
                        row.remove("cookies");
                        row.insert("signed_in".into(), json!(!r.cookies.is_empty()));
                    }
                    value
                })
                .collect::<Vec<_>>())),
            "downloads.add" => {
                let urls: Vec<String> = match params.get("urls").or_else(|| params.get("url")) {
                    Some(serde_json::Value::String(one)) => vec![one.clone()],
                    Some(serde_json::Value::Array(many)) => many
                        .iter()
                        .filter_map(|u| u.as_str().map(str::to_owned))
                        .collect(),
                    _ => return Err("give `urls`: a link or a list of links".into()),
                };
                // Nothing to add is a mistake in the asking, not success.
                if urls.iter().all(|u| u.trim().is_empty()) {
                    return Err("give `urls`: a link or a list of links; that list is empty".into());
                }
                let text = |key: &str| {
                    params
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                        .filter(|s| !s.is_empty())
                };
                let later = params
                    .get("later")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let with = dm_core::AddOptions {
                    later,
                    queue: if later {
                        MAIN_QUEUE.to_string()
                    } else {
                        String::new()
                    },
                    save_dir: text("folder"),
                    save_name: text("name"),
                    description: text("description").unwrap_or_default(),
                    ..dm_core::AddOptions::default()
                };
                let added = self.add(&urls, with)?;
                Ok(json!({ "added": added }))
            }
            // Up to 256 requests in one, answered in order: faster for a
            // script than a round trip each.
            "batch" => {
                let steps = params
                    .get("steps")
                    .and_then(|v| v.as_array())
                    .ok_or("give `steps`: a list of {method, params}")?;
                if steps.len() > 256 {
                    return Err("a batch is at most 256 steps".into());
                }
                let mut results = Vec::new();
                for step in steps {
                    let method = step.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    if method == "batch" || method == "ui.screenshot" {
                        results.push(
                            json!({ "ok": false, "error": format!("{method} cannot be batched") }),
                        );
                        continue;
                    }
                    let params = step
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    results.push(match self.control_call(method, &params) {
                        Ok(result) => json!({ "ok": true, "result": result }),
                        Err(error) => json!({ "ok": false, "error": error }),
                    });
                }
                Ok(json!(results))
            }
            other => Err(format!("no method is called {other}")),
        }
    }

    pub fn set_clipboard_watch(&mut self, on: bool) {
        self.clipboard.on = on;
        if !on {
            self.clipboard.offered = None;
            return;
        }
        // Seeded with whatever is there now, so turning it on does not offer
        // a link copied long ago for another reason.
        self.clipboard.last = self.backend.platform.clipboard();
        self.clipboard.polled = Some(Instant::now());
    }

    /// Looks at the clipboard once, now.
    pub fn check_clipboard(&mut self) {
        if !self.clipboard.on {
            return;
        }
        self.clipboard.polled = Some(Instant::now());
        let text = self.backend.platform.clipboard();
        if text == self.clipboard.last {
            return;
        }
        self.clipboard.last = text.clone();
        let url = text.as_deref().and_then(looks_downloadable);
        self.clipboard.offered = url.filter(|url| {
            !self.clipboard.dismissed.contains(url) && !self.rows.iter().any(|r| &r.url == url)
        });
    }

    pub fn clipboard_offer(&self) -> Option<&str> {
        self.clipboard.offered.as_deref()
    }

    /// Puts settings into force, and puts the window's copy back when that
    /// fails rather than leaving it showing what is not in force.
    pub fn apply_settings(&mut self, wanted: Settings) -> Result<(), String> {
        let watch_changed = wanted.watch_clipboard != self.applied.watch_clipboard;
        match self.backend.set_settings(wanted) {
            Ok(now) => {
                crate::i18n::set(crate::i18n::chosen(&now.language));
                // A search saved or changed is looked at on the next frame,
                // not in a minute: one saved to run every hour runs now.
                if self.applied.site_searches != now.site_searches {
                    self.searches_checked = None;
                }
                self.applied = now;
                if watch_changed {
                    self.set_clipboard_watch(self.applied.watch_clipboard);
                }
                Ok(())
            }
            Err(e) => {
                self.applied = self.backend.settings();
                Err(e)
            }
        }
    }

    /* ----------------------------------------------------------- places */

    pub fn shows(&self, row: &Entry) -> bool {
        self.within(&self.place, row)
    }

    /// Whether `row` belongs in `place`.
    fn within(&self, place: &Place, row: &Entry) -> bool {
        match place {
            Place::All => true,
            Place::Kind(c) => row.category == Some(*c),
            Place::Custom(name) => self.custom_of(row).as_deref() == Some(name.as_str()),
            // What is under way: running, waiting its turn, or held by the
            // schedule or the download limit. Not a failed row or one a
            // person stopped, which the status bar does not count as
            // downloading either.
            Place::Unfinished => {
                matches!(row.state, State::Running | State::Queued)
                    || (row.state == State::Paused && row.paused_by_schedule)
            }
            Place::Finished => row.state == State::Completed,
            Place::Queue(name) => &row.queue == name,
        }
    }

    /// The category a person made that `row` belongs to, by the rule its
    /// file was (or will be) placed by.
    pub fn custom_of(&self, row: &Entry) -> Option<String> {
        let categories = self.applied.categories();
        let name = name_of(row);
        categories
            .custom_for(&name, &dm_core::category::host_of(&row.url))
            .map(|c| c.name.clone())
    }

    pub fn count(&self, place: &Place) -> usize {
        self.rows.iter().filter(|r| self.within(place, r)).count()
    }

    /// The rows the table shows, in the order it shows them.
    pub fn visible(&self) -> Vec<Entry> {
        let mut rows: Vec<Entry> = self
            .rows
            .iter()
            .filter(|r| self.shows(r))
            .cloned()
            .collect();
        // What a dialog learned of a row's size, until the row knows it.
        for row in &mut rows {
            if row.progress.total.is_none() {
                if let Some(size) = self.sizes_told.get(&row.id) {
                    row.progress.total = Some(*size);
                }
            }
        }
        if let Some((column, ascending)) = self.sort {
            rows.sort_by(|a, b| {
                let order = compare(column, a, b);
                if ascending {
                    order
                } else {
                    order.reverse()
                }
            });
        }
        rows
    }

    /// The rows a command acts on: the selection, in table order.
    pub fn targets(&self) -> Vec<Entry> {
        self.visible()
            .into_iter()
            .filter(|r| self.selected.contains(&r.id))
            .collect()
    }

    /// The queue a queue command means: the one shown, or the main one.
    pub fn current_queue(&self) -> String {
        match &self.place {
            Place::Queue(name) => name.clone(),
            _ => MAIN_QUEUE.to_string(),
        }
    }

    /* --------------------------------------------------------- commands */

    pub fn enabled(&self, command: Command) -> bool {
        let targets = self.targets();
        let any = |f: fn(&Entry) -> bool| targets.iter().any(f);
        match command {
            Command::DownloadNow => any(|r| matches!(r.state, State::Paused | State::Failed(_))),
            Command::StopDownload => any(|r| matches!(r.state, State::Running | State::Queued)),
            // Enabled only for finished or paused downloads —
            // a running one is stopped first, so nothing is removed from
            // under its own writer by a slip of the Delete key.
            Command::Remove => {
                !targets.is_empty()
                    && targets
                        .iter()
                        .all(|r| !matches!(r.state, State::Running | State::Queued))
            }
            Command::MoveToQueue | Command::Properties => !targets.is_empty(),
            Command::Redownload => !targets.is_empty(),
            Command::UpdateLink => {
                targets.len() == 1 && !matches!(targets[0].state, State::Running | State::Completed)
            }
            Command::RemoveFromQueue => any(|r| !r.queue.is_empty()),
            Command::MoveUp | Command::MoveDown => targets.len() == 1,
            Command::ShowProgress => any(|r| r.state != State::Completed),
            // Open folder stays: it says the file is gone, which is an answer.
            Command::OpenFolder => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && !targets[0].path.is_empty()
            }
            Command::OpenFile | Command::OpenWith | Command::MoveRename => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && !targets[0].path.is_empty()
                    && !crate::table::gone(&targets[0])
            }
            Command::ResumeAll => self.rows.iter().any(|r| r.state == State::Paused),
            Command::StopAll => self
                .rows
                .iter()
                .any(|r| matches!(r.state, State::Running | State::Queued)),
            Command::DeleteCompleted => self.rows.iter().any(|r| r.state.clearable()),
            Command::FindNext => !self.dialogs.find_text.is_empty(),
            _ => true,
        }
    }

    /// Runs one command, as a menu item, button, key or the control
    /// protocol asked for it.
    pub fn run(&mut self, command: Command) {
        if !self.enabled(command) {
            return;
        }
        let targets = self.targets();
        match command {
            Command::AddUrl => self.dialogs.open_add_url(self.backend.platform.clipboard()),
            Command::AddSeries => self.dialogs.open_series(),
            Command::AddFromClipboard => {
                let text = self.backend.platform.clipboard().unwrap_or_default();
                let found = links_in(&text);
                if found.is_empty() {
                    self.said = "There are no links on the clipboard.".into();
                } else {
                    self.dialogs.clip_links = Some(found.into_iter().map(|l| (l, true)).collect());
                }
            }
            Command::Import => {
                if let Some(path) = self.backend.platform.choose_file(false, "") {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => {
                            let before: HashSet<Id> =
                                self.backend.list().iter().map(|r| r.id).collect();
                            let added = self.backend.import(&text);
                            // A list is not a run of progress windows.
                            self.own.extend(
                                self.backend
                                    .list()
                                    .iter()
                                    .map(|r| r.id)
                                    .filter(|id| !before.contains(id)),
                            );
                            self.said =
                                crate::i18n::Said::new("Imported {} download(s).", &[&added]);
                        }
                        Err(e) => {
                            self.said = crate::i18n::Said::new(
                                "{} could not be read: {}",
                                &[&path.display(), &e],
                            )
                        }
                    }
                }
            }
            Command::Export => {
                if let Some(path) = self.backend.platform.choose_file(true, "downloads.dmlist") {
                    self.said = match std::fs::write(&path, self.backend.export()) {
                        Ok(()) => crate::i18n::Said::new("Exported to {}.", &[&path.display()]),
                        Err(e) => crate::i18n::Said::new(
                            "{} could not be written: {}",
                            &[&path.display(), &e],
                        ),
                    };
                }
            }
            Command::SiteFiles => self.dialogs.open_site_files(),
            Command::Quit => self.ask_quit(),

            Command::DownloadNow => {
                // A row the schedule or the download limit holds is asked
                // about: start it now, outside the schedule, or let it wait.
                // With nobody to ask (headless), it is said instead.
                let held: Vec<Id> = targets
                    .iter()
                    .filter(|r| r.state == State::Paused && r.paused_by_schedule)
                    .map(|r| r.id)
                    .collect();
                if !held.is_empty() {
                    if self.headless {
                        self.said = crate::i18n::Said::new(
                            "Waiting for the schedule or the download limit; it starts by itself when allowed.",
                            &[],
                        );
                    } else {
                        self.dialogs.confirm_outside = Some(held);
                    }
                }
                for row in &targets {
                    match row.state {
                        // Held: asking would only requeue it to be held again.
                        State::Paused if row.paused_by_schedule => {}
                        State::Paused => self.backend.resume(row.id),
                        State::Failed(_) => self.backend.retry(row.id),
                        _ => {}
                    }
                    if self.applied.show_progress {
                        self.dialogs.progress.insert(row.id);
                    }
                }
            }
            Command::StopDownload => {
                for row in &targets {
                    self.backend.pause(row.id);
                }
            }
            Command::Remove => {
                // Asked first, whichever way it was asked for: an
                // unfinished download's fetched bytes go with it, and a
                // finished one leaves the list.
                // Headless, with nobody to ask, finished rows go as before.
                let ids: Vec<Id> = targets.iter().map(|r| r.id).collect();
                if self.headless && targets.iter().all(|r| r.state == State::Completed) {
                    self.remove_now(&ids);
                } else {
                    self.dialogs.confirm_remove = Some(ids);
                }
            }
            Command::Redownload => {
                for row in &targets {
                    self.backend.redownload(row.id);
                }
            }
            Command::UpdateLink => {
                self.dialogs.focus_on("Update download link");
                self.dialogs.update_link = Some((targets[0].id, String::new(), String::new()))
            }
            Command::MoveToQueue => {
                self.dialogs.focus_on("Move to queue");
                self.dialogs.move_to_queue = Some((
                    targets.iter().map(|r| r.id).collect(),
                    // As the dialog shows it; taken back to dm's own on OK.
                    crate::i18n::name(MAIN_QUEUE),
                ))
            }
            Command::RemoveFromQueue => {
                for row in &targets {
                    self.backend.set_queue(row.id, "");
                }
            }
            Command::MoveUp => self.backend.shift(targets[0].id, -1),
            Command::MoveDown => self.backend.shift(targets[0].id, 1),
            Command::Properties => {
                for row in &targets {
                    self.dialogs.open_properties(row);
                }
            }
            Command::ShowProgress => {
                for row in targets.iter().filter(|r| r.state != State::Completed) {
                    self.dialogs.progress.insert(row.id);
                }
            }
            Command::OpenFile => {
                if let Err(e) = self.backend.platform.open(&targets[0].path) {
                    self.said = e.into();
                }
            }
            Command::OpenWith => {
                if let Some(app) = self.backend.platform.choose_app() {
                    if let Err(e) = self.backend.platform.open_with(&targets[0].path, &app) {
                        self.said = e.into();
                    }
                }
            }
            Command::OpenFolder => {
                if let Err(e) = self.backend.platform.reveal(&targets[0].path) {
                    self.said = e.into();
                }
            }
            Command::MoveRename => {
                self.dialogs.focus_on("Move/Rename");
                self.dialogs.rename = Some((targets[0].id, targets[0].filename.clone()))
            }
            Command::SelectAll => {
                self.selected = self.visible().iter().map(|r| r.id).collect();
            }

            Command::ResumeAll => {
                self.backend.resume_all();
            }
            Command::StopAll => {
                self.backend.pause_all();
            }
            Command::DeleteCompleted => {
                let n = self.backend.clear_finished();
                self.said = crate::i18n::Said::new(
                    "Deleted {} completed or cancelled download(s) from the list.",
                    &[&n],
                );
            }
            Command::Find => {
                self.dialogs.find_open = true;
                self.dialogs.focus_on("Find");
            }
            Command::FindNext => self.find_next(),
            Command::Scheduler => self.dialogs.open_scheduler(&self.applied),
            Command::StartQueue => {
                let queue = self.current_queue();
                let n = self.backend.start_queue(&queue);
                self.said =
                    crate::i18n::Said::naming("Started {} download(s) in {}.", &[&n, &queue]);
                // Asked in the background: a server at a time, each up to
                // its timeouts, which held the window for minutes when it
                // was done here. The counts are said when it is done.
                if queue == dm_core::RECHECK_QUEUE {
                    let (q, done) = (self.backend.queue.clone(), Arc::clone(&self.rechecked));
                    self.backend.runtime.spawn(async move {
                        let (checked, changed) = q.synchronize(dm_core::RECHECK_QUEUE).await;
                        if !changed.is_empty() {
                            dm_shell::shell::wake(&q);
                        }
                        *dm_core::lock(&done) = Some((checked, changed.len()));
                    });
                }
            }
            Command::StopQueue => {
                let queue = self.current_queue();
                let n = self.backend.stop_queue(&queue);
                self.said =
                    crate::i18n::Said::naming("Stopped {} download(s) in {}.", &[&n, &queue]);
            }
            Command::SpeedLimiterToggle => {
                let mut wanted = self.applied.clone();
                if wanted.speed_limit > 0 {
                    self.last_limit = wanted.speed_limit;
                    wanted.last_speed_limit = wanted.speed_limit;
                    wanted.speed_limit = 0;
                } else {
                    wanted.speed_limit = self.last_limit.max(1);
                }
                if let Err(e) = self.apply_settings(wanted) {
                    self.said = e.into();
                } else {
                    self.said = if self.applied.speed_limit > 0 {
                        crate::i18n::Said::new(
                            "Speed limiter on: {} KB/s.",
                            &[&self.applied.speed_limit],
                        )
                    } else {
                        crate::i18n::Said::new("Speed limiter off.", &[])
                    };
                }
            }
            Command::SpeedLimiterSettings => self.dialogs.open_limiter(&self.applied),
            Command::WhenDoneNothing => self.when_done = crate::power::WhenDone::Nothing,
            Command::WhenDoneQuit => self.when_done = crate::power::WhenDone::Quit,
            Command::WhenDoneSleep => self.when_done = crate::power::WhenDone::Sleep,
            Command::WhenDoneShutDown => self.when_done = crate::power::WhenDone::ShutDown,
            Command::WhenDoneHibernate => self.when_done = crate::power::WhenDone::Hibernate,
            Command::Options => self.dialogs.open_options(&self.applied),

            Command::ToggleCategories => self.show_categories = !self.show_categories,
            Command::ToggleToolbar => self.show_toolbar = !self.show_toolbar,
            Command::SortByName => self.sort_by(Column::Name),
            Command::SortBySize => self.sort_by(Column::Size),
            Command::SortByStatus => self.sort_by(Column::Status),
            Command::SortByTimeLeft => self.sort_by(Column::TimeLeft),
            Command::SortByRate => self.sort_by(Column::Rate),
            Command::SortByDate => self.sort_by(Column::LastTry),
            Command::SortByDescription => self.sort_by(Column::Description),

            Command::About => self.dialogs.about_open = true,
            Command::Help => {
                if let Err(e) = self.backend.platform.open(crate::update::HELP) {
                    self.said = e.into();
                }
            }
            Command::CheckForUpdates => {
                self.dialogs.about_open = true;
                self.dialogs.check_update = true;
            }
        }
        self.refresh();
    }

    /// Takes rows out of the list, with their staged parts; finished files
    /// stay on disk.
    pub fn remove_now(&mut self, ids: &[Id]) {
        self.backend.remove_many(ids);
        // What the status line said may have named one of them.
        self.said = Default::default();
        for id in ids {
            self.dialogs.forget(*id);
        }
        self.selected.clear();
        self.refresh();
    }

    /// Each column on or off — View › Columns, and a right click on a
    /// heading. The file name always shows.
    ///
    /// And moved, so the columns can be put in any order:
    /// each but the file name can go a place earlier or later. Kept in the
    /// settings, so the list looks the same at the next launch.
    pub fn columns_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let order = self.column_order.clone();
        for (at, column) in order.iter().copied().enumerate().skip(1) {
            ui.horizontal(|ui| {
                let mut on = !self.hidden_columns.contains(&column);
                if ui.checkbox(&mut on, column.title()).clicked() {
                    if on {
                        self.hidden_columns.remove(&column);
                    } else {
                        self.hidden_columns.insert(column);
                    }
                    changed = true;
                }
                // The file name stays first, so nothing moves before it.
                if let Some(to) = move_buttons(ui, column.title(), at > 1, at + 1 < order.len()) {
                    self.column_order.swap(at, to(at));
                    changed = true;
                }
            });
        }
        if changed {
            self.save_columns();
        }
    }

    /// Writes the columns' order and which are off into the settings.
    fn save_columns(&mut self) {
        let mut wanted = self.applied.clone();
        wanted.column_order = self
            .column_order
            .iter()
            .map(|c| c.key().to_string())
            .collect();
        let mut hidden: Vec<String> = self
            .hidden_columns
            .iter()
            .map(|c| c.key().to_string())
            .collect();
        hidden.sort();
        wanted.columns_hidden = hidden;
        if let Err(e) = self.apply_settings(wanted) {
            self.said = e.into();
        }
    }

    /// Sorts by a column; asked for again, the other way round. Kept in
    /// the settings, so the list is sorted the same way at the next launch.
    pub fn sort_by(&mut self, column: Column) {
        self.sort = match self.sort {
            Some((c, ascending)) if c == column => Some((c, !ascending)),
            _ => Some((column, true)),
        };
        if let Some((column, ascending)) = self.sort {
            let mut wanted = self.applied.clone();
            wanted.sort_column = column.key().to_string();
            wanted.sort_ascending = ascending;
            if let Err(e) = self.apply_settings(wanted) {
                self.said = e.into();
            }
        }
    }

    /// Program › Exit, Cmd+Q and the tray's Quit: asked first while
    /// anything is downloading, since quitting stops it. A window with
    /// nobody to ask (headless) quits.
    pub fn ask_quit(&mut self) {
        if self.headless || self.downloading() == 0 {
            self.quit_asked = true;
        } else {
            self.dialogs.confirm_exit = true;
        }
    }

    /// Clears a sentence about one row once that row has run again and
    /// stopped, or is gone: "Trying … again with the sign-in." stood on
    /// after the download it spoke of had long finished.
    fn forget_said_about(&mut self) {
        let Some((id, said, seen_running)) = self.said_about.as_mut() else {
            return;
        };
        if *said != self.said {
            self.said_about = None;
            return;
        }
        let state = self.rows.iter().find(|r| r.id == *id).map(|r| &r.state);
        match state {
            Some(State::Running | State::Queued) => *seen_running = true,
            Some(_) if !*seen_running => {}
            _ => {
                self.said = crate::i18n::Said::default();
                self.said_about = None;
            }
        }
    }

    /// How many downloads are running or about to.
    pub fn downloading(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| matches!(r.state, State::Running | State::Queued))
            .count()
    }

    /// A command's name as its menu item and button show it now: Resume on
    /// rows that failed is a retry, and says so.
    pub fn title_of(&self, command: Command) -> &'static str {
        if command == Command::DownloadNow && self.failed_only() {
            return t("Retry");
        }
        command.title()
    }

    /// Whether every selected row failed, so Resume would retry each.
    fn failed_only(&self) -> bool {
        let targets = self.targets();
        !targets.is_empty() && targets.iter().all(|r| matches!(r.state, State::Failed(_)))
    }

    /// Why held rows wait, and until when: the download limit's time left,
    /// or when the schedule next opens. `None` when nothing holds them.
    pub fn held_because(&self) -> Option<String> {
        if let Some(wait) = self.backend.queue.allowance_spent() {
            return Some(tf(
                "Held by the download limit; it goes on by itself in {}.",
                &[&format::span(wait)],
            ));
        }
        let schedule = &self.applied.schedule;
        if schedule.enabled && !schedule.open() {
            return Some(match opens_next(schedule) {
                Some(at) => tf("Held by the schedule; it starts by itself at {}.", &[&at]),
                None => t("Held by the schedule.").to_string(),
            });
        }
        None
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

    /// Adds downloads, as "Start now" and "Add for later" do.
    pub fn add(&mut self, urls: &[String], with: dm_core::AddOptions) -> Result<usize, String> {
        self.add_sized(urls, with, &HashMap::new())
    }

    /// The same, with the sizes a dialog already learned for some of the
    /// links, shown on their rows until each learns its own.
    pub fn add_sized(
        &mut self,
        urls: &[String],
        with: dm_core::AddOptions,
        sizes: &HashMap<String, u64>,
    ) -> Result<usize, String> {
        let mut added = 0;
        let mut last_error = None;
        self.last_added.clear();
        let items = urls.iter().map(|u| (u.clone(), with.clone())).collect();
        for (url, outcome) in urls.iter().zip(self.backend.add_each(items)) {
            match outcome {
                Ok(id) => {
                    added += 1;
                    self.last_added.push(id);
                    self.own.insert(id);
                    if let Some(size) = sizes.get(url) {
                        self.sizes_told.insert(id, *size);
                    }
                    if !with.later && self.applied.show_progress && urls.len() == 1 {
                        self.dialogs.progress.insert(id);
                    }
                }
                Err(e) => last_error = Some(e),
            }
        }
        self.refresh();
        let failed = urls.len().saturating_sub(added);
        match (added, last_error) {
            (0, Some(e)) => Err(e),
            (_, Some(e)) => {
                // The rest went in; the ones that did not are said, not lost.
                self.problem(match failed {
                    1 => format!("One of the links was not added: {e}"),
                    n => format!("{n} of the links were not added: {e}"),
                });
                Ok(added)
            }
            _ => Ok(added),
        }
    }

    /* ---------------------------------------------------------- drawing */

    /// The window, drawn once.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.searches_running > 0 {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        // The window itself is what a screen reader names first; egui leaves
        // its root node without a name, which Orca read as a nameless frame.
        ctx.accesskit_node_builder(egui::accesskit_root_id(), |root| root.set_label("dm"));
        // "system" follows the computer's light or dark setting.
        // On Linux the window library cannot say, so the desktop is asked.
        let system_dark = crate::system_theme::prefers_dark()
            .or_else(|| ctx.system_theme().map(|t| t == egui::Theme::Dark))
            .unwrap_or(false);
        let theme = crate::theme::Theme::resolve(&self.applied.theme, system_dark);
        // Applied again once the embedded fonts are in force (a frame after
        // they are set), so headings take Selawik's semibold.
        let ready = crate::theme::fonts_ready(&ctx);
        self.dark = theme.tokens().dark;
        if self.theme_in_force != Some(theme) || ready != self.fonts_ready {
            crate::theme::apply(&ctx, theme);
            self.theme_in_force = Some(theme);
            self.fonts_ready = ready;
        }
        self.logic(&ctx);
        // A name in a script dm's own typeface lacks (Chinese, Korean,
        // Arabic…) has the computer's font for it found, rather than
        // drawn as blanks.
        crate::theme::cover(
            &ctx,
            self.rows
                .iter()
                .flat_map(|r| [&r.filename, &r.path, &r.description])
                .map(String::as_str),
        );
        self.keys(&ctx);
        self.take_drops(&ctx);

        let t = crate::theme::Tokens::get(&ctx);
        // The modern look: surfaces meet without lines; the classic look keeps them.
        let lines = !t.modern;
        let flat = |fill: egui::Color32, x: i8, y: i8| {
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(x, y))
        };
        egui::Panel::top("menu")
            .show_separator_line(lines)
            .frame(flat(t.frame, 6, 2))
            .show(ui, |ui| self.menu_bar(ui));
        if self.show_toolbar {
            egui::Panel::top("toolbar")
                .show_separator_line(lines)
                .frame(flat(t.frame, 8, if t.modern { 4 } else { 0 }))
                .show(ui, |ui| self.toolbar(ui));
        }
        egui::Panel::bottom("status")
            .show_separator_line(lines)
            .frame(flat(t.frame, 12, 4))
            .show(ui, |ui| self.status_bar(ui));
        if !self.problems.is_empty() {
            egui::Panel::bottom("problems").show(ui, |ui| self.problem_list(ui));
        }
        if self.show_categories {
            egui::Panel::left("categories")
                .resizable(true)
                .show_separator_line(lines)
                .default_size(if t.modern { 230.0 } else { 200.0 })
                .frame(flat(t.frame, 8, 4))
                .show(ui, |ui| {
                    // More places than fit scroll, rather than being cut off.
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.categories(ui));
                });
        }
        // The list on its own surface: in the modern look, a card with rounded
        // corners and a hairline edge on the window's background.
        let card = egui::Frame::new()
            .fill(t.list)
            .stroke(egui::Stroke::new(
                1.0,
                if t.modern { t.border } else { t.list },
            ))
            .corner_radius(t.card_radius)
            .inner_margin(egui::Margin::same(if t.modern { 6 } else { 2 }));
        egui::CentralPanel::default_margins()
            .frame(flat(
                t.frame,
                if t.modern { 8 } else { 0 },
                if t.modern { 4 } else { 0 },
            ))
            .show(ui, |ui| {
                card.show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.clipboard_bar(ui);
                    crate::table::show(self, ui);
                });
            });

        crate::dialogs::show(self, &ctx);
        self.focus_ring(&ctx);

        for command in std::mem::take(&mut self.queued) {
            self.run(command);
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // F3 types nothing: it finds the next one even from Find's own field.
        if self.dialogs.find_open && ctx.egui_wants_keyboard_input() {
            let f3 = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F3);
            if ctx.input_mut(|i| i.consume_shortcut(&f3)) {
                self.queued.push(Command::FindNext);
            }
        }
        // Not while typing: Delete in a text field must delete a letter, not
        // a download.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // A dialog in front has the keys: Delete or Enter pressed in Options
        // must not remove or open the downloads selected behind it. The
        // windows each download opens on its own are not in front of the list
        // that way, and leave it its keys.
        let behind = self.dialogs.in_front();
        for command in ctx.input_mut(Command::pressed) {
            // F3 is what Find is open for: it finds behind its own dialog.
            let finding = command == Command::FindNext && self.dialogs.find_open;
            if !(behind && command.acts_on_the_list()) || finding {
                self.queued.push(command);
            }
        }
        if !behind && ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !self.selected.is_empty() {
            self.queued.push(Command::ShowProgress);
        }
        // Up, Down, Home and End move the selection; with Shift they extend
        // it from where it was started. Not while a menu is open: its
        // keys are its own.
        if !behind && !self.popup_was_open {
            let pressed = ctx.input(|i| {
                [
                    (egui::Key::ArrowUp, crate::table::Step::Up),
                    (egui::Key::ArrowDown, crate::table::Step::Down),
                    (egui::Key::Home, crate::table::Step::First),
                    (egui::Key::End, crate::table::Step::Last),
                ]
                .into_iter()
                .find(|(key, _)| i.key_pressed(*key))
                .map(|(_, step)| (step, i.modifiers.shift))
            });
            if let Some((step, shift)) = pressed {
                let ids: Vec<Id> = self.visible().iter().map(|r| r.id).collect();
                let (selected, anchor, cursor) = crate::table::stepped(
                    &ids,
                    &self.selected,
                    self.anchor,
                    self.cursor,
                    step,
                    shift,
                );
                (self.selected, self.anchor, self.cursor) = (selected, anchor, cursor);
                self.scroll_to = cursor;
                // The list has the keys now: egui's own move of the focus
                // to a control above or below is not wanted beside it.
                ctx.memory_mut(|m| m.stop_text_input());
            }
        }
    }

    /// Whether the keyboard is in use, and a ring around what has its
    /// focus — a control, or the list's row the keys are on — so it can be
    /// seen where Tab and the arrows went. Not drawn after a click.
    fn focus_ring(&mut self, ctx: &egui::Context) {
        let (pointer, keys) = ctx.input(|i| {
            let keys = i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::Tab
                            | egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                            | egui::Key::Home
                            | egui::Key::End,
                        pressed: true,
                        ..
                    }
                )
            });
            (i.pointer.any_pressed(), keys)
        });
        if pointer {
            self.keyboard = false;
        } else if keys {
            self.keyboard = true;
        }
        self.focus_ring = None;
        if !self.keyboard {
            return;
        }
        let t = crate::theme::Tokens::get(ctx);
        let stroke = egui::Stroke::new(2.0, t.accent);
        let focused = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id));
        let (rect, layer) = match focused {
            Some(r) => (r.interact_rect, r.layer_id),
            None => match self.cursor.and_then(|id| self.row_rects.get(&id)) {
                Some(rect) => (*rect, egui::LayerId::background()),
                None => return,
            },
        };
        if rect.is_positive() {
            // Two lines: the accent one, and inside it one in a light
            // colour, which is what shows on a primary button or a
            // selected row: those are filled with the accent colour itself.
            let painter = ctx.layer_painter(layer);
            let radius = egui::CornerRadius::same(t.radius);
            painter.rect_stroke(rect.shrink(1.0), radius, stroke, egui::StrokeKind::Inside);
            painter.rect_stroke(
                rect.shrink(3.0),
                radius,
                egui::Stroke::new(1.5, t.focus_inner),
                egui::StrokeKind::Inside,
            );
            self.focus_ring = Some(rect);
        }
    }

    fn take_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut links = Vec::new();
        for file in dropped {
            links.extend(dropped_links(file.as_ref()));
        }
        if !links.is_empty() {
            let _ = self.add(&links, Default::default());
        }
    }

    /// A menu item for a command: its label, its key, and greyed out when
    /// it has nothing to act on.
    fn item(&mut self, ui: &mut egui::Ui, command: Command) {
        let mut button = egui::Button::new(self.title_of(command));
        if let Some(shortcut) = command.shortcut() {
            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
        }
        if ui.add_enabled(self.enabled(command), button).clicked() {
            self.queued.push(command);
            ui.close();
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t("Program"), |ui| {
                for c in [
                    Command::AddUrl,
                    Command::AddSeries,
                    Command::AddFromClipboard,
                ] {
                    self.item(ui, c);
                }
                ui.separator();
                self.item(ui, Command::Export);
                self.item(ui, Command::Import);
                ui.separator();
                self.item(ui, Command::SiteFiles);
                ui.separator();
                self.item(ui, Command::Quit);
            });
            ui.menu_button(t("File"), |ui| {
                for c in [
                    Command::DownloadNow,
                    Command::StopDownload,
                    Command::ShowProgress,
                ] {
                    self.item(ui, c);
                }
                ui.separator();
                for c in [
                    Command::Remove,
                    Command::Redownload,
                    Command::UpdateLink,
                    Command::MoveRename,
                ] {
                    self.item(ui, c);
                }
                ui.separator();
                for c in [
                    Command::OpenFile,
                    Command::OpenWith,
                    Command::OpenFolder,
                    Command::Properties,
                ] {
                    self.item(ui, c);
                }
                ui.separator();
                for c in [
                    Command::MoveToQueue,
                    Command::RemoveFromQueue,
                    Command::MoveUp,
                    Command::MoveDown,
                    Command::SelectAll,
                ] {
                    self.item(ui, c);
                }
            });
            ui.menu_button(t("Downloads"), |ui| {
                for c in [
                    Command::ResumeAll,
                    Command::StopAll,
                    Command::DeleteCompleted,
                ] {
                    self.item(ui, c);
                }
                ui.separator();
                self.item(ui, Command::Find);
                self.item(ui, Command::FindNext);
                ui.separator();
                for c in [Command::Scheduler, Command::StartQueue, Command::StopQueue] {
                    self.item(ui, c);
                }
                ui.menu_button(t("Speed limiter"), |ui| {
                    // Ticked while a limit is in force: the item says
                    // whether the limiter is on, not only that it toggles.
                    // A menu item like the one under it, its tick at the
                    // right: as a checkbox its words started further in
                    // than the item below (gap 140).
                    let on = self.applied.speed_limit > 0;
                    let title = Command::SpeedLimiterToggle.title();
                    let item = ui.add(egui::Button::new(title).right_text("    "));
                    item.widget_info(|| {
                        egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, title)
                    });
                    if on {
                        let r = item.rect;
                        let at = egui::pos2(r.right() - 12.0, r.center().y);
                        let colour = ui.style().interact(&item).fg_stroke.color;
                        ui.painter().line(
                            vec![
                                at + egui::vec2(-5.0, 0.0),
                                at + egui::vec2(-1.5, 3.5),
                                at + egui::vec2(5.0, -4.0),
                            ],
                            egui::Stroke::new(1.6, colour),
                        );
                    }
                    if item.clicked() {
                        self.queued.push(Command::SpeedLimiterToggle);
                        ui.close();
                    }
                    self.item(ui, Command::SpeedLimiterSettings);
                });
                ui.menu_button(t("When done"), |ui| {
                    use crate::power::WhenDone;
                    for (c, w) in [
                        (Command::WhenDoneNothing, WhenDone::Nothing),
                        (Command::WhenDoneQuit, WhenDone::Quit),
                        (Command::WhenDoneSleep, WhenDone::Sleep),
                        (Command::WhenDoneHibernate, WhenDone::Hibernate),
                        (Command::WhenDoneShutDown, WhenDone::ShutDown),
                    ] {
                        // Greyed where this computer has no such command
                        // (macOS does not hibernate), rather than failing
                        // when the queue runs dry.
                        let can = matches!(w, WhenDone::Nothing | WhenDone::Quit)
                            || crate::power::way(w, std::env::consts::OS).is_some();
                        if ui
                            .add_enabled(
                                can,
                                egui::RadioButton::new(self.when_done == w, c.title()),
                            )
                            .clicked()
                        {
                            self.queued.push(c);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                self.item(ui, Command::Options);
            });
            ui.menu_button(t("View"), |ui| {
                let mut categories = self.show_categories;
                if ui.checkbox(&mut categories, t("Categories")).clicked() {
                    self.queued.push(Command::ToggleCategories);
                }
                let mut toolbar = self.show_toolbar;
                if ui.checkbox(&mut toolbar, t("Toolbar")).clicked() {
                    self.queued.push(Command::ToggleToolbar);
                }
                // "Columns": each column on or off; the file
                // name stays, since a row without one is no row.
                ui.menu_button(t("Columns"), |ui| self.columns_menu(ui));
                ui.menu_button(t("Customize toolbar"), |ui| self.toolbar_menu(ui));
                // View › Language.
                ui.menu_button(t("Language"), |ui| {
                    let now = crate::i18n::chosen(&self.applied.language);
                    if let Some(code) = crate::i18n::pick(ui, now) {
                        let mut wanted = self.applied.clone();
                        wanted.language = code.to_string();
                        if let Err(e) = self.apply_settings(wanted) {
                            self.said = e.into();
                        }
                        ui.close();
                    }
                });
                ui.menu_button(t("Sort by"), |ui| {
                    for c in [
                        Command::SortByName,
                        Command::SortBySize,
                        Command::SortByStatus,
                        Command::SortByTimeLeft,
                        Command::SortByRate,
                        Command::SortByDate,
                        Command::SortByDescription,
                    ] {
                        self.item(ui, c);
                    }
                });
            });
            ui.menu_button(t("Help"), |ui| {
                self.item(ui, Command::Help);
                self.item(ui, Command::CheckForUpdates);
                self.item(ui, Command::About);
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let chosen = ToolStyle::named(&self.applied.toolbar_style);
        let buttons: Vec<_> = toolbar_in(&self.applied.toolbar_order)
            .into_iter()
            .filter(|(command, _, _)| {
                !self
                    .applied
                    .toolbar_hidden
                    .iter()
                    .any(|h| h == command.id())
            })
            .collect();
        let failed_only = self.failed_only();
        // Each button with the name it shows and the widest it can show,
        // which is the room it takes: Resume becoming Retry moved every
        // button after it (gap 140).
        let buttons: Vec<(Command, Icon, &'static str, &'static str)> = buttons
            .into_iter()
            .map(|(command, icon, label)| {
                let names = if command == Command::DownloadNow {
                    vec![crate::i18n::t(label), crate::i18n::t("Retry")]
                } else {
                    vec![crate::i18n::t(label)]
                };
                let shown = if command == Command::DownloadNow && failed_only {
                    crate::i18n::t("Retry")
                } else {
                    crate::i18n::t(label)
                };
                let widest = names
                    .into_iter()
                    .max_by(|a, b| {
                        modern_width(ui, a, false).total_cmp(&modern_width(ui, b, false))
                    })
                    .unwrap_or(shown);
                (command, icon, shown, widest)
            })
            .collect();
        // Every button keeps its name. The modern toolbar puts each name
        // beside its picture where one row has room for that, under it
        // where it has not (a narrow window, or longer words, as German's),
        // and goes on to a second row only where even that does not fit.
        // Pictures alone, at those widths, left most of it to be guessed.
        let t = crate::theme::Tokens::get(ui.ctx());
        let spacing = ui.spacing().item_spacing.x;
        let room = ui.available_width();
        let width = |stacked: bool| -> f32 {
            buttons
                .iter()
                .map(|(_, _, _, widest)| modern_width(ui, widest, stacked) + spacing)
                .sum()
        };
        let modern = t.modern && chosen == ToolStyle::Both;
        let stacked = modern && width(false) > room;
        // Wrapped where even that does not fit, or where the classic
        // looks' fixed buttons do not.
        let wrap = (stacked && width(true) > room) || !modern;
        let mut draw = |ui: &mut egui::Ui| {
            for (command, icon, name, widest) in &buttons {
                // The modern toolbar leads with one primary action.
                let primary = *command == Command::AddUrl;
                let enabled = self.enabled(*command);
                let response = if stacked {
                    modern_tool(ui, *icon, (name, widest), enabled, primary, &t, true)
                } else {
                    tool(ui, *icon, (name, widest), enabled, chosen, primary)
                };
                if response.on_hover_text(self.title_of(*command)).clicked() {
                    self.queued.push(*command);
                }
            }
        };
        if wrap {
            ui.horizontal_wrapped(|ui| draw(ui));
        } else {
            ui.horizontal(|ui| draw(ui));
        }
        ui.add_space(2.0);
    }

    /// Customising the toolbar: the style, which buttons show, and their
    /// order. Saved with the settings.
    fn toolbar_menu(&mut self, ui: &mut egui::Ui) {
        let mut wanted = self.applied.clone();
        let current = ToolStyle::named(&wanted.toolbar_style);
        for (style, name) in TOOL_STYLES {
            if ui.radio(current == style, t(name)).clicked() {
                wanted.toolbar_style = style.name().into();
            }
        }
        ui.separator();
        let buttons = toolbar_in(&wanted.toolbar_order);
        let mut order: Vec<String> = buttons.iter().map(|(c, _, _)| c.id().to_string()).collect();
        for (i, (command, _, label)) in buttons.iter().enumerate() {
            let id = command.id().to_string();
            ui.horizontal(|ui| {
                let mut on = !wanted.toolbar_hidden.contains(&id);
                if ui.checkbox(&mut on, t(label)).changed() {
                    if on {
                        wanted.toolbar_hidden.retain(|h| *h != id);
                    } else {
                        wanted.toolbar_hidden.push(id.clone());
                    }
                }
                if let Some(to) =
                    move_buttons(ui, crate::i18n::t(label), i > 0, i + 1 < buttons.len())
                {
                    order.swap(i, to(i));
                }
            });
        }
        if order
            != buttons
                .iter()
                .map(|(c, _, _)| c.id().to_string())
                .collect::<Vec<_>>()
        {
            wanted.toolbar_order = order;
        }
        if wanted.toolbar_style != self.applied.toolbar_style
            || wanted.toolbar_hidden != self.applied.toolbar_hidden
            || wanted.toolbar_order != self.applied.toolbar_order
        {
            if let Err(e) = self.apply_settings(wanted) {
                self.said = e.into();
            }
        }
    }

    fn categories(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let mut chosen = None;
        // What a place's right-click menu asked for.
        let mut asked: Option<(Place, TreeAction)> = None;
        let folder = crate::theme::Tokens::get(ui.ctx()).folder;
        let tokens = crate::theme::Tokens::get(ui.ctx());
        let mut place = |ui: &mut egui::Ui, app: &DmApp, place: Place, label: String| {
            let n = app.count(&place);
            let text = format!("{label} ({n})");
            ui.horizontal(|ui| {
                let response = if tokens.modern {
                    nav_item(ui, app.place == place, &label, n, &tokens)
                } else {
                    // The tree is folders.
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                    icons::folder(ui.painter(), rect, folder);
                    ui.selectable_label(app.place == place, text)
                };
                if response.clicked() {
                    chosen = Some(place.clone());
                }
                response.context_menu(|ui| {
                    for (label, action) in tree_menu(&place) {
                        if ui.button(t(label)).clicked() {
                            asked = Some((place.clone(), action));
                            ui.close();
                        }
                    }
                });
            });
        };
        egui::CollapsingHeader::new(t("Everything"))
            .default_open(true)
            .show(ui, |ui| {
                place(ui, self, Place::All, t("All").into());
                for c in Category::ALL {
                    let label = crate::i18n::name(&format::titled(&category_word(c)));
                    place(ui, self, Place::Kind(c), label);
                }
                for custom in &self.applied.custom_categories {
                    let name = custom.name.trim().to_string();
                    if !name.is_empty() {
                        place(ui, self, Place::Custom(name.clone()), name);
                    }
                }
            });
        place(ui, self, Place::Unfinished, t("In progress").into());
        place(ui, self, Place::Finished, t("Done").into());
        let queues = self.backend.queues();
        egui::CollapsingHeader::new(t("Queues"))
            .default_open(true)
            .show(ui, |ui| {
                for name in queues {
                    let label = crate::i18n::name(&name);
                    place(ui, self, Place::Queue(name.clone()), label);
                }
            });
        if let Some(place) = chosen {
            self.place = place;
        }
        if let Some((place, action)) = asked {
            self.tree_action(place, action);
        }
    }

    /// Runs what a place's right-click menu asked for. The place is chosen
    /// first, as a right-click selects it.
    pub fn tree_action(&mut self, place: Place, action: TreeAction) {
        self.place = place.clone();
        match action {
            TreeAction::Run(command) => self.run(command),
            TreeAction::EditCategories => {
                self.dialogs.open_options(&self.applied);
                if let Some(options) = self.dialogs.options.as_mut() {
                    options.tab = crate::dialogs::Tab::SaveTo;
                }
            }
            TreeAction::OpenFolder => {
                let root = self.applied.download_dir.clone();
                let categories = self.applied.categories();
                let dir = match &place {
                    Place::Kind(kind) => categories.destination(&root, *kind),
                    Place::Custom(name) => root.join(name),
                    _ => root,
                };
                if let Err(e) = self.backend.platform.open(&dir.to_string_lossy()) {
                    self.said = e.into();
                }
            }
        }
    }

    fn clipboard_bar(&mut self, ui: &mut egui::Ui) {
        let Some(url) = self.clipboard.offered.clone() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(t("Link on your clipboard:"));
            ui.label(RichText::new(format::shown_url(&url)).monospace());
            if ui.button(t("Download")).clicked() {
                self.clipboard.dismissed.insert(url.clone());
                self.clipboard.offered = None;
                self.dialogs.open_file_info(vec![url.clone()]);
            }
            if ui.button(t("Dismiss")).clicked() {
                self.clipboard.dismissed.insert(url.clone());
                self.clipboard.offered = None;
            }
        });
        ui.separator();
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let running: Vec<Entry> = self
            .rows
            .iter()
            .filter(|r| r.state == State::Running)
            .cloned()
            .collect();
        let rate: f64 = running.iter().map(|r| r.progress.speed).sum();
        ui.horizontal(|ui| {
            ui.label(if running.is_empty() {
                t("Nothing downloading").to_string()
            } else {
                tf(
                    "{} downloading at {}",
                    &[&running.len(), &format::rate(rate)],
                )
            });
            if self.applied.speed_limit > 0 {
                ui.label(tf("· limited to {} KB/s", &[&self.applied.speed_limit]));
            }
            if let Some(wait) = self.backend.queue.allowance_spent() {
                ui.label(tf(
                    "· download limit of {} MB reached, held for {}",
                    &[&self.applied.limit_mb, &format::span(wait)],
                ));
            } else if let Some(at) = self.schedule_holds() {
                // Said while the schedule is what downloads wait for, so a
                // queue that does nothing is not taken for a broken one.
                ui.label(match at {
                    Some(at) => tf("· waiting for the schedule (opens at {})", &[&at]),
                    None => t("· waiting for the schedule").to_string(),
                });
            }
            // The limiter's own sentence is not said again beside the
            // standing "limited to" above. Nor is one left standing long
            // after: it named rows that may be gone since.
            if self
                .said_at
                .as_ref()
                .is_none_or(|(said, _)| *said != self.said)
            {
                self.said_at = Some((self.said.clone(), Instant::now()));
            }
            let fresh = self
                .said_at
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() < SAID_FOR);
            // "Waiting for the schedule" only while something waits for it:
            // left standing, it described a download running since.
            let stale = self.said.is_schedule_wait()
                && !self
                    .rows
                    .iter()
                    .any(|r| r.state == State::Paused && r.paused_by_schedule);
            if fresh && !stale && !self.said.is_empty() && !self.said.is_limiter() {
                // Its first line, cut short to the room there is with the
                // whole of it on hover: an opener's error output ran off the
                // window's edge.
                let whole = self.said.shown();
                let short = whole.lines().next().unwrap_or_default().to_string();
                let room = (ui.available_width() - 120.0).max(80.0);
                ui.scope(|ui| {
                    ui.set_max_width(room);
                    let drawn = ui.add(
                        egui::Label::new(format!("· {}", crate::bidi::visual(&short))).truncate(),
                    );
                    crate::bidi::said_as(&drawn, &format!("· {short}"));
                    drawn.on_hover_text(whole);
                });
                ui.ctx().request_repaint_after(SAID_FOR);
            }
            // The folder in what room is left, cut short with an ellipsis
            // (its whole path on hover) rather than drawn over the words.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.available_width() > 40.0 {
                    let folder = self.applied.download_dir.display().to_string();
                    let drawn = ui.add(
                        egui::Label::new(
                            RichText::new(&*crate::bidi::visual(&folder)).small().weak(),
                        )
                        .truncate(),
                    );
                    crate::bidi::said_as(&drawn, &folder);
                    drawn.on_hover_text(folder);
                }
            });
        });
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

    fn problem_list(&mut self, ui: &mut egui::Ui) {
        let mut dismissed = None;
        for (i, problem) in self.problems.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(Color32::from_rgb(0xd9, 0x7a, 0x2b), problem);
                if ui.button(t("Dismiss")).clicked() {
                    dismissed = Some(i);
                }
            });
        }
        if let Some(i) = dismissed {
            self.problems.remove(i);
        }
    }
}

/// What a right-click on a place in the category tree can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeAction {
    Run(Command),
    /// Options › Save to, where categories are added, edited and deleted.
    EditCategories,
    /// The folder a category's files go to.
    OpenFolder,
}

/// The ◀ ▶ pair that moves a column or a toolbar button a place earlier or
/// later, named for a screen reader and on hover; where the click moves it
/// to, as a function of where it is.
fn move_buttons(
    ui: &mut egui::Ui,
    name: &str,
    earlier: bool,
    later: bool,
) -> Option<fn(usize) -> usize> {
    let mut moved: Option<fn(usize) -> usize> = None;
    for (arrow, enabled, said, to) in [
        (
            "◀",
            earlier,
            tf("Move {} left", &[&name]),
            (|at: usize| at - 1) as fn(usize) -> usize,
        ),
        ("▶", later, tf("Move {} right", &[&name]), |at: usize| {
            at + 1
        }),
    ] {
        let button = ui.add_enabled(enabled, egui::Button::new(arrow).small());
        button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &said));
        if button.on_hover_text(said.clone()).clicked() {
            moved = Some(to);
        }
    }
    moved
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
    words.extend(crate::dialogs::QUALITIES.iter().map(|(_, w)| *w));
    words.extend(crate::dialogs::DAYS);
    words.extend(TOOL_STYLES.iter().map(|(_, w)| *w));
    words.extend(TOOLBAR.iter().map(|(_, _, w)| *w));
    words.extend(Command::ALL.iter().map(|c| c.label()));
    words.extend(Column::ALL.iter().map(|c| c.english()));
    words.extend(crate::power::WhenDone::ALL.iter().map(|w| w.verb()));
    words.extend(crate::i18n::BUILT_IN_NAMES);
    words.extend(crate::format::MONTHS);
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
            ("Stop all", TreeAction::Run(Command::StopAll)),
        ],
        Place::Finished => vec![(
            "Clear all finished",
            TreeAction::Run(Command::DeleteCompleted),
        )],
        Place::Queue(_) => vec![
            ("Start queue", TreeAction::Run(Command::StartQueue)),
            ("Stop queue", TreeAction::Run(Command::StopQueue)),
            ("Scheduler…", TreeAction::Run(Command::Scheduler)),
        ],
    }
}

/// How wide a modern toolbar button is: its name beside its picture, or
/// (`stacked`) under it in smaller type.
fn modern_width(ui: &egui::Ui, label: &str, stacked: bool) -> f32 {
    let size = if stacked { 11.5 } else { 14.0 };
    let text = ui
        .painter()
        .layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(size),
            egui::Color32::WHITE,
        )
        .size()
        .x;
    if stacked {
        (text + 14.0).max(46.0)
    } else {
        12.0 + 20.0 + 8.0 + text + 12.0
    }
}

/// A modern toolbar button: the picture beside its name, 32 px tall,
/// subtle until hovered; the primary one is filled with the brand colour.
/// `stacked`, the name goes under the picture, where a row is too narrow
/// for them side by side.
fn modern_tool(
    ui: &mut egui::Ui,
    icon: Icon,
    (label, widest): (&str, &str),
    enabled: bool,
    primary: bool,
    t: &crate::theme::Tokens,
    stacked: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(if stacked { 11.5 } else { 14.0 });
    let text_color = if !enabled {
        t.disabled
    } else if primary {
        t.on_brand
    } else {
        t.text
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, text_color);
    let size = if stacked {
        egui::vec2(modern_width(ui, widest, true), 44.0)
    } else {
        egui::vec2(modern_width(ui, widest, false), 32.0)
    };
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let pressed = response.is_pointer_button_down_on();
    let fill = match (primary && enabled, response.hovered() && enabled, pressed) {
        (true, _, true) => t.brand_hover,
        (true, true, _) => t.brand_hover,
        (true, false, _) => t.brand,
        (false, _, true) => t.pressed,
        (false, true, _) => t.hover,
        _ => egui::Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    let picture = if !enabled {
        t.disabled
    } else if primary {
        t.on_brand
    } else {
        t.tool(icon, true)
    };
    let (icon_at, text_at) = if stacked {
        (
            egui::pos2(rect.center().x, rect.top() + 14.0),
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.bottom() - 6.0 - galley.size().y,
            ),
        )
    } else {
        (
            egui::pos2(rect.left() + 12.0 + 10.0, rect.center().y),
            egui::pos2(rect.left() + 40.0, rect.center().y - galley.size().y / 2.0),
        )
    };
    let icon_rect = egui::Rect::from_center_size(icon_at, egui::vec2(18.0, 18.0));
    icons::paint(ui.painter(), icon_rect, icon, picture);
    ui.painter().galley(text_at, galley, text_color);
    response
}

/// One place in the modern navigation pane: a rounded row, the selected one
/// marked with the brand indicator, its count at the right. Its name for a
/// screen reader is "Label (n)", as the classic tree's was.
fn nav_item(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    count: usize,
    t: &crate::theme::Tokens,
) -> egui::Response {
    let width = ui.available_width().max(120.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let name = format!("{label} ({count})");
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    let fill = if selected {
        t.selection
    } else if response.hovered() {
        t.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius, fill);
    if selected {
        let pill = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 3.0, rect.center().y),
            egui::vec2(3.0, 16.0),
        );
        painter.rect_filled(pill, 2.0, t.brand);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 20.0, rect.center().y),
        egui::vec2(16.0, 14.0),
    );
    icons::folder_outline(
        painter,
        icon_rect,
        if selected { t.accent } else { t.folder },
    );
    let family = if selected {
        crate::theme::bold(ui.ctx())
    } else {
        egui::FontFamily::Proportional
    };
    // A long name is shortened with "…" before the count, never over it.
    let room = (rect.width() - 36.0 - 34.0).max(20.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_string(),
        egui::FontId::new(14.0, family),
        t.text,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let painter = ui.painter();
    painter.galley(
        egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    painter.text(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(12.0),
        t.weak,
    );
    response
}

/// The toolbar's buttons, in their default order.
const TOOLBAR: [(Command, Icon, &str); 11] = [
    (Command::AddUrl, Icon::Add, "Add URL"),
    (Command::DownloadNow, Icon::Resume, "Resume"),
    (Command::StopDownload, Icon::Stop, "Stop"),
    (Command::StopAll, Icon::StopAll, "Stop all"),
    (Command::Remove, Icon::Delete, "Delete"),
    (
        Command::DeleteCompleted,
        Icon::DeleteCompleted,
        "Clear finished",
    ),
    (Command::Options, Icon::Options, "Options"),
    (Command::Scheduler, Icon::Scheduler, "Scheduler"),
    (Command::StartQueue, Icon::StartQueue, "Start queue"),
    (Command::StopQueue, Icon::StopQueue, "Stop queue"),
    (Command::SiteFiles, Icon::SiteFiles, "Site files"),
];

/// The toolbar's buttons in the order a person chose (by command id), with
/// any not named after them in the default order. Unknown ids are skipped.
pub fn toolbar_in(order: &[String]) -> Vec<(Command, Icon, &'static str)> {
    let mut buttons: Vec<(Command, Icon, &'static str)> = Vec::new();
    // A hand-edited settings file can name a button twice; it is drawn once.
    for id in order {
        if let Some(button) = TOOLBAR.iter().find(|(c, _, _)| c.id() == id) {
            if !buttons.iter().any(|(c, _, _)| *c == button.0) {
                buttons.push(*button);
            }
        }
    }
    for button in TOOLBAR {
        if !buttons.iter().any(|(c, _, _)| *c == button.0) {
            buttons.push(button);
        }
    }
    buttons
}

/// The toolbar styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStyle {
    Both,
    Icons,
    Text,
}

impl ToolStyle {
    pub fn named(name: &str) -> ToolStyle {
        match name {
            "icons" => ToolStyle::Icons,
            "text" => ToolStyle::Text,
            _ => ToolStyle::Both,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ToolStyle::Both => "both",
            ToolStyle::Icons => "icons",
            ToolStyle::Text => "text",
        }
    }
}

/// One toolbar button. Small ones show only the icon, text-only ones only
/// the name; the name stays for a screen reader either way.
fn tool(
    ui: &mut egui::Ui,
    icon: Icon,
    (label, widest): (&str, &str),
    enabled: bool,
    style: ToolStyle,
    primary: bool,
) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    if t.modern && style == ToolStyle::Both {
        return modern_tool(ui, icon, (label, widest), enabled, primary, &t, false);
    }
    let small = style == ToolStyle::Icons;
    let size = match style {
        ToolStyle::Icons => egui::vec2(34.0, 30.0),
        ToolStyle::Text => egui::vec2(
            label.chars().count().max(widest.chars().count()) as f32 * 7.0 + 18.0,
            28.0,
        ),
        ToolStyle::Both => egui::vec2(72.0, 52.0),
    };
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let visuals = if enabled {
        ui.style().interact(&response)
    } else {
        &ui.style().visuals.widgets.noninteractive
    };
    if response.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, 4.0, ui.style().visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        ui.style().visuals.weak_text_color()
    };
    if style == ToolStyle::Text {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
        return response;
    }
    let icon_rect = if small {
        egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0))
    } else {
        egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 18.0),
            egui::vec2(22.0, 22.0),
        )
    };
    // The picture in colour, its name in the text colour.
    let picture = if enabled {
        crate::theme::Tokens::get(ui.ctx()).tool(icon, true)
    } else {
        color
    };
    icons::paint(ui.painter(), icon_rect, icon, picture);
    if small {
        return response;
    }
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 10.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.0),
        color,
    );
    response
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

/// How two rows compare under a column, for sorting.
fn compare(column: Column, a: &Entry, b: &Entry) -> std::cmp::Ordering {
    match column {
        // "part 2" before "part 10", as a person reads them.
        Column::Name => format::natural(&name_of(a), &name_of(b)),
        Column::Queue => a.queue.cmp(&b.queue),
        // By the size the column shows, which for a file of unknown length
        // is what has arrived of it.
        Column::Size => crate::table::shown_size(a).cmp(&crate::table::shown_size(b)),
        // By state, then how far along: the same order in every language,
        // and 9% before 45% rather than after it as text would put it.
        Column::Status => {
            let rank = |e: &Entry| match e.state {
                State::Running => 0,
                State::Queued => 1,
                State::Paused => 2,
                State::Failed(_) => 3,
                State::Cancelled => 4,
                State::Completed => 5,
            };
            rank(a).cmp(&rank(b)).then_with(|| {
                a.progress
                    .fraction()
                    .partial_cmp(&b.progress.fraction())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        }
        // Only a running row shows a time left or a speed; a stopped one's
        // last figures are stale, and sorted as the blank they are shown as.
        Column::TimeLeft => {
            let left = |e: &Entry| {
                (e.state == State::Running)
                    .then_some(e.progress.eta)
                    .flatten()
                    .filter(|d| !d.is_zero())
            };
            left(a).cmp(&left(b))
        }
        Column::Rate => {
            let speed = |e: &Entry| {
                (e.state == State::Running && e.progress.speed > 1.0).then_some(e.progress.speed)
            };
            speed(a)
                .partial_cmp(&speed(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        Column::LastTry => a.finished_at.cmp(&b.finished_at),
        Column::Description => a.description.cmp(&b.description),
        Column::Referrer => a.referrer.cmp(&b.referrer),
        Column::Folder => folder_of(a).cmp(&folder_of(b)),
    }
}

/// What "When done" watches the list for, apart from the window so it can
/// be tested: see `DmApp::when_queue_dry` for what counts as done.
#[derive(Debug, Default)]
struct DryWatch {
    /// Something was downloading since the queue last ran dry.
    busy_seen: bool,
    /// A row finished or failed since the queue last ran dry.
    settled_seen: bool,
}

impl DryWatch {
    /// One look at the list, with each row's state at the last look. True
    /// once, when a stretch of downloading has just ended with something
    /// finished or failed and nothing left that dm will finish by itself.
    fn look(&mut self, rows: &[Entry], last: &HashMap<Id, State>) -> bool {
        let settled = rows.iter().any(|r| {
            matches!(r.state, State::Completed | State::Failed(_))
                && last.get(&r.id).is_some_and(|was| *was != r.state)
        });
        if settled {
            self.settled_seen = true;
        }
        let busy = rows.iter().any(|r| match r.state {
            State::Running | State::Queued => true,
            State::Paused => r.paused_by_schedule,
            _ => false,
        });
        if busy {
            self.busy_seen = true;
            return false;
        }
        // Both taken at every dry look, so neither outlives the stretch it
        // was seen in: a row that settled while nothing was busy (a late
        // outcome written over a paused row) once left `settled_seen` set,
        // and a later stretch ended by Stop all fired (gap 92).
        let busy_seen = std::mem::take(&mut self.busy_seen);
        let settled_seen = std::mem::take(&mut self.settled_seen);
        busy_seen && settled_seen
    }
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
mod status_tests {
    use super::*;

    pub(super) fn paused(by_schedule: bool) -> Entry {
        Entry {
            id: 1,
            url: "https://e.com/f.bin".into(),
            state: State::Paused,
            progress: dm_core::Progress {
                downloaded: 0,
                total: None,
                speed: 0.0,
                eta: None,
            },
            filename: "f.bin".into(),
            finished_at: None,
            category: None,
            checksum: None,
            verified: false,
            mirrors: 0,
            path: String::new(),
            referrer: String::new(),
            paused_by_schedule: by_schedule,
            description: String::new(),
            queue: String::new(),
            save_dir: None,
            save_name: None,
            validator: None,
            quality: None,
            parts_dir: String::new(),
            cookies: String::new(),
            replaces: None,
        }
    }

    #[test]
    fn a_closed_schedule_says_when_it_next_opens() {
        let schedule = dm_core::Schedule {
            enabled: true,
            from: "01:00".into(),
            to: "07:00".into(),
            days: vec![1],
        };
        // Monday 00:30: later today. Monday 08:00: next Monday.
        assert_eq!(opens_after(&schedule, 1, 30), Some((0, 60)));
        assert_eq!(opens_after(&schedule, 1, 480), Some((7, 60)));
        let never = dm_core::Schedule {
            days: vec![],
            from: "bad".into(),
            ..schedule
        };
        // A time that is not one is no schedule: open at once.
        assert_eq!(opens_after(&never, 1, 480), Some((0, 481)));
        assert!(needs_sign_in("the server answered 401 Unauthorized for x"));
        assert!(!needs_sign_in(
            "the server answered 404 Not Found for /4013"
        ));
    }

    #[test]
    fn a_row_held_by_the_schedule_or_the_limit_is_waiting_not_stopped() {
        assert_eq!(status_in_english(&paused(true)), "Waiting");
        assert_eq!(status_in_english(&paused(false)), "Stopped");
        let mut part_way = paused(false);
        part_way.progress.downloaded = 45;
        part_way.progress.total = Some(100);
        assert_eq!(status_in_english(&part_way), "Stopped · 45%");
    }

    #[test]
    fn a_download_of_unknown_length_says_what_has_arrived_and_how_fast() {
        let mut row = paused(false);
        row.state = State::Running;
        assert_eq!(status_in_english(&row), "Starting…");
        row.progress.downloaded = 3 * 1024 * 1024;
        row.progress.speed = 2048.0;
        assert_eq!(status_in_english(&row), "3.0 MB · 2.0 KB/s");
    }

    #[test]
    fn columns_sort_by_what_they_show() {
        use std::cmp::Ordering::*;
        let named = |name: &str| {
            let mut row = paused(false);
            row.filename = name.into();
            row
        };
        // Names: numbers by value.
        assert_eq!(
            compare(Column::Name, &named("part 2"), &named("part 10")),
            Less
        );
        // Size: what has arrived of a file of unknown length is its size.
        let (mut big, mut small) = (named("a"), named("b"));
        big.progress.downloaded = 5000;
        small.progress.total = Some(10);
        assert_eq!(compare(Column::Size, &big, &small), Greater);
        // Speed and time left: a stopped row's last figures are not shown,
        // and do not sort it ahead of one that is running.
        let (mut stopped, mut running) = (named("a"), named("b"));
        stopped.progress.speed = 9e9;
        stopped.progress.eta = Some(std::time::Duration::from_secs(1));
        running.state = State::Running;
        running.progress.speed = 100.0;
        running.progress.eta = Some(std::time::Duration::from_secs(500));
        assert_eq!(compare(Column::Rate, &stopped, &running), Less);
        assert_eq!(compare(Column::TimeLeft, &stopped, &running), Less);
    }
}

#[cfg(test)]
mod toolbar_tests {
    use super::*;

    #[test]
    fn the_toolbar_keeps_a_chosen_order_and_puts_the_rest_after_it() {
        let ids = |order: &[&str]| -> Vec<&'static str> {
            let order: Vec<String> = order.iter().map(|s| s.to_string()).collect();
            toolbar_in(&order).iter().map(|(c, _, _)| c.id()).collect()
        };
        let default = ids(&[]);
        assert_eq!(default.len(), TOOLBAR.len());
        assert_eq!(default[0], Command::AddUrl.id());
        let moved = ids(&[
            "program.site_files",
            "no.such.command",
            "program.site_files",
        ]);
        assert_eq!(moved[0], "program.site_files");
        assert_eq!(
            moved.len(),
            TOOLBAR.len(),
            "each button once, unknown ids dropped"
        );
        assert_eq!(ToolStyle::named("text"), ToolStyle::Text);
        assert_eq!(ToolStyle::named("anything"), ToolStyle::Both);
    }

    #[test]
    fn a_browser_dragged_url_is_read_from_its_path_when_bytes_cannot_be_read() {
        let path = std::env::temp_dir().join(format!("dm-browser-drag-{}.txt", std::process::id()));
        std::fs::write(&path, b"https://example.com/file.bin\n").unwrap();

        #[derive(Debug)]
        struct FakeDroppedFile {
            path: std::path::PathBuf,
            bytes: Result<Vec<u8>, String>,
        }

        impl egui::DroppedFile for FakeDroppedFile {
            fn path(&self) -> &std::path::Path {
                &self.path
            }

            fn bytes(&self) -> Result<Vec<u8>, String> {
                self.bytes.clone()
            }
        }

        let file = FakeDroppedFile {
            path: path.clone(),
            bytes: Err("this source does not expose file bytes".to_string()),
        };

        let found = dropped_links(&file);
        assert_eq!(found, vec!["https://example.com/file.bin"]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_dropped_file_is_read_for_links_only_if_it_is_text_and_only_its_start() {
        #[derive(Debug)]
        struct Dropped(std::path::PathBuf);
        impl egui::DroppedFile for Dropped {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                // What a drop hands over: the whole file.
                std::fs::read(&self.0).map_err(|e| e.to_string())
            }
        }
        let dir = tempfile::tempdir().unwrap();

        // A PDF with an address inside it queues nothing.
        let pdf = dir.path().join("paper.pdf");
        std::fs::write(&pdf, b"%PDF-1.7 https://example.com/cited.zip").unwrap();
        assert!(dropped_links(&Dropped(pdf)).is_empty());

        // A list is read, but only its first 64 KB.
        let list = dir.path().join("links.txt");
        let mut text = String::from("https://example.com/first.bin\n");
        text.push_str(&" ".repeat(DROP_LOOK as usize));
        text.push_str("https://example.com/past-the-start.bin\n");
        std::fs::write(&list, text).unwrap();
        assert_eq!(
            dropped_links(&Dropped(list)),
            vec!["https://example.com/first.bin"]
        );

        // A file with no extension that is not text is not read for links.
        let blob = dir.path().join("blob");
        std::fs::write(&blob, b"\0\x01https://example.com/x.bin").unwrap();
        assert!(dropped_links(&Dropped(blob)).is_empty());
    }
}

#[cfg(test)]
mod dry_tests {
    use super::*;

    fn row(id: Id, state: State) -> Entry {
        let mut row = status_tests::paused(false);
        row.id = id;
        row.state = state;
        row
    }

    fn states(rows: &[Entry]) -> HashMap<Id, State> {
        rows.iter().map(|r| (r.id, r.state.clone())).collect()
    }

    /// Gap 92: a row that settled while nothing was busy (a late outcome
    /// over a paused row) must not count for a later stretch that a person
    /// ended with Stop all.
    #[test]
    fn a_settle_seen_while_idle_does_not_carry_into_the_next_stretch() {
        let mut watch = DryWatch::default();
        let paused = vec![row(1, State::Paused)];
        let mut last = states(&paused);
        let done = vec![row(1, State::Completed)];
        assert!(!watch.look(&done, &last), "nothing was busy");
        last = states(&done);

        // A new stretch, ended by a person's stop with nothing finished.
        let running = vec![row(1, State::Completed), row(2, State::Running)];
        assert!(!watch.look(&running, &last));
        last = states(&running);
        let stopped = vec![row(1, State::Completed), row(2, State::Paused)];
        assert!(
            !watch.look(&stopped, &last),
            "fired on a queue a person stopped"
        );
    }

    #[test]
    fn a_stretch_that_finishes_something_fires_once() {
        let mut watch = DryWatch::default();
        let running = vec![row(1, State::Running)];
        assert!(!watch.look(&running, &HashMap::new()));
        let done = vec![row(1, State::Completed)];
        assert!(watch.look(&done, &states(&running)));
        assert!(!watch.look(&done, &states(&done)), "fired twice");
    }
}
