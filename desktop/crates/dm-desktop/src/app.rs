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
use crate::commands::Command;
use crate::dialogs::Dialogs;
use crate::format;
use crate::icons::{self, Icon};
use crate::update::Update;

/// How often the list is read back from the queue while the window shows.
const REFRESH: Duration = Duration::from_millis(250);
/// The smallest the window may be, on screen at 100% text.
pub const MIN_WINDOW: (f32, f32) = (720.0, 460.0);

/// How often it is read back while nothing downloads.
const IDLE_REFRESH: Duration = Duration::from_secs(1);
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
    if !row.filename.is_empty() {
        return row.filename.clone();
    }
    // The name chosen in Download details, until the file is saved under
    // it: a row added for later showed its address's name (round 13).
    match row.save_name.as_deref().map(str::trim) {
        Some(chosen) if !chosen.is_empty() => chosen.to_string(),
        _ => format::provisional(&row.url),
    }
}

/// A row's kind: what its name says, else what dm learned from the server.
/// Queued rows had none, so Videos listed one of a hundred queued videos
/// and their icons were blank (UI round 6); and with what the server said
/// first, a file renamed to "report (1).pdf" stayed under Other (UI round
/// 13). One named "get.php" is still known by what the server sent.
pub fn kind_of(row: &Entry) -> Option<Category> {
    dm_core::category::by_name(&name_of(row)).or(row.category)
}

/// Whether the pointer is in `rect`.
fn at_inside(ctx: &egui::Context, rect: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.interact_pos())
        .is_some_and(|at| rect.contains(at))
}

/// What the Status column says.
/// A site search's name and what it found, or why it could not.
type SearchResult = (String, Result<dm_shell::lists::Crawled, String>);

pub fn status_of(row: &Entry) -> String {
    status_in(row, t)
}

thread_local! {
    /// The rows a range is being retried for, as the last refresh found
    /// them: the attempt and how many there may be (0 for no end). Kept
    /// beside the rows, which the engine hands over without it.
    static RETRYING: std::cell::RefCell<HashMap<Id, (u32, u32)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// What the engine now says about retries, for the statuses drawn next.
pub fn set_retrying(now: HashMap<Id, (u32, u32)>) {
    RETRYING.with(|r| *r.borrow_mut() = now);
}

thread_local! {
    /// Whether the download limit, not the schedule, is what holds rows
    /// that wait by themselves, as the last refresh found it.
    static LIMIT_HOLDS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What holds waiting rows now, for the statuses drawn next.
pub fn set_limit_holds(now: bool) {
    LIMIT_HOLDS.with(|l| l.set(now));
}

pub(crate) fn retrying(id: Id) -> Option<(u32, u32)> {
    RETRYING.with(|r| r.borrow().get(&id).copied())
}

/// What the engine said of a failure, word for word, where the status
/// says it in plainer words: shown on hover (gap 153).
pub fn failure_as_said(row: &Entry) -> Option<&str> {
    match &row.state {
        State::Failed(raw) if format::reason(raw).is_some() => Some(raw),
        _ => None,
    }
}

/// A running row that has every byte of a file of known length: it is
/// joining its parts, not fetching.
pub fn fetched_all(row: &Entry) -> bool {
    row.progress
        .total
        .is_some_and(|total| total > 0 && row.progress.downloaded >= total)
}

/// Whether a row was added for later and waits for its queue: stopped,
/// in a queue, nothing fetched and not held by the schedule.
pub fn waits_for_its_queue(row: &Entry) -> bool {
    row.state == State::Paused
        && !row.paused_by_schedule
        && !row.queue.is_empty()
        && row.progress.downloaded == 0
}

/// How soon the same address handed over again is taken for the same
/// click.
const HANDED_TWICE_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// A fraction as a whole percent, rounded down.
pub fn whole_percent(fraction: f64) -> u32 {
    (fraction * 100.0).clamp(0.0, 100.0).floor() as u32
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
        // Said while a range waits to be tried again: the bar drops back
        // and climbs, which read as a fault (gap 155).
        State::Running if retrying(row.id).is_some() => match retrying(row.id) {
            Some((attempt, of)) if of > 0 => {
                crate::i18n::fill(words("Retrying (attempt {} of {})"), &[&attempt, &of])
            }
            Some((attempt, _)) => crate::i18n::fill(words("Retrying (attempt {})"), &[&attempt]),
            None => String::new(),
        },
        State::Running => match row.progress.fraction() {
            // Whole percents everywhere, rounded down so 99.9 is not 100:
            // the list said 28.88% beside a bar saying 28% (UI round 1).
            Some(f) => format!("{}%", whole_percent(f)),
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
            // A torrent with nothing yet is looking for peers, for as long
            // as that takes; "Starting…" read as stuck (torrent check,
            // 2026-10-19).
            None if dm_core::is_torrent(&row.url) => words("Finding peers…").into(),
            None => words("Starting…").into(),
        },
        State::Queued => words("Queued").into(),
        // Held by the schedule or the download limit, not by a person: it
        // goes on by itself, which "Stopped" would not say.
        // Saying what for: "Waiting" alone read as stuck (UI round 8).
        State::Paused if row.paused_by_schedule => {
            if LIMIT_HOLDS.with(std::cell::Cell::get) {
                words("Waiting for the download limit").into()
            } else {
                words("Waiting for the schedule").into()
            }
        }
        // Added for later: it waits for its queue to be started, and does
        // not start by itself, which "Queued" did not say (gap 167).
        State::Paused if waits_for_its_queue(row) => words("Waiting for Start queue").into(),
        // How far it got, so a stopped row is not mistaken for one that
        // never started.
        State::Paused => match row.progress.fraction() {
            Some(f) if f > 0.0 => format!("{} · {}%", words("Stopped"), whole_percent(f)),
            _ => words("Stopped").into(),
        },
        // Its file moved or deleted since: said so in Properties and to the
        // control protocol too, as the list says it (UI round 8: Properties
        // said "Complete").
        State::Completed if crate::table::gone(row) => words("File missing").into(),
        State::Completed => words("Complete").into(),
        // A server that wants a sign-in says where to give one.
        State::Failed(reason) if needs_sign_in(reason) => format!(
            "{} — {}",
            crate::i18n::fill(words("Error: {}"), &[reason]),
            words("it needs a sign-in: Properties › Login")
        ),
        // In plain words where dm knows them; the engine's own on hover.
        State::Failed(reason) => crate::i18n::fill(
            words("Error: {}"),
            &[&format::reason(reason).map_or(reason.as_str(), words)],
        ),
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

/// Whether `text` holds a second address after its first: two links
/// pasted into one field, run together ("…/a.ziphttps://…") or with a
/// space between them (gap 142). A scheme after `=`, `/`, `?`, `&`, `#` or
/// `%` is part of the first address — a redirect's parameter, or an
/// archive's copy of a page — and is not counted.
pub fn two_addresses(text: &str) -> bool {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let schemes = ["http://", "https://", "ftp://", "magnet:?"];
    let starts = |at: usize| {
        lower
            .get(at..)
            .is_some_and(|rest| schemes.iter().any(|s| rest.starts_with(s)))
    };
    if text.split_whitespace().count() > 1
        && text
            .split_whitespace()
            .skip(1)
            .any(|w| looks_downloadable(w).is_some())
    {
        return true;
    }
    lower.char_indices().skip(1).any(|(at, _)| {
        starts(at)
            && lower[..at]
                .chars()
                .next_back()
                .is_some_and(|before| !"=/?&#%".contains(before))
    })
}

/// Links inside some text: a dropped `.webloc` or `.url` file, a list, the
/// clipboard.
///
/// Words are split on white space, quotes and angle brackets only — never
/// on `=`, which every query string holds: splitting there queued
/// `download.php?id=5` as `download.php?id` (gap 162). A word that is not
/// an address itself is looked into for one that starts after `=`, `;`,
/// `(` or `[`: a `.url` file's `URL=` or `BASEURL=`, a page's unquoted
/// `href=` or `data-src=`, a refresh's `0;url=`, a forum's `[url=…]`. And
/// `&amp;` — how a page or a `.webloc` plist writes `&` — is read as `&`.
pub fn links_in(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || "<>\"'".contains(c)) {
        let word = sentence_trimmed(&address_in(word).replace("&amp;", "&")).to_string();
        if looks_downloadable(&word).is_some() && !found.contains(&word) {
            found.push(word);
        }
    }
    found
}

/// An address without the sentence around it: a full stop, comma or
/// other mark after it, and a `)` or `]` that closes a bracket opened
/// before it. "Get http://h/e.zip." fetched `e.zip.` (round 13). A
/// bracket the address opened itself — an IPv6 host's — stays.
fn sentence_trimmed(address: &str) -> &str {
    let mut address = address;
    loop {
        let before = address.len();
        address = address.trim_end_matches(['.', ',', ';', ':', '!', '?']);
        for (open, close) in [('(', ')'), ('[', ']')] {
            if address.ends_with(close)
                && address.matches(open).count() < address.matches(close).count()
            {
                address = &address[..address.len() - 1];
            }
        }
        if address.len() == before {
            return address;
        }
    }
}

/// The address in `word`: all of it where it starts with one, else from
/// the first scheme after `=`, `;`, `(` or `[` — up to the `]` or `)`
/// that closes that bracket. The word itself where there is none.
fn address_in(word: &str) -> &str {
    if looks_downloadable(word).is_some() {
        return word;
    }
    let lower = word.to_ascii_lowercase();
    let schemes = ["http://", "https://", "ftp://", "magnet:?"];
    let start = word.char_indices().find_map(|(at, c)| {
        let next = at + c.len_utf8();
        ("=;([".contains(c)
            && lower
                .get(next..)
                .is_some_and(|rest| schemes.iter().any(|s| rest.starts_with(s))))
        .then_some((next, c))
    });
    let Some((at, opener)) = start else {
        return word;
    };
    let address = &word[at..];
    let closer = match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        // `[url=…]`: the bracket opened before the `=`.
        _ if word[..at].contains('[') => Some(']'),
        _ => None,
    };
    let Some(close) = closer else {
        return address;
    };
    // Up to the bracket that closes the one before the address; brackets
    // opened inside it (an IPv6 host's) are its own.
    let open = if close == ')' { '(' } else { '[' };
    let mut depth = 0usize;
    for (at, c) in address.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return &address[..at];
            }
            depth -= 1;
        }
    }
    address
}

/// How much of a dropped file is looked at for links.
const DROP_LOOK: u64 = 64 * 1024;

/// The kinds of file a drop is read for links: a browser's link file, a
/// list, a page (a `.torrent` is taken whole, as the download itself). Anything else — a PDF, an ISO, a photo — is not a list of
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
    // A .torrent is itself the download, read from where it lies: a drop
    // of one was ignored (torrent check, 2026-10-19).
    if kind.as_deref() == Some("torrent") && path.is_file() {
        return std::path::absolute(path)
            .ok()
            .and_then(|full| dm_core::torrent::file_url(&full))
            .into_iter()
            .collect();
    }
    if kind
        .as_deref()
        .is_some_and(|kind| !DROP_KINDS.contains(&kind))
    {
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
    // A binary plist (a .webloc as some Macs write it) keeps its strings
    // as plain bytes between others.
    if head.starts_with(b"bplist00") {
        let text: String = String::from_utf8_lossy(&head)
            .chars()
            .map(|c| if c.is_ascii_graphic() { c } else { ' ' })
            .collect();
        return links_in(&text);
    }
    // A file with no extension is read only if it looks like text.
    if head.contains(&0) {
        return Vec::new();
    }
    links_in_file(kind.as_deref(), &String::from_utf8_lossy(&head))
}

/// The links a file of this kind is for. A `.webloc` is the one address
/// under its `URL` key, not the address of the plist format its first lines
/// name (Apple's DTD was queued beside the link); a `.url` is its `URL=`
/// line, not its icon's address. Anything else is read for every link.
pub fn links_in_file(kind: Option<&str>, text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    if kind == Some("webloc") || lower.contains("<plist") {
        let value = lower.find("<key>url</key>").and_then(|key| {
            let rest = &text[key..];
            let open = rest.find("<string>")? + "<string>".len();
            let close = rest[open..].find("</string>")?;
            Some(&rest[open..open + close])
        });
        if let Some(value) = value {
            return links_in(value);
        }
    }
    if kind == Some("url") || lower.contains("[internetshortcut]") {
        let line = text.lines().find(|l| {
            l.trim_start()
                .get(..4)
                .is_some_and(|k| k.eq_ignore_ascii_case("url="))
        });
        if let Some(line) = line {
            return links_in(line);
        }
    }
    links_in(text)
}

/// What a drop with no link in it is answered with, and what the empty list
/// says: a link dragged from a browser is not handed to dm by the window
/// system, only files are.
pub const DROP_HINT: &str =
    "Drop a .torrent, a .url or .webloc file, or a text file of links, here. A link dragged straight from a browser cannot be dropped yet: use Add URL or the browser's right-click menu.";

/// The clipboard watcher's memory: offered once, never again once dismissed,
/// and nothing offered for what was copied before it was turned on.
#[derive(Default)]
pub struct Clipboard {
    pub on: bool,
    pub last: Option<String>,
    pub offered: Option<String>,
    /// Every link offered, when the clipboard holds several and nothing
    /// else: only the first was offered and the rest dropped (UI round 14).
    pub offered_all: Vec<String>,
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
    /// A row being dragged to another place in the queue's order.
    pub reordering: Option<Id>,
    /// Where each row was drawn last frame, in window points, so the control
    /// protocol can tell a test where to point.
    pub row_rects: HashMap<Id, egui::Rect>,
    /// What of the list's rows could be seen last frame, in window points.
    pub list_clip: Option<egui::Rect>,
    /// Where the list's up-and-down scroll bar is drawn: at the list's
    /// visible right edge, whatever is scrolled sideways (gap 152).
    pub list_bar: Option<egui::Rect>,
    /// A place that bar was dragged or wheeled to, for the list to take.
    pub list_bar_moved: Option<f32>,
    /// Whether the list has been drawn since launch: it starts at its
    /// left edge, with Name in view, whatever egui remembered.
    pub list_drawn: bool,
    /// How many rows Page Up and Page Down move: a screenful, less one.
    pub list_page: usize,
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
    /// Why the system could not open a file or folder, said when it is known.
    opener_said: Arc<std::sync::Mutex<Option<String>>>,
    /// What had the keyboard when a dialog came in front, given it back
    /// when none is: the window behind takes no clicks meanwhile, and a
    /// control that cannot be used lets the keyboard go.
    focus_before: Option<egui::Id>,
    was_behind: bool,
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
    /// See [`Self::place_rows`].
    placed: HashMap<Id, (Option<Category>, Option<String>)>,
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
    /// Addresses the browser handed over in the last few seconds.
    recent_hands: Vec<(String, Instant)>,
    /// Where the menu bar ends, last frame: windows per download open
    /// below it, never over it (gap 168).
    pub menu_bottom: f32,
    /// Where the toolbar ends (the menu bar's bottom with no toolbar):
    /// windows a download opens on its own start below it, not over Add URL.
    pub bars_bottom: f32,
    /// Each finished file's size on disk, by the path it was read at: a
    /// stream put into an MP4 is not the size of the joined stream the
    /// engine counted (gap 156). Read once a path.
    pub saved: HashMap<Id, (String, u64)>,
    /// The status line's sentence and when it was said: an old one is not
    /// left standing beside rows it no longer describes.
    said_at: Option<(crate::i18n::Said, Instant)>,
}

/// How long the status line keeps its last sentence.
// Long enough to read; not standing over what was done since (UI round 9).
const SAID_FOR: Duration = Duration::from_secs(12);

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
            reordering: None,
            row_rects: HashMap::new(),
            list_clip: None,
            list_bar: None,
            list_bar_moved: None,
            list_drawn: false,
            list_page: 1,
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
            opener_said: Arc::default(),
            focus_before: None,
            was_behind: false,
            cancelled_hand_offs: std::collections::HashMap::new(),
            dry: DryWatch::default(),
            countdown: None,
            headless: false,
            control: None,
            own: HashSet::new(),
            last_added: Vec::new(),
            said_about: None,
            sizes_told: HashMap::new(),
            placed: HashMap::new(),
            rail_language: None,
            columns_changed: false,
            recent_hands: Vec::new(),
            min_size_zoom: None,
            open_menu_by_key: false,
            menus: Vec::new(),
            field_menu: None,
            field_edits: Vec::new(),
            field_selection: None,
            row_menu_by_key: false,
            menu_bottom: 0.0,
            bars_bottom: 0.0,
            saved: HashMap::new(),
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
        self.place_rows();
        set_retrying(self.backend.retrying());
        set_limit_holds(self.backend.queue.allowance_spent().is_some());
        self.saved_sizes();
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
            let started: Vec<Id> = self
                .rows
                .iter()
                .filter(|row| {
                    !self.last_state.contains_key(&row.id)
                        && !self.own.contains(&row.id)
                        && matches!(row.state, State::Running | State::Queued)
                })
                .map(|row| row.id)
                .collect();
            // One window for one download: a batch arriving at once opened a
            // stack of them over the list (a design review counted ten).
            if let [only] = started.as_slice() {
                self.dialogs.progress.insert(*only);
            }
        }
        // A browser's hand-off of an address already listed: added by the
        // inbox before the window hears of it, and the browser has let its
        // own download go, so there is nothing left to ask. Said, as Add
        // URL would have asked (gap 159).
        if self.notify_ready {
            let fresh = |row: &&Entry| {
                !self.last_state.contains_key(&row.id) && !self.own.contains(&row.id)
            };
            let again = self.rows.iter().filter(fresh).find(|row| {
                let address = crate::dialogs::bare(&row.url);
                self.rows.iter().any(|other| {
                    other.id != row.id
                        && other.state != State::Cancelled
                        && self.last_state.contains_key(&other.id)
                        && crate::dialogs::bare(&other.url) == address
                })
            });
            if let Some(row) = again {
                self.said = crate::i18n::Said::new(
                    "Added {} again: that address was already in the list.",
                    &[&name_of(row)],
                );
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
        self.problems.extend(
            self.backend
                .problems()
                .into_iter()
                .map(|p| worded_problem(&p)),
        );
    }

    /// A finished row's size is its file's: read once from the disk for
    /// each path, and put in place of the bytes the engine counted, which
    /// for a stream remuxed into an MP4 were the joined stream's.
    fn saved_sizes(&mut self) {
        let saved = &mut self.saved;
        saved.retain(|id, _| {
            self.rows
                .iter()
                .any(|r| r.id == *id && r.state == State::Completed)
        });
        for row in &mut self.rows {
            if row.state != State::Completed || row.path.is_empty() {
                continue;
            }
            let known = saved.get(&row.id).filter(|(path, _)| *path == row.path);
            let size = match known {
                Some((_, size)) => *size,
                None => match std::fs::metadata(&row.path) {
                    Ok(meta) if meta.is_file() => {
                        saved.insert(row.id, (row.path.clone(), meta.len()));
                        meta.len()
                    }
                    _ => continue,
                },
            };
            row.progress.total = Some(size);
            row.progress.downloaded = size;
        }
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
        // The same address handed over again within a few seconds is one
        // click, not two: a double-click on a link in the browser saved
        // `setup (1).deb` and `setup (2).deb` (round 15).
        if request.links.is_empty() && self.just_taken(&request.url) {
            return;
        }
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
            match self
                .backend
                .add_with(&request.url, dm_shell::shell::add_options(&request))
            {
                Ok(_) => self.taken_from_browser(&request.url),
                Err(e) => self.said = e.into(),
            }
        } else {
            self.dialogs.open_handed(request);
        }
    }

    /// A download the browser handed over was added: one handed over again
    /// in the next few seconds is the same click.
    pub fn taken_from_browser(&mut self, url: &str) {
        self.recent_hands.push((url.to_string(), Instant::now()));
    }

    /// Whether `url` was taken from the browser a moment ago. One handed
    /// over and cancelled is not remembered: clicked again, it is meant.
    fn just_taken(&mut self, url: &str) -> bool {
        let now = Instant::now();
        self.recent_hands
            .retain(|(_, at)| now.duration_since(*at) < HANDED_TWICE_WITHIN);
        self.recent_hands.iter().any(|(taken, _)| taken == url)
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
        if let Some(why) = dm_core::lock(&self.opener_said).take() {
            self.said = why.into();
        }
        if let Some((checked, changed)) = dm_core::lock(&self.rechecked).take() {
            self.said = crate::i18n::Said::counting(
                "Checked {} finished file with its server. Changed since, and downloaded again: {}.||Checked {} finished files with their servers. Changed since, and downloaded again: {}.",
                checked as u64,
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
        // Four times a second only while something moves: idle, in the
        // tray with four finished rows, dm drew itself four times a second
        // and spent a third of a core doing it (round 15). A hand-off, a
        // control request and a tray click wake it at once regardless.
        let moving = self.countdown.is_some()
            || self
                .rows
                .iter()
                .any(|r| matches!(r.state, State::Running | State::Queued));
        ctx.request_repaint_after(if moving { REFRESH } else { IDLE_REFRESH });
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
            "ui.inspect" => {
                // Each row under the name the window shows it by, the one it
                // will be saved as (UI round 6: the control protocol said
                // the name a second copy would not get).
                let mut categories = self.applied.categories();
                let rows: Vec<_> = self
                    .visible()
                    .iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "name": self.name_it_will_get(r, &mut categories),
                            "status": status_in_english(r),
                            "queue": r.queue,
                            "description": r.description,
                            "url": format::shown_url(&r.url),
                            "rect": self.row_rects.get(&r.id).map(|b| json!([b.min.x, b.min.y, b.max.x, b.max.y])),
                        })
                    })
                    .collect();
                Ok(json!({
                    "place": format!("{:?}", self.place),
                    "selected": self.selected,
                    "sort": self.sort.map(|(c, up)| json!({ "column": c.english(), "ascending": up })),
                    "categories": self.show_categories,
                    "toolbar": self.show_toolbar,
                    "dark": self.dark,
                    "said": self.said.english(),
                    "problems": self.problems,
                    "dialogs": self.dialogs.open_names(),
                    "rows": rows,
                }))
            }
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
            self.clipboard.offered_all.clear();
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
        let fresh = |url: &String| {
            !self.clipboard.dismissed.contains(url) && !self.rows.iter().any(|r| &r.url == url)
        };
        // Text that is links and nothing else, one or several; a sentence
        // with a link in it is not an offer (UI round 12).
        let words: Vec<&str> = text.as_deref().unwrap_or("").split_whitespace().collect();
        let links: Vec<String> =
            if !words.is_empty() && words.iter().all(|w| looks_downloadable(w).is_some()) {
                let mut links: Vec<String> = Vec::new();
                for w in words {
                    if !links.iter().any(|l| l == w) {
                        links.push(w.to_string());
                    }
                }
                links.into_iter().filter(fresh).collect()
            } else {
                text.as_deref()
                    .and_then(looks_downloadable)
                    .filter(fresh)
                    .into_iter()
                    .collect()
            };
        self.clipboard.offered = links.first().cloned();
        self.clipboard.offered_all = links;
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
                // Categories of a person's own may have changed.
                self.place_rows();
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
            // A row whose kind is not known yet, and in no category of a
            // person's own, is Other: queued `.bin` rows were in no place
            // and the counts did not add up (UI round 12).
            Place::Kind(c) => match self.placed(row) {
                (Some(kind), _) => kind == *c,
                (None, custom) => *c == Category::Other && custom.is_none(),
            },
            Place::Custom(name) => self.placed(row).1.as_deref() == Some(name.as_str()),
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

    /// Each row's kind and category of a person's own, worked out once a
    /// refresh: the places' counts asked for them per row, per place, per
    /// frame, and 500 rows went past the frame budget (CI, 909e714).
    fn place_rows(&mut self) {
        let categories = self.applied.categories();
        self.placed = self
            .rows
            .iter()
            .map(|row| {
                let custom = categories
                    .custom_for(&name_of(row), &dm_core::category::host_of(&row.url))
                    .map(|c| c.name.clone());
                (row.id, (kind_of(row), custom))
            })
            .collect();
    }

    /// A row's kind and own category, from [`Self::place_rows`] when it has
    /// been placed.
    fn placed(&self, row: &Entry) -> (Option<Category>, Option<String>) {
        match self.placed.get(&row.id) {
            Some(placed) => placed.clone(),
            None => (kind_of(row), self.custom_of(row)),
        }
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
        // The selected rows as they stand, neither copied nor sorted: asked
        // for every button and menu item each frame, `targets` copied and
        // sorted the whole list each time, which a thousand rows made slow
        // (UI round 6). Order does not matter to whether a command applies.
        let targets: Vec<&Entry> = self
            .rows
            .iter()
            .filter(|r| self.selected.contains(&r.id) && self.shows(r))
            .collect();
        let any = |f: fn(&Entry) -> bool| targets.iter().any(|r| f(r));
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
            // Only where there is something to fetch again: a row that has
            // fetched nothing yet would only start (UI round 12).
            // A failure with nothing fetched is Retry's (UI round 13).
            Command::Redownload => any(|r| {
                matches!(r.state, State::Completed | State::Cancelled) || r.progress.downloaded > 0
            }),
            Command::UpdateLink => {
                targets.len() == 1
                    && (!matches!(targets[0].state, State::Running | State::Completed)
                        || retrying(targets[0].id).is_some())
            }
            Command::RemoveFromQueue => any(|r| !r.queue.is_empty()),
            // Not where it would do nothing: at the top or the bottom of
            // the order (UI round 13).
            Command::MoveUp => {
                targets.len() == 1 && self.rows.first().is_some_and(|r| r.id != targets[0].id)
            }
            Command::MoveDown => {
                targets.len() == 1 && self.rows.last().is_some_and(|r| r.id != targets[0].id)
            }
            Command::ShowProgress => any(|r| r.state != State::Completed),
            // Open folder stays: it says the file is gone, which is an answer.
            Command::OpenFolder => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && !targets[0].path.is_empty()
            }
            Command::OpenFile | Command::OpenWith => {
                targets.len() == 1
                    && targets[0].state == State::Completed
                    && !targets[0].path.is_empty()
                    && !crate::table::gone(targets[0])
            }
            // One to move and rename, or several to move together.
            Command::MoveRename => {
                !targets.is_empty()
                    && targets.iter().all(|r| {
                        r.state == State::Completed && !r.path.is_empty() && !crate::table::gone(r)
                    })
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
        // A question first: only Exit, which asks one of its own, gets past.
        if self.dialogs.asking() && command != Command::Quit {
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
                    // In the language in force (UI round 6: English always).
                    self.said = crate::i18n::Said::new("There are no links on the clipboard.", &[]);
                } else {
                    // One already in the list is offered unticked, and said
                    // so: it was fetched again as `e (1).zip` (round 14).
                    let listed = crate::dialogs::listed_addresses(self);
                    self.dialogs.clip_links = Some(
                        found
                            .into_iter()
                            .map(|l| {
                                let new = !listed.contains(&crate::dialogs::bare(&l));
                                (l, new)
                            })
                            .collect(),
                    );
                }
            }
            Command::Import => {
                if !self.backend.platform.can_choose_file() {
                    // No chooser to ask with: the path is typed (gap 144).
                    self.dialogs.focus_on("List file");
                    self.dialogs.list_file = Some(crate::dialogs::ListFile::default());
                } else if let Some(path) = self.backend.platform.choose_file(false, "") {
                    let _ = self.import_from(&path);
                }
            }
            Command::Export => {
                if !self.backend.platform.can_choose_file() {
                    self.dialogs.focus_on("List file");
                    self.dialogs.list_file = Some(crate::dialogs::ListFile {
                        save: true,
                        path: crate::dialogs::default_list_path(&self.applied)
                            .display()
                            .to_string(),
                        says: String::new(),
                    });
                } else if let Some(path) =
                    self.backend.platform.choose_file(true, "downloads.dmlist")
                {
                    let _ = self.export_to(&path);
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
                    // Not one its person closed: fifteen Resumes opened it
                    // fifteen times over the list (round 15). Show progress
                    // still opens it.
                    if self.applied.show_progress && !self.dialogs.progress_closed.contains(&row.id)
                    {
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
                self.open_file(&targets[0].path);
            }
            Command::OpenWith => {
                // Nothing to pick an application with: said, rather than a
                // menu item that did nothing.
                if !self.backend.platform.can_choose_file() {
                    self.said = crate::i18n::Said::new(
                        "No file chooser on this desktop to pick an application with; Open uses the file's usual one.",
                        &[],
                    );
                } else if let Some(app) = self.backend.platform.choose_app() {
                    if let Err(e) = self.backend.platform.open_with(&targets[0].path, &app) {
                        self.said = e.into();
                    }
                }
            }
            Command::OpenFolder => {
                self.reveal_file(&targets[0].path);
            }
            Command::MoveRename => {
                self.dialogs.focus_on("Move/Rename");
                self.dialogs.rename = Some(crate::dialogs::MoveRename {
                    ids: targets.iter().map(|r| r.id).collect(),
                    name: targets[0].filename.clone(),
                    folder: std::path::Path::new(&targets[0].path)
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                })
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
                self.said = crate::i18n::Said::counting(
                    "Deleted {} completed or cancelled download from the list.||Deleted {} completed or cancelled downloads from the list.",
                    n as u64, &[&n],
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
                // "Started 0 downloads" said nothing useful (UI round 9).
                self.said = if n == 0 {
                    crate::i18n::Said::naming("Nothing in {} was waiting to start.", &[&queue])
                } else if self.applied.schedule.enabled && !self.applied.schedule.open() {
                    // "Started" while the schedule held them, which nothing
                    // was (UI round 10).
                    crate::i18n::Said::naming(
                        "{} is waiting for the schedule; it starts by itself when the schedule opens.",
                        &[&queue],
                    )
                } else {
                    crate::i18n::Said::counting_naming(
                        "Started {} download in {}.||Started {} downloads in {}.",
                        n as u64,
                        &[&n, &queue],
                    )
                };
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
                self.said = crate::i18n::Said::counting_naming(
                    "Stopped {} download in {}.||Stopped {} downloads in {}.",
                    n as u64,
                    &[&n, &queue],
                );
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

    /// Imports the list in the file at `path`. Addresses already in the
    /// list are asked about first, as Add URL asks (gap 143).
    /// Reads a list into the queue; why it could not, as said, if not.
    pub fn import_from(&mut self, path: &std::path::Path) -> Result<(), String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                self.said = crate::i18n::Said::new(
                    "{} could not be read: {}",
                    &[&path.display(), &format::file_error(&e)],
                );
                return Err(self.said.shown());
            }
        };
        let listed = Backend::listed(&text);
        let already = listed
            .iter()
            .filter(|url| crate::dialogs::already_listed(self, url).is_some())
            .count();
        if already > 0 {
            self.dialogs.import_ask = Some(crate::dialogs::ImportAsk {
                text,
                already,
                total: listed.len(),
            });
            return Ok(());
        }
        self.import_text(&text, false);
        Ok(())
    }

    /// Adds the downloads a list holds; `skip_listed`, not those whose
    /// address is already a row.
    pub fn import_text(&mut self, text: &str, skip_listed: bool) {
        let before: HashSet<Id> = self.backend.list().iter().map(|r| r.id).collect();
        let listed: HashSet<String> = if skip_listed {
            Backend::listed(text)
                .into_iter()
                .filter(|url| crate::dialogs::already_listed(self, url).is_some())
                .collect()
        } else {
            HashSet::new()
        };
        let added = self.backend.import_where(text, |url| !listed.contains(url));
        // A list is not a run of progress windows.
        self.own.extend(
            self.backend
                .list()
                .iter()
                .map(|r| r.id)
                .filter(|id| !before.contains(id)),
        );
        // Lines of a plain list with no address dm can read are said, not
        // passed over: "127.0.0.1:8000/notes.txt" went without a word (UI
        // round 13).
        let unread = if text.trim_start().starts_with('{') {
            0
        } else {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .filter(|l| !l.contains("://") && !l.to_ascii_lowercase().contains("magnet:?"))
                .count()
        };
        self.said = if unread > 0 {
            crate::i18n::Said::naming(
                "Imported: {}. Lines skipped, with no address dm can read (one starts with http://, https://, ftp:// or magnet:): {}.",
                &[&added, &unread],
            )
        } else {
            crate::i18n::Said::counting(
                "Imported {} download.||Imported {} downloads.",
                added as u64,
                &[&added],
            )
        };
        self.refresh();
    }

    /// Writes the list to `path`, and says where or why not.
    /// Writes the list to `path`; why it could not, as said, if not.
    pub fn export_to(&mut self, path: &std::path::Path) -> Result<(), String> {
        match std::fs::write(path, self.backend.export()) {
            Ok(()) => {
                self.said = crate::i18n::Said::new("Exported to {}.", &[&path.display()]);
                Ok(())
            }
            Err(e) => {
                self.said = crate::i18n::Said::new(
                    "{} could not be written: {}",
                    &[&path.display(), &format::file_error(&e)],
                );
                Err(self.said.shown())
            }
        }
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
            // The widths go with the places, not the columns: Status took
            // Size's 76 points and read "Compl…" (UI round 14). Fitted
            // afresh to what is shown now.
            self.columns_changed = true;
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

    /// Sorts by a column; asked for again, the other way round; a third
    /// time, back in the queue's order. Kept in the settings, so the list
    /// is sorted the same way at the next launch.
    pub fn sort_by(&mut self, column: Column) {
        // Up, down, then the queue's own order again: a list once sorted
        // could not go back to it, where rows are dragged into another
        // order (UI round 6).
        self.sort = match self.sort {
            Some((c, true)) if c == column => Some((c, false)),
            Some((c, false)) if c == column => None,
            _ => Some((column, true)),
        };
        let mut wanted = self.applied.clone();
        match self.sort {
            Some((column, ascending)) => {
                wanted.sort_column = column.key().to_string();
                wanted.sort_ascending = ascending;
            }
            None => wanted.sort_column = String::new(),
        }
        if let Err(e) = self.apply_settings(wanted) {
            self.said = e.into();
        }
    }

    /// Where a row not saved anywhere yet will go, for the Folder column
    /// to show until it is: the download folder, or the kind's folder under
    /// it where Options › Folders sorts by kind — as the queue places it.
    /// Nothing where that waits on the server: a name that does not say its
    /// kind (`download.php`, `.bin`) is placed by the type the server gives
    /// (gap 167). Only shown, never written into the row: what a command
    /// reads of a row is what the person chose.
    pub fn planned_folder(
        &self,
        row: &Entry,
        categories: &mut dm_core::Categories,
    ) -> Option<String> {
        // Not for one finished or cancelled; a failed one still goes there
        // once retried, and lost its Folder (round 16).
        if !row.path.is_empty()
            || row.save_dir.is_some()
            || matches!(row.state, State::Completed | State::Cancelled)
        {
            return None;
        }
        if !categories.enabled {
            return Some(self.applied.download_dir.display().to_string());
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
        Some(
            categories
                .place(&self.applied.download_dir, kind, &name)
                .display()
                .to_string(),
        )
    }

    /// The name a row's file will have: for one not yet placed, with the
    /// ` (1)` it will get where its folder already has that name — a file
    /// added again said `big.bin` until it finished as `big (1).bin` (gap
    /// 47). The folder is asked at most every two seconds per row.
    pub fn name_it_will_get(&self, row: &Entry, categories: &mut dm_core::Categories) -> String {
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
            // later all read "slow (1).bin" (UI round 12).
            let before = self
                .rows
                .iter()
                .filter(|r| {
                    r.id < row.id
                        && !r.state.settled()
                        && r.path.is_empty()
                        && r.save_dir == row.save_dir
                        && name_of(r) == name
                })
                .count();
            let said =
                dm_core::download::nth_free_name(std::path::Path::new(&folder), &name, before);
            free.insert(key, (Instant::now(), said.clone()));
            said
        })
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

    /// How many downloads are running, and how many are in line to start.
    pub fn running_and_waiting(&self) -> (usize, usize) {
        let count = |state: State| self.rows.iter().filter(|r| r.state == state).count();
        (count(State::Running), count(State::Queued))
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
        // A row the schedule or the download limit holds is not resumed but
        // started outside it, as its progress window says (UI round 11).
        if command == Command::DownloadNow && self.held_only() {
            return t("Start now");
        }
        command.title()
    }

    /// Whether every selected row is held by the schedule or the limit.
    fn held_only(&self) -> bool {
        let targets = self.targets();
        !targets.is_empty()
            && targets
                .iter()
                .all(|r| r.state == State::Paused && r.paused_by_schedule)
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
        // What a text field's menu chose on the frame before, given to the
        // field as the keys would give it.
        // Escape closes a field's menu first, not the dialog under it,
        // which took the key and left the menu standing (UI round 9).
        if self.field_menu.is_some()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.field_menu = None;
        }
        if !self.field_edits.is_empty() {
            let edits = std::mem::take(&mut self.field_edits);
            ctx.input_mut(|i| i.events.extend(edits));
        }
        // The smallest window is the same on screen at any text size: given
        // in points, it grew with the zoom, and at 200% a relaunched window
        // opened past the bottom of the screen and could not be made
        // smaller (UI round 3).
        let zoom = ctx.zoom_factor();
        if self.min_size_zoom != Some(zoom) {
            self.min_size_zoom = Some(zoom);
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(
                egui::vec2(MIN_WINDOW.0, MIN_WINDOW.1) / zoom.max(0.5),
            ));
        }
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
        // they are set), so headings take Inter's semibold.
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
        let behind = self.behind_a_dialog();
        if behind && !self.was_behind {
            self.focus_before = ui.ctx().memory(|m| m.focused());
        } else if !behind && self.was_behind {
            if let Some(id) = self.focus_before.take() {
                ui.ctx().memory_mut(|m| m.request_focus(id));
            }
        }
        self.was_behind = behind;
        let menu = egui::Panel::top("menu")
            .show_separator_line(lines)
            .frame(flat(t.frame, 6, 2))
            .show(ui, |ui| self.menu_bar(ui));
        self.menu_bottom = menu.response.rect.bottom();
        // For the dialogs, which keep below it.
        ui.ctx()
            .data_mut(|d| d.insert_temp(crate::dialogs::menu_bottom_id(), self.menu_bottom));
        self.bars_bottom = self.menu_bottom;
        if self.show_toolbar {
            let bar = egui::Panel::top("toolbar")
                .show_separator_line(lines)
                .frame(flat(t.frame, 8, if t.modern { 4 } else { 0 }))
                .show(ui, |ui| {
                    let behind = self.behind_a_dialog();
                    ui.add_enabled_ui(!behind, |ui| self.toolbar(ui));
                });
            self.bars_bottom = bar.response.rect.bottom();
        }
        egui::Panel::bottom("status")
            .show_separator_line(lines)
            .frame(flat(t.frame, 12, 4))
            .show(ui, |ui| self.status_bar(ui));
        if !self.problems.is_empty() {
            egui::Panel::bottom("problems").show(ui, |ui| self.problem_list(ui));
        }
        // The rail gives way where the list would have too little room
        // left: at 200% text in the usual window it left two columns
        // (round 16). View › Categories still turns it off and on.
        let room = ui.available_width();
        let rail_gone = self.show_categories && room < 640.0;
        if self.show_categories && room >= 640.0 {
            // Fitted afresh in another language: kept from a German run,
            // the places stayed wide in English and cut the list's last
            // column (UI round 13).
            let language = self.applied.language.clone();
            if self.rail_language.as_ref().is_some_and(|l| *l != language) {
                let id = ui.scope_id().with_salt(egui::IdSalt::new("categories"));
                ui.ctx().data_mut(|d| d.remove::<egui::PanelState>(id));
            }
            self.rail_language = Some(language);
            egui::Panel::left("categories")
                .resizable(true)
                .show_separator_line(lines)
                .default_size(self.rail_width(ui, if t.modern { 230.0 } else { 200.0 }))
                // And never narrower than its longest name needs: a width
                // kept from English cut German names after a switch and a
                // relaunch (UI round 12).
                .min_size(self.rail_width(ui, 120.0))
                .frame(flat(t.frame, 8, 4))
                .show(ui, |ui| {
                    // More places than fit scroll, rather than being cut off,
                    // with a bar that shows it: hidden until hovered, a tree
                    // cut by a two-row toolbar looked as though it ended there.
                    ui.spacing_mut().scroll = crate::theme::steady_scroll();
                    let behind = self.behind_a_dialog();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_enabled_ui(!behind, |ui| self.categories(ui))
                        });
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
                    if rail_gone {
                        self.place_picker(ui);
                    }
                    self.clipboard_bar(ui);
                    crate::table::show(self, ui);
                });
            });

        crate::dialogs::show(self, &ctx);
        self.focus_ring(&ctx);
        self.field_menu(&ctx);

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
        // a download. Only a text field: a row or a button with the
        // keyboard's focus counted as typing, and Delete did nothing for a
        // person using the keyboard alone (round 14).
        if ctx.text_edit_focused() {
            return;
        }
        // A focused button takes Enter for itself.
        let focused = ctx.egui_wants_keyboard_input();
        // F10: the menu bar; Shift+F10 or the Menu key: the row menu of the
        // row with the keyboard's place, as a right-click opens it.
        // Shift+F10 first: egui matches a key given no Shift with Shift held
        // too, so plain F10 took it for itself.
        let row_menu = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Menu)
        });
        let f10 = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F10));
        if f10 && !self.dialogs.asking() {
            self.open_menu_by_key = true;
            ctx.request_repaint();
        }
        if row_menu && !self.dialogs.in_front() && self.cursor.is_some() {
            self.row_menu_by_key = true;
            ctx.request_repaint();
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
        if !behind
            && !focused
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
            && !self.selected.is_empty()
        {
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
                    (
                        egui::Key::PageUp,
                        crate::table::Step::PageUp(self.list_page),
                    ),
                    (
                        egui::Key::PageDown,
                        crate::table::Step::PageDown(self.list_page),
                    ),
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
                            | egui::Key::End
                            | egui::Key::PageUp
                            | egui::Key::PageDown,
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
        let (rect, layer, row) = match focused {
            Some(r) => (r.interact_rect, r.layer_id, false),
            // Kept inside the list's frame: a row is as wide as every
            // column, and its ring ran past the frame's edge (gap 148).
            None => match self.cursor.and_then(|id| self.row_rects.get(&id)) {
                Some(rect) => (
                    self.list_clip.map_or(*rect, |clip| rect.intersect(clip)),
                    egui::LayerId::background(),
                    true,
                ),
                None => return,
            },
        };
        if rect.is_positive() {
            // Two lines: the accent one, and inside it one in a light
            // colour, which is what shows on a primary button or a
            // selected row: those are filled with the accent colour itself.
            let painter = ctx.layer_painter(layer);
            let radius = egui::CornerRadius::same(t.radius);
            let inner = egui::Stroke::new(1.5, t.focus_inner);
            if row {
                // A row's inside its frame, where the list ends.
                painter.rect_stroke(rect.shrink(1.0), radius, stroke, egui::StrokeKind::Inside);
                painter.rect_stroke(rect.shrink(3.0), radius, inner, egui::StrokeKind::Inside);
            } else {
                // Round a control, not over it: inside, it crossed a
                // checkbox's box and its last letter (UI round 6).
                painter.rect_stroke(rect, radius, inner, egui::StrokeKind::Outside);
                painter.rect_stroke(rect.expand(1.5), radius, stroke, egui::StrokeKind::Outside);
            }
            self.focus_ring = Some(rect);
        }
    }

    /// Cut, Copy and Paste on a right-click in a text field, as in any
    /// program: the keys worked, the menu was not there (UI round 8).
    fn field_menu(&mut self, ctx: &egui::Context) {
        let (clicked, right_down, at) = ctx.input(|i| {
            (
                i.pointer.secondary_clicked(),
                i.pointer.secondary_down() || i.pointer.secondary_pressed(),
                i.pointer.interact_pos(),
            )
        });
        let focused = ctx
            .memory(|m| m.focused())
            .filter(|_| ctx.egui_wants_keyboard_input());
        // The field's selection, kept from before the right-click: the
        // press puts the caret where it lands, and Cut and Copy then had
        // nothing to act on (UI round 9).
        if !right_down && !clicked && self.field_menu.is_none() {
            self.field_selection = focused.and_then(|id| {
                egui::text_edit::TextEditState::load(ctx, id)
                    .and_then(|state| state.cursor.char_range())
                    .filter(|range| !range.is_empty())
                    .map(|range| (id, range))
            });
        }
        if clicked {
            // The field the right-click put the caret in.
            let field = focused
                .and_then(|id| ctx.read_response(id).map(|r| (id, r.rect)))
                .filter(|(_, rect)| at.is_some_and(|at| rect.contains(at)));
            self.field_menu = field.zip(at).map(|((id, _), at)| (id, at));
            return;
        }
        let Some((id, at)) = self.field_menu else {
            return;
        };
        // Gone with its dialog, it does not stay over the list (UI round 9).
        if ctx.read_response(id).is_none() {
            self.field_menu = None;
            return;
        }
        let selected = self
            .field_selection
            .filter(|(field, _)| *field == id)
            .map(|(_, range)| range);
        let mut chosen: Option<u8> = None;
        let shown = egui::Area::new(egui::Id::unique("dm-field-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    // Items of one width and no frame, as in every menu.
                    ui.set_min_width(140.0);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        for (n, word) in [t("Cut"), t("Copy"), t("Paste")].into_iter().enumerate() {
                            let enabled = n == 2 || selected.is_some();
                            let item = egui::Button::new(word).frame(false);
                            if ui.add_enabled(enabled, item).clicked() {
                                chosen = Some(n as u8);
                            }
                        }
                    });
                })
            });
        // The clipboard read only for a paste.
        let chosen = chosen.map(|n| match n {
            0 => egui::Event::Cut,
            1 => egui::Event::Copy,
            _ => egui::Event::Paste(self.backend.platform.clipboard().unwrap_or_default()),
        });
        let away = ctx.input(|i| i.pointer.any_pressed()) && !at_inside(ctx, shown.response.rect);
        if let Some(edit) = chosen {
            // The selection put back, for Cut and Copy to act on.
            if let (Some(range), Some(mut state)) =
                (selected, egui::text_edit::TextEditState::load(ctx, id))
            {
                state.cursor.set_char_range(Some(range));
                state.store(ctx, id);
            }
            self.field_edits.push(edit);
            ctx.memory_mut(|m| m.request_focus(id));
            self.field_menu = None;
        } else if away {
            self.field_menu = None;
        }
    }

    fn take_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut links = Vec::new();
        for file in &dropped {
            links.extend(dropped_links(file.as_ref()));
        }
        if !links.is_empty() {
            let _ = self.add(&links, Default::default());
        } else if !dropped.is_empty() {
            // A drop with no link in it was ignored without a word (gap
            // 150). A link dragged from a browser does not reach dm at all
            // (the window system hands over files only), so the empty
            // list says the same.
            self.said = crate::i18n::Said::new(DROP_HINT, &[]);
        }
    }

    /// A menu item for a command: its label, its key, and greyed out when
    /// it has nothing to act on.
    fn item(&mut self, ui: &mut egui::Ui, command: Command) {
        let title = self.title_of(command);
        let shown = if command.asks_more() && !title.ends_with('…') {
            format!("{title}…")
        } else {
            title.to_string()
        };
        let mut button = egui::Button::new(shown);
        if let Some(shortcut) = command.shortcut() {
            button = button.shortcut_text(format::shortcut(ui.ctx(), &shortcut));
        }
        let enabled = self.enabled(command);
        let response = ui.add_enabled(enabled, button);
        // Said by its name alone: the "…" is for the eye.
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, title));
        if response.clicked() {
            self.queued.push(command);
            ui.close();
        }
    }

    /// How wide the places start: room for the longest name in them,
    /// queues included, up to a third of the window. German and Russian
    /// queue names were cut short in a rail that had the room (UI round 11).
    fn rail_width(&self, ui: &egui::Ui, least: f32) -> f32 {
        let mut names: Vec<String> = self
            .backend
            .queues()
            .iter()
            .map(|q| crate::i18n::name(q))
            .collect();
        names.extend(
            Category::ALL
                .iter()
                .map(|c| crate::i18n::name(&format::titled(&category_word(*c)))),
        );
        names.extend(["In progress", "Done"].map(|w| t(w).to_string()));
        let widest = names
            .into_iter()
            .map(|n| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap(n, egui::FontId::proportional(14.0), egui::Color32::WHITE)
                        .size()
                        .x
                })
            })
            .fold(0.0_f32, f32::max);
        // The icon and count beside it, the branch's indent and the margins.
        (widest + 36.0 + 34.0 + 18.0 + 16.0).clamp(least, (ui.available_width() / 3.0).max(least))
    }

    /// Whether a question or a dialog that keeps the clicks is open: the
    /// menu bar, the toolbar and the places then take none (UI round 11:
    /// behind Options, Add URL opened over it and Done was picked).
    fn behind_a_dialog(&self) -> bool {
        self.dialogs.asking() || self.dialogs.modal()
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        // Greyed while a question waits, as the list and toolbar take no
        // clicks then: it opened bright over the dimmed window (round 16).
        // And while any dialog keeps the clicks: behind Options its menus
        // opened and their items did nothing (UI round 10).
        let behind = self.behind_a_dialog();
        if behind {
            self.open_menu_by_key = false;
        }
        ui.add_enabled_ui(!behind, |ui| self.menu_bar_items(ui));
    }

    fn menu_bar_items(&mut self, ui: &mut egui::Ui) {
        // F10 opens the first menu, as on any desktop: the menus could be
        // reached only by Tabbing to them (UI round 4). Opened before the bar
        // draws, by the id it had last frame: egui closes a popup opened
        // after its frame's drawing.
        if std::mem::take(&mut self.open_menu_by_key) {
            if let Some(id) = self.menus.first() {
                egui::Popup::open_id(ui.ctx(), *id);
            }
        }
        // Left and Right go from one open menu to the next, round the bar,
        // as on any desktop (UI round 5: only Up and Down walked a menu).
        let ctx = ui.ctx().clone();
        if let Some(at) = self
            .menus
            .iter()
            .position(|id| egui::Popup::is_id_open(&ctx, *id))
        {
            let step = ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                    Some(1)
                } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                    Some(self.menus.len() - 1)
                } else {
                    None
                }
            });
            if let Some(step) = step {
                let next = self.menus[(at + step) % self.menus.len()];
                egui::Popup::open_id(&ctx, next);
            }
        }
        egui::MenuBar::new().ui(ui, |ui| {
            let program = ui.menu_button(t("Program"), |ui| {
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
            let file = ui.menu_button(t("File"), |ui| {
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
            let downloads = ui.menu_button(t("Downloads"), |ui| {
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
                    let item = ticked_item(ui, Command::SpeedLimiterToggle.title(), on);
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
                            || crate::power::available(w);
                        if ui
                            .add_enabled(
                                can,
                                egui::RadioButton::new(self.when_done == w, c.title()),
                            )
                            // Why, where it is greyed (UI round 14).
                            .on_disabled_hover_text(t("This computer gives dm no way to do this."))
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
            let view = ui.menu_button(t("View"), |ui| {
                // Menu items with their tick at the right, so their words
                // start where the items under them do (gap 148).
                if ticked_item(ui, t("Categories"), self.show_categories).clicked() {
                    self.queued.push(Command::ToggleCategories);
                    ui.close();
                }
                if ticked_item(ui, t("Toolbar"), self.show_toolbar).clicked() {
                    self.queued.push(Command::ToggleToolbar);
                    ui.close();
                }
                // "Columns": each column on or off; the file
                // name stays, since a row without one is no row.
                ui.menu_button(t("Columns"), |ui| self.columns_menu(ui));
                ui.menu_button(t("Customize toolbar"), |ui| self.toolbar_menu(ui));
                // View › Text size: the window's zoom, kept across launches,
                // which the keyboard's Ctrl (Cmd) with + and − also changes.
                ui.menu_button(t("Text size"), |ui| {
                    let ctx = ui.ctx().clone();
                    let zoom = ctx.zoom_factor();
                    ui.label(RichText::new(tf("{}%", &[&((zoom * 100.0).round() as u32)])).weak());
                    if ui.button(t("Larger")).clicked() {
                        ctx.set_zoom_factor((zoom + 0.1).min(3.0));
                    }
                    if ui.button(t("Smaller")).clicked() {
                        ctx.set_zoom_factor((zoom - 0.1).max(0.5));
                    }
                    if ui.button(t("Normal size")).clicked() {
                        ctx.set_zoom_factor(1.0);
                    }
                });
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
            let help = ui.menu_button(t("Help"), |ui| {
                self.item(ui, Command::Help);
                self.item(ui, Command::CheckForUpdates);
                self.item(ui, Command::About);
            });
            self.menus = [program, file, downloads, view, help]
                .iter()
                .map(|menu| egui::Popup::default_response_id(&menu.response))
                .collect();
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
        let held_only = self.held_only();
        // Each button with the name it shows and the widest it can show,
        // which is the room it takes: Resume becoming Retry moved every
        // button after it (gap 140).
        let buttons: Vec<(Command, Icon, &'static str, &'static str)> = buttons
            .into_iter()
            .map(|(command, icon, label)| {
                let names = if command == Command::DownloadNow {
                    vec![
                        crate::i18n::t(label),
                        crate::i18n::t("Retry"),
                        crate::i18n::t("Start now"),
                    ]
                } else {
                    vec![crate::i18n::t(label)]
                };
                let shown = if command == Command::DownloadNow && failed_only {
                    crate::i18n::t("Retry")
                } else if command == Command::DownloadNow && held_only {
                    // As the row menu says (UI round 12).
                    crate::i18n::t("Start now")
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
        // Where even stacked words do not fit on one row, icons alone, their
        // names on hover: wrapped to three rows at 150% text in a small
        // window, the toolbar left one row of the list (UI round 3).
        let icons_only = stacked && width(true) > room;
        // And icons alone go on to a second row where even they do not fit:
        // at 200% text in the smallest window they ran past the right edge
        // (UI round 5).
        let wrap = !modern || icons_only;
        let in_front = self.dialogs.in_front();
        let mut draw = |ui: &mut egui::Ui| {
            for (command, icon, name, widest) in &buttons {
                // The modern toolbar leads with one primary action.
                let primary = *command == Command::AddUrl;
                // Greyed behind a dialog too: Add URL stayed bright blue
                // under the dim (UI round 11).
                let enabled = self.enabled(*command) && ui.is_enabled();
                let response = if icons_only {
                    tool(
                        ui,
                        *icon,
                        (name, widest),
                        enabled,
                        ToolStyle::Icons,
                        primary,
                    )
                } else if stacked {
                    modern_tool(ui, *icon, (name, widest), enabled, primary, &t, true)
                } else {
                    tool(ui, *icon, (name, widest), enabled, chosen, primary)
                };
                // What the button does in full, with its key: only where that
                // says more than the button's own word (UI round 1: every
                // tooltip repeated its label).
                let title = self.title_of(*command);
                let key = command
                    .shortcut()
                    .map(|k| format!(" ({})", format::shortcut(ui.ctx(), &k)))
                    .unwrap_or_default();
                let said = format!("{title}{key}");
                // None while a dialog is in front: one drawn over its title
                // read as part of it (UI round 8).
                let response =
                    if (said != *name || chosen == ToolStyle::Icons || icons_only) && !in_front {
                        response.on_hover_text(said)
                    } else {
                        response
                    };
                if response.clicked() {
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
        // Kept open or shut by an id of its own, not its words: a language
        // switch opened a shut one again (UI round 10).
        egui::CollapsingHeader::new(t("Everything"))
            .id_salt("tree-everything")
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
            .id_salt("tree-queues")
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

    /// The places as one drop-down above the list, where the window has no
    /// room for them beside it: at large text they went, and All, Done and
    /// the queues could not be reached at all (UI round 14).
    fn place_picker(&mut self, ui: &mut egui::Ui) {
        let mut places: Vec<(Place, String)> = vec![(Place::All, t("All").into())];
        for c in Category::ALL {
            places.push((
                Place::Kind(c),
                crate::i18n::name(&format::titled(&category_word(c))),
            ));
        }
        for custom in &self.applied.custom_categories {
            let name = custom.name.trim().to_string();
            if !name.is_empty() {
                places.push((Place::Custom(name.clone()), name));
            }
        }
        places.push((Place::Unfinished, t("In progress").into()));
        places.push((Place::Finished, t("Done").into()));
        for queue in self.backend.queues() {
            let label = crate::i18n::name(&queue);
            places.push((Place::Queue(queue), label));
        }
        let shown = |place: &Place, label: &str| format!("{label} ({})", self.count(place));
        let current = places
            .iter()
            .find(|(p, _)| *p == self.place)
            .map(|(p, l)| shown(p, l))
            .unwrap_or_default();
        let mut chosen = None;
        ui.horizontal(|ui| {
            let name = ui.label(t("Place"));
            egui::ComboBox::from_id_salt("place-picker")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (place, label) in &places {
                        if ui
                            .selectable_label(*place == self.place, shown(place, label))
                            .clicked()
                        {
                            chosen = Some(place.clone());
                        }
                    }
                })
                .response
                .labelled_by(name.id);
        });
        if let Some(place) = chosen {
            self.place = place;
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
                self.open_file(&dir.to_string_lossy());
            }
        }
    }

    fn clipboard_bar(&mut self, ui: &mut egui::Ui) {
        let Some(url) = self.clipboard.offered.clone() else {
            return;
        };
        let all = self.clipboard.offered_all.clone();
        if all.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(tf("Links on your clipboard: {}", &[&all.len()]));
                // The dialog that lists them, each to tick or not.
                if ui.button(format!("{}…", t("Download"))).clicked() {
                    self.clipboard.dismissed.extend(all.iter().cloned());
                    self.clipboard.offered = None;
                    self.clipboard.offered_all.clear();
                    self.queued.push(Command::AddFromClipboard);
                }
                if ui.button(t("Dismiss")).clicked() {
                    self.clipboard.dismissed.extend(all.iter().cloned());
                    self.clipboard.offered = None;
                    self.clipboard.offered_all.clear();
                }
            });
            ui.separator();
            return;
        }
        ui.horizontal(|ui| {
            ui.label(t("Link on your clipboard:"));
            ui.label(RichText::new(format::shown_url(&url)).weak());
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
        // A row with every byte is only putting its file together: said
        // to be finishing, not counted as downloading at 0 B/s (gap 168).
        let (finishing, running): (Vec<&Entry>, Vec<&Entry>) = self
            .rows
            .iter()
            .filter(|r| r.state == State::Running)
            .partition(|r| fetched_all(r));
        let rate: f64 = running.iter().map(|r| r.progress.speed).sum();
        let (finishing, running) = (finishing.len(), running.len());
        let later = self.rows.iter().filter(|r| waits_for_its_queue(r)).count();
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
        let showing = fresh && !stale && !self.said.is_empty() && !self.said.is_limiter();
        ui.horizontal(|ui| {
            // Each "· …" part starts with its own space: the usual gap as
            // well read as two spaces before the dot (round 16).
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(if running == 0 && finishing == 0 {
                t("Nothing downloading").to_string()
            } else if running == 0 {
                crate::i18n::tn(
                    "{} finishing||{} finishing",
                    finishing as u64,
                    &[&finishing],
                )
            } else {
                crate::i18n::tn(
                    "{} downloading at {}||{} downloading at {}",
                    running as u64,
                    &[&running, &format::rate(rate)],
                )
            });
            // A sentence being said takes the room of the counts below,
            // whose words cut off its end, which says what to do (UI round 7).
            if running > 0 && finishing > 0 && !showing {
                ui.label(crate::i18n::tn(
                    "· {} finishing||· {} finishing",
                    finishing as u64,
                    &[&finishing],
                ));
            }
            // Rows added for later are counted, and said to wait for Start
            // queue, so a list of them is not taken for a stuck one (gap 167).
            if later > 0 && !showing {
                ui.label(crate::i18n::tn(
                    "· {} waiting for Start queue||· {} waiting for Start queue",
                    later as u64,
                    &[&later],
                ));
            }
            if self.applied.speed_limit > 0 {
                ui.label(tf("· limited to {} KB/s", &[&self.applied.speed_limit]));
            }
            if let Some(wait) = self.backend.queue.allowance_spent() {
                ui.label(tf(
                    "· download limit of {} MB reached, held for {}",
                    &[&self.applied.limit_mb, &format::span(wait)],
                ));
            } else if let Some(at) = self
                .schedule_holds()
                // Not beside a sentence that says so already: it was said
                // twice and the second cut off (UI round 11).
                .filter(|_| !(showing && self.said.is_schedule_wait()))
            {
                // Said while the schedule is what downloads wait for, so a
                // queue that does nothing is not taken for a broken one.
                ui.label(match at {
                    Some(at) => tf("· waiting for the schedule (opens at {})", &[&at]),
                    None => t("· waiting for the schedule").to_string(),
                });
            }
            if showing {
                // Its first line, cut short to the room there is with the
                // whole of it on hover: an opener's error output ran off the
                // window's edge.
                let whole = self.said.shown();
                let short = whole.lines().next().unwrap_or_default().to_string();
                let room = (ui.available_width() - 8.0).max(80.0);
                ui.scope(|ui| {
                    ui.set_max_width(room);
                    let drawn = ui.add(
                        egui::Label::new(format!("· {}", crate::bidi::visual(&short)))
                            .truncate()
                            // The whole is on hover below; egui's own cut-off
                            // tooltip made two (UI round 7).
                            .show_tooltip_when_elided(false),
                    );
                    crate::bidi::said_as(&drawn, &format!("· {short}"));
                    // Read out when it changes, without moving the screen
                    // reader's place: what dm says here was seen only
                    // (gap 48).
                    ui.ctx().accesskit_node_builder(drawn.id, |node| {
                        node.set_live(egui::accesskit::Live::Polite);
                    });
                    drawn.on_hover_text(whole);
                });
                ui.ctx().request_repaint_after(SAID_FOR);
            }
            // The folder in what room is left, cut short with an ellipsis
            // (its whole path on hover) rather than drawn over the words.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Not while a sentence is said, whose end it cut off in the
                // smallest window (UI round 8).
                if ui.available_width() > 40.0 && !showing {
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

    /// Opens `path` with the system's opener, off the window's thread.
    pub fn open_file(&self, path: &str) {
        let said = Arc::clone(&self.opener_said);
        let report: crate::power::Report = Box::new(move |why| *dm_core::lock(&said) = Some(why));
        self.backend.platform.open_later(path, report);
    }

    /// Shows `path` in the file manager, off the window's thread.
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

    fn problem_list(&mut self, ui: &mut egui::Ui) {
        let mut dismissed = None;
        for (i, problem) in self.problems.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(crate::theme::Tokens::get(ui.ctx()).warn, problem);
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

/// A problem from outside the window, in the language in force where the
/// window knows its words: a saved proxy turned off at launch is said in
/// the language the person reads (gap 141). Others are shown as written.
pub fn worded_problem(problem: &str) -> String {
    let proxy = problem
        .strip_prefix("the saved proxy could not be used (")
        .and_then(|rest| rest.split_once("), so it is turned off"));
    match proxy {
        Some((why, _)) => crate::i18n::tf(
            "The saved proxy could not be used ({}), so it is turned off and downloads go out directly. Fix it under Options › Proxy and tick it again.",
            &[&why],
        ),
        None => problem.to_string(),
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
    words.push(DROP_HINT);
    words.extend(format::REASONS.iter().map(|(_, plain)| *plain));
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
        Place::Finished => vec![("Clear finished", TreeAction::Run(Command::DeleteCompleted))],
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
    let font = egui::FontId::proportional(if stacked { 12.0 } else { 14.0 });
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
    // A name cut short is whole on hover.
    let response = if galley.elided {
        response.on_hover_text(label)
    } else {
        response
    };
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
    (Command::Remove, Icon::Delete, "Remove"),
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

/// A menu item that is on or off: a plain item, its words where the other
/// items' start, with a tick drawn at its right while it is on. As a
/// checkbox its words started further in than the items around it.
fn ticked_item(ui: &mut egui::Ui, title: &str, on: bool) -> egui::Response {
    let item = ui.add(egui::Button::new(title).right_text("    "));
    item.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, title));
    let r = item.rect;
    let at = egui::pos2(r.right() - 12.0, r.center().y);
    let colour = ui.style().interact(&item).fg_stroke.color;
    // A box whether on or off, so the item reads as one that is ticked or
    // not, rather than a tick that is there or is not (gap 168).
    ui.painter().rect_stroke(
        egui::Rect::from_center_size(at, egui::vec2(12.0, 12.0)),
        2.0,
        egui::Stroke::new(1.0, colour.gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    if on {
        ui.painter().line(
            vec![
                at + egui::vec2(-3.5, 0.0),
                at + egui::vec2(-1.0, 2.5),
                at + egui::vec2(3.5, -3.0),
            ],
            egui::Stroke::new(1.6, colour),
        );
    }
    item
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
    // Each button as wide as its longest name needs, measured in the font
    // it is drawn in: a fixed 72 points ran German's names into each other
    // in the classic look (gap 145).
    let fits = |size: f32| {
        [label, widest]
            .iter()
            .map(|text| {
                ui.painter()
                    .layout_no_wrap(
                        text.to_string(),
                        egui::FontId::proportional(size),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max)
    };
    let size = match style {
        ToolStyle::Icons => egui::vec2(34.0, 30.0),
        ToolStyle::Text => egui::vec2(fits(12.0) + 18.0, 28.0),
        ToolStyle::Both => egui::vec2((fits(11.0) + 12.0).max(72.0), 52.0),
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
    // The one primary action keeps its fill when the names give way to
    // icons alone, as at 200% or in Russian (UI round 7: it lost it).
    let filled = primary && t.modern && enabled;
    if filled {
        ui.painter().rect_filled(rect, f32::from(t.radius), t.brand);
    } else if response.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, 4.0, ui.style().visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if filled {
        t.on_brand
    } else if enabled {
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
    // On the brand fill, in the colour that shows on it: the "+" was drawn
    // dark on blue (UI round 12).
    let picture = if filled {
        t.on_brand
    } else if enabled {
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
        // Nothing under 12 points, the smallest step of the type ramp.
        egui::FontId::proportional(12.0),
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
mod two_addresses_tests {
    use super::*;

    #[test]
    fn the_saved_proxy_turned_off_at_launch_is_said_in_the_windows_words() {
        let said = worded_problem(
            "the saved proxy could not be used (the proxy has no host), so it is turned off and downloads \
             go out directly. Fix it under Options › Proxy and tick it again.",
        );
        assert!(
            said.starts_with("The saved proxy could not be used (the proxy has no host)"),
            "{said}"
        );
        assert_eq!(worded_problem("something else"), "something else");
    }

    #[test]
    fn two_links_pasted_into_one_field_are_seen() {
        for two in [
            "https://e.com/a.ziphttps://e.com/b.zip",
            "https://e.com/a.zip https://e.com/b.zip",
            "http://e.com/a.zipftp://e.com/b",
            "https://e.com/a.zipmagnet:?xt=urn:btih:abc",
        ] {
            assert!(two_addresses(two), "{two}");
        }
        for one in [
            "https://e.com/a.zip",
            "https://e.com/go?to=https://f.com/b.zip",
            "https://archive.example/web/2020/https://e.com/a.zip",
            "https://e.com/r?u=https%3A//f.com&next=http://g.com",
            "magnet:?xt=urn:btih:abc&tr=http://tracker.example/announce",
        ] {
            assert!(!two_addresses(one), "{one}");
        }
    }
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
        // Saying what it waits for (UI round 8).
        set_limit_holds(false);
        assert_eq!(status_in_english(&paused(true)), "Waiting for the schedule");
        set_limit_holds(true);
        assert_eq!(
            status_in_english(&paused(true)),
            "Waiting for the download limit"
        );
        set_limit_holds(false);
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
        // A torrent with nothing yet is finding its peers.
        let mut magnet = paused(false);
        magnet.state = State::Running;
        magnet.url = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into();
        assert_eq!(status_in_english(&magnet), "Finding peers…");
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

        // A .torrent is the download itself, by its file address.
        let torrent = dir.path().join("a set.torrent");
        std::fs::write(&torrent, b"d4:infodee").unwrap();
        let taken = dropped_links(&Dropped(torrent.clone()));
        assert_eq!(taken, [dm_core::torrent::file_url(&torrent).unwrap()]);
        assert!(dm_core::is_torrent(&taken[0]) && dm_core::queue::accepted(&taken[0]));
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
