// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! File properties, a window for each download, with the egui window's
//! words: its name, status and next step, size, category and where it is
//! (or will be) saved, the page it came from; its address, sign-in and
//! note, which OK keeps; a torrent's files with their priorities; a
//! finished file's SHA-256 and a box to check it against a digest the
//! person was given; a finished ZIP's files (B44's Properties half). What
//! OK keeps is dm's (`dm_desktop_base::row_edits::keep_properties`).

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

use crate::list::Downloads;
use crate::native;
use crate::popup::{self, Pick};
use crate::screen::{Screen, Screens};
use crate::theme::Theme;
use crate::zoom::px;

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
    /// A digest the person was given, to check the file against.
    check: Entity<InputState>,
    /// A torrent's files ticked to be fetched, as edited.
    files: Vec<bool>,
    /// When each of a torrent's files is fetched, as edited.
    priorities: Vec<TorrentPriority>,
    sums: Sums,
    zip: Option<ZipSlot>,
    says: Option<SharedString>,
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
        let (files, priorities) = (self.files.clone(), self.priorities.clone());
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
                    row_edits::keep_properties(
                        backend,
                        &kept_row,
                        &Edited {
                            description: &description,
                            address: &address,
                            files: &files,
                            priorities: &priorities,
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
            let refused = view.scripted.run_refused();
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
        let size = dm_desktop_base::rows::shown_size(&row)
            .map(format::bytes)
            .unwrap_or_default();
        // As the tree places it: a person's own category, the kind, or
        // Other for a kind not yet known.
        let mut categories = settings.categories();
        let kind = match (
            dm_shell::app::places::custom_of(&row, &categories),
            dm_shell::app::names::kind_of(&row),
        ) {
            (Some(own), _) => own,
            (None, kind) => dm_desktop_base::i18n::name(&format::titled(
                &dm_shell::app::names::category_word(kind.unwrap_or(dm_core::Category::Other)),
            )),
        };
        let saved_to = if row.path.is_empty() {
            row.save_dir
                .clone()
                .filter(|d| !d.trim().is_empty())
                .or_else(|| {
                    dm_shell::app::names::planned_folder(
                        &row,
                        &settings.download_dir,
                        &mut categories,
                    )
                })
                .map(|f| std::path::Path::new(&f).join(&named).display().to_string())
                .unwrap_or_default()
        } else {
            row.path.clone()
        };
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

        let labels = [
            t("File name"),
            t("Status"),
            t("Size"),
            t("Category"),
            t("Will be saved to"),
            t("Saved to"),
            t("Referrer"),
            t("Address"),
            t("Login"),
            t("Password"),
            t("Note"),
            t("Files"),
            "SHA-256",
            t("Check against"),
            t("Contents"),
        ];
        let width = native::label_width(&labels);

        // A torrent's files, each with a tick (and, beside others, when
        // it is fetched): ticks only while it is stopped.
        if self.files.len() != row.torrent_files.len() {
            self.files = row.torrent_files.iter().map(|f| f.wanted).collect();
        }
        if self.priorities.len() != row.torrent_files.len() {
            self.priorities = row.torrent_files.iter().map(|f| f.priority).collect();
        }
        let torrent = (!row.torrent_files.is_empty()).then(|| {
            let can = !matches!(row.state, State::Running | State::Queued | State::Completed);
            let several = row.torrent_files.len() > 1;
            let files = row.torrent_files.iter().enumerate().map(|(i, file)| {
                let on = self.files.get(i).copied().unwrap_or(file.wanted);
                let priority = self.priorities.get(i).copied().unwrap_or(file.priority);
                let words: SharedString =
                    format!("{} ({})", file.name, format::bytes(file.size)).into();
                div()
                    .flex()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(
                        native::checkbox(("torrent-file", i), words, on, theme, &screen).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if let Some(w) = this.files.get_mut(i) {
                                    *w = !*w;
                                }
                                cx.notify();
                            }),
                        ),
                    )
                    .when(several, |d| {
                        let choices = PRIORITIES
                            .iter()
                            .map(|p| SharedString::from(row_edits::priority_words(*p)))
                            .collect();
                        let at = PRIORITIES.iter().position(|p| *p == priority);
                        d.child(native::enabled(
                            on,
                            popup::button_of(
                                "priority",
                                i,
                                row_edits::priority_words(priority).into(),
                                choices,
                                at,
                                theme,
                                &screen,
                            ),
                        ))
                    })
            });
            line(
                t("Files"),
                width,
                &screen,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(native::enabled(
                        can,
                        div()
                            .id("torrent-files")
                            .max_h(px(160.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .children(files),
                    ))
                    .when(matches!(row.state, State::Running | State::Queued), |d| {
                        d.child(native::note(
                            t("Stop the download to change which files are fetched."),
                            theme.muted,
                            &screen,
                        ))
                    }),
            )
        });

        // A finished file's digest, worked out beside the window, and a
        // box to check it against one the person was given.
        let mut waiting = false;
        let digest = finished.then(|| {
            use dm_core::{Algorithm, Checksum};
            dm_shell::app::files::hash_later(&backend, &self.sums, &row.path, Algorithm::Sha256);
            let sha = match dm_shell::app::files::sum_of(&self.sums, Algorithm::Sha256) {
                None => {
                    waiting = true;
                    native::note(t("Working it out…"), theme.muted, &screen).into_any_element()
                }
                // Text to select and copy, as the system's labels can be,
                // in a fixed-width face, which a digest reads best in.
                Some(Ok(hex)) => div()
                    // Held to the values' column, so it wraps rather than
                    // running past the window's edge.
                    .w_full()
                    .min_w_0()
                    .font_family(crate::native::mono(cx))
                    .text_size(px(native::SMALL_TEXT))
                    // Recorded as drawn, as a label is: read and found by
                    // its words like the rest of the window's.
                    .child(screen.mark(hex.clone(), gpui_base::SelectableText::new("sha-256", hex)))
                    .into_any_element(),
                Some(Err(why)) => native::note(why, theme.warning, &screen).into_any_element(),
            };
            let checked = Checksum::parse(&self.check.read(cx).value()).map(|given| {
                dm_shell::app::files::hash_later(&backend, &self.sums, &row.path, given.algorithm);
                match dm_shell::app::files::sum_of(&self.sums, given.algorithm) {
                    None => {
                        waiting = true;
                        native::note(t("Working it out…"), theme.muted, &screen)
                    }
                    Some(Ok(found)) if found.eq_ignore_ascii_case(&given.expected) => div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(screen.label(t("It matches."))),
                    Some(Ok(_)) => native::note(t("It does not match."), theme.warning, &screen),
                    Some(Err(why)) => native::note(why, theme.warning, &screen),
                }
            });
            div()
                .flex()
                .flex_col()
                .gap(px(native::ROW_GAP))
                .child(native::row(Some("SHA-256"), width, &screen, sha))
                .child(native::row(
                    Some(t("Check against")),
                    width,
                    &screen,
                    native::field(&self.check, "Check against", theme),
                ))
                .when_some(checked, |d, checked| {
                    d.child(native::row(None, width, &screen, checked))
                })
        });

        // A finished ZIP's files, by name and size, read from the file
        // beside the window: text only, never opened.
        if finished && self.zip.is_none() && zip_list::is_zip_address(&row.path) {
            self.zip = Some(dm_shell::app::files::list_zip_file(
                &backend,
                std::path::PathBuf::from(&row.path),
                || {},
            ));
        }
        let contents = match zip_list::listing(&self.zip) {
            Listing::Nothing => None,
            Listing::Reading => {
                waiting = true;
                Some(line(
                    t("Contents"),
                    width,
                    &screen,
                    native::note(zip_list::reading_words(), theme.muted, &screen),
                ))
            }
            Listing::Files(files, more) => Some(line(
                t("Contents"),
                width,
                &screen,
                div()
                    .id("zip-contents")
                    .max_h(px(140.0))
                    .overflow_y_scroll()
                    .rounded(px(native::RADIUS))
                    .border_1()
                    .border_color(theme.separator)
                    .bg(theme.background)
                    .p(px(4.0))
                    .children(files.into_iter().map(|file| {
                        let size = (!file.is_folder()).then(|| format::bytes(file.size));
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
                                d.child(div().text_color(theme.muted).child(screen.label(size)))
                            })
                    }))
                    .when(more > 0, |d| {
                        d.child(native::note(
                            tf("and {} more", &[&more.to_string()]),
                            theme.muted,
                            &screen,
                        ))
                    }),
            )),
        };
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
        let path = row.path.clone();
        let row_for_details = row.clone();
        crate::native::dialog("properties", t("File properties"), theme, cx)
            .on_action(cx.listener(|this, pick: &Pick, _, cx| this.pick(pick, cx)))
            .child(crate::fit::fitted(
                native::body(theme)
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
                    ))
                    .child(field_line(
                        "Address",
                        width,
                        &screen,
                        &self.address,
                        editable,
                        theme,
                    ))
                    .child(field_line(
                        "Login",
                        width,
                        &screen,
                        &self.login,
                        editable,
                        theme,
                    ))
                    .child(field_line(
                        "Password",
                        width,
                        &screen,
                        &self.password,
                        editable,
                        theme,
                    ))
                    .child(field_line("Note", width, &screen, &self.note, true, theme))
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
                    // Open and Copy details at the leading side; Cancel, and
                    // OK rightmost, as on a Mac.
                    .child(native::buttons(
                        vec![native::enabled(
                            finished,
                            native::button("open", t("Open").into(), false, theme, &screen)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_file(path.clone(), cx)
                                })),
                        )
                        .into_any_element()]
                        .into_iter()
                        .chain(failed.then(|| {
                            native::button(
                                "copy-details",
                                t("Copy details").into(),
                                false,
                                theme,
                                &screen,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.copy_details(&row_for_details, cx)
                            }))
                            .into_any_element()
                        }))
                        .collect(),
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
    // Each further one a step down and across, as the egui window places
    // them.
    let nth = downloads.read(cx).properties_open(cx);
    let mut bounds = Bounds::centered(None, size(px(560.0), px(420.0)), cx);
    bounds.origin.x += px(24.0 * nth as f32);
    bounds.origin.y += px(24.0 * nth as f32);
    let watched = downloads.clone();
    let opened = crate::screen::open_window(
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
            let check =
                cx.new(|cx| InputState::new(window, cx).placeholder(t("A digest you were given")));
            // A server that wants a sign-in: the keyboard goes to Login;
            // else to the address where it can be changed, else the note.
            let wants_login = matches!(&row.state,
                State::Failed(why) if dm_desktop_base::rows::needs_sign_in(why));
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
                for input in [&address, &login, &password, &note] {
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
                    check,
                    files: row.torrent_files.iter().map(|f| f.wanted).collect(),
                    priorities: row.torrent_files.iter().map(|f| f.priority).collect(),
                    sums: Sums::default(),
                    zip: None,
                    says: None,
                    polling: false,
                    done: false,
                    screen,
                }
            });
            crate::native::close_is_cancel(&properties, window, cx);
            cx.new(|cx| gpui_component::Root::new(properties, window, cx))
        },
    );
    match opened {
        Ok(handle) => {
            downloads.update(cx, |downloads, _| {
                downloads.properties_opened(id, handle.into())
            });
        }
        Err(e) => eprintln!("dm-gpui: File properties could not be opened: {e}"),
    }
}

impl crate::native::CloseButton for Properties {
    fn close_button(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.cancel(window);
    }
}
