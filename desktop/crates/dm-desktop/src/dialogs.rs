//! The window's dialogs: Add URL, Download details, a progress window per
//! download, Download complete, Properties, numbered series, files on a
//! site, Update download link, Move to queue, Move/Rename, Find,
//! Scheduler, Speed limiter, Options and About.
//!
//! Each is drawn from state held here and acts on the app when its button
//! is pressed; none of them reaches the queue except through the backend.

use crate::i18n::{t, tf};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use dm_core::{AddOptions, Category, Id, Schedule, State, MAIN_QUEUE};
use dm_shell::settings::SiteLogin;
use dm_shell::Settings;
use egui::{Color32, RichText};

use crate::app::{category_word, name_of, status_of, DmApp};
use crate::format;

const WARN: Color32 = Color32::from_rgb(0xd9, 0x7a, 0x2b);

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
}

/// What dm learned of one link by asking its server.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub name: String,
    pub kind: String,
    pub size: Option<u64>,
    pub html: bool,
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

#[derive(Default)]
pub enum Grab {
    #[default]
    Idle,
    Looking,
    /// The files found, each with whether it is ticked, and how many pages
    /// were read.
    Found(Vec<(String, bool)>, usize),
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
            last_run: 0,
            seen: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// The proxy's bypass list, as typed.
    pub bypass: String,
    /// The categories a person made, as typed: name, file types, sites.
    pub custom: Vec<[String; 3]>,
    /// A row was just added to one of the tab's lists: its first field is
    /// scrolled into view and takes the keyboard.
    pub fresh: bool,
}

#[derive(Default)]
pub struct Dialogs {
    pub add_url: Option<AddUrl>,
    pub file_info: Option<FileInfo>,
    pub progress: BTreeSet<Id>,
    pub complete: BTreeSet<Id>,
    pub properties: BTreeMap<Id, Properties>,
    pub series: Option<Series>,
    pub clip_links: Option<Vec<(String, bool)>>,
    /// The "Download all links" import dialog, for a page's links.
    pub links: Option<LinksImport>,
    pub site_files: Option<SiteFiles>,
    pub update_link: Option<(Id, String, String)>,
    pub move_to_queue: Option<(Vec<Id>, String)>,
    pub rename: Option<(Id, String)>,
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
    /// An address typed in that is already a row: the address and the row.
    pub duplicate: Option<(String, Id)>,
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
        self.file_info = Some(FileInfo {
            urls,
            queue: MAIN_QUEUE.into(),
            ..FileInfo::default()
        });
    }

    /// The dialog for a video the browser's panel handed over.
    pub fn open_handed(&mut self, request: dm_shell::host::Request) {
        self.file_info = Some(FileInfo {
            urls: vec![request.url],
            referrer: request.referrer,
            quality: request.quality,
            label: request.label,
            name: request.save_name.unwrap_or_default(),
            queue: MAIN_QUEUE.into(),
            handed: true,
            cookies: request.cookies,
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
                if let Ok(Ok(probe)) = asked {
                    let kind =
                        dm_core::category::classify(&probe.filename, probe.content_type.as_deref());
                    dm_core::lock(&found).insert(
                        url,
                        Found {
                            name: probe.filename,
                            kind: crate::app::category_word(kind),
                            size: probe.size,
                            html: dm_core::ytdlp::is_page(probe.content_type.as_deref()),
                        },
                    );
                }
            });
        }
        self.links = Some(LinksImport {
            page: request.referrer,
            sign_in,
            cookies: request.cookies,
            items: request.links.into_iter().map(|u| (u, true)).collect(),
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
        self.focus_on("Files on a site");
        if !self.keep("Files on a site", self.site_files.is_some()) {
            self.site_files = Some(Default::default());
        }
    }

    /// Whether a dialog is in front of the list, which then keeps its
    /// keys from the list. The windows a download opens on its own (its
    /// progress, its completion) are not counted.
    pub fn in_front(&self) -> bool {
        self.open_names()
            .iter()
            .any(|n| n != "Download progress" && n != "Download complete")
    }

    pub fn open_scheduler(&mut self, settings: &Settings) {
        if self.keep("Scheduler", self.scheduler.is_some()) {
            return;
        }
        self.scheduler = Some((settings.schedule.clone(), String::new()));
    }

    pub fn open_limiter(&mut self, settings: &Settings) {
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
            bypass: settings.proxy.bypass.join(" "),
            custom: settings
                .custom_categories
                .iter()
                .map(|c| [c.name.clone(), c.extensions.join(" "), c.sites.join(" ")])
                .collect(),
            fresh: false,
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
        say(self.confirm_remove.is_some(), "Delete unfinished downloads");
        say(self.duplicate.is_some(), "Already in the list");
        say(self.skip_offer.is_some(), "Leave this site to the browser?");
        say(self.add_url.is_some(), "Add URL");
        say(self.file_info.is_some(), "Download details");
        say(!self.progress.is_empty(), "Download progress");
        say(!self.complete.is_empty(), "Download complete");
        say(!self.properties.is_empty(), "File properties");
        say(self.series.is_some(), "Add numbered series");
        say(self.clip_links.is_some(), "Links on the clipboard");
        say(self.site_files.is_some(), "Files on a site");
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
    pub fn close_top(&mut self) -> bool {
        if std::mem::take(&mut self.confirm_exit)
            || self.confirm_remove.take().is_some()
            || self.duplicate.take().is_some()
            || self.skip_offer.take().is_some()
        {
            return true;
        }
        if std::mem::take(&mut self.find_open) {
            return true;
        }
        if self.limiter.take().is_some()
            || self.scheduler.take().is_some()
            || self.update_link.take().is_some()
            || self.move_to_queue.take().is_some()
            || self.rename.take().is_some()
            || self.add_url.take().is_some()
            || self.clip_links.take().is_some()
            || self.series.take().is_some()
            || self.links.take().is_some()
            || self.site_files.take().is_some()
            || self.file_info.take().is_some()
            || self.options.take().is_some()
        {
            return true;
        }
        if std::mem::take(&mut self.about_open) {
            return true;
        }
        if let Some(id) = self.properties.keys().last().copied() {
            self.properties.remove(&id);
            return true;
        }
        if let Some(id) = self.complete.iter().last().copied() {
            self.complete.remove(&id);
            return true;
        }
        if let Some(id) = self.progress.iter().last().copied() {
            self.progress.remove(&id);
            return true;
        }
        false
    }

    /// Closes everything about a row that is gone.
    pub fn forget(&mut self, id: Id) {
        self.progress.remove(&id);
        self.complete.remove(&id);
        self.properties.remove(&id);
    }
}

/// A dialog window: titled, not collapsible, placed in the middle.
/// A dialog, titled in the language in force but known by its English
/// title: its place and size survive a change of language, and two titles
/// that translate alike stay two windows.
fn window(title: &'static str) -> egui::Window<'static> {
    egui::Window::new(t(title))
        .id(window_id(title))
        .collapsible(false)
        .resizable(false)
        // Wide enough for its title in the 18 px heading.
        .min_width(440.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
}

/// The id of a dialog's window, by its English title.
fn window_id(title: &'static str) -> egui::Id {
    egui::Id::unique(("dm-dialog", title))
}

/// A progress bar: a thin track with the brand fill and its words
/// beside it; the classic look keeps egui's own bar.
fn bar(ui: &mut egui::Ui, fraction: f32, width: f32, words: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    if !t.modern {
        ui.add(
            egui::ProgressBar::new(fraction)
                .desired_width(width)
                .text(words),
        );
        return;
    }
    ui.horizontal(|ui| {
        let (track, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
        let line = egui::Rect::from_center_size(track.center(), egui::vec2(width, 4.0));
        ui.painter().rect_filled(line, 2.0, t.border);
        let mut done = line;
        done.set_width(line.width() * fraction);
        ui.painter().rect_filled(done, 2.0, t.brand);
        ui.label(RichText::new(words).small().color(t.weak));
    });
}

/// A dialog's main action: the primary button, filled with the brand
/// colour (the classic look draws it like any other).
fn primary(ui: &mut egui::Ui, text: &'static str) -> egui::Response {
    // The button's words in the language in force.
    let text = crate::i18n::t(text);
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return ui.button(text);
    }
    ui.add(
        egui::Button::new(RichText::new(text).color(t.on_brand))
            .fill(t.brand)
            .stroke(egui::Stroke::new(1.0, t.brand)),
    )
}

/// A text field with a name a screen reader says.
fn field(ui: &mut egui::Ui, label: &str, text: &mut String, width: f32) -> egui::Response {
    let name = ui.label(label);
    ui.add(egui::TextEdit::singleline(text).desired_width(width))
        .labelled_by(name.id)
}

/// Enter pressed in a field: the dialog's default button, as in any
/// dialog. A one-line field lets the keyboard go on Enter.
fn entered(field: &egui::Response) -> bool {
    field.lost_focus() && field.ctx.input(|i| i.key_pressed(egui::Key::Enter))
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
        ui.add(egui::TextEdit::singleline(text).desired_width(380.0))
            .labelled_by(name.id);
        browse(ui, platform, text);
    });
}

/// A Browse… button beside a folder's field: the folder picked goes in it.
fn browse(ui: &mut egui::Ui, platform: &dyn crate::backend::Platform, folder: &mut String) {
    if ui.button(t("Browse…")).clicked() {
        if let Some(picked) = platform.choose_folder() {
            *folder = picked.display().to_string();
        }
    }
}

pub fn show(app: &mut DmApp, ctx: &egui::Context) {
    for id in std::mem::take(&mut app.dialogs.raise) {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
    }
    countdown(app, ctx);
    confirm_exit(app, ctx);
    links(app, ctx);
    confirm_remove(app, ctx);
    duplicate(app, ctx);
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
        app.dialogs.close_top();
    }
    app.popup_was_open = ctx.any_popup_open();
}

fn confirm_remove(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ids) = app.dialogs.confirm_remove.clone() else {
        return;
    };
    let (mut yes, mut no) = (false, false);
    window("Delete unfinished downloads").show(ctx, |ui| {
        // Whole sentences, one for each number, since languages do not build
        // them from the same pieces.
        ui.label(if ids.len() == 1 {
            t("This download is not finished. Deleting it throws away what has been fetched.")
        } else {
            t("Some of these are not finished. Deleting them throws away what has been fetched.")
        });
        ui.horizontal(|ui| {
            yes = ui.button(t("Yes, delete")).clicked();
            no = ui.button(t("Keep them")).clicked();
        });
    });
    if yes {
        app.dialogs.confirm_remove = None;
        app.remove_now(&ids);
    } else if no {
        app.dialogs.confirm_remove = None;
    }
}

/// Asked when Exit is chosen while downloads run: quitting stops them, and
/// they go on from where they were the next time dm opens.
fn confirm_exit(app: &mut DmApp, ctx: &egui::Context) {
    if !app.dialogs.confirm_exit {
        return;
    }
    let running = app.downloading();
    // Finished meanwhile: nothing is left to ask about.
    if running == 0 {
        app.dialogs.confirm_exit = false;
        app.quit_asked = true;
        return;
    }
    let (mut exit, mut stay) = (false, false);
    window("Exit dm?").show(ctx, |ui| {
        ui.label(tf(
            "{} download(s) are still running. Exiting stops them; they go on from where they were the next time dm opens.",
            &[&running],
        ));
        ui.horizontal(|ui| {
            exit = primary(ui, "Exit").clicked();
            stay = ui.button(t("Keep downloading")).clicked();
        });
    });
    if exit {
        app.dialogs.confirm_exit = false;
        app.quit_asked = true;
    } else if stay {
        app.dialogs.confirm_exit = false;
    }
}

/// Asked after two handed-over downloads from one site were cancelled in a
/// row: whether to leave that site's downloads to the browser from now on,
/// which puts it on Options › Browser's list of sites never taken.
fn skip_offer(app: &mut DmApp, ctx: &egui::Context) {
    let Some(site) = app.dialogs.skip_offer.clone() else {
        return;
    };
    let (mut leave, mut keep) = (false, false);
    window("Leave this site to the browser?").show(ctx, |ui| {
        ui.label(tf(
            "You cancelled two downloads handed over from {}. Leave its downloads to the browser from now on?",
            &[&site],
        ));
        ui.horizontal(|ui| {
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
fn bare(url: &str) -> String {
    url::Url::parse(url)
        .map(|mut u| {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.to_string()
        })
        .unwrap_or_else(|_| url.to_string())
}

/// The row already holding `address`, if one does and was not cancelled.
fn already_listed(app: &DmApp, address: &str) -> Option<Id> {
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
    window("Already in the list").show(ctx, |ui| {
        ui.label(RichText::new(format::shown_url(&address)).monospace());
        ui.label(tf("This address is already in the list ({}).", &[&status]));
        ui.horizontal(|ui| {
            again = primary(ui, "Download again").clicked();
            show = ui.button(t("Show it")).clicked();
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
    let Some(d) = app.dialogs.add_url.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    window("Add URL").open(&mut open).show(ctx, |ui| {
        let address = focused(field(ui, t("Address"), &mut d.address, 420.0), focus);
        ok |= entered(&address);
        ui.checkbox(&mut d.authorize, t("Needs a sign-in"));
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
        if !d.says.is_empty() {
            ui.colored_label(WARN, &d.says);
        }
        ui.horizontal(|ui| {
            ok |= primary(ui, "OK").clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if cancel || !open {
        app.dialogs.add_url = None;
        return;
    }
    if !ok {
        return;
    }
    let mut address = d.address.trim().to_string();
    if crate::app::looks_downloadable(&address).is_none() {
        d.says = "That is not a link dm can download.".into();
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
    // Already a row: asked, rather than a second row nobody meant.
    if let Some(id) = already_listed(app, &address) {
        app.dialogs.duplicate = Some((address, id));
        return;
    }
    app.dialogs.open_file_info(vec![address]);
}

fn file_info(app: &mut DmApp, ctx: &egui::Context) {
    let Some(d) = app.dialogs.file_info.as_mut() else {
        return;
    };
    let queues = app.backend.queues();
    let default_dir = app.applied.download_dir.display().to_string();
    // The folder an empty field means: sorted into kinds' folders only
    // where Options › Folders sorts them.
    let default_dir = if app.applied.categorize {
        tf("{} (sorted by kind)", &[&default_dir])
    } else {
        default_dir
    };
    let platform = std::sync::Arc::clone(&app.backend.platform);
    let mut open = true;
    let mut chosen: Option<bool> = None; // Some(later)
    let mut cancel = false;
    window("Download details").open(&mut open).show(ctx, |ui| {
        let first = d.urls.first().cloned().unwrap_or_default();
        ui.label(RichText::new(format::shown_url(&first)).monospace());
        if d.urls.len() > 1 {
            ui.label(tf("and {} more", &[&(d.urls.len() - 1)]));
        }
        ui.separator();
        egui::Grid::new("file-info")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
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
                    browse(ui, &*platform, &mut d.folder);
                });
                ui.end_row();
                if d.urls.len() == 1 {
                    let name = ui.label(t("Save as"));
                    let save_as = ui
                        .add(
                            egui::TextEdit::singleline(&mut d.name)
                                .hint_text(format::provisional(&first))
                                .desired_width(360.0),
                        )
                        .labelled_by(name.id);
                    if entered(&save_as) {
                        chosen = Some(false);
                    }
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
        if !d.says.is_empty() {
            ui.colored_label(WARN, &d.says);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if primary(ui, "Start now").clicked() {
                chosen = Some(false);
            }
            if ui.button(t("Add for later")).clicked() {
                chosen = Some(true);
            }
            egui::ComboBox::from_label(t("into"))
                .selected_text(crate::i18n::name(&d.queue))
                .show_ui(ui, |ui| {
                    for q in &queues {
                        ui.selectable_value(&mut d.queue, q.clone(), crate::i18n::name(q));
                    }
                });
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
        save_name: Some(d.name.trim().to_string()).filter(|n| !n.is_empty()),
        later,
        referrer: d.referrer.clone(),
        quality: d.quality.clone(),
        cookies: d.cookies.clone(),
    };
    let urls = d.urls.clone();
    match app.add(&urls, with) {
        Ok(_) => app.dialogs.file_info = None,
        Err(e) => {
            if let Some(d) = app.dialogs.file_info.as_mut() {
                d.says = e;
            }
        }
    }
}

fn progress(app: &mut DmApp, ctx: &egui::Context) {
    let ids: Vec<Id> = app.dialogs.progress.iter().copied().collect();
    for id in ids {
        let Some(row) = app.row(id).cloned() else {
            app.dialogs.progress.remove(&id);
            continue;
        };
        let connections = app.backend.connections(id);
        let resumable = app.backend.resumable(id);
        let mut open = true;
        let (mut pause, mut resume, mut cancel) = (false, false, false);
        egui::Window::new(tf("Download progress — {}", &[&name_of(&row)]))
            .id(egui::Id::unique(("progress", id)))
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(RichText::new(format::shown_url(&row.url)).small().weak());
                let p = &row.progress;
                egui::Grid::new(("progress-grid", id))
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(t("Status"));
                        ui.label(status_of(&row));
                        ui.end_row();
                        ui.label(t("File size"));
                        ui.label(
                            p.total
                                .map(format::bytes)
                                .unwrap_or_else(|| t("unknown").into()),
                        );
                        ui.end_row();
                        ui.label(t("Downloaded"));
                        ui.label(match p.fraction() {
                            Some(f) => {
                                format!("{} ({:.2}%)", format::bytes(p.downloaded), f * 100.0)
                            }
                            None => format::bytes(p.downloaded),
                        });
                        ui.end_row();
                        ui.label(t("Speed"));
                        ui.label(if row.state == State::Running {
                            format::rate(p.speed)
                        } else {
                            String::new()
                        });
                        ui.end_row();
                        ui.label(t("Time left"));
                        ui.label(format::clock(p.eta));
                        ui.end_row();
                        ui.label(t("Can resume"));
                        // What the server said when the download was planned, not
                        // a guess from the size: a known size says nothing about
                        // whether a server serves ranges.
                        ui.label(match resumable {
                            Some(true) => t("Yes"),
                            Some(false) => t("No — a stop starts it again from the beginning"),
                            None => t("Unknown until the server answers"),
                        });
                        ui.end_row();
                    });
                let fraction = p.fraction().unwrap_or(0.0) as f32;
                bar(ui, fraction, 420.0, &format!("{:.0}%", fraction * 100.0));
                egui::CollapsingHeader::new(t("Connections"))
                    .default_open(true)
                    .id_salt(("connections", id))
                    .show(ui, |ui| {
                        if connections.is_empty() {
                            ui.label(RichText::new(t("Nothing fetched yet.")).weak());
                        }
                        // The file is cut into more pieces than connections,
                        // and each connection takes the next piece when it is
                        // done: the pieces under way are the connections at
                        // work, and the rest is a count.
                        let span_of = |c: &dm_core::Connection| (c.end - c.start).saturating_add(1);
                        let finished = connections.iter().filter(|c| c.done >= span_of(c)).count();
                        if !connections.is_empty() {
                            ui.label(
                                RichText::new(tf(
                                    "{} of {} pieces done",
                                    &[&finished, &connections.len()],
                                ))
                                .weak(),
                            );
                        }
                        let working = connections
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.done > 0 && c.done < span_of(c));
                        for (n, c) in working {
                            let span = span_of(c);
                            ui.horizontal(|ui| {
                                ui.label(format!("{:>3}", n + 1));
                                bar(
                                    ui,
                                    c.done as f32 / span.max(1) as f32,
                                    260.0,
                                    &format!(
                                        "{} of {}",
                                        format::bytes(c.done),
                                        format::bytes(span)
                                    ),
                                );
                            });
                        }
                    });
                ui.horizontal(|ui| {
                    if matches!(row.state, State::Running | State::Queued) {
                        pause = ui.button(t("Pause")).clicked();
                    }
                    // A failed download is tried again, and the button says so.
                    if matches!(row.state, State::Failed(_)) {
                        resume = ui.button(t("Retry")).clicked();
                    }
                    if row.state == State::Paused {
                        resume = ui.button(t("Resume")).clicked();
                    }
                    if !row.state.settled() {
                        cancel = ui.button(t("Cancel")).clicked();
                    }
                });
            });
        if pause {
            app.backend.pause(id);
        }
        if resume {
            match row.state {
                State::Failed(_) => app.backend.retry(id),
                _ => app.backend.resume(id),
            }
        }
        if cancel {
            app.backend.cancel(id);
        }
        if !open || cancel {
            app.dialogs.progress.remove(&id);
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
    let rows: Vec<dm_core::Entry> = app
        .dialogs
        .complete
        .iter()
        .filter_map(|id| app.row(*id).cloned())
        .collect();
    app.dialogs
        .complete
        .retain(|id| rows.iter().any(|r| r.id == *id));
    if rows.is_empty() {
        return;
    }
    let mut done: Vec<Id> = Vec::new();
    let mut close = false;
    let mut shut = true;
    window("Download complete").open(&mut shut).show(ctx, |ui| {
        if rows.len() > 1 {
            ui.label(tf("{} downloads finished.", &[&rows.len()]));
            ui.separator();
        }
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for row in &rows {
                    let name = name_of(row);
                    ui.label(RichText::new(&name).strong());
                    ui.label(format::bytes(
                        row.progress.total.unwrap_or(row.progress.downloaded),
                    ));
                    ui.label(RichText::new(&row.path).small().weak());
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
                            if let Err(e) = app.backend.platform.open(&row.path) {
                                app.said = e.into();
                            }
                            done.push(row.id);
                        }
                        if named(ui, t("Open folder"), tf("Open the folder of {}", &[&name])) {
                            if let Err(e) = app.backend.platform.reveal(&row.path) {
                                app.said = e.into();
                            }
                            done.push(row.id);
                        }
                    });
                    ui.add_space(6.0);
                }
            });
        ui.separator();
        close = ui.button(t("Close")).clicked();
    });
    if close || !shut {
        app.dialogs.complete.clear();
    }
    for id in done {
        app.dialogs.complete.remove(&id);
    }
}

fn properties(app: &mut DmApp, ctx: &egui::Context) {
    let ids: Vec<Id> = app.dialogs.properties.keys().copied().collect();
    for id in ids {
        let Some(row) = app.row(id).cloned() else {
            app.dialogs.properties.remove(&id);
            continue;
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
        let room = (ctx.content_rect().width() - 48.0).clamp(320.0, 620.0);
        // A close box in its title bar, as every other window has.
        let mut shut = true;
        egui::Window::new(t("File properties"))
            .id(egui::Id::unique(("properties", id)))
            .collapsible(false)
            .resizable(false)
            .open(&mut shut)
            .max_width(room)
            .constrain(true)
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
                        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Wrap));
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
                                said(ui, &d.name);
                                ui.end_row();
                                ui.label(t("Status"));
                                ui.label(status_of(&row));
                                ui.end_row();
                                ui.label(t("Size"));
                                said(
                                    ui,
                                    &row.progress.total.map(format::bytes).unwrap_or_default(),
                                );
                                ui.end_row();
                                ui.label(t("Category"));
                                // In the window's language, as the category tree
                                // names it.
                                said(
                                    ui,
                                    &row.category
                                        .map(|c| {
                                            crate::i18n::name(&format::titled(&category_word(c)))
                                        })
                                        .unwrap_or_default(),
                                );
                                ui.end_row();
                                ui.label(t("Saved to"));
                                said(ui, &row.path);
                                ui.end_row();
                                ui.label(t("Referrer"));
                                said(ui, &row.referrer);
                                ui.end_row();
                                let name = ui.label(t("Address"));
                                let editable =
                                    !matches!(row.state, State::Running | State::Completed);
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
                    ui.colored_label(WARN, &d.says);
                }
                ui.horizontal(|ui| {
                    // Properties runs a finished file from here.
                    let can_open = row.state == State::Completed && !crate::table::gone(&row);
                    open_file = ui
                        .add_enabled(can_open, egui::Button::new(t("Open")))
                        .clicked();
                    ok |= primary(ui, "OK").clicked();
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
            let result = if address.trim() != row.url {
                app.backend.set_url(id, &address)
            } else {
                Ok(())
            };
            match result {
                Ok(()) => {
                    app.dialogs.properties.remove(&id);
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
    window("Add numbered series")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(t("An address with * where the number or letter goes:"));
            let mut enter = entered(&focused(
                field(ui, t("Address with *"), &mut d.template, 420.0),
                focus,
            ));
            ui.horizontal(|ui| {
                enter |= entered(&field(ui, t("From"), &mut d.from, 60.0));
                enter |= entered(&field(ui, t("To"), &mut d.to, 60.0));
                let name = ui.label(t("Wildcard size"));
                ui.add(egui::DragValue::new(&mut d.width).range(0..=9))
                    .labelled_by(name.id);
            });
            match dm_shell::lists::series(&d.template, &d.from, &d.to, d.width) {
                Ok(urls) => {
                    ui.label(tf("{} addresses, the first {}", &[&urls.len(), &urls[0]]));
                }
                Err(e) if !d.template.is_empty() => {
                    ui.colored_label(WARN, e);
                }
                Err(_) => {}
            }
            if !d.says.is_empty() {
                ui.colored_label(WARN, &d.says);
            }
            ui.horizontal(|ui| {
                if primary(ui, "Start now").clicked() || enter {
                    add = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    add = Some(true);
                }
            });
        });
    if !open {
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
                    app.said = crate::i18n::Said::new("Added {} download(s).", &[&n]);
                    app.dialogs.series = None;
                }
                Err(e) => d_says(&mut app.dialogs.series, e),
            }
        }
        Err(e) => d.says = e,
    }
}

fn d_says(series: &mut Option<Series>, e: String) {
    if let Some(d) = series.as_mut() {
        d.says = e;
    }
}

/// A list of links to tick before adding: the clipboard's, or a site's.
fn pick_list(ui: &mut egui::Ui, links: &mut [(String, bool)]) {
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
                ui.checkbox(on, format::shown_url(link));
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
    window("Download links").open(&mut open).show(ctx, |ui| {
        ui.label(tf(
            "{} links on {}",
            &[&d.items.len(), &format::shown_url(&d.page)],
        ));
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.hide_html, t("Hide HTML files"));
            if ui.button(t("Check all")).clicked() {
                d.items.iter_mut().for_each(|l| l.1 = true);
            }
            if ui.button(t("Uncheck all")).clicked() {
                d.items.iter_mut().for_each(|l| l.1 = false);
            }
        });
        let found = dm_core::lock(&d.found).clone();
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
                            // The name is the checkbox's, so a screen reader says it.
                            ui.checkbox(on, &name).on_hover_text(format::shown_url(url));
                            ui.label(
                                what.map(|f| format::titled(&f.kind))
                                    .unwrap_or_else(|| "…".into()),
                            );
                            ui.label(
                                what.and_then(|f| f.size)
                                    .map(format::bytes)
                                    .unwrap_or_default(),
                            );
                            ui.end_row();
                        }
                    });
            });
        ui.separator();
        ui.horizontal(|ui| {
            if primary(ui, "Start now").clicked() {
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
                app.said =
                    crate::i18n::Said::naming("Added {} download(s) to {}.", &[&n, &MAIN_QUEUE])
            }
            Ok(n) => app.said = crate::i18n::Said::new("Started {} download(s).", &[&n]),
            Err(e) => app.said = e.into(),
        }
        return;
    }
    if cancel || !open {
        app.dialogs.links = None;
    }
}

fn clip_links(app: &mut DmApp, ctx: &egui::Context) {
    let Some(links) = app.dialogs.clip_links.as_mut() else {
        return;
    };
    let mut open = true;
    // Some(later): Start now, or Add for later.
    let mut add: Option<bool> = None;
    let mut cancel = false;
    window("Links on the clipboard")
        .open(&mut open)
        .show(ctx, |ui| {
            pick_list(ui, links);
            ui.separator();
            ui.horizontal(|ui| {
                if primary(ui, "Start now").clicked() {
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
                app.said =
                    crate::i18n::Said::naming("Added {} download(s) to {}.", &[&n, &MAIN_QUEUE])
            }
            Ok(n) => app.said = crate::i18n::Said::new("Started {} download(s).", &[&n]),
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
    let focus = app.dialogs.take_focus("Files on a site");
    let Some(d) = app.dialogs.site_files.as_mut() else {
        return;
    };
    let mut open = true;
    let mut add: Option<Vec<String>> = None;
    let mut look = false;
    let mut save = false;
    let mut run_saved: Option<dm_shell::lists::Search> = None;
    let mut delete_saved: Option<String> = None;
    let saved = app.applied.site_searches.clone();
    window("Files on a site").open(&mut open).show(ctx, |ui| {
        look |= entered(&focused(
            field(ui, t("Start page"), &mut d.page, 420.0),
            focus,
        ));
        field(ui, t("File types"), &mut d.types, 420.0);
        ui.horizontal(|ui| {
            let name = ui.label(t("Pages deep (0 for the start page only)"));
            ui.add(egui::DragValue::new(&mut d.depth).range(0..=dm_shell::lists::MAX_DEPTH))
                .labelled_by(name.id);
        });
        field(ui, t("Only paths containing"), &mut d.include, 420.0);
        field(ui, t("Skip paths containing"), &mut d.exclude, 420.0);
        // Saving and the saved list come before the results, which can
        // run long enough to push anything below them off the screen.
        ui.horizontal(|ui| {
            field(ui, t("Save this search as"), &mut d.name, 160.0);
            let name = ui.label(t("Run every, hours (0 only by hand)"));
            ui.add(egui::DragValue::new(&mut d.every_hours).range(0..=24 * 31))
                .labelled_by(name.id);
            save = ui.button(t("Save search")).clicked();
        });
        if !d.says.is_empty() {
            ui.colored_label(WARN, &d.says);
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
        look |= ui.button(t("Find files")).clicked();
        let mut state = dm_core::lock(&d.state);
        match &mut *state {
            Grab::Idle => {}
            Grab::Looking => {
                ui.label(t("Reading the page…"));
            }
            Grab::Failed(e) => {
                ui.colored_label(WARN, e.as_str());
            }
            Grab::Found(links, _) if links.is_empty() => {
                ui.label(t("No files of those types are linked from that page."));
            }
            Grab::Found(links, pages) => {
                ui.label(tf("{} files found on {} pages", &[&links.len(), pages]));
                pick_list(ui, links);
                if ui.button(t("Add for later")).clicked() {
                    add = Some(links.iter().filter(|l| l.1).map(|l| l.0.clone()).collect());
                }
            }
        }
    });
    if look {
        let search = d.search();
        let state = Arc::clone(&d.state);
        *dm_core::lock(&state) = Grab::Looking;
        let ctx = ctx.clone();
        app.backend.runtime.spawn(async move {
            let found = dm_shell::lists::crawl(&client, &search).await;
            *dm_core::lock(&state) = match found {
                Ok(found) => Grab::Found(
                    found.files.into_iter().map(|l| (l, true)).collect(),
                    found.pages,
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
            if let Some(before) = settings
                .site_searches
                .iter()
                .find(|s| s.name == search.name)
            {
                search.seen = before.seen.clone();
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
            app.said = crate::i18n::Said::naming("Added {} download(s) to {}.", &[&n, &MAIN_QUEUE]);
        }
        open = false;
    }
    if !open {
        app.dialogs.site_files = None;
    }
}

fn update_link(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Update download link");
    let Some((id, address, says)) = app.dialogs.update_link.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    window("Update download link")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(t("The download's link has expired? Paste a fresh one:"));
            ok = entered(&focused(field(ui, t("New address"), address, 420.0), focus));
            if !says.is_empty() {
                ui.colored_label(WARN, says.as_str());
            }
            ok |= primary(ui, "OK").clicked();
        });
    if ok {
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
    let Some((ids, queue)) = app.dialogs.move_to_queue.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    let known = app.backend.queues();
    // The queue is held as shown, so the field reads in the language in
    // force, and taken back to dm's own name on OK: typing "Principal" in
    // Spanish is the Main queue, not a new one beside it.
    window("Move to queue").open(&mut open).show(ctx, |ui| {
        for q in &known {
            let shown = crate::i18n::name(q);
            ui.radio_value(queue, shown.clone(), shown);
        }
        ok = entered(&focused(
            field(ui, t("Or a new queue"), queue, 240.0),
            focus,
        ));
        ok |= primary(ui, "OK").clicked();
    });
    if ok && !queue.trim().is_empty() {
        let named = crate::i18n::queue_named(queue);
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
    let Some((id, name)) = app.dialogs.rename.as_mut() else {
        return;
    };
    let mut open = true;
    let mut ok = false;
    window("Move/Rename").open(&mut open).show(ctx, |ui| {
        ok = entered(&focused(field(ui, t("New name"), name, 320.0), focus));
        ok |= primary(ui, "OK").clicked();
    });
    if ok {
        match app.backend.rename(*id, name) {
            Ok(()) => open = false,
            Err(e) => app.said = e.into(),
        }
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
    window("Find").open(&mut open).show(ctx, |ui| {
        let r = focused(
            field(ui, t("Find what"), &mut app.dialogs.find_text, 260.0),
            focus,
        );
        next = ui.button(t("Find next")).clicked() || entered(&r);
    });
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
    window("Scheduler").open(&mut open).show(ctx, |ui| {
        ui.label(tf(
            "When {} and every other download may run.",
            &[&crate::i18n::name(MAIN_QUEUE)],
        ));
        ui.checkbox(&mut schedule.enabled, t("Only download between set hours"));
        ui.add_enabled_ui(schedule.enabled, |ui| {
            ui.horizontal(|ui| {
                apply |= entered(&field(ui, t("Start at"), &mut schedule.from, 60.0));
                apply |= entered(&field(ui, t("Stop at"), &mut schedule.to, 60.0));
            });
            ui.horizontal(|ui| {
                for (day, word) in DAYS.iter().enumerate() {
                    let day = day as u8;
                    let mut on = schedule.days.contains(&day);
                    if ui.checkbox(&mut on, t(word)).changed() {
                        if on {
                            schedule.days.push(day);
                            schedule.days.sort_unstable();
                        } else {
                            schedule.days.retain(|d| *d != day);
                        }
                    }
                }
            });
            ui.label(RichText::new(t("HH:MM, local time; no days ticked means every day. A window ending before it starts runs overnight.")).small().weak());
        });
        if !says.is_empty() {
            ui.colored_label(WARN, says.as_str());
        }
        apply |= ui.button(t("Apply")).clicked();
    });
    if apply
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
    let Some((limit, says, mb, hours)) = app.dialogs.limiter.as_mut() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    window("Speed limiter").open(&mut open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let name = ui.label(t("Maximum, KB/s across every download (0 for none)"));
            ui.add(egui::DragValue::new(limit).range(0..=1024 * 1024).speed(16))
                .labelled_by(name.id);
        });
        ui.add_space(8.0);
        // Download limits, for a link with a fair-use
        // allowance: downloads are held once it is used and go on after.
        ui.horizontal(|ui| {
            let name = ui.label(t("Download no more than, MB (0 for no limit)"));
            ui.add(
                egui::DragValue::new(mb)
                    .range(0..=1024 * 1024 * 1024)
                    .speed(16),
            )
            .labelled_by(name.id);
        });
        ui.horizontal(|ui| {
            let name = ui.label(t("every, hours"));
            ui.add(egui::DragValue::new(hours).range(1..=24 * 31))
                .labelled_by(name.id);
        });
        if !says.is_empty() {
            ui.colored_label(WARN, says.as_str());
        }
        apply = ui.button(t("Apply")).clicked();
    });
    if apply {
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
    let tall = (ctx.content_rect().height() - 60.0).clamp(360.0, 680.0);
    let Some(d) = app.dialogs.options.as_mut() else {
        return;
    };
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    window("Options")
        .fixed_size(egui::vec2(660.0, tall - 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (tab, label) in TABS {
                if ui.selectable_label(d.tab == tab, t(label)).clicked() {
                    d.tab = tab;
                }
            }
        });
        ui.separator();
        // Room kept below for a warning and the buttons.
        let body = (ui.available_height() - 80.0).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("options-tab")
            .max_height(body)
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
                ui.horizontal(|ui| {
                    let now = crate::i18n::chosen(&s.language);
                    // Named by its label, so a screen reader says what it is.
                    egui::ComboBox::from_label(t("Language"))
                        .selected_text(crate::i18n::name_of(now))
                        .show_ui(ui, |ui| {
                            if let Some(code) = crate::i18n::pick(ui, now) {
                                s.language = code.to_string();
                            }
                        });
                });
                ui.checkbox(&mut s.start_on_login, t("Start dm when you log in, so a browser hand-off always has an app to land in"));
                ui.checkbox(&mut s.check_updates, t("Look for a new version about once a week (asks GitHub; nothing about you is sent)"));
                ui.checkbox(&mut s.watch_clipboard, t("Watch the clipboard for links and offer to download them"));
                ui.checkbox(&mut s.show_progress, t("Show a progress window when a download starts"));
                ui.checkbox(&mut s.show_complete, t("Show a window when a download finishes"));
                ui.checkbox(&mut s.sound_on_complete, t("Play a sound when a download finishes"));
                ui.checkbox(&mut s.sound_on_failure, t("Play a sound when a download fails"));
                ui.checkbox(&mut s.ask_panel_downloads, t("Ask where to save a video from the browser's panel"));
                ui.horizontal(|ui| {
                    ui.label(t("Look"));
                    for (word, name) in THEMES {
                        ui.radio_value(&mut s.theme, word.to_string(), t(name));
                    }
                });
            }
            Tab::FileTypes => {
                let c = &mut s.catch;
                ui.checkbox(&mut c.enabled, t("Take downloads of these types from the browser"));
                ui.add_enabled_ui(c.enabled, |ui| {
                    let name = ui.label(t("File types"));
                    ui.add(egui::TextEdit::multiline(&mut d.types).desired_rows(3).desired_width(480.0))
                        .labelled_by(name.id);
                    ui.label(RichText::new(t("Without dots, separated by spaces. Empty takes every type.")).small().weak());
                    ui.horizontal(|ui| {
                        let name = ui.label(t("Leave files smaller than, KB (0 for none)"));
                        ui.add(egui::DragValue::new(&mut c.min_size_kb).range(0..=1024 * 1024)).labelled_by(name.id);
                    });
                    field(ui, t("Never take downloads from these sites"), &mut d.skip, 480.0);
                });
                ui.label(RichText::new(t("Read by the browser extension when it starts and every few minutes. The right-click menu and the toolbar button hand anything over regardless.")).small().weak());
            }
            Tab::SaveTo => {
                let mut dir = s.download_dir.display().to_string();
                let before = dir.clone();
                folder_field(ui, t("Download folder"), &mut dir, &*platform);
                if dir != before {
                    s.download_dir = dir.into();
                }
                folder_field(ui, t("Folder for unfinished downloads (empty for dm's own)"), &mut s.parts_dir, &*platform);
                ui.label(RichText::new(t("A download already begun keeps the folder it began in.")).small().weak());
                ui.checkbox(&mut s.categorize, t("Sort finished files into folders by kind"));
                ui.add_enabled_ui(s.categorize, |ui| {
                    // Two kinds a row: seven rows pushed the categories
                    // below out of sight.
                    egui::Grid::new("folders").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                        for (i, c) in Category::ALL.iter().enumerate() {
                            let name = ui.label(format::titled(&category_word(*c)));
                            if let Some(folder) = s.category_folders.get_mut(i) {
                                ui.add(egui::TextEdit::singleline(folder).desired_width(180.0))
                                    .labelled_by(name.id);
                            }
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                    ui.separator();
                    ui.label(t("Your categories, looked at first: a file of these types, from these sites (any when empty), goes in a folder of the category's name."));
                    let mut remove = None;
                    let count = d.custom.len();
                    for (i, [name, types, sites]) in d.custom.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
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
                egui::Grid::new("downloads").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    let name = ui.label(t("Connections per file"));
                    ui.add(egui::DragValue::new(&mut s.connections).range(1..=32)).labelled_by(name.id);
                    ui.end_row();
                    let name = ui.label(t("Files at once"));
                    ui.add(egui::DragValue::new(&mut s.concurrency).range(1..=10)).labelled_by(name.id);
                    ui.end_row();
                    let choices = QUALITIES;
                    let current = choices.iter().find(|(v, _)| *v == s.stream_quality).map(|(_, l)| *l).unwrap_or("Best available");
                    ui.label(t("Video quality"));
                    egui::ComboBox::from_id_salt("quality").selected_text(t(current)).show_ui(ui, |ui| {
                        for (value, label) in choices {
                            ui.selectable_value(&mut s.stream_quality, value.to_string(), t(label));
                        }
                    });
                    ui.end_row();
                });
                ui.checkbox(&mut s.start_queue_on_launch, t("Start the Main queue when dm opens"));
                ui.checkbox(&mut s.subtitles, t("Embed subtitles in videos from player pages (needs ffmpeg)"));
                ui.checkbox(&mut s.remux_ts, t("Save streams that arrive as .ts as MP4 (needs ffmpeg)"));
                ui.add_space(4.0);
                ui.label(t("Queues that run fewer files at once than the number above:"));
                let mut remove = None;
                for (i, limit) in s.queue_limits.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        // Chosen from the queues there are, shown in the
                        // language in force and kept by dm's own name: a
                        // typed name had to be the English one to match.
                        egui::ComboBox::new(("queue limit", i), t("Queue"))
                            .selected_text(crate::i18n::name(&limit.queue))
                            .show_ui(ui, |ui| {
                                let mut offered = queues.clone();
                                if !offered.contains(&limit.queue) {
                                    offered.push(limit.queue.clone());
                                }
                                for q in offered {
                                    let shown = crate::i18n::name(&q);
                                    ui.selectable_value(&mut limit.queue, q, shown);
                                }
                            });
                        let name = ui.label(t("Files at once"));
                        ui.add(egui::DragValue::new(&mut limit.files).range(1..=10)).labelled_by(name.id);
                        if ui.button(t("Delete queue limit")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    s.queue_limits.remove(i);
                }
                if ui.button(t("Add a queue limit")).clicked() {
                    s.queue_limits.push(dm_shell::settings::QueueLimit { queue: dm_core::MAIN_QUEUE.into(), files: 1 });
                }
                ui.horizontal(|ui| {
                    let name = ui.label(t("Ask the Recheck queue's servers for changes every, hours (0 only when asked)"));
                    ui.add(egui::DragValue::new(&mut s.recheck_hours).range(0..=24 * 31)).labelled_by(name.id);
                });
                ui.add_space(8.0);
                ui.label(t("When a download finishes, run (a virus checker, a script); the file's path is added at the end:"));
                field(ui, t("Command"), &mut s.on_complete, 320.0);
            }
            Tab::Connection => {
                ui.horizontal(|ui| {
                    let name = ui.label(t("Speed limit, KB/s (0 for none)"));
                    ui.add(egui::DragValue::new(&mut s.speed_limit).range(0..=1024 * 1024).speed(16)).labelled_by(name.id);
                });
                ui.horizontal(|ui| {
                    let name = ui.label(t("Attempts per connection (0 to keep trying)"));
                    ui.add(egui::DragValue::new(&mut s.attempts).range(0..=100)).labelled_by(name.id);
                });
                ui.horizontal(|ui| {
                    let name = ui.label(t("Give up on a connection that sends nothing for, seconds"));
                    ui.add(egui::DragValue::new(&mut s.read_timeout).range(5..=600)).labelled_by(name.id);
                });
                ui.add_space(8.0);
                ui.label(t("Sites that take fewer connections at once than the setting:"));
                let mut remove = None;
                let count = s.host_limits.len();
                for (i, limit) in s.host_limits.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        first(field(ui, t("Site"), &mut limit.host, 200.0), i + 1 == count);
                        let name = ui.label(t("Connections"));
                        ui.add(egui::DragValue::new(&mut limit.connections).range(1..=32)).labelled_by(name.id);
                        if ui.button(t("Delete site limit")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    s.host_limits.remove(i);
                }
                if ui.button(t("Add a site limit")).clicked() {
                    s.host_limits.push(dm_core::HostLimit { host: String::new(), connections: 1 });
                    d.fresh = true;
                }
                ui.add_space(8.0);
                ui.label(t("For downloads that need you signed in: a cookies.txt exported from your browser."));
                ui.horizontal(|ui| {
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
            }
            Tab::Proxy => {
                use dm_core::proxy::Kind;
                let p = &mut s.proxy;
                ui.checkbox(&mut p.enabled, t("Connect through a proxy"));
                ui.add_enabled_ui(p.enabled, |ui| {
                    let kind_name = |kind: Kind| match kind {
                        Kind::System => t("The computer's proxy settings").to_string(),
                        other => format!("{other:?}").to_uppercase(),
                    };
                    egui::ComboBox::from_label(t("Proxy type"))
                        .selected_text(kind_name(p.kind))
                        .show_ui(ui, |ui| {
                            for kind in [Kind::System, Kind::Http, Kind::Https, Kind::Socks4, Kind::Socks5] {
                                ui.selectable_value(&mut p.kind, kind, kind_name(kind));
                            }
                        });
                    ui.add_enabled_ui(p.kind != Kind::System, |ui| {
                        field(ui, t("Host"), &mut p.host, 260.0);
                        ui.horizontal(|ui| {
                            let name = ui.label(t("Port"));
                            ui.add(egui::DragValue::new(&mut p.port).range(1..=65535)).labelled_by(name.id);
                        });
                        field(ui, t("Username, if it needs one"), &mut p.username, 200.0);
                        let name = ui.label(t("Password"));
                        ui.add(egui::TextEdit::singleline(&mut p.password).password(true).desired_width(200.0)).labelled_by(name.id);
                        field(ui, t("Not for these sites (fetched directly)"), &mut d.bypass, 260.0);
                    });
                    ui.label(RichText::new(t("Every download of dm's own, yt-dlp and a torrent's SOCKS5 connections go through it; FTP through SOCKS or HTTP CONNECT.")).small().weak());
                });
            }
            Tab::SiteLogins => {
                ui.label(t("A user and password used for every download from a host."));
                ui.label(RichText::new(t("Sent as a Basic login, or as a Digest where the server asks for one.")).small().weak());
                let mut remove = None;
                let count = s.site_logins.len();
                for (i, login) in s.site_logins.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        first(field(ui, t("Host"), &mut login.host, 150.0), i + 1 == count);
                        field(ui, t("User"), &mut login.username, 110.0);
                        let name = ui.label(t("Password"));
                        ui.add(egui::TextEdit::singleline(&mut login.password).password(true).desired_width(110.0)).labelled_by(name.id);
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
                ui.label(RichText::new(t("Kept in dm's settings file on this computer, readable by your account.")).small().weak());
            }
        }
            });
        if !d.says.is_empty() {
            ui.colored_label(WARN, &d.says);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = primary(ui, "OK").clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
    if cancel || !open {
        app.dialogs.options = None;
        return;
    }
    if ok {
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| c.is_whitespace() || c == ',')
                .map(|w| w.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|w| !w.is_empty())
                .collect()
        };
        let mut wanted = d.draft.clone();
        wanted.catch.extensions = words(&d.types);
        wanted.catch.skip_hosts = words(&d.skip);
        wanted.proxy.bypass = words(&d.bypass);
        wanted.custom_categories = d
            .custom
            .iter()
            .filter(|[name, ..]| !name.trim().is_empty())
            .map(|[name, types, sites]| dm_core::Custom {
                name: name.trim().to_string(),
                extensions: words(types),
                sites: words(sites),
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
    let mut cancel = false;
    let mut now = false;
    window("Downloads are done").show(ctx, |ui| {
        ui.label(tf("dm will {} in {} seconds.", &[&t(action.verb()), &left]));
        ui.horizontal(|ui| {
            now = ui.button(t("Now")).clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
    });
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
    window("About dm").open(&mut open).show(ctx, |ui| {
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
