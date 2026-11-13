// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details, a window of its own (B02), with the egui window's
//! words: where the file goes (its category, its folder, chosen in the
//! system's own folder panel), its name (the server's, once asked), a
//! video's qualities and a part of it, a ZIP's files (one taken out alone),
//! its size, a note, when it is deleted, and Start now, Add for later to a
//! queue, or Cancel. Opened for an address typed in Add URL, or for a
//! download the browser handed over, with what the browser sent (the page,
//! the sign-in, the quality picked in its panel, the browser's name). What
//! it offers and adds is dm's (`dm_desktop_base::file_info`).

use dm_core::Id;
use dm_desktop_base::file_info::{self, Clash, Form};
use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::zip_list::{self, Listing, TakeSlot, ZipSlot};
use dm_shell::app::backend::Backend;
use dm_shell::app::lookups::Probed;
use dm_shell::Settings;
use gpui::{
    div, prelude::*, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent,
    PathPromptOptions, Role, SharedString, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::popup::{self, Pick};
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

/// What a browser's hand-off brought with the address; nothing for one
/// typed in Add URL.
#[derive(Debug, Clone, Default)]
pub struct Handed {
    /// Handed over by the browser, rather than typed in: cancelling it is a
    /// word against that site's downloads being handed over.
    pub handed: bool,
    /// The page it came from, sent as Referer.
    pub referrer: String,
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
}

impl Handed {
    /// The sign-in for `url`, from what the browser sent.
    fn sign_in(&self, url: &str) -> Option<dm_core::cookies::SignIn> {
        dm_core::cookies::SignIn::with_headers(url, &self.cookies, &self.headers)
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
    replace: bool,
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
    fn probed(&mut self, found: Probed, window: &mut Window, cx: &mut Context<Self>) {
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
            self.handed.sign_in(&url),
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
        crate::playlist::open(view.clone(), url, Some(playlist), cx);
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
            cookies: self.handed.cookies.clone(),
            cookies_once: self.handed.cookies_once,
            headers: self.handed.headers.clone(),
            post: self.handed.post.clone(),
            replace: self.replace && matches!(clash, Some(Clash::OnDisk { .. })),
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
        let added = match self.early {
            // Begun while it asked: placed where it was asked to go, not
            // added again.
            Some(id) => self.backend.settle_early(
                id,
                with.save_dir.clone().map(std::path::PathBuf::from),
                with.save_name.clone(),
                &with.description,
                &with.queue,
                later,
            ),
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
                            view.selected.iter().copied().collect::<Vec<_>>().as_slice(),
                        ) {
                            view.sizes_told.insert(*id, size);
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

    /// Browse…: the system's own folder panel.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                if let Some(path) = paths.first() {
                    let path = path.display().to_string();
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.folder
                            .update(cx, |f, cx| f.set_value(path, window, cx));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
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
            self.handed.sign_in(&first),
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
        .role(Role::RadioButton)
        .aria_label(words.clone())
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .h(px(native::CONTROL))
        .px(px(12.0))
        .flex()
        .items_center()
        .rounded(px(native::RADIUS))
        .when(on, |d| d.bg(theme.accent).text_color(theme.on_accent))
        .when(!on, |d| d.bg(theme.control))
        .child(screen.label(words))
}

impl Render for Details {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let first = self.first();
        let single = self.urls.len() == 1;
        let folder = self.folder.read(cx).value().trim().to_string();
        let clash = self.clash_shown(cx);
        let page = self.probed.as_ref().is_some_and(|p| p.page);
        let problem = self.probed.as_ref().and_then(|p| p.problem.clone());
        let video = self.video();
        let youtube = self.youtube();

        let places = file_info::places(&self.settings);
        let as_type = SharedString::from(t("As its type says"));
        let mut categories = vec![as_type.clone()];
        categories.extend(
            places
                .iter()
                .map(|(name, _)| SharedString::from(name.clone())),
        );
        let category_at = if folder.is_empty() {
            Some(0)
        } else {
            places.iter().position(|(_, p)| *p == folder).map(|i| i + 1)
        };
        let category_words = category_at
            .and_then(|i| categories.get(i).cloned())
            .unwrap_or(as_type);

        let times = file_info::delete_after_times();
        let time_at = times
            .iter()
            .position(|(_, s)| *s == self.delete_after)
            .unwrap_or(0);
        let queues: Vec<SharedString> = self
            .queues
            .iter()
            .map(|q| dm_desktop_base::i18n::name(q).into())
            .collect();
        let queue_at = self.queues.iter().position(|q| *q == self.queue);
        let queue_words = SharedString::from(dm_desktop_base::i18n::name(&self.queue));

        // "Checking…" while the server is asked, then the size beside what the folder has free
        // (in the warning colour where it will not fit), or that it is unknown (gaps row U17).
        let folder_now = self.folder.read(cx).value().trim().to_string();
        if self
            .free
            .as_ref()
            .is_none_or(|(asked, _)| *asked != folder_now)
        {
            let dir = if folder_now.is_empty() {
                self.settings.download_dir.clone()
            } else {
                dm_shell::settings::full_path(&folder_now)
                    .unwrap_or_else(|| std::path::PathBuf::from(&folder_now))
            };
            // The folder may not be made yet: what holds it is asked.
            let there = dir
                .ancestors()
                .find(|d| d.is_dir())
                .map(std::path::Path::to_path_buf)
                .unwrap_or(dir);
            self.free = Some((folder_now, dm_core::free_space(&there)));
        }
        let free = self.free.as_ref().and_then(|(_, free)| *free);
        let size = self.probed.as_ref().and_then(|p| p.size);
        let will_not_fit = matches!((size, free), (Some(size), Some(free)) if size > free);
        let size_words: SharedString = match (&self.probed, size) {
            (None, _) if !page => t("Checking…").into(),
            (_, Some(size)) => match free {
                Some(free) => tf(
                    "{} · {} free",
                    &[&format::bytes(size), &format::bytes(free)],
                )
                .into(),
                None => format::bytes(size).into(),
            },
            _ if page => t("Known once the video starts").into(),
            _ => t("unknown").into(),
        };
        // A form as macOS lays one out: the labels right-aligned in a
        // column of their own, every control (and what is said of one) in a
        // second.
        let width = native::label_width(&[
            t("Category"),
            t("Save to folder"),
            t("Save as"),
            t("Quality"),
            t("Contents"),
            t("Size"),
            t("Note"),
            t("Delete file after"),
        ]);
        let row = |label: &'static str, control: gpui::AnyElement| {
            native::row(Some(t(label)), width, &screen, control)
        };
        let under = |control: gpui::AnyElement| native::row(None, width, &screen, control);

        // A video's qualities: each its class and container over its size,
        // the one the setting would take picked until another is.
        let quality_row = video.then(|| {
            let control = match &self.choices {
                None => div()
                    .text_color(theme.muted)
                    .child(screen.label(t("Reading the video's qualities…")))
                    .into_any_element(),
                Some(list) if list.is_empty() => div()
                    .text_color(theme.muted)
                    .child(screen.label("—"))
                    .into_any_element(),
                Some(list) => {
                    let setting = dm_core::Quality::named(&self.settings.stream_quality);
                    let picked = self
                        .quality
                        .clone()
                        .or_else(|| file_info::default_choice(list, setting));
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .children(list.iter().enumerate().map(|(i, choice)| {
                            let on = picked.as_deref() == Some(choice.quality.as_str());
                            let words = file_info::chip_words(choice);
                            let (top, bottom) = words
                                .split_once('\n')
                                .map(|(a, b)| (a.to_string(), Some(b.to_string())))
                                .unwrap_or((words.clone(), None));
                            let picked_quality = choice.quality.clone();
                            let picked_label = choice.label.clone();
                            div()
                                .id(("quality", i))
                                .role(Role::RadioButton)
                                .aria_label(words.replace('\n', " "))
                                .px(px(10.0))
                                .py(px(4.0))
                                .min_w(px(84.0))
                                .flex()
                                .flex_col()
                                .items_center()
                                .rounded(px(native::RADIUS))
                                .when(on, |d| d.bg(theme.accent).text_color(theme.on_accent))
                                .when(!on, |d| d.bg(theme.control))
                                .child(screen.label(top))
                                .when_some(bottom, |d, b| {
                                    d.child(
                                        div()
                                            .text_size(px(native::SMALL_TEXT))
                                            .when(!on, |d| d.text_color(theme.muted))
                                            .child(screen.label(b)),
                                    )
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.quality = Some(picked_quality.clone());
                                    this.label = Some(picked_label.clone());
                                    cx.notify();
                                }))
                        }))
                        .into_any_element()
                }
            };
            row("Quality", control)
        });

        // A ZIP's files, by name and size: text only, never opened.
        let contents_row = match zip_list::listing(&self.zip) {
            Listing::Nothing => None,
            Listing::Reading => Some(row(
                "Contents",
                div()
                    .text_color(theme.muted)
                    .child(screen.label(zip_list::reading_words()))
                    .into_any_element(),
            )),
            Listing::Files(files, more) => {
                let taken = zip_list::taken_words(&self.taken, &self.taking);
                Some(row(
                    "Contents",
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .id("zip-contents")
                                .max_h(px(140.0))
                                .overflow_y_scroll()
                                .rounded(px(native::RADIUS))
                                .border_1()
                                .border_color(theme.separator)
                                .bg(theme.background)
                                .p(px(4.0))
                                .children(files.into_iter().enumerate().map(|(i, file)| {
                                    let takeable =
                                        !file.is_folder() && file.packed <= dm_core::ZIP_MOST_TAKEN;
                                    let size =
                                        (!file.is_folder()).then(|| format::bytes(file.size));
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .h(px(20.0))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis()
                                                .child(screen.label(file.name.clone())),
                                        )
                                        .when_some(size, |d, size| {
                                            d.child(
                                                div()
                                                    .text_color(theme.muted)
                                                    .child(screen.label(size)),
                                            )
                                        })
                                        .when(takeable, |d| {
                                            d.child(
                                                div()
                                                    .id(("take", i))
                                                    .role(Role::Button)
                                                    .aria_label(t("Take out"))
                                                    .text_color(theme.accent)
                                                    .child(screen.label(t("Take out")))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.take_out(file.clone(), window, cx)
                                                        },
                                                    )),
                                            )
                                        })
                                }))
                                .when(more > 0, |d| {
                                    d.child(native::note(
                                        tf("and {} more", &[&more.to_string()]),
                                        theme.muted,
                                        &screen,
                                    ))
                                }),
                        )
                        .when_some(taken, |d, words| {
                            d.child(native::note(words, theme.muted, &screen))
                        })
                        .into_any_element(),
                ))
            }
        };

        // A part of the video, from one moment to another: cut from the
        // streams dm fetches itself, which today are a YouTube video's.
        let part_rows = (youtube || self.handed.handed || self.part).then(|| {
            let starts = dm_core::youtube::start_of(&first);
            let part = self.part;
            let gif_possible = dm_core::gif::possible_for(&first);
            let mut rows = vec![row(
                "Part",
                div()
                    .flex()
                    // The offer of a link's moment onto a second line where the line is full:
                    // Windows' wider letters pushed "From 0:34" past the dialog's edge, out of
                    // sight and reach. Not the part's fields, which keep each to its words.
                    .when(!self.part, |d| d.flex_wrap())
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(
                        native::checkbox(
                            "part",
                            t("Only a part of the video").into(),
                            part,
                            theme,
                            &screen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.part = !this.part;
                            cx.notify();
                        })),
                    )
                    // A link copied at a moment of the video: that moment
                    // offered as the part's start.
                    .when_some(starts.filter(|_| !part), |d, at| {
                        let at = file_info::clock(at);
                        let from = at.clone();
                        d.child(native::note(
                            tf("This link starts at {}.", &[&at]),
                            theme.muted,
                            &screen,
                        ))
                        .child(
                            native::button(
                                "from-here",
                                tf("From {}", &[&at]).into(),
                                false,
                                theme,
                                &screen,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.part = true;
                                    let from = from.clone();
                                    this.part_from
                                        .update(cx, |f, cx| f.set_value(from, window, cx));
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    .when(part, |d| {
                        d.child(
                            screen.label(t("part:from").trim_start_matches("part:").to_string()),
                        )
                        .child(div().w(px(64.0)).child(native::field(
                            &self.part_from,
                            "part:from",
                            theme,
                        )))
                        .child(screen.label(t("part:to").trim_start_matches("part:").to_string()))
                        .child(div().w(px(64.0)).child(native::field(
                            &self.part_to,
                            "part:to",
                            theme,
                        )))
                    })
                    .into_any_element(),
            )];
            if part {
                // The part's shape: as it is, or the middle of the picture
                // made vertical or square, nothing re-encoded.
                use dm_core::shape::Shape;
                let shape = self.shape;
                rows.push(row(
                    "Shape",
                    div()
                        .flex()
                        .gap(px(4.0))
                        .children(
                            [
                                (None, t("As it is")),
                                (Some(Shape::Vertical), t("Vertical 9:16")),
                                (Some(Shape::Square), t("Square 1:1")),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(i, (choice, words))| {
                                segment(("shape", i), words.into(), shape == choice, theme, &screen)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.shape = choice;
                                        cx.notify();
                                    }))
                            }),
                        )
                        .into_any_element(),
                ));
                // As a GIF: moving pictures with no sound.
                if gif_possible {
                    let gif = self.gif;
                    rows.push(row(
                        "Part as",
                        div()
                            .flex()
                            .gap(px(4.0))
                            .child(
                                segment("as-video", t("Video").into(), !gif, theme, &screen)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.gif = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                segment("as-gif", "GIF".into(), gif, theme, &screen).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.gif = true;
                                        cx.notify();
                                    }),
                                ),
                            )
                            .into_any_element(),
                    ));
                }
            }
            rows
        });

        crate::native::dialog("download-details", t("Download details"), theme, cx)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, pick: &Pick, window, cx| this.pick(pick, window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" {
                    this.add(false, window, cx);
                }
            }))
            .child(crate::fit::fitted(
                native::body(theme)
                    // The address, as the window's subtitle says it.
                    .child(
                        div()
                            .text_color(theme.muted)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(screen.label(format::shown_url(&first))),
                    )
                    // Where a handed-over download came from, as the host
                    // saw it.
                    .when_some(
                        self.handed.browser.clone().filter(|_| self.handed.handed),
                        |d, browser| {
                            d.child(
                                div()
                                    .text_color(theme.muted)
                                    .child(screen.label(tf("From {}", &[&browser]))),
                            )
                        },
                    )
                    .when(self.urls.len() > 1, |d| {
                        d.child(
                            div()
                                .text_color(theme.muted)
                                .child(screen.label(tf("and {} more", &[&(self.urls.len() - 1)]))),
                        )
                    })
                    .child(div().h(px(1.0)).my(px(4.0)).bg(theme.separator))
                    .child(row(
                        "Category",
                        popup::button(
                            "category",
                            category_words,
                            categories,
                            category_at,
                            theme,
                            &screen,
                        )
                        .into_any_element(),
                    ))
                    .child(row(
                        "Save to folder",
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(native::LABEL_GAP))
                            .child(div().flex_1().min_w_0().child(native::path_field(
                                &self.folder,
                                "Save to folder",
                                gpui::Focusable::focus_handle(self.folder.read(cx), cx).is_focused(window),
                                theme,
                                cx,
                            )))
                            .child(
                                native::button(
                                    "browse",
                                    t("Browse…").into(),
                                    false,
                                    theme,
                                    &screen,
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.browse(window, cx)),
                                ),
                            )
                            .into_any_element(),
                    ))
                    // A download the browser caught: the next ones can go
                    // straight to their folder.
                    .when(self.handed.caught, |d| {
                        let on = self.always_here;
                        d.child(under(
                            native::checkbox(
                                "always-here",
                                t("Always save here without asking").into(),
                                on,
                                theme,
                                &screen,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.always_here = !this.always_here;
                                cx.notify();
                            }))
                            .into_any_element(),
                        ))
                    })
                    .when(single, |d| {
                        d.child(row(
                            "Save as",
                            div()
                                .flex_1()
                                .child(native::field(&self.name, "Save as", theme))
                                .into_any_element(),
                        ))
                    })
                    .children(quality_row)
                    .children(contents_row)
                    .children(part_rows.into_iter().flatten())
                    .when(single, |d| {
                        d.child(row(
                            "Size",
                            div()
                                .text_color(if will_not_fit {
                                    theme.warning
                                } else {
                                    theme.muted
                                })
                                .child(screen.label(size_words))
                                .into_any_element(),
                        ))
                    })
                    .child(row(
                        "Note",
                        div()
                            .flex_1()
                            .child(native::field(&self.note, "Note", theme))
                            .into_any_element(),
                    ))
                    .child(row(
                        "Delete file after",
                        popup::button(
                            "delete-after",
                            times[time_at].0.into(),
                            times.iter().map(|(w, _)| SharedString::from(*w)).collect(),
                            Some(time_at),
                            theme,
                            &screen,
                        )
                        .into_any_element(),
                    ))
                    // The quality picked in the browser's panel, where the
                    // dialog lists none of its own.
                    .when_some(self.label.clone().filter(|_| !video), |d, label| {
                        d.child(row(
                            "Quality",
                            div().child(screen.label(label)).into_any_element(),
                        ))
                    })
                    .when_some(clash.clone(), |d, clash| match clash {
                        Clash::Listed => d.child(under(
                            native::note(
                                t("A download with this name exists. This one will be numbered."),
                                theme.warning,
                                &screen,
                            )
                            .into_any_element(),
                        )),
                        Clash::OnDisk { free, .. } => {
                            let replace = self.replace;
                            d.child(under(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .child(native::note(
                                        t("A file of that name is in the folder already."),
                                        theme.warning,
                                        &screen,
                                    ))
                                    .child(
                                        native::radio(
                                            "keep-both",
                                            tf("Save this one as {}", &[&free]).into(),
                                            !replace,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.replace = false;
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(
                                        native::radio(
                                            "replace",
                                            t("Replace that file once this one is whole").into(),
                                            replace,
                                            theme,
                                            &screen,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.replace = true;
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .into_any_element(),
                            ))
                        }
                    })
                    .when_some(problem, |d, why| {
                        d.child(under(
                            native::note(file_info::problem_words(&why), theme.warning, &screen)
                                .into_any_element(),
                        ))
                    })
                    .when_some(self.says.clone(), |d, says| {
                        d.child(under(
                            native::note(says, theme.warning, &screen).into_any_element(),
                        ))
                    })
                    // As on a Mac: Add for later, to a queue, at the leading
                    // side; Cancel, then Start now (the default) at the end.
                    .child(native::buttons(
                        vec![
                            native::button(
                                "later",
                                t("Add for later").into(),
                                false,
                                theme,
                                &screen,
                            )
                            .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx)))
                            .into_any_element(),
                            // "to" by itself is many words in other
                            // languages: the key says which one.
                            screen
                                .label(t("queue:to").trim_start_matches("queue:").to_string())
                                .into_any_element(),
                            popup::button("queue", queue_words, queues, queue_at, theme, &screen)
                                .into_any_element(),
                        ],
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.cancel(window, cx)),
                                )
                                .into_any_element(),
                            native::button("start", t("Start now").into(), true, theme, &screen)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add(false, window, cx)),
                                )
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens Download details for `urls` typed in Add URL (several: a batch,
/// in one).
pub fn open(view: WeakEntity<Downloads>, urls: Vec<String>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, urls, Handed::default(), None, cx));
}

/// Opens Download details for a download the browser handed over, with
/// what the browser sent: the page, the sign-in, the quality picked in its
/// panel and its "Part" line, the browser's name.
pub fn open_handed(view: WeakEntity<Downloads>, request: dm_shell::host::Request, cx: &mut App) {
    // The form for the address it was sent to, before that is moved below.
    let post = dm_shell::shell::posted(&request.url, request.post.as_ref());
    let handed = Handed {
        handed: true,
        referrer: request.referrer,
        quality: request.quality,
        sound: request.sound,
        label: request.label,
        cookies: request.cookies,
        cookies_once: request.cookies_once,
        headers: request.headers,
        post,
        browser: request.browser,
        caught: request.caught,
    };
    // The panel's "Part" line: the dialog opens with the part on.
    let part = (!(request.from.is_empty() && request.to.is_empty())).then(|| {
        let from = if request.from.is_empty() && !request.to.is_empty() {
            "0:00".to_string()
        } else {
            request.from
        };
        (from, request.to, request.save_name.unwrap_or_default())
    });
    let urls = vec![request.url];
    cx.defer(move |cx| open_now(view, urls, handed, part, cx));
}

fn open_now(
    view: WeakEntity<Downloads>,
    urls: Vec<String>,
    handed: Handed,
    part: Option<(String, String, String)>,
    cx: &mut App,
) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let settings = backend.settings();
    let queues = backend.queues();
    let first = urls.first().cloned().unwrap_or_default();
    // Begun at once while it asks, where Options says so, before the window
    // is made: its first request then does not wait for a new window (its
    // Direct3D or Metal surface, its fields) to be drawn.
    let form = Form {
        quality: handed.quality.clone(),
        post: handed.post.clone(),
        ..Form::default()
    };
    let youtube = urls.len() == 1 && dm_core::youtube::video_id(&first).is_some();
    let early = file_info::begins_early(&settings, &urls, &form, youtube)
        .then(|| {
            backend
                .add_with(
                    &first,
                    dm_core::AddOptions {
                        referrer: handed.referrer.clone(),
                        cookies: handed.cookies.clone(),
                        cookies_once: handed.cookies_once,
                        headers: handed.headers.clone(),
                        ..dm_core::AddOptions::default()
                    },
                )
                .ok()
        })
        .flatten();
    // What a window that will not open leaves to undo.
    let (gone_with, gone_view, was_handed) = (backend.clone(), view.clone(), handed.handed);
    // What the server calls the file and how big it is, asked now too,
    // before the window is made; the dialog hears the answer once it is.
    let told = ask_server(&backend, &first, urls.len(), settings.remux_ts, &handed);
    let height = if urls.len() == 1 { 360.0 } else { 300.0 };
    let bounds = Bounds::centered(None, size(px(600.0), px(height)), cx);
    let opened = crate::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Download details").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let hint = file_info::default_folder_words(&settings);
            let remux = settings.remux_ts;
            let titled = handed.quality.is_some();
            let folder = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
            let name = cx.new(|cx| {
                InputState::new(window, cx).placeholder(file_info::name_hint(&first, titled, remux))
            });
            let note = cx.new(|cx| InputState::new(window, cx));
            let part_from = cx.new(|cx| InputState::new(window, cx).placeholder(t("m:ss")));
            let part_to = cx.new(|cx| InputState::new(window, cx).placeholder(t("m:ss")));
            let mut part_on = false;
            if let Some((from, to, save_name)) = &part {
                part_on = true;
                part_from.update(cx, |f, cx| f.set_value(from.clone(), window, cx));
                part_to.update(cx, |f, cx| f.set_value(to.clone(), window, cx));
                if !save_name.is_empty() {
                    name.update(cx, |n, cx| n.set_value(save_name.clone(), window, cx));
                }
            }
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            // A ZIP's files, read once from the end of the archive.
            let web = first.starts_with("http://") || first.starts_with("https://");
            let (zip_tell, zip_told) = async_channel::bounded::<()>(1);
            let zip = (urls.len() == 1 && web && zip_list::is_zip_address(&first)).then(|| {
                dm_shell::app::files::list_zip_at(
                    &backend,
                    first.clone(),
                    handed.page(),
                    handed.sign_in(&first),
                    move || {
                        let _ = zip_tell.try_send(());
                    },
                )
            });
            let details = cx.new(|cx| {
                let focus = cx.focus_handle();
                // Return in a field is Start now, as in the egui window;
                // what is typed is weighed again (the name's clash).
                for input in [&folder, &name, &note, &part_from, &part_to] {
                    cx.subscribe_in(input, window, |this: &mut Details, _, event, window, cx| {
                        match event {
                            InputEvent::PressEnter { .. } => this.add(false, window, cx),
                            InputEvent::Change => {
                                this.offer_folder(window, cx);
                                cx.notify();
                            }
                            _ => {}
                        }
                    })
                    .detach();
                }
                Details {
                    view: view.clone(),
                    backend: backend.clone(),
                    settings: settings.clone(),
                    urls: urls.clone(),
                    quality: handed.quality.clone(),
                    label: handed.label.clone(),
                    handed: handed.clone(),
                    folder: folder.clone(),
                    name: name.clone(),
                    note,
                    part_from,
                    part_to,
                    probed: None,
                    free: None,
                    folder_offered: false,
                    always_here: false,
                    queue: dm_core::MAIN_QUEUE.into(),
                    queues,
                    delete_after: None,
                    replace: false,
                    choices: None,
                    choices_asked: false,
                    part: part_on,
                    shape: None,
                    gif: false,
                    zip,
                    taken: None,
                    taking: String::new(),
                    early,
                    clash_seen: None,
                    clash_looking: false,
                    says: None,
                    done: false,
                    focus,
                    screen,
                }
            });
            // Start now has the keyboard, as in the egui window.
            details.update(cx, |d, cx| window.focus(&d.focus, cx));
            hear_server(&details, told, window, cx);
            // A YouTube video's qualities are asked at once, from its own
            // data; a page's once the server says it is one.
            details.update(cx, |d, cx| d.ask_choices(window, cx));
            let weak = details.downgrade();
            window
                .spawn(cx, async move |cx| {
                    while zip_told.recv().await.is_ok() {
                        if weak.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                })
                .detach();
            if early.is_some() {
                let _ = view.update(cx, |v, cx| {
                    v.read();
                    cx.notify();
                });
            }
            crate::native::close_is_cancel(&details, window, cx);
            cx.new(|cx| gpui_component::Root::new(details, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: Download details could not be opened: {e}");
        // As Cancel ends it: the row begun for it goes, and the next
        // hand-off is not held back behind one that is never asked.
        if let Some(id) = early {
            gone_with.remove_later(id);
        }
        let _ = gone_view.update(cx, |view, cx| {
            if was_handed {
                view.handed_closed();
            }
            view.read();
            cx.notify();
        });
    }
}

/// The server asked what it calls the file and how big it is, once, as the
/// dialog opens (not a torrent's: what the server says of a .torrent is the
/// metainfo's few kilobytes and its file name). Asked before the dialog's
/// window is made, so the request is not held up by it; `None` where there
/// is nothing to ask, which is known at once.
fn ask_server(
    backend: &Backend,
    url: &str,
    count: usize,
    remux: bool,
    handed: &Handed,
) -> Option<async_channel::Receiver<Probed>> {
    let web = url.starts_with("http://") || url.starts_with("https://");
    if count != 1 || !web || dm_core::is_torrent(url) {
        return None;
    }
    let (tell, told) = async_channel::bounded(1);
    dm_shell::app::lookups::probe_later(
        backend,
        url.to_string(),
        handed.referrer.clone(),
        handed.sign_in(url),
        remux,
        move |found| {
            let _ = tell.try_send(found);
        },
    );
    Some(told)
}

/// The dialog told what the server answered (`ask_server`), when it does;
/// at once where nothing was asked.
fn hear_server(
    details: &Entity<Details>,
    told: Option<async_channel::Receiver<Probed>>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(told) = told else {
        details.update(cx, |d, cx| d.probed(Probed::default(), window, cx));
        return;
    };
    let details = details.downgrade();
    window
        .spawn(cx, async move |cx| {
            if let Ok(found) = told.recv().await {
                let _ = details.update_in(cx, |d, window, cx| d.probed(found, window, cx));
            }
        })
        .detach();
}

impl crate::native::CloseButton for Details {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(window, cx);
    }
}
