// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details, a window of its own (B02): where the file goes (its
//! category, its folder, chosen in the system's own folder panel), its name
//! (the server's, once asked), a video's qualities and a part of it, a
//! ZIP's files (one taken out alone), its size, a note, when it is deleted,
//! and Start now, Add for later to a queue, or Cancel. Opened for an
//! address typed in Add URL, or for a download the browser handed over,
//! with what the browser sent (the page, the sign-in, the quality picked in
//! its panel, the browser's name). What it offers and adds is dm's
//! (`dm_desktop_base::file_info`).

use dm_core::Id;
use dm_desktop_base::file_info::{self, Clash, Form};
use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::zip_list::{self, Listing, TakeSlot, ZipSlot};
use dm_shell::app::backend::Backend;
use dm_shell::app::lookups::Probed;
use dm_shell::Settings;
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent, Role,
    SharedString, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::widgets::popup::{self, Pick};
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod opening;
mod sections;

pub use opening::{open, open_handed};

use sections::Look;

/// What a browser's hand-off brought with the address; nothing for one
/// typed in Add URL.
#[derive(Debug, Clone, Default)]
pub struct Handed {
    /// Handed over by the browser, rather than typed in: cancelling it is a
    /// word against that site's downloads being handed over.
    pub handed: bool,
    /// The page it came from, sent as Referer.
    pub referrer: String,
    /// That page's title: names a stream, whose address names only its playlist (gaps row 214).
    pub title: String,
    /// The quality picked in the browser's panel.
    pub quality: Option<String>,
    /// A separate sound track, fetched and joined with it.
    pub sound: Option<String>,
    /// What the panel's list called that quality.
    pub label: Option<String>,
    /// The site's sign-in the browser handed over.
    pub cookies: String,
    /// Those cookies are for this download alone.
    pub cookies_once: bool,
    /// The request headers the browser sent.
    pub headers: Vec<(String, String)>,
    /// The form it is the answer to.
    pub post: Option<dm_core::cookies::Posted>,
    /// The browser it came from, where the host could tell.
    pub browser: Option<String>,
    /// Caught by the browser extension (not asked for from its panel or a
    /// menu): offered "Always save here without asking".
    pub caught: bool,
    /// From a private browser window: "Keep no record" opens ticked (gaps row W17).
    pub keep_no_record: bool,
}

impl Handed {
    /// The sign-in for `url`, from what the browser sent, with its site rule's `User-Agent` and
    /// headers (Options › Advanced), as the download will be asked with them.
    fn sign_in(&self, settings: &Settings, url: &str) -> Option<dm_core::cookies::SignIn> {
        let headers = settings.site_headers(url, &self.headers);
        dm_core::cookies::SignIn::with_headers(url, &self.cookies, &headers)
    }

    /// The page as Referer, where there is one.
    fn page(&self) -> Option<String> {
        Some(self.referrer.clone()).filter(|r| !r.is_empty())
    }
}

/// The Download details window's view.
pub struct Details {
    view: WeakEntity<Downloads>,
    backend: Backend,
    settings: Settings,
    urls: Vec<String>,
    handed: Handed,
    folder: Entity<InputState>,
    name: Entity<InputState>,
    note: Entity<InputState>,
    part_from: Entity<InputState>,
    part_to: Entity<InputState>,
    /// What the server said, once it has; `None` while it is asked.
    probed: Option<Probed>,
    /// The folder last asked how much it has free, and its answer: asked again only when the
    /// folder changes, not at every frame (gaps row U17).
    free: Option<(String, Option<u64>)>,
    /// The folder this kind of file went to last has been offered.
    folder_offered: bool,
    /// "Always save here without asking" is ticked.
    always_here: bool,
    queue: String,
    queues: Vec<String>,
    delete_after: Option<u64>,
    /// "Unpack when done" ticked for this download (gaps row W14).
    unpack: bool,
    replace: bool,
    /// "Keep no record" is ticked: the row goes once the file is whole (gaps row W17).
    pub(crate) keep_no_record: bool,
    /// A video's qualities with their sizes: asked once it is known to be
    /// a video; `None` until they are read.
    choices: Option<Vec<dm_core::videopage::Choice>>,
    choices_asked: bool,
    /// The quality picked, and its name.
    quality: Option<String>,
    label: Option<String>,
    /// "Only a part of the video", its shape, and as a GIF.
    part: bool,
    shape: Option<dm_core::shape::Shape>,
    gif: bool,
    /// A ZIP's files, read from its end, and the one taken out alone.
    zip: Option<ZipSlot>,
    taken: Option<TakeSlot>,
    taking: String,
    /// The row begun at once while the dialog asks.
    early: Option<Id>,
    /// Where the name is taken, as last worked out off the window's thread
    /// for the folder and name typed then, and when (`clash_shown`).
    clash_seen: Option<(String, String, Option<Clash>, std::time::Instant)>,
    clash_looking: bool,
    says: Option<SharedString>,
    /// Answered: a second Return in the same moment adds nothing.
    done: bool,
    pub(crate) focus: FocusHandle,
    screen: Screen,
}

/// How long Download details trusts what it found of the name's clash.
const CLASH_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

impl Details {
    /// The address the dialog is about (the first of several).
    fn first(&self) -> String {
        self.urls.first().cloned().unwrap_or_default()
    }

    /// The name the server gave, once it has.
    fn server_name(&self) -> Option<&str> {
        self.probed.as_ref().and_then(|p| p.name.as_deref())
    }

    /// Whether this is a YouTube video's address.
    fn youtube(&self) -> bool {
        self.urls.len() == 1 && dm_core::youtube::video_id(&self.first()).is_some()
    }

    /// Whether it is a video: a YouTube video, or a page with one.
    fn video(&self) -> bool {
        self.urls.len() == 1 && (self.youtube() || self.probed.as_ref().is_some_and(|p| p.page))
    }

    /// The folder last used for this kind of file, offered once its name
    /// says its kind, where nothing is typed.
    fn offer_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        if self.folder_offered || self.urls.len() != 1 || name.trim().is_empty() {
            return;
        }
        self.folder_offered = true;
        if self.folder.read(cx).value().trim().is_empty() {
            if let Some(last) = file_info::last_folder(&self.settings, &name) {
                self.folder
                    .update(cx, |f, cx| f.set_value(last, window, cx));
            }
        }
    }

    /// What the server said: its name put in Save as where nothing is
    /// typed, and a video's qualities asked for once it is one.
    fn probed(&mut self, mut found: Probed, window: &mut Window, cx: &mut Context<Self>) {
        // Its https:// form answered: that is the address downloaded (gaps row H6).
        if let (Some(secure), [only]) = (found.upgraded.clone(), self.urls.as_mut_slice()) {
            *only = secure;
        }
        // A stream is named after the page's title, as the engine names it (gaps row 214): the
        // server's name for it here, so Save as is not taken for a name typed.
        let stream = dm_shell::app::lookups::stream_name(&self.first(), None, false).is_some();
        if stream && !self.handed.title.trim().is_empty() {
            found.name = found
                .name
                .map(|name| dm_core::stream_titled(&name, Some(&self.handed.title), None));
        }
        // A web page that is not YouTube's: the engine reads it for its video and names the
        // file after the page's title (`dm_core::pagemedia`), so neither the address's host
        // ("vt.tiktok.com") nor the page's own name is offered as the file's.
        if found.page && !self.youtube() {
            found.name = None;
            self.name.update(cx, |n, cx| {
                n.set_placeholder(t("Named after the page's title"), window, cx)
            });
        }
        if let Some(name) = found.name.clone() {
            if self.name.read(cx).value().is_empty() {
                self.name.update(cx, |n, cx| n.set_value(name, window, cx));
            }
        }
        self.probed = Some(found);
        self.offer_folder(window, cx);
        self.ask_choices(window, cx);
        cx.notify();
    }

    /// A video's qualities, each with its size, read once (its own data,
    /// under a second for YouTube); another site's page that lists several
    /// videos is its videos, listed in the Playlist window instead.
    fn ask_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.choices_asked || !self.video() {
            return;
        }
        self.choices_asked = true;
        let url = self.first();
        let (tell, told) = async_channel::bounded(1);
        dm_shell::app::lookups::list_choices_later(
            &self.backend,
            url.clone(),
            self.handed.page(),
            self.handed.sign_in(&self.settings, &url),
            self.youtube(),
            move |listed, several| {
                let _ = tell.try_send((listed, several));
            },
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok((listed, several)) = told.recv().await {
                let _ = this.update_in(cx, |this, window, cx| {
                    if let Some(playlist) = several {
                        this.becomes_playlist(playlist, window, cx);
                        return;
                    }
                    this.choices = Some(listed);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// A page that turned out to list several videos: its videos listed
    /// to be ticked, as a YouTube playlist's are, rather than one download
    /// refused.
    fn becomes_playlist(
        &mut self,
        playlist: dm_core::youtube::Playlist,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.done {
            return;
        }
        self.done = true;
        let url = self.first();
        let view = self.view.clone();
        crate::dialogs::playlist::open(view.clone(), url, Some(playlist), cx);
        let _ = view.update(cx, |view, _| view.handed_closed());
        window.remove_window();
    }

    /// Where the name is taken already, as the dialog stands.
    fn clash(&self, cx: &App) -> Option<Clash> {
        let [url] = self.urls.as_slice() else {
            return None;
        };
        let view = self.view.upgrade()?;
        let rows = &view.read(cx).rows;
        file_info::clash(
            &self.settings,
            rows,
            &self.folder.read(cx).value(),
            url,
            &self.name.read(cx).value(),
            self.early,
        )
    }

    /// Where the name is taken, as a frame draws it: what was last worked
    /// out, worked out again off the window's thread where the folder or
    /// the name typed changed or a second has gone by. Each look went over
    /// every row and asked the disk for the file and its free names, at
    /// every frame (gaps row 180). Start now and Add for later still ask
    /// `clash` itself, at the moment they are pressed.
    fn clash_shown(&mut self, cx: &mut Context<Self>) -> Option<Clash> {
        let folder = self.folder.read(cx).value().to_string();
        let name = self.name.read(cx).value().to_string();
        let current = self
            .clash_seen
            .as_ref()
            .filter(|(f, n, _, _)| *f == folder && *n == name);
        let fresh = current.is_some_and(|(_, _, _, at)| at.elapsed() < CLASH_EVERY);
        let shown = current
            .or(self.clash_seen.as_ref())
            .and_then(|(_, _, c, _)| c.clone());
        if fresh || self.clash_looking {
            return shown;
        }
        let ([url], Some(view)) = (self.urls.as_slice(), self.view.upgrade()) else {
            return None;
        };
        self.clash_looking = true;
        let rows = view.read(cx).rows.clone();
        let (settings, url, early) = (self.settings.clone(), url.clone(), self.early);
        let looked = cx.background_spawn(async move {
            let clash = file_info::clash(&settings, &rows, &folder, &url, &name, early);
            (folder, name, clash)
        });
        cx.spawn(async move |this, cx| {
            let (folder, name, clash) = looked.await;
            let _ = this.update(cx, |this, cx| {
                this.clash_looking = false;
                this.clash_seen = Some((folder, name, clash, std::time::Instant::now()));
                // Drawn again: with what was found, or to look again where
                // the folder or the name was typed into meanwhile.
                cx.notify();
            });
        })
        .detach();
        shown
    }

    /// Start now (`later` false) or Add for later.
    fn add(&mut self, later: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        // The part's ends, where one was asked for; a pair that is not a
        // span keeps the dialog, with the shape they take.
        let span = match file_info::span_of(
            self.part,
            &self.part_from.read(cx).value(),
            &self.part_to.read(cx).value(),
        ) {
            Ok(span) => span,
            Err(words) => {
                self.says = Some(words.into());
                cx.notify();
                return;
            }
        };
        let clash = self.clash(cx);
        let name = self.name.read(cx).value().to_string();
        let server_name = self.server_name().map(str::to_string);
        let form = Form {
            referrer: self.handed.referrer.clone(),
            title: self.handed.title.clone(),
            quality: self.quality.clone(),
            sound: self.handed.sound.clone(),
            span,
            shape: self.shape,
            gif: self.gif,
            folder: self.folder.read(cx).value().to_string(),
            name_typed: !name.trim().is_empty() && server_name.as_deref() != Some(name.trim()),
            name: name.clone(),
            description: self.note.read(cx).value().to_string(),
            queue: self.queue.clone(),
            delete_after: self.delete_after,
            unpack: self.unpack,
            cookies: self.handed.cookies.clone(),
            cookies_once: self.handed.cookies_once,
            headers: self.handed.headers.clone(),
            post: self.handed.post.clone(),
            replace: self.replace && matches!(clash, Some(Clash::OnDisk { .. })),
            keep_no_record: self.keep_no_record,
        };
        let with = file_info::options(&form, later, server_name.as_deref(), clash.as_ref());
        // Where this kind of file was put, for next time; the kind's own
        // folder chosen again forgets it.
        if self.urls.len() == 1 && !name.trim().is_empty() {
            let kind = file_info::kind_key(name.trim());
            let folder = Some(form.folder.trim().to_string()).filter(|f| !f.is_empty());
            if self.settings.last_folders.get(kind) != folder.as_ref() {
                if let Err(e) = self.backend.remember_folder(kind, folder.as_deref()) {
                    self.says = Some(e.into());
                }
            }
        }
        // "Always save here without asking": caught downloads go to their
        // folder unasked from now on.
        if self.handed.caught && self.always_here {
            if let Err(e) = self.backend.set_ask_caught_downloads(false) {
                self.says = Some(e.into());
            }
        }
        self.early_keeps_no_record();
        let added = match self.early {
            // Begun while it asked: placed where it was asked to go, not
            // added again.
            Some(id) => {
                // Unpack when done, for a row begun while it asked (gaps row W14).
                if with.unpack {
                    self.backend.set_unpack(id, true);
                }
                self.backend.settle_early(
                    id,
                    with.save_dir.clone().map(std::path::PathBuf::from),
                    with.save_name.clone(),
                    &with.description,
                    &with.queue,
                    later,
                )
            }
            None => {
                let urls = self.urls.clone();
                // The size it learned, kept for the row until the row knows its own: a row
                // added for later shows it.
                let size = (urls.len() == 1)
                    .then(|| self.probed.as_ref().and_then(|p| p.size))
                    .flatten();
                self.view
                    .update(cx, |view, cx| {
                        view.add(urls, with, cx)?;
                        if let (Some(size), [id]) = (
                            size,
                            view.list
                                .selected
                                .iter()
                                .copied()
                                .collect::<Vec<_>>()
                                .as_slice(),
                        ) {
                            view.known.sizes_told.insert(*id, size);
                            view.read();
                        }
                        Ok(())
                    })
                    .unwrap_or_else(|_| Err(String::new()))
            }
        };
        match added {
            Ok(_) => {
                self.done = true;
                let urls = self.urls.clone();
                let handed = self.handed.handed;
                let early = self.early.is_some();
                let _ = self.view.update(cx, |view, cx| {
                    // Taken from the browser: the same click a moment later
                    // is not taken twice.
                    if handed {
                        for url in &urls {
                            view.session
                                .hands
                                .recent
                                .taken(url, std::time::Instant::now());
                        }
                        view.handed_closed();
                    }
                    // A row begun while it asked is placed now: the list
                    // read again (an added one was read as it was added).
                    if early {
                        view.read();
                    }
                    cx.notify();
                });
                window.remove_window();
            }
            Err(e) => {
                self.says = Some(e.into());
                cx.notify();
            }
        }
    }

    /// Cancel, Escape or the close button: what was fetched while it asked
    /// goes with it, and a handed-over download cancelled is a word against
    /// that site's downloads being handed over.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        if let Some(id) = self.early.take() {
            self.backend.remove_later(id);
        }
        let site = self
            .handed
            .handed
            .then(|| dm_core::host_of(&self.first()))
            .filter(|host| !host.is_empty());
        let handed = self.handed.handed;
        let _ = self.view.update(cx, |view, cx| {
            if handed {
                view.handed_closed();
            }
            if let Some(site) = site {
                view.hand_off_cancelled(&site, cx);
            }
            view.read();
            cx.notify();
        });
        window.remove_window();
    }

    /// Browse…: the system's own folder panel (`services::ask`, as every
    /// Browse… asks it); with none on the desktop, the field says to type
    /// the path.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.backend.platform.can_choose_folder() {
            crate::widgets::savefolder::no_chooser(&self.folder.clone(), window, cx);
            return;
        }
        crate::platform::services::ask_then_in(
            crate::platform::services::Asked::folder(),
            window,
            cx,
            |this: &mut Self, picked, window, cx| {
                let path = picked.display().to_string();
                this.folder
                    .update(cx, |f, cx| f.set_value(path, window, cx));
            },
        );
    }

    /// Take out: one file of the ZIP, alone, before the archive is
    /// fetched, into the folder the archive is going to.
    fn take_out(&mut self, entry: dm_core::ZipEntry, window: &mut Window, cx: &mut Context<Self>) {
        let first = self.first();
        let folder = file_info::folder_for(
            &self.settings,
            &self.folder.read(cx).value(),
            &first,
            self.name.read(cx).value().trim(),
        );
        self.taking = dm_core::sanitize_name(&entry.name);
        let (tell, told) = async_channel::bounded(1);
        self.taken = Some(dm_shell::app::files::take_from_zip(
            &self.backend,
            first.clone(),
            self.handed.page(),
            self.handed.sign_in(&self.settings, &first),
            entry,
            folder,
            move || {
                let _ = tell.try_send(());
            },
        ));
        cx.spawn_in(window, async move |this, cx| {
            if told.recv().await.is_ok() {
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
        cx.notify();
    }

    /// A choice from one of the pop-ups.
    fn pick(&mut self, pick: &Pick, window: &mut Window, cx: &mut Context<Self>) {
        match pick.field {
            "category" => {
                let folder = match pick.index {
                    0 => String::new(),
                    i => file_info::places(&self.settings)
                        .get(i - 1)
                        .map(|(_, path)| path.clone())
                        .unwrap_or_default(),
                };
                self.folder
                    .update(cx, |f, cx| f.set_value(folder, window, cx));
            }
            "delete-after" => {
                if let Some((_, seconds)) = file_info::delete_after_times().get(pick.index) {
                    self.delete_after = *seconds;
                }
            }
            "queue" => {
                if let Some(queue) = self.queues.get(pick.index) {
                    self.queue.clone_from(queue);
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

/// A segmented choice, as the system's segmented control: each choice a
/// segment, the chosen one in the accent colour.
fn segment(
    id: impl Into<gpui::ElementId>,
    words: SharedString,
    on: bool,
    theme: Theme,
    screen: &Screen,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .map(|c| crate::widgets::native::faced(c, theme.control, on, theme))
        .role(Role::RadioButton)
        .aria_label(words.clone())
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .h(px(native::CONTROL))
        .px(px(space::L))
        .flex()
        .items_center()
        .rounded(px(native::RADIUS))
        .child(screen.label(words))
}

impl Render for Details {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let look = Look::new(theme, self.screen.clone());
        let first = self.first();
        let single = self.urls.len() == 1;
        let folder = self.folder.read(cx).value().trim().to_string();
        let clash = self.clash_shown(cx);
        let page = self.probed.as_ref().is_some_and(|p| p.page);
        let problem = self.probed.as_ref().and_then(|p| p.problem.clone());
        let video = self.video();
        let youtube = self.youtube();
        let size = self.size_said(page, cx);
        let quality_row = self.quality_row(&look, video, cx);
        let contents_row = self.contents_row(&look, cx);
        let part_rows = self.part_rows(&look, &first, youtube, cx);

        // Figures of one width each (gaps row E33).
        let body = self.address(
            &look,
            &first,
            native::body(theme).font_features(native::figures()),
        );
        let body = self
            .place(&look, &folder, single, window, cx, body)
            .children(quality_row)
            .children(contents_row)
            .children(part_rows.into_iter().flatten());
        // A note stands under what it is about: the link's problem under its size, a name
        // already taken after the name's fields, before the switches below them.
        let body = self.facts(&look, single, video, size, problem, body);
        let body = self
            .warnings(&look, clash, cx, body)
            .child(self.no_record_row(&look, cx));
        let body = self.unpack_box(&look, cx, body);
        let body = self.says_row(&look, body.child(self.queue_row(&look)));
        let body = body.child(self.buttons(&look, cx));

        let root =
            crate::widgets::native::dialog("download-details", t("Download details"), theme, cx)
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, pick: &Pick, window, cx| this.pick(pick, window, cx)))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter"
                        && crate::widgets::native::return_is_default(window, Some(&this.focus), cx)
                    {
                        this.add(false, window, cx);
                    }
                }))
                .child(crate::widgets::fit::fitted(body));
        crate::window::safe::kept_clear(root, window)
    }
}

impl crate::widgets::native::CloseButton for Details {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(window, cx);
    }
}
