// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! File properties' sections, each a piece of the window's form that
//! `render` puts together in its order.

use dm_shell::app::backend::Backend;
use dm_shell::settings::Settings;

use super::*;

/// What each section is drawn with: the look in force, the screen its words
/// are recorded on, and the labels' column.
pub(super) struct Look {
    pub(super) theme: Theme,
    pub(super) screen: Screen,
    pub(super) width: f32,
}

impl Look {
    /// The sections' look, with `screen` to record their words on.
    pub(super) fn new(theme: Theme, screen: Screen) -> Self {
        let labels = [
            t("File name"),
            t("Status"),
            t("Size"),
            t("Category"),
            t("Will be saved to"),
            t("Saved to"),
            t("Referrer"),
            t("Address"),
            t("User name"),
            t("Password"),
            "User-Agent",
            t("Request headers"),
            t("Note"),
            t("Delete file after"),
            t("Files"),
            "SHA-256",
            t("Check against"),
            t("Contents"),
        ];
        let width = native::label_width(&labels);
        Self {
            theme,
            screen,
            width,
        }
    }
}

/// The row's category as the tree places it, and where it is (or will be)
/// saved, as `named`.
pub(super) fn placed(row: &Entry, settings: &Settings, named: &str) -> (String, String) {
    // As the tree places it: a person's own category, the kind, or
    // Other for a kind not yet known.
    let mut categories = settings.categories();
    let kind = match (
        dm_shell::app::places::custom_of(row, &categories),
        dm_shell::app::names::kind_of(row),
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
                    row,
                    &settings.folders.download_dir,
                    &mut categories,
                )
            })
            .map(|f| std::path::Path::new(&f).join(named).display().to_string())
            .unwrap_or_default()
    } else {
        row.path.clone()
    };
    (kind, saved_to)
}

impl Properties {
    /// A torrent's files, each with a tick (and, beside others, when it is
    /// fetched): ticks only while it is stopped.
    pub(super) fn torrent(
        &mut self,
        look: &Look,
        row: &Entry,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let (theme, screen, width) = (look.theme, &look.screen, look.width);
        if self.files.len() != row.torrent_files.len() {
            self.files = row.torrent_files.iter().map(|f| f.wanted).collect();
        }
        if self.priorities.len() != row.torrent_files.len() {
            self.priorities = row.torrent_files.iter().map(|f| f.priority).collect();
        }
        (!row.torrent_files.is_empty()).then(|| {
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
                        native::checkbox(("torrent-file", i), words, on, theme, screen).on_click(
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
                                screen,
                            ),
                        ))
                    })
            });
            line(
                t("Files"),
                width,
                screen,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::XS))
                    .child(native::enabled(
                        can,
                        div()
                            .id("torrent-files")
                            .max_h(px(160.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(space::XS))
                            .children(files),
                    ))
                    .when(matches!(row.state, State::Running | State::Queued), |d| {
                        d.child(native::note(
                            t("Stop the download to change which files are fetched."),
                            theme.muted,
                            screen,
                        ))
                    }),
            )
        })
    }

    /// A finished file's digest, and the box to check it against one the
    /// person was given; `waiting` is set while either is worked out.
    pub(super) fn digest(
        &self,
        look: &Look,
        row: &Entry,
        backend: &Backend,
        waiting: &mut bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        use dm_core::{Algorithm, Checksum};
        let (theme, screen, width) = (look.theme, &look.screen, look.width);
        dm_shell::app::files::hash_later(backend, &self.sums, &row.path, Algorithm::Sha256);
        // The file now against its digest kept as it finished (gaps row H5).
        let since =
            match (
                dm_shell::app::files::sum_of(&self.sums, Algorithm::Sha256),
                row.kept_sha256.as_deref(),
            ) {
                (Some(Ok(now)), Some(kept)) if now.eq_ignore_ascii_case(kept) => Some(
                    native::note(t("Unchanged since it finished."), theme.muted, screen),
                ),
                (Some(Ok(_)), Some(_)) => Some(native::note(
                    t("Changed since it finished: its SHA-256 then was another."),
                    theme.warning,
                    screen,
                )),
                _ => None,
            };
        let sha = match dm_shell::app::files::sum_of(&self.sums, Algorithm::Sha256) {
            None => {
                *waiting = true;
                native::note(t("Working it out…"), theme.muted, screen).into_any_element()
            }
            // Text to select and copy, as the system's labels can be,
            // in a fixed-width face, which a digest reads best in.
            Some(Ok(hex)) => div()
                // Held to the values' column, so it wraps rather than
                // running past the window's edge.
                .w_full()
                .min_w_0()
                .font_family(crate::widgets::native::mono(cx))
                .text_size(px(native::SMALL_TEXT))
                // Recorded as drawn, as a label is: read and found by
                // its words like the rest of the window's.
                .child(screen.mark(hex.clone(), gpui_base::SelectableText::new("sha-256", hex)))
                .into_any_element(),
            Some(Err(why)) => native::note(why, theme.warning, screen).into_any_element(),
        };
        let checked = Checksum::parse(&self.check.read(cx).value()).map(|given| {
            dm_shell::app::files::hash_later(backend, &self.sums, &row.path, given.algorithm);
            match dm_shell::app::files::sum_of(&self.sums, given.algorithm) {
                None => {
                    *waiting = true;
                    native::note(t("Working it out…"), theme.muted, screen)
                }
                Some(Ok(found)) if found.eq_ignore_ascii_case(&given.expected) => div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(screen.label(t("It matches."))),
                Some(Ok(_)) => native::note(t("It does not match."), theme.warning, screen),
                Some(Err(why)) => native::note(why, theme.warning, screen),
            }
        });
        div()
            .flex()
            .flex_col()
            .gap(px(native::ROW_GAP))
            .child(native::row_wide(Some("SHA-256"), width, screen, sha))
            .when_some(since, |d, since| {
                d.child(native::row_wide(None, width, screen, since))
            })
            .child(native::row_wide(
                Some(t("Check against")),
                width,
                screen,
                native::field(&self.check, "Check against", theme),
            ))
            .when_some(checked, |d, checked| {
                d.child(native::row(None, width, screen, checked))
            })
    }

    /// A finished ZIP's files, by name and size, read from the file beside
    /// the window: text only, never opened. `waiting` is set while it is read.
    pub(super) fn contents(
        &mut self,
        look: &Look,
        row: &Entry,
        finished: bool,
        backend: &Backend,
        waiting: &mut bool,
    ) -> Option<gpui::Div> {
        let (theme, screen, width) = (look.theme, &look.screen, look.width);
        if finished && self.zip.is_none() && zip_list::is_zip_address(&row.path) {
            self.zip = Some(dm_shell::app::files::list_zip_file(
                backend,
                std::path::PathBuf::from(&row.path),
                || {},
            ));
        }
        match zip_list::listing(&self.zip) {
            Listing::Nothing => None,
            Listing::Reading => {
                *waiting = true;
                Some(line(
                    t("Contents"),
                    width,
                    screen,
                    native::note(zip_list::reading_words(), theme.muted, screen),
                ))
            }
            Listing::Files(files, more) => Some(line(
                t("Contents"),
                width,
                screen,
                crate::widgets::ziplist::zip_box(
                    files.iter().enumerate().map(|(i, file)| {
                        crate::widgets::ziplist::zip_line(i, file, theme, screen).into_any_element()
                    }),
                    more,
                    theme,
                    screen,
                ),
            )),
        }
    }

    /// The fields OK keeps, under the lines that say what the download is.
    pub(super) fn fields(&self, look: &Look, editable: bool, d: gpui::Div) -> gpui::Div {
        let (theme, screen, width) = (look.theme, &look.screen, look.width);
        // A line between what the download is and what may be changed, as the Grabber divides
        // its search from its saving: the facts ran straight into the fields.
        d.child(
            div()
                .h(px(crate::widgets::space::LINE))
                .my(px(crate::widgets::space::XS))
                .bg(theme.separator),
        )
        .child(field_line(
            "Address",
            width,
            screen,
            &self.address,
            editable,
            theme,
        ))
        .child(field_line(
            "User name",
            width,
            screen,
            &self.login,
            editable,
            theme,
        ))
        .child(field_line(
            "Password",
            width,
            screen,
            &self.password,
            editable,
            theme,
        ))
        .child(field_line(
            "User-Agent",
            width,
            screen,
            &self.agent,
            editable,
            theme,
        ))
        .child(field_line(
            "Request headers",
            width,
            screen,
            &self.headers,
            editable,
            theme,
        ))
        .child(field_line("Note", width, screen, &self.note, true, theme))
    }

    /// "Delete file after", with the time chosen here or the row's own.
    pub(super) fn delete_after_line(&self, look: &Look, row: &Entry) -> gpui::Div {
        let (theme, screen, width) = (look.theme, &look.screen, look.width);
        let times = dm_desktop_base::file_info::delete_after_times();
        let now = self.delete_after.unwrap_or(row.delete_after);
        let at = times.iter().position(|(_, s)| *s == now);
        let words = at
            .and_then(|i| times.get(i))
            .map_or(t("Never"), |(w, _)| *w);
        let picker = popup::button(
            "delete-after",
            words.into(),
            times.iter().map(|(w, _)| SharedString::from(*w)).collect(),
            at,
            theme,
            screen,
        );
        // A control, named as the fields above it are, not as a detail, and its own width: as a
        // detail's value it stretched across the window (seen in ui-shot.py's picture).
        native::row(Some(t("Delete file after")), width, screen, picker)
    }

    /// Open and Copy details at the leading side; Cancel, and OK rightmost,
    /// as on a Mac.
    pub(super) fn buttons(
        look: &Look,
        finished: bool,
        failed: bool,
        row: &Entry,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let (theme, screen) = (look.theme, &look.screen);
        let path = row.path.clone();
        let row_for_details = row.clone();
        native::buttons(
            // Its page of the guide first (gaps row E11).
            vec![native::help_button("properties", theme, screen).into_any_element()]
                .into_iter()
                // Open once the file is there: greyed before, it said nothing of why, and a greyed
                // button takes no pointer to say it on (the ui-skills rule, gaps row E50).
                .chain(finished.then(|| {
                    native::button("open", t("Open").into(), false, theme, screen)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.open_file(path.clone(), cx)),
                        )
                        .into_any_element()
                }))
                .chain(failed.then(|| {
                    native::button(
                        "copy-details",
                        t("Copy details").into(),
                        false,
                        theme,
                        screen,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.copy_details(&row_for_details, cx)),
                    )
                    .into_any_element()
                }))
                .collect(),
            vec![
                native::button("cancel", t("Cancel").into(), false, theme, screen)
                    .on_click(cx.listener(|this, _, window, _| this.cancel(window)))
                    .into_any_element(),
                native::button("ok", t("Save").into(), true, theme, screen)
                    .on_click(cx.listener(|this, _, window, cx| this.ok(window, cx)))
                    .into_any_element(),
            ],
        )
    }
}
