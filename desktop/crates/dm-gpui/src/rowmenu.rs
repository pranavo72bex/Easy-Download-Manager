// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The row menu, where a right-click (or Control-click) lands, drawn as a
//! macOS context menu: rounded and shadowed, the item under the pointer in
//! the selection colour, what does not apply to the rows chosen greyed. Its
//! items and their words are the egui window's row menu's.

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::Command;
use gpui::{anchored, deferred, div, prelude::*, Context, Pixels, Point, Role};

use crate::list::Downloads;
use crate::menus::Run;
use crate::screen::Screen;
use crate::theme::Theme;
use crate::zoom::px;

/// The menu's items, in groups, each with the words it shows (the egui row
/// menu's own, where it says them differently from the command's title):
/// Resume as `resume` says it for the rows chosen (Retry, Start now).
fn groups(resume: &'static str) -> Vec<Vec<(Command, &'static str)>> {
    use Command as C;
    let title = |c: Command| t(c.label());
    vec![
        vec![
            (C::OpenFile, title(C::OpenFile)),
            (C::OpenWith, title(C::OpenWith)),
            (C::Preview, title(C::Preview)),
            (C::OpenFolder, title(C::OpenFolder)),
            (C::ExtractHere, title(C::ExtractHere)),
            (C::ExtractTo, title(C::ExtractTo)),
            (C::MoveRename, title(C::MoveRename)),
            (C::Redownload, title(C::Redownload)),
        ],
        vec![
            (C::DownloadNow, t(resume)),
            (C::StopDownload, t("Stop download")),
            (C::UpdateLink, t("Update download link")),
            (C::CopyLink, t("Copy link")),
            (C::CopyDetails, t("Copy details")),
            (C::Remove, t("Delete")),
            (C::DeleteFile, title(C::DeleteFile)),
        ],
        vec![
            (C::MoveToQueue, t("Move to queue")),
            (C::RemoveFromQueue, title(C::RemoveFromQueue)),
            (C::MoveUp, title(C::MoveUp)),
            (C::MoveDown, title(C::MoveDown)),
        ],
        vec![(C::Properties, t("Properties"))],
    ]
}

/// What one entry of the row menu does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    /// A command of the menus', on the rows chosen.
    Run(Command),
    /// "Delete file after" this many seconds (`None`: Never), on the rows that have a file.
    DeleteAfter(Option<u64>),
}

/// One entry: what it does, its words, whether it applies, whether it is ticked, and whether a
/// separator (or, for "Delete file after", that heading) comes before it.
pub struct Entry {
    pub act: Act,
    pub words: &'static str,
    pub enabled: bool,
    pub ticked: bool,
    pub before: Before,
}

/// What stands above an entry.
#[derive(Clone, Copy, PartialEq)]
pub enum Before {
    Nothing,
    Separator,
    Heading(&'static str),
}

/// The row menu's entries for the rows chosen, in order: the egui row menu's groups, then
/// "Delete file after" (gaps row U8) for rows that have, or will have, a file.
pub fn entries(view: &Downloads) -> Vec<Entry> {
    let resume = dm_desktop_base::rows::resume_word(&view.chosen());
    let mut all = Vec::new();
    for (g, group) in groups(resume).into_iter().enumerate() {
        for (n, (command, words)) in group.into_iter().enumerate() {
            all.push(Entry {
                act: Act::Run(command),
                words,
                enabled: view.enabled(command),
                ticked: false,
                before: if g > 0 && n == 0 {
                    Before::Separator
                } else {
                    Before::Nothing
                },
            });
        }
    }
    let chosen = view.chosen();
    let with_file: Vec<_> = chosen
        .iter()
        .filter(|r| {
            !matches!(
                r.state,
                dm_core::State::Failed(_) | dm_core::State::Cancelled
            )
        })
        .collect();
    if !with_file.is_empty() {
        let shared = chosen
            .first()
            .map(|r| r.delete_after)
            .filter(|first| chosen.iter().all(|r| r.delete_after == *first));
        for (n, (words, seconds)) in dm_desktop_base::file_info::delete_after_times()
            .into_iter()
            .enumerate()
        {
            all.push(Entry {
                act: Act::DeleteAfter(seconds),
                words,
                enabled: true,
                ticked: shared == Some(seconds),
                before: if n == 0 {
                    Before::Heading(t("Delete file after"))
                } else {
                    Before::Nothing
                },
            });
        }
    }
    all
}

impl Downloads {
    /// Carries out the row menu's entry `act`, closing the menu: a click on it, or Return with
    /// the keyboard on it (gaps row U6).
    pub fn act_from_menu(&mut self, act: Act, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.close_menu();
        match act {
            Act::Run(command) => window.dispatch_action(Box::new(Run(command)), cx),
            Act::DeleteAfter(seconds) => {
                let targets: Vec<dm_core::Id> = self
                    .chosen()
                    .into_iter()
                    .filter(|r| {
                        !matches!(
                            r.state,
                            dm_core::State::Failed(_) | dm_core::State::Cancelled
                        )
                    })
                    .map(|r| r.id)
                    .collect();
                for id in targets {
                    self.backend.set_delete_after(id, seconds);
                }
                self.read();
            }
        }
        cx.notify();
    }

    /// The keyboard's place in the open row menu moved by `by` entries, over those that apply,
    /// round from one end to the other as a system menu's.
    pub fn step_menu(&mut self, by: isize) {
        let all = entries(self);
        let usable: Vec<usize> = (0..all.len()).filter(|&i| all[i].enabled).collect();
        if usable.is_empty() {
            return;
        }
        let at = self
            .menu_at
            .and_then(|now| usable.iter().position(|&i| i == now));
        let next = match at {
            Some(p) => (p as isize + by).rem_euclid(usable.len() as isize) as usize,
            None if by < 0 => usable.len() - 1,
            None => 0,
        };
        self.menu_at = usable.get(next).copied();
    }

    /// Return in the open row menu: the entry the keyboard is on, carried out.
    pub fn confirm_menu(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let all = entries(self);
        if let Some(entry) = self.menu_at.and_then(|i| all.get(i)).filter(|e| e.enabled) {
            let act = entry.act;
            self.act_from_menu(act, window, cx);
        }
    }
}

/// The row menu, open at `at` (window points).
pub fn menu(
    view: &Downloads,
    at: Point<Pixels>,
    theme: Theme,
    screen: &Screen,
    cx: &mut Context<Downloads>,
) -> impl IntoElement {
    let mut items = Vec::new();
    for (i, entry) in entries(view).into_iter().enumerate() {
        match entry.before {
            Before::Nothing => {}
            Before::Separator => items.push(separator(theme)),
            Before::Heading(words) => {
                items.push(separator(theme));
                items.push(
                    div()
                        .mx_1()
                        .px_2()
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .text_size(px(crate::native::SMALL_TEXT))
                        .text_color(theme.muted)
                        .child(screen.label(words))
                        .into_any_element(),
                );
            }
        }
        let id: gpui::ElementId = match entry.act {
            Act::Run(command) => command.id().into(),
            Act::DeleteAfter(seconds) => ("delete-after", seconds.unwrap_or(0)).into(),
        };
        let keyed = view.menu_at == Some(i);
        let act = entry.act;
        items.push(
            div()
                .id(id)
                .role(Role::MenuItem)
                .aria_label(entry.words)
                .mx_1()
                .px_2()
                .h(px(22.0))
                .rounded(px(4.0))
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(crate::native::TEXT))
                .when(!entry.enabled, |d| d.text_color(theme.muted).opacity(0.6))
                .when(entry.enabled && keyed, |d| {
                    d.bg(theme.selection).text_color(theme.on_selection)
                })
                .when(entry.enabled, |d| {
                    d.hover(|s| s.bg(theme.selection).text_color(theme.on_selection))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.act_from_menu(act, window, cx);
                        }))
                })
                .when(matches!(act, Act::DeleteAfter(_)), |d| {
                    d.child(div().w(px(14.0)).child(if entry.ticked { "✓" } else { "" }))
                })
                .child(screen.label(entry.words))
                .into_any_element(),
        );
    }
    deferred(
        anchored().position(at).child(
            div()
                .id("row-menu")
                .role(Role::Menu)
                .aria_label("Row menu")
                .min_w(px(220.0))
                .py_1()
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.separator)
                .bg(theme.menu)
                .shadow_lg()
                .text_color(theme.text)
                // The menu takes its own clicks: a row under it took them too,
                // and Extract here, over another row, chose that row first and
                // extracted it instead (Linux, real input, 2026-11-11).
                .occlude()
                .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                    view.close_menu();
                    cx.notify();
                }))
                .children(items),
        ),
    )
    .with_priority(1)
}

fn separator(theme: Theme) -> gpui::AnyElement {
    div()
        .mx_2()
        .my_1()
        .h(px(1.0))
        .bg(theme.separator)
        .into_any_element()
}
