// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details' sections, top to bottom, each a piece of the dialog's
//! form that `render` puts together in this order.

use super::*;

mod no_record;

/// What each section is drawn with: the look in force, the screen its words
/// are recorded on, and the labels' column.
pub(super) struct Look {
    theme: Theme,
    screen: Screen,
    width: f32,
}

impl Look {
    /// The sections' look, with `screen` to record their words on.
    pub(super) fn new(theme: Theme, screen: Screen) -> Self {
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
            t("Queue"),
        ]);
        Self {
            theme,
            screen,
            width,
        }
    }

    /// A row of the form: `label`'s words in the labels' column, `control` beside them.
    fn row(&self, label: &'static str, control: gpui::AnyElement) -> gpui::Div {
        native::row(Some(t(label)), self.width, &self.screen, control)
    }

    /// A row with no label: `control` under the one above.
    fn under(&self, control: gpui::AnyElement) -> gpui::Div {
        native::row(None, self.width, &self.screen, control)
    }
}

impl Details {
    /// What the Size row says, and whether the file will not fit where it goes.
    pub(super) fn size_said(&mut self, page: bool, cx: &mut Context<Self>) -> (bool, SharedString) {
        // "Checking…" while the server is asked, then the size beside what the folder has free
        // (in the warning colour where it will not fit), or that it is unknown (gaps row U17).
        let folder_now = self.folder.read(cx).value().trim().to_string();
        if self
            .free
            .as_ref()
            .is_none_or(|(asked, _)| *asked != folder_now)
        {
            let dir = if folder_now.is_empty() {
                self.settings.folders.download_dir.clone()
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
        (will_not_fit, size_words)
    }

    /// The Quality row. A video's qualities: each its class and container over its size,
    /// the one the setting would take picked until another is.
    pub(super) fn quality_row(
        &self,
        look: &Look,
        video: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let (theme, screen) = (look.theme, &look.screen);
        video.then(|| {
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
                    let setting = dm_core::Quality::named(&self.settings.downloads.stream_quality);
                    let picked = self
                        .quality
                        .clone()
                        .or_else(|| file_info::default_choice(list, setting));
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(space::S))
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
                                .map(|c| crate::widgets::native::ringed(c, theme))
                                .role(Role::RadioButton)
                                .aria_label(words.replace('\n', " "))
                                .px(px(space::M_L))
                                .py(px(space::XS))
                                // A finger's least on a phone (gaps row E26).
                                .when(crate::platform::ON_A_PHONE, |d| {
                                    d.min_h(px(native::CONTROL)).justify_center()
                                })
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
            look.row("Quality", control)
        })
    }

    /// The Contents row: a ZIP's files, by name and size: text only, never opened.
    pub(super) fn contents_row(&self, look: &Look, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let (theme, screen) = (look.theme, &look.screen);
        match zip_list::listing(&self.zip) {
            Listing::Nothing => None,
            Listing::Reading => Some(
                look.row(
                    "Contents",
                    div()
                        .text_color(theme.muted)
                        .child(screen.label(zip_list::reading_words()))
                        .into_any_element(),
                ),
            ),
            Listing::Files(files, more) => {
                let taken = zip_list::taken_words(&self.taken, &self.taking);
                Some(
                    look.row(
                        "Contents",
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(space::XS))
                            .child(
                                div()
                                    .id("zip-contents")
                                    .max_h(px(140.0))
                                    .overflow_y_scroll()
                                    .rounded(px(native::RADIUS))
                                    .border_1()
                                    .border_color(theme.separator)
                                    .bg(theme.background)
                                    .p(px(space::XS))
                                    .children(files.into_iter().enumerate().map(|(i, file)| {
                                        let takeable = !file.is_folder()
                                            && file.packed <= dm_core::ZIP_MOST_TAKEN;
                                        let size =
                                            (!file.is_folder()).then(|| format::bytes(file.size));
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(space::M))
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
                                                        .map(|c| {
                                                            crate::widgets::native::ringed(c, theme)
                                                        })
                                                        .role(Role::Button)
                                                        .aria_label(t("Take out"))
                                                        .text_color(theme.accent_text)
                                                        .child(screen.label(t("Take out")))
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.take_out(
                                                                    file.clone(),
                                                                    window,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
                                                )
                                            })
                                    }))
                                    .when(more > 0, |d| {
                                        d.child(native::note(
                                            tf("and {} more", &[&more.to_string()]),
                                            theme.muted,
                                            screen,
                                        ))
                                    }),
                            )
                            .when_some(taken, |d, words| {
                                d.child(native::note(words, theme.muted, screen))
                            })
                            .into_any_element(),
                    ),
                )
            }
        }
    }

    /// The Part rows: a part of the video, from one moment to another: cut from the
    /// streams dm fetches itself, which today are a YouTube video's.
    pub(super) fn part_rows(
        &self,
        look: &Look,
        first: &str,
        youtube: bool,
        cx: &mut Context<Self>,
    ) -> Option<Vec<gpui::Div>> {
        let (theme, screen) = (look.theme, &look.screen);
        (youtube || self.handed.handed || self.part).then(|| {
            let starts = dm_core::youtube::start_of(first);
            let part = self.part;
            let gif_possible = dm_core::gif::possible_for(first);
            let mut rows = vec![look.row(
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
                        native::switch_inline(
                            "part",
                            t("Only a part of the video").into(),
                            part,
                            theme,
                            screen,
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
                            screen,
                        ))
                        .child(
                            native::button(
                                "from-here",
                                tf("From {}", &[&at]).into(),
                                false,
                                theme,
                                screen,
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
                self.shape_rows(look, gif_possible, &mut rows, cx);
            }
            rows
        })
    }

    // The part's Shape row, and Part as where it can be a GIF.
    fn shape_rows(
        &self,
        look: &Look,
        gif_possible: bool,
        rows: &mut Vec<gpui::Div>,
        cx: &mut Context<Self>,
    ) {
        let (theme, screen) = (look.theme, &look.screen);
        // The part's shape: as it is, or the middle of the picture
        // made vertical or square, nothing re-encoded.
        use dm_core::shape::Shape;
        let shape = self.shape;
        rows.push(
            look.row(
                "Shape",
                div()
                    .flex()
                    .gap(px(space::XS))
                    .children(
                        [
                            (None, t("As it is")),
                            (Some(Shape::Vertical), t("Vertical 9:16")),
                            (Some(Shape::Square), t("Square 1:1")),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(i, (choice, words))| {
                            segment(("shape", i), words.into(), shape == choice, theme, screen)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.shape = choice;
                                    cx.notify();
                                }))
                        }),
                    )
                    .into_any_element(),
            ),
        );
        // As a GIF: moving pictures with no sound.
        if gif_possible {
            let gif = self.gif;
            rows.push(
                look.row(
                    "Part as",
                    div()
                        .flex()
                        .gap(px(space::XS))
                        .child(
                            segment("as-video", t("Video").into(), !gif, theme, screen).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.gif = false;
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            segment("as-gif", "GIF".into(), gif, theme, screen).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.gif = true;
                                    cx.notify();
                                }),
                            ),
                        )
                        .into_any_element(),
                ),
            );
        }
    }

    /// The address, where a handed-over download came from, and the line under them.
    pub(super) fn address(&self, look: &Look, first: &str, body: gpui::Div) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        body
            // The address, as the window's subtitle says it.
            .child(
                div()
                    .text_color(theme.muted)
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .overflow_hidden()
                    .child(screen.label(format::shown_url(first))),
            )
            // Its host copies another in letters of another alphabet (gaps row H7).
            .children(
                crate::dialogs::addurl::lookalike_said(first)
                    .map(|said| native::note(said, theme.warning, screen)),
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
            .child(
                div()
                    .h(px(space::LINE))
                    .my(px(space::XS))
                    .bg(theme.separator),
            )
    }

    /// Where it goes and its name: Category, Save to folder, Always save here, Save as.
    pub(super) fn place(
        &self,
        look: &Look,
        folder: &str,
        single: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        body: gpui::Div,
    ) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
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
        body.child(
            look.row(
                "Category",
                popup::button(
                    "category",
                    category_words,
                    categories,
                    category_at,
                    theme,
                    screen,
                )
                .into_any_element(),
            ),
        )
        .child(
            look.row(
                "Save to folder",
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(div().flex_1().min_w_0().child(native::path_field(
                        &self.folder,
                        "Save to folder",
                        &file_info::default_folder_words(&self.settings),
                        gpui::Focusable::focus_handle(self.folder.read(cx), cx).is_focused(window),
                        theme,
                        cx,
                    )))
                    .child(
                        native::button("browse", t("Browse…").into(), false, theme, screen)
                            .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                    )
                    .into_any_element(),
            ),
        )
        // A download the browser caught: the next ones can go
        // straight to their folder.
        .when(self.handed.caught, |d| {
            let on = self.always_here;
            d.child(
                look.under(
                    native::switch_inline(
                        "always-here",
                        t("Always save here without asking").into(),
                        on,
                        theme,
                        screen,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.always_here = !this.always_here;
                        cx.notify();
                    }))
                    .into_any_element(),
                ),
            )
        })
        .when(single, |d| {
            d.child(
                look.row(
                    "Save as",
                    div()
                        .flex_1()
                        .child(native::field(&self.name, "Save as", theme))
                        .into_any_element(),
                ),
            )
        })
    }

    /// Size, Note, Delete file after, and the quality the browser's panel picked.
    pub(super) fn facts(
        &self,
        look: &Look,
        single: bool,
        video: bool,
        size: (bool, SharedString),
        body: gpui::Div,
    ) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        let (will_not_fit, size_words) = size;
        let times = file_info::delete_after_times();
        let time_at = times
            .iter()
            .position(|(_, s)| *s == self.delete_after)
            .unwrap_or(0);
        body.when(single, |d| {
            d.child(
                look.row(
                    "Size",
                    div()
                        .text_color(if will_not_fit {
                            theme.warning
                        } else {
                            theme.muted
                        })
                        .child(screen.label(size_words))
                        .into_any_element(),
                ),
            )
        })
        .child(
            look.row(
                "Note",
                div()
                    .flex_1()
                    .child(native::field(&self.note, "Note", theme))
                    .into_any_element(),
            ),
        )
        .child(
            look.row(
                "Delete file after",
                popup::button(
                    "delete-after",
                    times[time_at].0.into(),
                    times.iter().map(|(w, _)| SharedString::from(*w)).collect(),
                    Some(time_at),
                    theme,
                    screen,
                )
                .into_any_element(),
            ),
        )
        // The quality picked in the browser's panel, where the
        // dialog lists none of its own.
        .when_some(self.label.clone().filter(|_| !video), |d, label| {
            d.child(look.row(
                "Quality",
                div().child(screen.label(label)).into_any_element(),
            ))
        })
    }

    /// What is said under the form: the name's clash, the server's problem, a word.
    pub(super) fn warnings(
        &self,
        look: &Look,
        clash: Option<Clash>,
        problem: Option<String>,
        cx: &mut Context<Self>,
        body: gpui::Div,
    ) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        body.when_some(clash, |d, clash| match clash {
            Clash::Listed => d.child(
                look.under(
                    native::note(
                        t("A download with this name exists. This one will be numbered."),
                        theme.warning,
                        screen,
                    )
                    .into_any_element(),
                ),
            ),
            Clash::OnDisk { free, .. } => {
                let replace = self.replace;
                d.child(
                    look.under(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(space::S))
                            .child(native::note(
                                t("A file of that name is in the folder already."),
                                theme.warning,
                                screen,
                            ))
                            .child(
                                native::radio(
                                    "keep-both",
                                    tf("Save this one as {}", &[&free]).into(),
                                    !replace,
                                    theme,
                                    screen,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.replace = false;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                native::radio(
                                    "replace",
                                    t("Replace that file once this one is whole").into(),
                                    replace,
                                    theme,
                                    screen,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.replace = true;
                                        cx.notify();
                                    },
                                )),
                            )
                            .into_any_element(),
                    ),
                )
            }
        })
        .when_some(problem, |d, why| {
            d.child(
                look.under(
                    native::note(file_info::problem_words(&why), theme.warning, screen)
                        .into_any_element(),
                ),
            )
        })
        .map(
            |d| match native::says("details-says", self.says.clone(), theme, screen) {
                says if self.says.is_some() => d.child(look.under(says.into_any_element())),
                says => d.child(says),
            },
        )
    }

    /// The queue Add for later puts it in, a line of the form (gaps row E10): it was a word
    /// "to" and a pop-up among the buttons.
    pub(super) fn queue_row(&self, look: &Look) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        let queues: Vec<SharedString> = self
            .queues
            .iter()
            .map(|q| dm_desktop_base::i18n::name(q).into())
            .collect();
        let queue_at = self.queues.iter().position(|q| *q == self.queue);
        let queue_words = SharedString::from(dm_desktop_base::i18n::name(&self.queue));
        look.row(
            "Queue",
            popup::button("queue", queue_words, queues, queue_at, theme, screen).into_any_element(),
        )
    }

    /// The foot (gaps row E10): Cancel, Add for later, then Start now, the one filled.
    pub(super) fn buttons(&self, look: &Look, cx: &mut Context<Self>) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        native::buttons(
            // Its page of the guide (gaps row E11).
            vec![native::help_button("download-details", theme, screen).into_any_element()],
            vec![
                native::button("cancel", t("Cancel").into(), false, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx)))
                    .into_any_element(),
                native::button("later", t("Add for later").into(), false, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.add(true, window, cx)))
                    .into_any_element(),
                native::button("start", t("Start now").into(), true, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.add(false, window, cx)))
                    .into_any_element(),
            ],
        )
    }
}

/// Unpack when done, for this download alone (gaps row W14).
impl Details {
    /// Its box, under the form's facts, for an archive dm unpacks (by the name in Save as or
    /// in an address), or one ticked already: ticked, the archive is unpacked into a folder of
    /// its name once it finishes, whatever its kind's choice in Options › Folders.
    pub(super) fn unpack_box(
        &self,
        look: &Look,
        cx: &mut Context<Self>,
        body: gpui::Div,
    ) -> gpui::Div {
        let archive = |text: &str| {
            let path = text.split(['?', '#']).next().unwrap_or(text);
            dm_core::extract::is_archive(path.trim())
        };
        let name = self.name.read(cx).value().to_string();
        let any_url = self.urls.iter().any(|u| archive(u.as_str()));
        let shown = self.unpack || archive(name.as_str()) || any_url;
        if !shown {
            return body;
        }
        let (theme, screen) = (look.theme, &look.screen);
        let words: SharedString = t("Unpack when done").into();
        let check = native::switch_inline("unpack", words, self.unpack, theme, screen).on_click(
            cx.listener(|this, _, _, cx| {
                this.unpack = !this.unpack;
                cx.notify();
            }),
        );
        body.child(look.under(check.into_any_element()))
    }
}
