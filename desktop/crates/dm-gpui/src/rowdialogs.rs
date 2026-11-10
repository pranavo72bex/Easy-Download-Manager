// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The row menu's dialogs, each a window of its own as Add URL is, with
//! the egui window's words: Update download link, Move to queue and
//! Move/Rename. What OK does is dm's (`dm_desktop_base::row_edits`), so the
//! two windows cannot differ.

use dm_core::{Entry, Id};
use dm_desktop_base::i18n::{t, tn};
use dm_desktop_base::row_edits::{self, MoveRename};
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, KeyDownEvent, Role, SharedString,
    WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::list::Downloads;
use crate::native;
use crate::screen::{Screen, Screens};
use crate::theme::Theme;

/// Opens a dialog's window over the middle of the screen, `width` wide and
/// as tall as it turns out (`fit`), after the update that asked, so the
/// main window's view can be read.
fn open_window<V: Render + crate::native::CloseButton>(
    title: &'static str,
    width: f32,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App, Screen) -> Option<Entity<V>> + 'static,
) {
    cx.defer(move |cx| {
        let bounds = Bounds::centered(None, size(px(width), px(160.0)), cx);
        let opened = crate::screen::open_window(
            cx,
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(t(title).into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let screen = Screen::default();
                Screens::register(window.window_handle(), screen.clone(), true, cx);
                match build(window, cx, screen) {
                    Some(view) => {
                        crate::native::close_is_cancel(&view, window, cx);
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    }
                    // The row went while the window opened: an empty one,
                    // closed at once.
                    None => {
                        let empty = cx.new(|_| Gone);
                        window.defer(cx, |window, _| window.remove_window());
                        cx.new(|cx| gpui_component::Root::new(empty, window, cx))
                    }
                }
            },
        );
        if let Err(e) = opened {
            eprintln!("dm-gpui: {title} could not be opened: {e}");
        }
    });
}

/// A dialog whose row went before its window opened.
struct Gone;

impl Render for Gone {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Return in `input` is `ok`, and what was said of the last answer goes
/// once it is changed.
fn on_enter<V: 'static>(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<V>,
    ok: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    changed: impl Fn(&mut V) -> bool + 'static,
) {
    cx.subscribe_in(
        input,
        window,
        move |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => ok(this, window, cx),
            InputEvent::Change if changed(this) => cx.notify(),
            _ => {}
        },
    )
    .detach();
}

/* ------------------------------------------------- Update download link */

/// Update download link: a fresh address for a download whose link ran
/// out, or the page it came from opened again and the next download
/// clicked there taken as the link.
pub struct UpdateLink {
    view: WeakEntity<Downloads>,
    id: Id,
    /// The name the list shows.
    named: String,
    /// The address it has now, as shown.
    url: String,
    /// The page it came from, where that is a web page.
    page: Option<String>,
    address: Entity<InputState>,
    says: Option<SharedString>,
    done: bool,
    screen: Screen,
}

impl UpdateLink {
    /// OK: the address taken, or the reason it is not said.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let typed = self.address.read(cx).value().to_string();
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
        };
        let taken = row_edits::update_link(&view.read(cx).backend, self.id, &typed);
        match taken {
            Ok(()) => {
                self.done = true;
                window.remove_window();
                view.update(cx, |view, cx| {
                    view.read();
                    cx.notify();
                });
            }
            Err(e) => {
                self.says = Some(e.into());
                cx.notify();
            }
        }
    }

    /// Get it from the browser: the page opened again, and the next
    /// download clicked there taken as the link.
    fn take_from_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.page.clone() else {
            return;
        };
        self.done = true;
        window.remove_window();
        let id = self.id;
        let _ = self
            .view
            .update(cx, |view, cx| view.await_link(id, &page, cx));
    }

    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }
}

impl Render for UpdateLink {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        div()
            .id("update-link")
            .role(Role::Dialog)
            .aria_label(t("Update download link"))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    this.cancel(window);
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(screen.label(self.named.clone())),
                    )
                    .child(
                        div()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(native::note(self.url.clone(), theme.muted, &screen)),
                    )
                    .child(screen.label(t("The download's link has expired? Paste a fresh one:")))
                    .child(native::field(&self.address, "New address", theme))
                    .when(self.page.is_some(), |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(native::LABEL_GAP))
                                .child(div().text_color(theme.muted).child(screen.label(t("Or:"))))
                                .child(
                                    native::button(
                                        "from-browser",
                                        t("Get it from the browser").into(),
                                        false,
                                        theme,
                                        &screen,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.take_from_browser(window, cx),
                                    )),
                                ),
                        )
                    })
                    .when_some(self.says.clone(), |d, says| {
                        d.child(native::note(says, theme.warning, &screen))
                    })
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("OK").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens Update download link for row `id`.
pub fn update_link(view: WeakEntity<Downloads>, id: Id, cx: &mut App) {
    open_window(
        "Update download link",
        480.0,
        cx,
        move |window, cx, screen| {
            let downloads = view.upgrade()?;
            let (named, row) = {
                let downloads = downloads.read(cx);
                let row = downloads.rows.iter().find(|r| r.id == id)?;
                (downloads.name_shown(row), Entry::clone(row))
            };
            let address = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
            address.update(cx, |a, cx| a.focus(window, cx));
            Some(cx.new(|cx| {
                on_enter(
                    &address,
                    window,
                    cx,
                    |this: &mut UpdateLink, window, cx| this.ok(window, cx),
                    |this| this.says.take().is_some(),
                );
                UpdateLink {
                    view,
                    id,
                    named,
                    url: dm_desktop_base::format::shown_url(&row.url),
                    page: row_edits::is_web_page(&row.referrer).then(|| row.referrer.clone()),
                    address,
                    says: None,
                    done: false,
                    screen,
                }
            }))
        },
    );
}

/* -------------------------------------------------------- Move to queue */

/// Move to queue: one of the queues there are, or a new one named.
pub struct MoveToQueue {
    view: WeakEntity<Downloads>,
    ids: Vec<Id>,
    /// The queues there are, in dm's own names.
    known: Vec<String>,
    /// The one picked, as shown: taken back to dm's own on OK.
    picked: String,
    new: Entity<InputState>,
    done: bool,
    screen: Screen,
}

impl MoveToQueue {
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let new = self.new.read(cx).value().to_string();
        let Some(queue) = row_edits::queue_chosen(&self.picked, &new) else {
            return;
        };
        self.done = true;
        window.remove_window();
        let ids = self.ids.clone();
        let _ = self.view.update(cx, |view, cx| {
            for id in ids {
                view.backend.set_queue(id, &queue);
            }
            view.read();
            cx.notify();
        });
    }

    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// A queue picked: a new name typed beside it goes.
    fn pick(&mut self, shown: String, window: &mut Window, cx: &mut Context<Self>) {
        self.picked = shown;
        self.new.update(cx, |n, cx| n.set_value("", window, cx));
        cx.notify();
    }
}

impl Render for MoveToQueue {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let typing = !self.new.read(cx).value().trim().is_empty();
        let labels = [t("Or a new queue")];
        let width = native::label_width(&labels);
        let queues = self.known.iter().enumerate().map(|(i, q)| {
            let shown = dm_desktop_base::i18n::name(q);
            let on = !typing && self.picked == shown;
            native::radio(("queue", i), shown.clone().into(), on, theme, &screen).on_click(
                cx.listener(move |this, _, window, cx| this.pick(shown.clone(), window, cx)),
            )
        });
        div()
            .id("move-to-queue")
            .role(Role::Dialog)
            .aria_label(t("Move to queue"))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    this.cancel(window);
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    .child(
                        div()
                            .id("queues")
                            .role(Role::RadioGroup)
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .children(queues),
                    )
                    .child(native::row(
                        Some(labels[0]),
                        width,
                        &screen,
                        native::field(&self.new, "Or a new queue", theme),
                    ))
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("OK").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens Move to queue for `ids`, the Main queue picked.
pub fn move_to_queue(view: WeakEntity<Downloads>, ids: Vec<Id>, cx: &mut App) {
    open_window("Move to queue", 360.0, cx, move |window, cx, screen| {
        let downloads = view.upgrade()?;
        let known = dm_core::queue_names(downloads.read(cx).rows.iter().map(|r| &**r));
        let new = cx.new(|cx| InputState::new(window, cx));
        // The keyboard in the field for a new queue: Return there is OK
        // with the queue picked.
        new.update(cx, |n, cx| n.focus(window, cx));
        Some(cx.new(|cx| {
            // Typing a name unpicks the queue picked: drawn again.
            on_enter(
                &new,
                window,
                cx,
                |this: &mut MoveToQueue, window, cx| this.ok(window, cx),
                |_| true,
            );
            MoveToQueue {
                view,
                ids,
                known,
                picked: dm_desktop_base::i18n::name(dm_core::MAIN_QUEUE),
                new,
                done: false,
                screen,
            }
        }))
    });
}

/* ---------------------------------------------------------- Move/Rename */

/// Move/Rename: a new name for one finished download, or a pattern for
/// several, and the folder they go to.
pub struct MoveRenameDialog {
    view: WeakEntity<Downloads>,
    ids: Vec<Id>,
    name: Entity<InputState>,
    pattern: Entity<InputState>,
    folder: Entity<InputState>,
    done: bool,
    screen: Screen,
}

impl MoveRenameDialog {
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let asked = MoveRename {
            ids: self.ids.clone(),
            name: self.name.read(cx).value().to_string(),
            folder: self.folder.read(cx).value().to_string(),
            pattern: self.pattern.read(cx).value().to_string(),
        };
        self.done = true;
        window.remove_window();
        let _ = self.view.update(cx, |view, cx| {
            if let Some(said) = asked.carry(&view.backend) {
                view.say(said.shown());
            }
            view.read();
            cx.notify();
        });
    }

    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// Browse…: the system's folder panel.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let picked = view.read(cx).backend.platform.choose_folder();
        if let Some(picked) = picked {
            self.folder.update(cx, |f, cx| {
                f.set_value(picked.display().to_string(), window, cx)
            });
        }
    }
}

impl Render for MoveRenameDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let one = self.ids.len() == 1;
        let labels = [t("New name"), t("Name pattern"), t("Folder")];
        let width = native::label_width(&labels);
        div()
            .id("move-rename")
            .role(Role::Dialog)
            .aria_label(t("Move/Rename"))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    this.cancel(window);
                }
            }))
            .size_full()
            .bg(theme.window)
            .child(crate::fit::fitted(
                native::body(theme)
                    // A new name for one; for several, only where they go.
                    .when(one, |d| {
                        d.child(native::row(
                            Some(labels[0]),
                            width,
                            &screen,
                            native::field(&self.name, "New name", theme),
                        ))
                    })
                    .when(!one, |d| {
                        let n = self.ids.len();
                        d.child(screen.label(tn(
                            "Moves {} finished file.||Moves {} finished files.",
                            n as u64,
                            &[&n],
                        )))
                        .child(native::row(
                            Some(labels[1]),
                            width,
                            &screen,
                            native::field(&self.pattern, "Name pattern", theme),
                        ))
                    })
                    .child(native::row(
                        Some(labels[2]),
                        width,
                        &screen,
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(native::LABEL_GAP))
                            .child(div().flex_1().child(native::field(
                                &self.folder,
                                "Folder",
                                theme,
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
                            ),
                    ))
                    .child(native::buttons(
                        Vec::new(),
                        vec![
                            native::button("cancel", t("Cancel").into(), false, theme, &screen)
                                .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                                .into_any_element(),
                            native::button("ok", t("OK").into(), true, theme, &screen)
                                .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                                .into_any_element(),
                        ],
                    )),
            ))
    }
}

/// Opens Move/Rename for `rows`: the first one's name and folder put in.
pub fn move_rename(view: WeakEntity<Downloads>, rows: Vec<Entry>, cx: &mut App) {
    open_window("Move/Rename", 480.0, cx, move |window, cx, screen| {
        let start = MoveRename::of(&rows);
        let name = cx.new(|cx| {
            let mut n = InputState::new(window, cx);
            n.set_value(start.name.clone(), window, cx);
            n
        });
        let pattern = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t(
                "Empty keeps the names. {n} counts from 1, {name} is the old name",
            ))
        });
        let folder = cx.new(|cx| {
            let mut f = InputState::new(window, cx);
            f.set_value(start.folder.clone(), window, cx);
            f
        });
        let first = if start.ids.len() == 1 {
            &name
        } else {
            &pattern
        };
        first.update(cx, |f, cx| f.focus(window, cx));
        // The name without its ending chosen, as Finder's rename chooses
        // it: typing replaces the name and keeps the kind.
        let stem = start
            .name
            .rfind('.')
            .filter(|&dot| dot > 0)
            .unwrap_or(start.name.len());
        name.update(cx, |n, cx| n.set_selected_range(0..stem, cx));
        Some(cx.new(|cx| {
            for input in [&name, &pattern, &folder] {
                on_enter(
                    input,
                    window,
                    cx,
                    |this: &mut MoveRenameDialog, window, cx| this.ok(window, cx),
                    |_| false,
                );
            }
            MoveRenameDialog {
                view,
                ids: start.ids,
                name,
                pattern,
                folder,
                done: false,
                screen,
            }
        }))
    });
}

/* ------------------------------------------------ the main window's part */

impl Downloads {
    /// Get it from the browser: the page row `id` came from opened in the
    /// browser, and the next download clicked there taken as its link.
    pub(crate) fn await_link(&mut self, id: Id, page: &str, cx: &mut Context<Self>) {
        self.open_path(page);
        self.session.hands.await_link(id, std::time::Instant::now());
        cx.notify();
    }

    /// While a fresh link is awaited: what to do, and a way out.
    pub(crate) fn awaiting_bar(
        &self,
        theme: Theme,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let (id, _) = self.session.hands.awaiting_link?;
        let name = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| dm_shell::app::names::name_of(r))
            .unwrap_or_default();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .px(px(12.0))
                .py(px(6.0))
                .border_b_1()
                .border_color(theme.separator)
                .child(div().flex_1().child(screen.label(dm_desktop_base::i18n::tf(
                    "Click the download again on its page in your browser, and dm uses that as the new link for {}.",
                    &[&name],
                ))))
                .child(
                    native::button("stop-awaiting", t("Cancel").into(), false, theme, screen)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.hands.stop_awaiting_link();
                            cx.notify();
                        })),
                ),
        )
    }

    /// A link that stopped working part-way, as signed links run out:
    /// asked for a fresh one at once, which goes on from the parts already
    /// here, rather than left for a person to know of Update download
    /// link. Not over another dialog; the row's own failure says Retry,
    /// and the row menu has it, either way.
    pub(crate) fn offer_new_link(&mut self, expired: Option<Id>, cx: &mut Context<Self>) {
        let Some(id) = expired else {
            return;
        };
        if self.backend.settings().offer_new_link && !self.dialog_in_front(cx) {
            update_link(cx.entity().downgrade(), id, cx);
        }
    }
}

impl crate::native::CloseButton for UpdateLink {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}

impl crate::native::CloseButton for MoveToQueue {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}

impl crate::native::CloseButton for MoveRenameDialog {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
