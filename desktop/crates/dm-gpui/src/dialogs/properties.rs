// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! File properties, a window for each download: its name, status and next
//! step, size, category and where it is (or will be) saved, the page it
//! came from; its address, sign-in and note, which OK keeps; a torrent's
//! files with their priorities; a finished file's SHA-256 and a box to
//! check it against a digest the person was given; a finished ZIP's files
//! (B44's Properties half). What OK keeps is dm's
//! (`dm_desktop_base::row_edits::keep_properties`).

use std::time::Duration;

use dm_core::{Entry, Id, State, TorrentPriority};
use dm_desktop_base::format;
use dm_desktop_base::i18n::{t, tf};
use dm_desktop_base::row_edits::{self, Edited};
use dm_desktop_base::zip_list::{self, Listing, ZipSlot};
use dm_shell::app::files::Sums;
use gpui::{
    div, prelude::*, size, AnyElement, App, Bounds, Context, Entity, SharedString, WeakEntity,
    Window, WindowBounds, WindowOptions,
};
use gpui_component::input::{InputEvent, InputState};

use crate::widgets::native;
use crate::widgets::popup::{self, Pick};
use crate::widgets::space;
use crate::window::list::Downloads;
use crate::window::screen::{Screen, Screens};
use crate::window::theme::Theme;
use crate::window::zoom::px;

mod sections;

use sections::{placed, Look};

/// How often a digest or a ZIP's reading still under way is looked at.
const POLL: Duration = Duration::from_millis(200);

/// The window's view.
pub struct Properties {
    view: WeakEntity<Downloads>,
    id: Id,
    address: Entity<InputState>,
    login: Entity<InputState>,
    password: Entity<InputState>,
    note: Entity<InputState>,
    /// The download's own `User-Agent` and request headers (gaps row F1).
    agent: Entity<InputState>,
    headers: Entity<InputState>,
    /// A digest the person was given, to check the file against.
    check: Entity<InputState>,
    /// A torrent's files ticked to be fetched, as edited.
    files: Vec<bool>,
    /// When each of a torrent's files is fetched, as edited.
    priorities: Vec<TorrentPriority>,
    sums: Sums,
    zip: Option<ZipSlot>,
    says: Option<SharedString>,
    /// "Delete file after" as chosen here, once it is (`None` leaves it be): OK keeps it, and
    /// Never stops a countdown (gaps row U8).
    delete_after: Option<Option<u64>>,
    /// A look again is due: a digest or a ZIP is still being read.
    polling: bool,
    done: bool,
    screen: Screen,
}

impl Properties {
    /// OK: what was edited kept, or the reason it is not said here.
    fn ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        let Some(view) = self.view.upgrade() else {
            window.remove_window();
            return;
        };
        let Some(row) = view
            .read(cx)
            .rows
            .iter()
            .find(|r| r.id == self.id)
            .map(|r| Entry::clone(r))
        else {
            window.remove_window();
            return;
        };
        let address = row_edits::join_login(
            &self.address.read(cx).value(),
            &self.login.read(cx).value(),
            &self.password.read(cx).value(),
        );
        let description = self.note.read(cx).value().to_string();
        let agent = self.agent.read(cx).value().to_string();
        let headers = self.headers.read(cx).value().to_string();
        let (files, priorities) = (self.files.clone(), self.priorities.clone());
        let delete_after = self.delete_after;
        // Off the window's thread: a new address for a running download
        // stops it first, and that is waited for (gaps row 183). A second
        // OK meanwhile is not taken; the window closes, or says why not,
        // once it is done.
        self.done = true;
        let (me, dialog) = (cx.entity().downgrade(), window.window_handle());
        view.update(cx, |view, cx| {
            let kept_row = row.clone();
            view.later(
                move |backend| {
                    if let Some(seconds) = delete_after {
                        backend.set_delete_after(kept_row.id, seconds);
                    }
                    row_edits::keep_properties(
                        backend,
                        &kept_row,
                        &Edited {
                            description: &description,
                            address: &address,
                            files: &files,
                            priorities: &priorities,
                            agent: &agent,
                            headers: &headers,
                        },
                    )
                },
                move |view, kept, cx| match kept {
                    Ok(retried) => {
                        // A sign-in given to a download that failed for want
                        // of one: it is tried again with it.
                        if retried {
                            view.say(tf(
                                "Trying {} again with the sign-in.",
                                &[&dm_shell::app::names::name_of(&row)],
                            ));
                        }
                        view.read();
                        cx.defer(move |cx| {
                            let _ = dialog.update(cx, |_, window, _| window.remove_window());
                        });
                    }
                    Err(e) => {
                        let _ = me.update(cx, |me, cx| {
                            me.done = false;
                            me.says = Some(e.into());
                            cx.notify();
                        });
                    }
                },
                cx,
            );
        });
    }

    fn cancel(&mut self, window: &mut Window) {
        self.done = true;
        window.remove_window();
    }

    /// Open: the finished file, by the system's opener; not by a script's
    /// click a moment ago, which is said here.
    fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
        let refused = self.view.update(cx, |view, _| {
            let refused = view.port.scripted.run_refused();
            if refused.is_none() {
                view.open_path(&path);
            }
            refused
        });
        if let Ok(Some(why)) = refused {
            self.says = Some(why.into());
            cx.notify();
        }
    }

    /// Copy details: what a person can paste to whoever helps, with nothing
    /// secret in it.
    fn copy_details(&mut self, row: &Entry, cx: &mut Context<Self>) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let settings = view.read(cx).backend.settings();
        if let Some((text, count)) = dm_desktop_base::details::blocks_for(
            std::slice::from_ref(row),
            settings.connections,
            &settings.download_dir,
        ) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            view.update(cx, |view, cx| {
                view.say(dm_desktop_base::i18n::tn(
                    "Copied the details of {} download.||Copied the details of {} downloads.",
                    count as u64,
                    &[&count],
                ));
                cx.notify();
            });
        }
    }

    /// A priority picked for one of a torrent's files.
    fn pick(&mut self, pick: &Pick, cx: &mut Context<Self>) {
        if pick.field == "delete-after" {
            if let Some((_, seconds)) =
                dm_desktop_base::file_info::delete_after_times().get(pick.index)
            {
                self.delete_after = Some(*seconds);
                cx.notify();
            }
            return;
        }
        if pick.field != "priority" {
            return;
        }
        if let (Some(p), Some(chosen)) = (
            self.priorities.get_mut(pick.item),
            PRIORITIES.get(pick.index),
        ) {
            *p = *chosen;
            cx.notify();
        }
    }

    /// Looks again in a moment, while a digest or a ZIP's reading is still
    /// under way.
    fn poll(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(POLL).await;
            let _ = this.update(cx, |this, cx| {
                this.polling = false;
                cx.notify();
            });
        })
        .detach();
    }
}

/// A torrent file's priorities, in the order its pop-up lists them.
const PRIORITIES: [TorrentPriority; 3] = [
    TorrentPriority::High,
    TorrentPriority::Normal,
    TorrentPriority::Low,
];

/// One line of the form: `label` right-aligned in its column, at the top
/// of a value that may wrap.
fn line(label: &str, width: f32, screen: &Screen, value: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .items_start()
        .gap(px(native::LABEL_GAP))
        .child(
            div()
                .w(px(width))
                .flex_none()
                .flex()
                .justify_end()
                .whitespace_nowrap()
                .child(screen.label(label.to_string())),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// A field's line: its label beside the field, as Add URL's are.
fn field_line(
    label: &'static str,
    width: f32,
    screen: &Screen,
    input: &Entity<InputState>,
    on: bool,
    theme: Theme,
) -> gpui::Div {
    native::row(
        Some(t(label)),
        width,
        screen,
        native::field_if(input, label, on, theme),
    )
}

/// A value as said: wrapped, a plain dash where there is nothing to say.
fn said(text: &str, theme: Theme, screen: &Screen) -> AnyElement {
    if text.is_empty() {
        return div()
            .text_color(theme.muted)
            .child(screen.label("—"))
            .into_any_element();
    }
    div()
        .child(screen.label(text.to_string()))
        .into_any_element()
}

impl Render for Properties {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(window.appearance());
        self.screen.begin();
        let screen = self.screen.clone();
        let Some(view) = self.view.upgrade() else {
            return div().into_any_element();
        };
        let (row, settings, backend) = {
            let view = view.read(cx);
            let Some(row) = view.rows.iter().find(|r| r.id == self.id) else {
                // The download went: so does its window.
                window.defer(cx, |window, _| window.remove_window());
                return div().into_any_element();
            };
            (
                Entry::clone(row),
                view.backend.settings(),
                view.backend.clone(),
            )
        };
        let named = view.read(cx).name_shown(&row);
        // A row that has, or will have, a file can be given a time to the Trash.
        let has_file = !matches!(row.state, State::Failed(_) | State::Cancelled);
        let size = dm_desktop_base::rows::shown_size(&row)
            .map(format::bytes)
            .unwrap_or_default();
        let (kind, saved_to) = placed(&row, &settings, &named);
        let failed = matches!(row.state, State::Failed(_));
        let finished = row.state == State::Completed && !dm_desktop_base::rows::gone(&row);
        // Also while it waits to try again: a wrong address is what it most
        // needs then.
        let editable = !matches!(row.state, State::Running | State::Completed)
            || dm_desktop_base::rows::retrying(row.id).is_some();
        let next_step = match &row.state {
            State::Failed(why) => format::next_step(why).map(t),
            _ => None,
        };
        let raw_failure = dm_desktop_base::rows::failure_as_said(&row).map(str::to_string);

        let look = Look::new(theme, screen.clone());
        let width = look.width;
        let torrent = self.torrent(&look, &row, cx);

        // A finished file's digest, worked out beside the window, and a
        // box to check it against one the person was given.
        let mut waiting = false;
        let digest = finished.then(|| self.digest(&look, &row, &backend, &mut waiting, cx));
        let contents = self.contents(&look, &row, finished, &backend, &mut waiting);
        if waiting {
            self.poll(cx);
        }

        let status = dm_desktop_base::rows::status_of(&row);
        let status_value = div()
            .id("status")
            .child(said(&status, theme, &screen))
            .when_some(raw_failure, |d, raw| {
                let raw = SharedString::from(raw);
                d.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(raw.clone()).build(window, cx)
                })
            });
        let body = native::body(theme)
            .child(line(
                t("File name"),
                width,
                &screen,
                said(&named, theme, &screen),
            ))
            .child(line(t("Status"), width, &screen, status_value))
            .when_some(next_step, |d, step| {
                d.child(line("", width, &screen, said(step, theme, &screen)))
            })
            .child(line(t("Size"), width, &screen, said(&size, theme, &screen)))
            .child(line(
                t("Category"),
                width,
                &screen,
                said(&kind, theme, &screen),
            ))
            // Not yet, for one not yet placed.
            .child(line(
                if row.path.is_empty() {
                    t("Will be saved to")
                } else {
                    t("Saved to")
                },
                width,
                &screen,
                said(&saved_to, theme, &screen),
            ))
            .child(line(
                t("Referrer"),
                width,
                &screen,
                said(&row.referrer, theme, &screen),
            ));
        let body = self
            .fields(&look, editable, body)
            .when(has_file, |d| d.child(self.delete_after_line(&look, &row)))
            .children(torrent)
            .children(digest)
            .children(contents)
            .when_some(self.says.clone(), |d, says| {
                d.child(native::row(
                    None,
                    width,
                    &screen,
                    native::note(says, theme.warning, &screen),
                ))
            })
            .child(Self::buttons(&look, finished, failed, &row, cx));
        crate::widgets::native::dialog("properties", t("File properties"), theme, cx)
            .on_action(cx.listener(|this, pick: &Pick, _, cx| this.pick(pick, cx)))
            .child(crate::widgets::fit::fitted(body))
            .into_any_element()
    }
}

/// Opens File properties for row `id`, or brings its window forward where
/// it is open already, with what was typed in it.
pub fn open(view: WeakEntity<Downloads>, id: Id, cx: &mut App) {
    cx.defer(move |cx| open_now(view, id, cx));
}

fn open_now(view: WeakEntity<Downloads>, id: Id, cx: &mut App) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    if let Some(open) = downloads.read(cx).properties_window(id, cx) {
        let _ = open.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let Some(row) = downloads
        .read(cx)
        .rows
        .iter()
        .find(|r| r.id == id)
        .map(|r| Entry::clone(r))
    else {
        return;
    };
    // Each further one a step down and across.
    let nth = downloads.read(cx).properties_open(cx);
    let (width, height) = crate::widgets::native::WindowSize::Properties.here();
    let mut bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    bounds.origin.x += px(24.0 * nth as f32);
    bounds.origin.y += px(24.0 * nth as f32);
    let watched = downloads.clone();
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("File properties").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            let (address, login, password) = row_edits::split_login(&row.url);
            let filled = |value: String, masked: bool, window: &mut Window, cx: &mut App| {
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx).masked(masked);
                    input.set_value(value, window, cx);
                    input
                })
            };
            let address = filled(address, false, window, cx);
            let login = filled(login, false, window, cx);
            let password = filled(password, true, window, cx);
            let note = filled(row.description.clone(), false, window, cx);
            let (own_agent, own_headers) = row_edits::split_headers(&row.headers);
            let agent = filled(own_agent, false, window, cx);
            let headers = cx.new(|cx| {
                let mut input =
                    InputState::new(window, cx).placeholder(t("Name: value | Name: value"));
                input.set_value(own_headers, window, cx);
                input
            });
            let check =
                cx.new(|cx| InputState::new(window, cx).placeholder(t("A digest you were given")));
            // A server that wants a sign-in: the keyboard goes to Login;
            // else to the address where it can be changed, else the note.
            let wants_login = dm_desktop_base::rows::needs_sign_in(&row);
            let editable = !matches!(row.state, State::Running | State::Completed)
                || dm_desktop_base::rows::retrying(row.id).is_some();
            let first = match (editable, wants_login) {
                (true, true) => &login,
                (true, false) => &address,
                _ => &note,
            };
            first.update(cx, |f, cx| f.focus(window, cx));
            let properties = cx.new(|cx| {
                // Drawn again as the download moves on.
                cx.observe(&watched, |_, _, cx| cx.notify()).detach();
                for input in [&address, &login, &password, &agent, &headers, &note] {
                    cx.subscribe_in(
                        input,
                        window,
                        |this: &mut Properties, _, event, window, cx| match event {
                            InputEvent::PressEnter { .. } => this.ok(window, cx),
                            InputEvent::Change if this.says.take().is_some() => cx.notify(),
                            _ => {}
                        },
                    )
                    .detach();
                }
                cx.subscribe_in(&check, window, |_: &mut Properties, _, event, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
                .detach();
                Properties {
                    view,
                    id,
                    address,
                    login,
                    password,
                    note,
                    agent,
                    headers,
                    check,
                    files: row.torrent_files.iter().map(|f| f.wanted).collect(),
                    priorities: row.torrent_files.iter().map(|f| f.priority).collect(),
                    sums: Sums::default(),
                    zip: None,
                    says: None,
                    delete_after: None,
                    polling: false,
                    done: false,
                    screen,
                }
            });
            crate::widgets::native::close_is_cancel(&properties, window, cx);
            cx.new(|cx| gpui_component::Root::new(properties, window, cx))
        },
    );
    match opened {
        Ok(handle) => {
            downloads.update(cx, |downloads, _| {
                downloads.properties_opened(id, handle.into())
            });
        }
        Err(e) => dm_core::note!(error, "dm-gpui: File properties could not be opened: {e}"),
    }
}

impl crate::widgets::native::CloseButton for Properties {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
