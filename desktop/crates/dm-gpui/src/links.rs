// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "Download links", a window of its own: a page's links the browser handed
//! over, each with its name, kind, size, folder and words on the page as
//! its server and the page tell them; filtered, ticked, given a folder, and
//! added now or for later. What it lists and adds is dm's
//! (`dm_desktop_base::links_import`).

use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::links_import::{added_words, LinksImport, SaveTo};
use gpui::{
    div, prelude::*, px, size, uniform_list, App, Bounds, Context, Entity, FocusHandle,
    KeyDownEvent, PathPromptOptions, Role, SharedString, UniformListScrollHandle, WeakEntity,
    Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::popup::{self, Pick};
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

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
        self.d.folder = self.folder.read(cx).value().to_string();
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let settings = view.read(cx).backend.settings();
        let (items, _sizes) =
            match self
                .d
                .adds(later, &settings.download_dir, &settings.categories())
            {
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
            match view.add_items(items, cx) {
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

    /// Browse…: the system's own folder panel, for Every file to this
    /// folder.
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
                        this.d.save_to = SaveTo::OneFolder;
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// A choice from a pop-up.
    fn pick(&mut self, pick: &Pick, cx: &mut Context<Self>) {
        match pick.field {
            "links-category" => {
                if let Some(word) = categories(cx, &self.view).get(pick.index) {
                    self.d.category = word.clone();
                    self.d.save_to = SaveTo::OneCategory;
                }
            }
            "links-queue" => {
                if let Some(queue) = self.queues.get(pick.index) {
                    self.d.queue = queue.clone();
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

/// The categories "Every file to this category" offers, by their words:
/// each kind's, then each a person made.
fn categories(cx: &App, view: &WeakEntity<Downloads>) -> Vec<String> {
    let customs: Vec<String> = view
        .upgrade()
        .map(|v| v.read(cx).backend.settings().custom_categories.clone())
        .unwrap_or_default()
        .iter()
        .map(|c| c.name.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    dm_core::Category::ALL
        .iter()
        .map(|k| dm_shell::app::names::category_word(*k))
        .chain(customs)
        .collect()
}

/// A category's word as the window says it.
fn shown_category(word: &str) -> String {
    dm_desktop_base::i18n::name(&format::titled(word))
}

impl Render for Links {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        self.d.types = self.types.read(cx).value().to_string();
        self.d.untick_refused();
        let settings = self
            .view
            .upgrade()
            .map(|v| v.read(cx).backend.settings())
            .unwrap_or_default();
        let root = settings.download_dir.clone();
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
        let found = dm_core::lock(&self.d.found).clone();
        // Each listed row's words, worked out once for this frame.
        let rows: Vec<(usize, String, bool, String, String, String, String)> = listed
            .iter()
            .map(|&i| {
                let (url, on) = &self.d.items[i];
                let mut name = self.d.name_of(url);
                if self.d.listed.contains(&dm_shell::lists::bare(url)) {
                    name = tf("{} (already in the list)", &[&name]);
                }
                let what = found.get(url.as_str());
                let kind = match what {
                    Some(f) if f.failed.is_some() => t("Error").to_string(),
                    Some(f) => dm_desktop_base::i18n::name(&format::titled(&f.kind)),
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
        let cat_words = categories(cx, &self.view);
        let cat_at = cat_words.iter().position(|w| *w == self.d.category);
        let cat_shown: Vec<SharedString> =
            cat_words.iter().map(|w| shown_category(w).into()).collect();
        let queues: Vec<SharedString> = self
            .queues
            .iter()
            .map(|q| dm_desktop_base::i18n::name(q).into())
            .collect();
        let queue_at = self.queues.iter().position(|q| *q == self.d.queue);
        let save_to = self.d.save_to;
        let radio = |id: &'static str, words: &'static str, on: bool| {
            native::radio(id, t(words).into(), on, theme, &screen)
        };
        let check = |id: &'static str, words: &'static str, on: bool| {
            native::checkbox(id, t(words).into(), on, theme, &screen)
        };
        let column = |words: &'static str, width: Option<f32>| {
            let d = div()
                .px(px(4.0))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis();
            match width {
                Some(w) => d.w(px(w)).flex_none(),
                None => d.flex_1().min_w_0(),
            }
            .child(screen.label(t(words)))
        };
        div()
            .id("links")
            .role(Role::Dialog)
            .aria_label(t("Download links"))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, pick: &Pick, _, cx| this.pick(pick, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.cancel(window, cx),
                    "enter" => this.add(false, window, cx),
                    _ => {}
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(screen.label(tf(
                        "{} links on {}",
                        &[&count, &format::shown_url(&self.d.page)],
                    )))
                    .when(pages > 0, |d| {
                        d.child(native::note(
                            dm_desktop_base::i18n::tn(
                                "{} web page hidden||{} web pages hidden",
                                pages as u64,
                                &[&pages],
                            ),
                            theme.muted,
                            &screen,
                        ))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(native::LABEL_GAP))
                            .child(screen.label(t("Only these types")))
                            .child(
                                div()
                                    .w(px(200.0))
                                    .child(native::field(&self.types, "Only these types", theme)),
                            )
                            .child(div().flex_1())
                            .child(
                                native::button("check-all", t("Check all").into(), false, theme, &screen)
                                    .on_click(cx.listener(|this, _, _, cx| this.check_all(true, cx))),
                            )
                            .child(
                                native::button(
                                    "uncheck-all",
                                    t("Uncheck all").into(),
                                    false,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.check_all(false, cx))),
                            ),
                    )
                    // What is left out, as reference A offers it.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(native::BUTTON_GAP))
                            .child(check("hide-html", "Hide HTML files", self.d.hide_html).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.d.hide_html = !this.d.hide_html;
                                    cx.notify();
                                }),
                            ))
                            .child(check("hide-images", "Hide images", self.d.hide_images).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.d.hide_images = !this.d.hide_images;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                check("hide-repeats", "Hide repeated files", self.d.hide_repeats)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.d.hide_repeats = !this.d.hide_repeats;
                                        cx.notify();
                                    })),
                            ),
                    )
                    // The links: a heading row, then only the rows in view.
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .rounded(px(native::RADIUS))
                            .border_1()
                            .border_color(theme.separator)
                            .bg(theme.background)
                            .child(
                                div()
                                    .flex()
                                    .h(px(ROW))
                                    .items_center()
                                    .px(px(4.0))
                                    .border_b_1()
                                    .border_color(theme.separator)
                                    .text_size(px(native::SMALL_TEXT))
                                    .text_color(theme.muted)
                                    .child(column("File name", None))
                                    .child(column("Type", Some(80.0)))
                                    .child(column("Size", Some(72.0)))
                                    .child(column("Save to", Some(96.0)))
                                    .child(column("Link text", Some(110.0)))
                                    .child(div().w(px(52.0)).flex_none()),
                            )
                            .child(
                                div().h(px(ROW * SHOWN.min(count.max(1)) as f32)).child(
                                    uniform_list(
                                        "link-rows",
                                        count,
                                        cx.processor(
                                            move |this, range: std::ops::Range<usize>, _, cx| {
                                                let screen = this.screen.clone();
                                                range
                                                    .filter_map(|k| {
                                                        let (i, name, on, kind, size, last, words) =
                                                            rows.get(k)?.clone();
                                                        let cell = |text: String, w: f32| {
                                                            div()
                                                                .w(px(w))
                                                                .flex_none()
                                                                .px(px(4.0))
                                                                .whitespace_nowrap()
                                                                .overflow_hidden()
                                                                .text_ellipsis()
                                                                .text_color(theme.muted)
                                                                .child(screen.label(text))
                                                        };
                                                        Some(
                                                            div()
                                                                .w_full()
                                                                .h(px(ROW))
                                                                .flex()
                                                                .items_center()
                                                                .px(px(4.0))
                                                                .when(k % 2 == 1, |d| {
                                                                    d.bg(theme.stripe)
                                                                })
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .overflow_hidden()
                                                                        .child(
                                                                            native::checkbox(
                                                                                ("link", i),
                                                                                name.into(),
                                                                                on,
                                                                                theme,
                                                                                &screen,
                                                                            )
                                                                            .on_click(cx.listener(
                                                                                move |this, _, _, cx| {
                                                                                    if let Some(l) =
                                                                                        this.d
                                                                                            .items
                                                                                            .get_mut(i)
                                                                                    {
                                                                                        l.1 = !l.1;
                                                                                        cx.notify();
                                                                                    }
                                                                                },
                                                                            )),
                                                                        ),
                                                                )
                                                                .child(cell(kind, 80.0))
                                                                .child(cell(size, 72.0))
                                                                .child(cell(last, 96.0))
                                                                .child(cell(words, 110.0))
                                                                .child(
                                                                    div()
                                                                        .id(("edit", i))
                                                                        .w(px(52.0))
                                                                        .flex_none()
                                                                        .text_color(theme.accent)
                                                                        .child(screen.label(t("Edit")))
                                                                        .on_click(cx.listener(
                                                                            move |this, _, window, cx| {
                                                                                this.edit(i, window, cx)
                                                                            },
                                                                        )),
                                                                ),
                                                        )
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(&self.scroll)
                                    .size_full(),
                                ),
                            ),
                    )
                    // The row being edited: its name and its address.
                    .when_some(self.editing.clone(), |d, (_, name, address)| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(native::LABEL_GAP))
                                .child(screen.label(t("File name")))
                                .child(div().w(px(180.0)).child(native::field(&name, "File name", theme)))
                                .child(screen.label(t("Address")))
                                .child(div().flex_1().child(native::field(&address, "Address", theme)))
                                .child(
                                    native::button("done", t("Done").into(), false, theme, &screen)
                                        .on_click(cx.listener(|this, _, _, cx| this.finish_editing(cx))),
                                ),
                        )
                    })
                    .child(
                        check("use-texts", "Use link texts as descriptions", self.d.use_texts)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.d.use_texts = !this.d.use_texts;
                                cx.notify();
                            })),
                    )
                    // Where the ticked links go, as reference A offers it.
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(screen.label(t("Save to"))),
                    )
                    .child(
                        radio(
                            "by-category",
                            "Each file to the folder of its category",
                            save_to == SaveTo::ByCategory,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.d.save_to = SaveTo::ByCategory;
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(native::LABEL_GAP))
                            .child(
                                radio(
                                    "one-category",
                                    "Every file to this category",
                                    save_to == SaveTo::OneCategory,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.d.save_to = SaveTo::OneCategory;
                                    cx.notify();
                                })),
                            )
                            .child(popup::button(
                                "links-category",
                                shown_category(&self.d.category).into(),
                                cat_shown,
                                cat_at,
                                theme,
                                &screen,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(native::LABEL_GAP))
                            .child(
                                radio(
                                    "one-folder",
                                    "Every file to this folder",
                                    save_to == SaveTo::OneFolder,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.d.save_to = SaveTo::OneFolder;
                                    cx.notify();
                                })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(native::field(&self.folder, "Every file to this folder", theme)),
                            )
                            .child(
                                native::button("browse", t("Browse…").into(), false, theme, &screen)
                                    .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                            ),
                    )
                    .when_some(self.says.clone(), |d, says| {
                        d.child(native::note(says, theme.warning, &screen))
                    })
                    // As on a Mac: Add for later, to a queue, at the leading
                    // side; Cancel, then Start now (the default) at the end.
                    .child(native::buttons(
                        vec![
                            native::button("later", t("Add for later").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx)))
                                .into_any_element(),
                            screen
                                .label(t("queue:to").trim_start_matches("queue:").to_string())
                                .into_any_element(),
                            popup::button(
                                "links-queue",
                                dm_desktop_base::i18n::name(&self.d.queue).into(),
                                queues,
                                queue_at,
                                theme,
                                &screen,
                            )
                            .into_any_element(),
                        ],
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx)))
                                .into_any_element(),
                            native::button("start", t("Start now").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.add(false, window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens "Download links" for the page's links in `request`.
pub fn open(view: WeakEntity<Downloads>, request: dm_shell::host::Request, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, request, cx));
}

fn open_now(view: WeakEntity<Downloads>, request: dm_shell::host::Request, cx: &mut App) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let queues = backend.queues();
    let failed = view.clone();
    let bounds = Bounds::centered(None, size(px(760.0), px(560.0)), cx);
    let opened = cx.open_window(
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
            let folder = cx.new(|cx| InputState::new(window, cx));
            let links = cx.new(|cx| {
                // Typed types filter at once; Return in a field is Start
                // now.
                for input in [&types, &folder] {
                    cx.subscribe_in(input, window, |this: &mut Links, _, event, window, cx| {
                        match event {
                            InputEvent::PressEnter { .. } => this.add(false, window, cx),
                            InputEvent::Change => cx.notify(),
                            _ => {}
                        }
                    })
                    .detach();
                }
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
            crate::native::close_is_cancel(&links, window, cx);
            cx.new(|cx| gpui_component::Root::new(links, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: Download links could not be opened: {e}");
        let _ = failed.update(cx, |view, _| view.handed_closed());
    }
}

impl crate::native::CloseButton for Links {
    fn close_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel(window, cx);
    }
}
