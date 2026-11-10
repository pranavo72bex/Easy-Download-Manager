// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details, a window of its own (B02), with the egui window's
//! words: where the file goes (its category, its folder, chosen in the
//! system's own folder panel), its name (the server's, once asked), its size,
//! a note, when it is deleted, and Start now, Add for later to a queue, or
//! Cancel. What it offers and adds is dm's (`dm_desktop_base::file_info`).
//! A video's qualities and part, and a ZIP's files, are still the egui
//! window's (docs/gaps.md, G7).

use dm_core::Id;
use dm_desktop_base::file_info::{self, Clash, Form};
use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use dm_shell::app::backend::Backend;
use dm_shell::app::lookups::Probed;
use dm_shell::Settings;
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, FocusHandle, KeyDownEvent,
    PathPromptOptions, Role, SharedString, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::list::Downloads;
use crate::popup::{self, Pick};
use crate::question::button;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

/// The Download details window's view.
pub struct Details {
    view: WeakEntity<Downloads>,
    backend: Backend,
    settings: Settings,
    urls: Vec<String>,
    folder: Entity<InputState>,
    name: Entity<InputState>,
    note: Entity<InputState>,
    /// What the server said, once it has; `None` while it is asked.
    probed: Option<Probed>,
    /// The folder this kind of file went to last has been offered.
    folder_offered: bool,
    queue: String,
    queues: Vec<String>,
    delete_after: Option<u64>,
    replace: bool,
    /// The row begun at once while the dialog asks.
    early: Option<Id>,
    says: Option<SharedString>,
    /// Answered: a second Return in the same moment adds nothing.
    done: bool,
    focus: FocusHandle,
    screen: Screen,
}

impl Details {
    /// The name the server gave, once it has.
    fn server_name(&self) -> Option<&str> {
        self.probed.as_ref().and_then(|p| p.name.as_deref())
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

    /// What the server said: its name put in Save as where nothing is typed.
    fn probed(&mut self, found: Probed, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = found.name.clone() {
            if self.name.read(cx).value().is_empty() {
                self.name.update(cx, |n, cx| n.set_value(name, window, cx));
            }
        }
        self.probed = Some(found);
        self.offer_folder(window, cx);
        cx.notify();
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

    /// Start now (`later` false) or Add for later.
    fn add(&mut self, later: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let clash = self.clash(cx);
        let name = self.name.read(cx).value().to_string();
        let server_name = self.server_name().map(str::to_string);
        let form = Form {
            folder: self.folder.read(cx).value().to_string(),
            name_typed: !name.trim().is_empty() && server_name.as_deref() != Some(name.trim()),
            name: name.clone(),
            description: self.note.read(cx).value().to_string(),
            queue: self.queue.clone(),
            delete_after: self.delete_after,
            replace: self.replace && matches!(clash, Some(Clash::OnDisk { .. })),
            ..Form::default()
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
                self.view
                    .update(cx, |view, cx| view.add(urls, with, cx))
                    .unwrap_or_else(|_| Err(String::new()))
            }
        };
        match added {
            Ok(()) => {
                self.done = true;
                // A row begun while it asked is placed now: the list read
                // again (an added one was read as it was added).
                if self.early.is_some() {
                    let _ = self.view.update(cx, |view, cx| {
                        view.read();
                        cx.notify();
                    });
                }
                window.remove_window();
            }
            Err(e) => {
                self.says = Some(e.into());
                cx.notify();
            }
        }
    }

    /// Cancel, Escape or the close button: what was fetched while it asked
    /// goes with it.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        if let Some(id) = self.early.take() {
            self.backend.remove_later(id);
            let _ = self.view.update(cx, |view, cx| {
                view.read();
                cx.notify();
            });
        }
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

/// A label and what it labels, as a row of the dialog's form.
fn row(label: &'static str, screen: &Screen, what: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(120.0))
                .flex()
                .justify_end()
                .child(screen.label(t(label))),
        )
        .child(div().flex_1().flex().items_center().gap_2().child(what))
}

impl Render for Details {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let first = self.urls.first().cloned().unwrap_or_default();
        let single = self.urls.len() == 1;
        let folder = self.folder.read(cx).value().trim().to_string();
        let clash = self.clash(cx);
        let page = self.probed.as_ref().is_some_and(|p| p.page);
        let problem = self.probed.as_ref().and_then(|p| p.problem.clone());

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

        let size_words: SharedString = match self.probed.as_ref().and_then(|p| p.size) {
            Some(size) => format::bytes(size).into(),
            None if page => t("Known once the video starts").into(),
            None => "—".into(),
        };

        div()
            .id("download-details")
            .role(Role::Dialog)
            .aria_label(t("Download details"))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, pick: &Pick, window, cx| this.pick(pick, window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.cancel(window, cx),
                    "enter" => this.add(false, window, cx),
                    _ => {}
                }
            }))
            .size_full()
            .bg(theme.background)
            .text_color(theme.text)
            .text_sm()
            .child(crate::fit::fitted(
                div()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_color(theme.muted)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(screen.label(format::shown_url(&first))),
                    )
                    .when(self.urls.len() > 1, |d| {
                        d.child(screen.label(tf("and {} more", &[&(self.urls.len() - 1)])))
                    })
                    .child(div().h(px(1.0)).my_1().bg(theme.separator))
                    .child(row(
                        "Category",
                        &screen,
                        popup::button(
                            "category",
                            category_words,
                            categories,
                            category_at,
                            theme,
                            &screen,
                        ),
                    ))
                    .child(row(
                        "Save to folder",
                        &screen,
                        div()
                            .flex_1()
                            .flex()
                            .gap_2()
                            .child(
                                div().flex_1().child(
                                    Input::new(&self.folder).aria_label(t("Save to folder")),
                                ),
                            )
                            .child(
                                button("browse", t("Browse…").into(), false, theme, &screen)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.browse(window, cx)),
                                    ),
                            ),
                    ))
                    .when(single, |d| {
                        d.child(row(
                            "Save as",
                            &screen,
                            div()
                                .flex_1()
                                .child(Input::new(&self.name).aria_label(t("Save as"))),
                        ))
                        .child(row(
                            "Size",
                            &screen,
                            div()
                                .text_color(theme.muted)
                                .child(screen.label(size_words)),
                        ))
                    })
                    .child(row(
                        "Note",
                        &screen,
                        div()
                            .flex_1()
                            .child(Input::new(&self.note).aria_label(t("Note"))),
                    ))
                    .child(row(
                        "Delete file after",
                        &screen,
                        popup::button(
                            "delete-after",
                            times[time_at].0.into(),
                            times.iter().map(|(w, _)| SharedString::from(*w)).collect(),
                            Some(time_at),
                            theme,
                            &screen,
                        ),
                    ))
                    .when_some(clash.clone(), |d, clash| match clash {
                        Clash::Listed => {
                            d.child(div().text_color(theme.warning).child(screen.label(t(
                                "A download with this name exists. This one will be numbered.",
                            ))))
                        }
                        Clash::OnDisk { free, .. } => {
                            let replace = self.replace;
                            d.child(div().text_color(theme.warning).child(
                                screen.label(t("A file of that name is in the folder already.")),
                            ))
                            .child(
                                radio(
                                    "keep-both",
                                    tf("Save this one as {}", &[&free]).into(),
                                    !replace,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.replace = false;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                radio(
                                    "replace",
                                    t("Replace that file once this one is whole").into(),
                                    replace,
                                    theme,
                                    &screen,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.replace = true;
                                        cx.notify();
                                    },
                                )),
                            )
                        }
                    })
                    .when_some(problem, |d, why| {
                        d.child(
                            div()
                                .text_color(theme.warning)
                                .child(screen.label(file_info::problem_words(&why))),
                        )
                    })
                    .when_some(self.says.clone(), |d, says| {
                        d.child(div().text_color(theme.warning).child(screen.label(says)))
                    })
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                button("start", t("Start now").into(), true, theme, &screen)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add(false, window, cx)
                                    })),
                            )
                            .child(
                                button("later", t("Add for later").into(), false, theme, &screen)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add(true, window, cx)
                                    })),
                            )
                            // "to" by itself is many words in other languages: the
                            // key says which one.
                            .child(
                                screen
                                    .label(t("queue:to").trim_start_matches("queue:").to_string()),
                            )
                            .child(popup::button(
                                "queue",
                                queue_words,
                                queues,
                                queue_at,
                                theme,
                                &screen,
                            ))
                            .child(
                                button("cancel", t("Cancel").into(), false, theme, &screen)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.cancel(window, cx)),
                                    ),
                            ),
                    ),
            ))
    }
}

/// A radio button, as the system draws one.
fn radio(
    id: &'static str,
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
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(14.0))
                .rounded_full()
                .border_1()
                .border_color(theme.muted)
                .flex()
                .items_center()
                .justify_center()
                .when(on, |b| {
                    b.bg(theme.selection)
                        .child(div().size(px(6.0)).rounded_full().bg(theme.on_selection))
                }),
        )
        .child(screen.label(words))
}

/// Opens Download details for `urls` (several: a batch, in one).
pub fn open(view: WeakEntity<Downloads>, urls: Vec<String>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, urls, cx));
}

fn open_now(view: WeakEntity<Downloads>, urls: Vec<String>, cx: &mut App) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let settings = backend.settings();
    let queues = backend.queues();
    let height = if urls.len() == 1 { 360.0 } else { 300.0 };
    let bounds = Bounds::centered(None, size(px(600.0), px(height)), cx);
    let opened = cx.open_window(
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
            let first = urls.first().cloned().unwrap_or_default();
            let remux = settings.remux_ts;
            let folder = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
            let name = cx.new(|cx| {
                InputState::new(window, cx).placeholder(file_info::name_hint(&first, false, remux))
            });
            let note = cx.new(|cx| InputState::new(window, cx));
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            // Begun at once while it asks, where Options says so.
            let early = file_info::begins_early(&settings, &urls, &Form::default(), false)
                .then(|| {
                    backend
                        .add_with(&first, dm_core::AddOptions::default())
                        .ok()
                })
                .flatten();
            let details = cx.new(|cx| {
                let focus = cx.focus_handle();
                // Return in a field is Start now, as in the egui window;
                // what is typed is weighed again (the name's clash).
                for input in [&folder, &name, &note] {
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
                    folder: folder.clone(),
                    name: name.clone(),
                    note,
                    probed: None,
                    folder_offered: false,
                    queue: dm_core::MAIN_QUEUE.into(),
                    queues,
                    delete_after: None,
                    replace: false,
                    early,
                    says: None,
                    done: false,
                    focus,
                    screen,
                }
            });
            // Start now has the keyboard, as in the egui window.
            details.update(cx, |d, cx| window.focus(&d.focus, cx));
            ask_server(&details, &backend, &first, urls.len(), remux, window, cx);
            if early.is_some() {
                let _ = view.update(cx, |v, cx| {
                    v.read();
                    cx.notify();
                });
            }
            cx.new(|cx| gpui_component::Root::new(details, window, cx))
        },
    );
    if let Err(e) = opened {
        eprintln!("dm-gpui: Download details could not be opened: {e}");
    }
}

/// The server asked what it calls the file and how big it is, once, as the
/// dialog opens (not a torrent's: what the server says of a .torrent is the
/// metainfo's few kilobytes and its file name).
fn ask_server(
    details: &Entity<Details>,
    backend: &Backend,
    url: &str,
    count: usize,
    remux: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let web = url.starts_with("http://") || url.starts_with("https://");
    if count != 1 || !web || dm_core::is_torrent(url) {
        // Nothing to ask: known at once.
        details.update(cx, |d, cx| d.probed(Probed::default(), window, cx));
        return;
    }
    let (tell, told) = async_channel::bounded(1);
    let sign_in = dm_core::cookies::SignIn::with_headers(url, "", &[]);
    dm_shell::app::lookups::probe_later(
        backend,
        url.to_string(),
        String::new(),
        sign_in,
        remux,
        move |found| {
            let _ = tell.try_send(found);
        },
    );
    let details = details.downgrade();
    window
        .spawn(cx, async move |cx| {
            if let Ok(found) = told.recv().await {
                let _ = details.update_in(cx, |d, window, cx| d.probed(found, window, cx));
            }
        })
        .detach();
}
