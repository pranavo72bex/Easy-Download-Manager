// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Download links", a window of its own: a page's links the browser handed
//! over, each with its name, kind, size, folder and words on the page as
//! its server and the page tell them; filtered, ticked, given a folder, and
//! added now or for later. What it lists and adds is EDM's
//! (`edm_desktop_base::links_import`).

use edm_desktop_base::format;
use edm_desktop_base::i18n::{t, tf};
use edm_desktop_base::links_import::{added_words, LinksImport};
use gpui::{
    div, prelude::*, size, uniform_list, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent,
    SharedString, UniformListScrollHandle, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::widgets::popup::{self, Pick};
use crate::widgets::savefolder::{self, Saves};
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod sections;

use sections::Row;

/// A link's row, in points.
const ROW: f32 = 24.0;
/// The rows shown before the list scrolls.
const SHOWN: usize = 12;

/// The window's view.
pub struct Links {
    view: WeakEntity<Downloads>,
    d: LinksImport,
    types: Entity<InputState>,
    folder: Entity<InputState>,
    /// The row being edited: its name and address fields.
    editing: Option<(usize, Entity<InputState>, Entity<InputState>)>,
    queues: Vec<String>,
    says: Option<SharedString>,
    done: bool,
    focus: FocusHandle,
    screen: Screen,
    scroll: UniformListScrollHandle,
}

impl Links {
    /// Start now (`later` false) or Add for later.
    fn add(&mut self, later: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.d.types = self.types.read(cx).value().to_string();
        self.chosen(cx);
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let settings = view.read(cx).backend.settings();
        // The folder they all go to made first, if it is not there yet; one
        // that cannot be is said here, the dialog left open.
        if let Err(words) = self
            .d
            .save
            .ready(&settings.folders.download_dir, &settings.categories())
        {
            self.says = Some(words.into());
            cx.notify();
            return;
        }
        let (items, sizes) = match self.d.adds(
            later,
            &settings.folders.download_dir,
            &settings.categories(),
        ) {
            Ok(adds) => adds,
            Err(words) => {
                self.says = Some(words.into());
                cx.notify();
                return;
            }
        };
        let queue = self.d.queue_or_main();
        self.done = true;
        let _ = self.view.update(cx, |view, cx| {
            match view.add_items_sized(items, &sizes, cx) {
                Ok(n) => view.say(added_words(n, later, &queue)),
                Err(e) => view.say(e),
            }
            view.handed_closed();
            cx.notify();
        });
        window.remove_window();
    }

    /// Cancel, Escape or the close button.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        let _ = self.view.update(cx, |view, _| view.handed_closed());
        window.remove_window();
    }

    /// Check all or Uncheck all, of what is listed.
    fn check_all(&mut self, on: bool, cx: &mut Context<Self>) {
        let listed = self.d.visible();
        for (l, shown) in self.d.items.iter_mut().zip(&listed) {
            if *shown {
                l.1 = on;
            }
        }
        cx.notify();
    }

    /// Edit: the row's name and address, in fields under the list.
    fn edit(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_editing(cx);
        let Some((url, _)) = self.d.items.get(n).cloned() else {
            return;
        };
        let name_now = self
            .d
            .names
            .get(&url)
            .cloned()
            .unwrap_or_else(|| self.d.name_of(&url));
        let name = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(name_now, window, cx);
            s
        });
        let address = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(url, window, cx);
            s
        });
        self.editing = Some((n, name, address));
        cx.notify();
    }

    /// Done: the edited name and address kept.
    fn finish_editing(&mut self, cx: &mut Context<Self>) {
        let Some((n, name, address)) = self.editing.take() else {
            return;
        };
        let Some((url, _)) = self.d.items.get(n).cloned() else {
            return;
        };
        let name = name.read(cx).value().to_string();
        let address = address.read(cx).value().trim().to_string();
        if !address.is_empty() && address != url {
            if let Some(given) = self.d.names.remove(&url) {
                self.d.names.insert(address.clone(), given);
            }
            self.d.items[n].0 = address.clone();
        }
        let key = if address.is_empty() { url } else { address };
        // Kept once it differs from the name it would have.
        if self.d.names.contains_key(&key) || name.trim() != self.d.name_of(&key) {
            self.d.names.insert(key, name);
        }
        cx.notify();
    }

    /// A choice from a pop-up.
    fn pick(&mut self, pick: &Pick, cx: &mut Context<Self>) {
        let categories = savefolder::categories(cx, &self.view);
        if !savefolder::pick(self, pick, &categories) && pick.field == "links-queue" {
            if let Some(queue) = self.queues.get(pick.index) {
                self.d.queue = queue.clone();
            }
        }
        cx.notify();
    }
}

impl Saves for Links {
    fn choice(&mut self) -> &mut edm_desktop_base::save_to::SaveChoice {
        &mut self.d.save
    }

    fn folder(&self) -> &Entity<InputState> {
        &self.folder
    }

    fn view(&self) -> &WeakEntity<Downloads> {
        &self.view
    }
}

impl Render for Links {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin_in(window);
        let (screen, says) = (self.screen.clone(), self.says.clone());
        self.d.types = self.types.read(cx).value().to_string();
        // The Save to column says the folder as it is typed.
        self.chosen(cx);
        self.d.untick_refused();
        let settings = self
            .view
            .upgrade()
            .map(|v| v.read(cx).backend.settings())
            .unwrap_or_default();
        let root = settings.folders.download_dir.clone();
        let cats = settings.categories();
        let visible = self.d.visible();
        let listed: Vec<usize> = visible
            .iter()
            .enumerate()
            .filter(|(_, v)| **v)
            .map(|(i, _)| i)
            .collect();
        let pages = if self.d.hide_html {
            self.d
                .items
                .iter()
                .filter(|(u, _)| self.d.is_html(u))
                .count()
        } else {
            0
        };
        let found = edm_core::lock(&self.d.found).clone();
        // Each listed row's words, worked out once for this frame.
        let rows: Vec<Row> = listed
            .iter()
            .map(|&i| {
                let (url, on) = &self.d.items[i];
                let mut name = self.d.name_of(url);
                if self.d.listed.contains(&edm_shell::lists::bare(url)) {
                    name = tf("{} (already in the list)", &[&name]);
                }
                let what = found.get(url.as_str());
                let kind = match what {
                    Some(f) if f.failed.is_some() => t("Error").to_string(),
                    Some(f) => edm_desktop_base::i18n::name(&format::titled(&f.kind)),
                    None => "…".to_string(),
                };
                let size = what
                    .and_then(|f| f.size)
                    .map(format::bytes)
                    .unwrap_or_default();
                let folder = self.d.folder_of(url, &root, &cats);
                let last = std::path::Path::new(&folder)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or(folder);
                let words = self.d.texts.get(url.as_str()).cloned().unwrap_or_default();
                (i, name, *on, kind, size, last, words)
            })
            .collect();
        let count = rows.len();
        let cat_words = savefolder::categories(cx, &self.view);
        let save_to = savefolder::section(self, &cat_words, theme, &screen, cx);
        let queues: Vec<SharedString> = self
            .queues
            .iter()
            .map(|q| edm_desktop_base::i18n::name(q).into())
            .collect();
        let queue_at = self.queues.iter().position(|q| *q == self.d.queue);
        let check = |id: &'static str, words: &'static str, on: bool| {
            native::checkbox(id, t(words).into(), on, theme, &screen)
        };
        let body = native::body(theme)
            .child(screen.label(tf(
                "{} links on {}",
                &[&count, &format::shown_url(&self.d.page)],
            )))
            .when(pages > 0, |d| {
                d.child(native::note(
                    edm_desktop_base::i18n::tn(
                        "{} web page hidden||{} web pages hidden",
                        pages as u64,
                        &[&pages],
                    ),
                    theme.muted,
                    &screen,
                ))
            });
        let body = self
            .filters(theme, &screen, &check, body, cx)
            // The links: a heading row, then only the rows in view.
            .child(self.link_list(theme, &screen, rows, count, cx))
            // The row being edited: its name and its address.
            .when_some(self.editing.clone(), |d, (_, name, address)| {
                d.child(self.editing_row(&name, &address, theme, &screen, cx))
            })
            .child(
                check(
                    "use-texts",
                    "Use link texts as descriptions",
                    self.d.use_texts,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.d.use_texts = !this.d.use_texts;
                    cx.notify();
                })),
            )
            // Where the ticked links go, as reference A offers it.
            .child(save_to)
            .child(native::says("links-says", says, theme, &screen))
            // As on a Mac: Add for later, to a queue, at the leading
            // side; Cancel, then Start now (the default) at the end.
            .child(self.buttons(theme, &screen, queues, queue_at, cx));
        let root = crate::widgets::native::dialog("links", t("Download links"), theme, cx)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, pick: &Pick, _, cx| this.pick(pick, cx)))
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

/// Opens "Download links" for the page's links in `request`.
pub fn open(view: WeakEntity<Downloads>, request: edm_shell::host::Request, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, request, cx));
}

fn open_now(view: WeakEntity<Downloads>, request: edm_shell::host::Request, cx: &mut App) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let queues = backend.queues();
    let failed = view.clone();
    // Smaller on Windows, as Options is.
    let (width, height) = crate::widgets::native::WindowSize::Links.here();
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Download links").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            // Each server's answer, as it comes, drawn on this window's
            // thread.
            let (tell, told) = async_channel::unbounded::<()>();
            let d = LinksImport::open(request, &backend, move || {
                let _ = tell.try_send(());
            });
            let types = cx.new(|cx| InputState::new(window, cx).placeholder(t("zip pdf mp4")));
            let links =
                cx.new(|cx| {
                    // Typed types filter at once; Return in a field is Start
                    // now.
                    cx.subscribe_in(&types, window, |this: &mut Links, _, event, window, cx| {
                        match event {
                            InputEvent::PressEnter { .. } => this.add(false, window, cx),
                            InputEvent::Change => cx.notify(),
                            _ => {}
                        }
                    })
                    .detach();
                    let folder = savefolder::field(window, cx, |this: &mut Links, window, cx| {
                        this.add(false, window, cx)
                    });
                    Links {
                        view,
                        d,
                        types,
                        folder,
                        editing: None,
                        queues,
                        says: None,
                        done: false,
                        focus: cx.focus_handle(),
                        screen,
                        scroll: UniformListScrollHandle::new(),
                    }
                });
            links.update(cx, |l, cx| window.focus(&l.focus, cx));
            let weak = links.downgrade();
            window
                .spawn(cx, async move |cx| {
                    while told.recv().await.is_ok() {
                        if weak.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                })
                .detach();
            crate::widgets::native::close_is_cancel(&links, window, cx);
            cx.new(|cx| gpui_component::Root::new(links, window, cx))
        },
    );
    if let Err(e) = opened {
        edm_core::note!(error, "edm-gpui: Download links could not be opened: {e}");
        let _ = failed.update(cx, |view, _| view.handed_closed());
    }
}

impl crate::widgets::native::CloseButton for Links {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(window, cx);
    }
}
