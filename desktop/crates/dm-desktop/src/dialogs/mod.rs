// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's dialogs: Add URL, Download details, a progress window per
//! download, Download complete, Properties, numbered series, files on a
//! site, Update download link, Move to queue, Move/Rename, Find,
//! Scheduler, Speed limiter, Options and About.
//!
//! Each is drawn from state held here and acts on the app when its button
//! is pressed; none of them reaches the queue except through the backend.

use crate::commands::Shown as _;
use crate::i18n::{t, tf};
use crate::update::Said as _;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use dm_core::{AddOptions, Category, Id, State, MAIN_QUEUE};
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
pub use about::{tell_a_friend, tips};
pub use add::*;
pub use asks::NameAsk;
use asks::*;
use complete::*;
use confirm::*;
/// The room dm's own window has: its content rectangle. Inside a dialog, which is a window of
/// its own (contract row B42), `ctx.content_rect()` is the dialog's, and a dialog that sized
/// itself by it was held to its own first, small size (File properties, 2026-11-07). Read for
/// the main window wherever it is asked, so every dialog measures the same room.
pub(crate) fn room(ctx: &egui::Context) -> egui::Rect {
    ctx.input_for(egui::ViewportId::ROOT, |i| i.content_rect())
}

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
/// one, and the folder they go to (`dm_desktop_base::row_edits`, both
/// windows).
pub use dm_desktop_base::row_edits::MoveRename;

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
    /// The last file of the ZIP taken out alone ("Take out"): `None` until
    /// one is asked for.
    pub taken: Option<TakeSlot>,
    /// That file's name, said while it is taken out.
    pub taking: String,
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
    /// The form it is the answer to, where the page sent one.
    pub post: Option<dm_core::cookies::Posted>,
    /// Browse… was pressed on a desktop with no folder chooser: said in
    /// this opening of the dialog only (it stayed for the next).
    pub no_chooser: bool,
    /// The name the server gives the file, once asked, and
    /// whether a person typed in Save as since, which it then leaves alone.
    pub probed: Option<std::sync::Arc<std::sync::Mutex<Option<Probed>>>>,
    /// Whether a person typed in Save as.
    pub name_typed: bool,
    /// The row begun at once, while the dialog asks (`Settings::start_while_asking`), and
    /// whether that was tried.
    pub early: Option<dm_core::Id>,
    /// Whether a start was tried for this dialog.
    pub early_tried: bool,
    /// The folder last used for this kind of file has been offered (once
    /// the name says the kind), so clearing it is not undone.
    pub folder_offered: bool,
    /// A file of its name is in the folder already, and the person chose
    /// to replace it once this one is whole rather than take a number.
    pub replace: bool,
}

/// What Download details learned by asking the server, and the name an HLS
/// stream is saved under: the application's (`dm_shell::app::lookups`).
pub use dm_shell::app::lookups::{stream_name, Probed};

/// "Download links": what each link's server said, the dialog's state and
/// rules, and where the ticked links go; dm's (`dm_desktop_base::links_import`),
/// as every window's.
pub use dm_desktop_base::links_import::{Found, LinksImport, SaveTo};

pub use dm_shell::app::names::category_folder;

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
    /// When each of a torrent's files is fetched, as edited (gaps row 149).
    pub priorities: Vec<dm_core::TorrentPriority>,
}

/// The files of a ZIP, as a background reading leaves them: `None` while it
/// is under way, an empty list where there was nothing to list.
pub use dm_shell::app::files::{TakeSlot, ZipSlot};

/// What the background hashing of a finished file has found so far.
pub use dm_shell::app::files::Sums;

/// An address's sign-in taken out for its own fields, and put back on OK
/// (`dm_desktop_base::row_edits`, both windows).
use dm_desktop_base::row_edits::{join_login, split_login};

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

/// The Grabber's search as typed, and how a run of it is going: dm's
/// (`dm_desktop_base::grabber`), as every window's.
pub use dm_desktop_base::grabber::{Grab, SiteFiles};

/// Options' pages, the settings each puts back, and their names
/// (`dm_desktop_base::options`, both windows).
pub use dm_desktop_base::options::{tab_settings, Tab, TABS, THEMES};

impl OptionsDialog {
    /// Puts the tab's settings back to what a new install has, in the draft:
    /// OK keeps them, Cancel leaves everything as it was. The words typed for
    /// the lists are read again from what they now hold.
    pub fn restore_defaults(&mut self) {
        self.draft = dm_desktop_base::options::restore_defaults(self.tab, &self.draft);
        let typed = dm_desktop_base::options::Typed::of(&self.draft);
        self.types = typed.types;
        self.skip = typed.skip;
        self.once = typed.once;
        self.bypass = typed.bypass;
        self.custom = typed.custom;
    }
}

/// Options › Downloads' video qualities: the setting's word, and its name.
pub use dm_desktop_base::file_info::QUALITIES;

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
    /// The tip Tip of the day shows, while it is open.
    pub tip: Option<usize>,
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
            room(ctx),
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
    tip_of_the_day(app, ctx);
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
    let room = room(ctx);
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

#[cfg(test)]
mod links_import_tests {
    use super::*;

    fn import(urls: &[&str]) -> LinksImport {
        LinksImport {
            items: urls.iter().map(|u| (u.to_string(), true)).collect(),
            ..LinksImport::default()
        }
    }

    #[test]
    fn only_these_types_go_by_the_servers_name_or_the_addresses() {
        let mut d = import(&[
            "http://h/a.zip",
            "http://h/b.PDF?x=1",
            "http://h/get?id=3",
            "http://h/c.mp4",
        ]);
        d.types = ".zip, pdf".into();
        // The third is named by its server: a zip.
        dm_core::lock(&d.found).insert(
            "http://h/get?id=3".into(),
            Found {
                name: "named.zip".into(),
                ..Found::default()
            },
        );
        assert_eq!(d.visible(), [true, true, true, false]);
        assert_eq!(
            d.chosen(),
            ["http://h/a.zip", "http://h/b.PDF?x=1", "http://h/get?id=3"]
        );
        d.types = "  ".into();
        assert!(d.visible().iter().all(|v| *v), "nothing typed lists all");
    }

    #[test]
    fn the_folder_every_link_goes_to_is_the_choice_made() {
        let root = std::path::Path::new("downloads");
        let sorted = dm_core::Categories {
            enabled: true,
            ..dm_core::Categories::default()
        };
        let mut d = import(&["http://h/a.zip", "http://h/b.mp4"]);
        assert_eq!(d.save.save_dir(root, &sorted), None, "each to its own");
        assert_eq!(
            d.folder_of("http://h/b.mp4", root, &sorted),
            sorted
                .destination(root, dm_core::Category::Videos)
                .display()
                .to_string()
        );
        d.save.save_to = SaveTo::OneFolder;
        d.save.folder = "   ".into();
        assert_eq!(d.save.save_dir(root, &sorted), None, "no folder typed");
        d.save.folder = " elsewhere ".into();
        assert_eq!(d.save.save_dir(root, &sorted).as_deref(), Some("elsewhere"));
        assert_eq!(d.folder_of("http://h/a.zip", root, &sorted), "elsewhere");
        d.save.save_to = SaveTo::OneCategory;
        d.save.category = crate::app::category_word(dm_core::Category::Documents);
        assert_eq!(
            d.save.save_dir(root, &sorted),
            Some(
                sorted
                    .destination(root, dm_core::Category::Documents)
                    .display()
                    .to_string()
            )
        );
        // A person's own category is a folder of its name.
        d.save.category = "Receipts".into();
        assert_eq!(
            category_folder(&d.save.category, root, &sorted),
            root.join("Receipts")
        );
    }

    #[test]
    fn a_name_given_with_edit_wins_over_the_servers() {
        let mut d = import(&["http://h/a.zip"]);
        dm_core::lock(&d.found).insert(
            "http://h/a.zip".into(),
            Found {
                name: "server.zip".into(),
                ..Found::default()
            },
        );
        assert_eq!(d.name_of("http://h/a.zip"), "server.zip");
        d.names.insert("http://h/a.zip".into(), "  ".into());
        assert_eq!(d.name_of("http://h/a.zip"), "server.zip", "blank is none");
        d.names.insert("http://h/a.zip".into(), " mine.zip ".into());
        assert_eq!(d.name_of("http://h/a.zip"), "mine.zip");
    }
}
