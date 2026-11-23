// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The row menu, where a right-click (or Control-click), Shift+F10 or the Menu key lands: the
//! system's own menu (an `NSMenu` on macOS, Windows' own, a drawn one on Linux), opened as the
//! title menus and the toolbar's arrows open theirs (`NativeMenu`), so the system walks it with
//! the keyboard. What does not apply to the rows chosen is greyed, a group none of whose items
//! applies is left out, and each item's key is shown beside it where the window has one: after
//! a tab on Windows, as its own menus write it; on Linux the drawn menu shows the key bound to
//! each item's action itself; on macOS, where `NativeMenu` cannot give an item a key, dm builds
//! the `NSMenu` itself with each item's key as its key equivalent (`platform::popmenu`, gaps row
//! U27).

use dm_desktop_base::i18n::t;
use dm_shell::app::commands::{Chord, Command, Key};
use gpui::{Action, Context, Menu, MenuItem, Pixels, Point};

use crate::window::list::Downloads;
use crate::window::menus::Run;

/// The menu's items, in groups (`reach::ROW`), each with the words it shows (the row menu's
/// own, where it says them differently from the command's title): Resume as `resume` says it
/// for the rows chosen (Retry, Start now).
fn groups(resume: &'static str) -> Vec<Vec<(Command, &'static str)>> {
    crate::window::reach::ROW
        .iter()
        .map(|group| group.iter().map(|&c| (c, words(c, resume))).collect())
        .collect()
}

/// A row menu item's words.
fn words(command: Command, resume: &'static str) -> &'static str {
    use Command as C;
    match command {
        C::DownloadNow => t(resume),
        C::StopDownload => t("Stop download"),
        C::UpdateLink => t("Update download link"),
        C::CopyLink => t("Copy link"),
        C::CopyDetails => t("Copy details"),
        C::Remove => t("Delete"),
        C::MoveToQueue => t("Move to queue"),
        C::Properties => t("Properties"),
        _ => t(command.label()),
    }
}

/// "Delete file after" this many seconds (`None`: Never), chosen in the row menu: set on the
/// rows chosen that have, or will have, a file (gaps row U8).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct DeleteAfter {
    /// The time, in seconds; `None` for Never.
    pub seconds: Option<u64>,
}

/// A phone's Share in a card's menu: the finished file chosen offered to the phone's other apps
/// through its share sheet.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct ShareFile;

/// A phone card menu's Select: several cards chosen with taps from this one on (gaps row N23).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = dm, no_json)]
pub struct ChooseSeveral;

/// The row menu as a phone's card menu shows it: only what applies to the card (a phone's menus
/// do not grey what they cannot do), and Share after Open where the card's file is whole.
pub(crate) fn card_menu(view: &Downloads) -> Menu {
    card_menu_on(view, crate::platform::ON_A_PHONE)
}

/// [`card_menu`] as a phone (`phone`) or a computer's narrowed window draws it.
pub(crate) fn card_menu_on(view: &Downloads, phone: bool) -> Menu {
    use crate::window::reach::{drawn, Act};
    let full = row_menu(view, false);
    let finished = view
        .chosen()
        .iter()
        .any(|r| r.state == dm_core::State::Completed);
    let mut items: Vec<MenuItem> = Vec::new();
    for item in full.items {
        match &item {
            MenuItem::Action { disabled: true, .. } => continue,
            // No separator first, nor two in a row.
            MenuItem::Separator
                if items.is_empty() || matches!(items.last(), Some(MenuItem::Separator)) =>
            {
                continue
            }
            _ => {}
        }
        let opens = matches!(
            &item,
            MenuItem::Action { action, .. }
                if action.as_any().downcast_ref::<Run>().map(|r| r.0) == Some(Command::OpenFile)
        );
        items.push(item);
        if opens && finished && drawn(Act::Share, phone) {
            items.push(MenuItem::action(t("Share"), ShareFile));
        }
    }
    while matches!(items.last(), Some(MenuItem::Separator)) {
        items.pop();
    }
    // Choosing several, from this card on, where the cards are a phone's (gaps row N23).
    if phone || view.layout.narrow.laid_out {
        items.push(MenuItem::action(t("Select"), ChooseSeveral));
    }
    // Named after its card's file, which its sheet shows at the top.
    let name: gpui::SharedString = match view.chosen().as_slice() {
        [only] => crate::window::table::shown_name(only, &view.known.shown_names),
        _ => t("Row menu").into(),
    };
    Menu::new(name).items(items)
}

/// One item of the row menu: what it runs, its words, and whether it applies to the rows chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Item {
    pub command: Command,
    pub words: &'static str,
    pub enabled: bool,
}

/// The row menu's groups, in order, each item with whether it applies (`applies`): a group none
/// of whose items applies is left out, as the system's own context menus leave out what has
/// nothing to offer; the rest keep their greyed items, in their places.
pub(crate) fn kept(resume: &'static str, applies: impl Fn(Command) -> bool) -> Vec<Vec<Item>> {
    groups(resume)
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .map(|(command, words)| Item {
                    command,
                    words,
                    enabled: applies(command),
                })
                .collect::<Vec<_>>()
        })
        .filter(|group| group.iter().any(|item| item.enabled))
        .collect()
}

/// Whether a row in `state` has, or will have, a file: "Delete file after" is for those.
fn has_file(state: &dm_core::State) -> bool {
    !matches!(state, dm_core::State::Failed(_) | dm_core::State::Cancelled)
}

/// The key that runs `command` over the list, as Windows' menus write it ("Ctrl+R", "Del"): its
/// first chord; Copy link's is Copy's, Ctrl+C, which copies the chosen rows' links (gaps row
/// U9). `None` where it has none.
pub(crate) fn hint(command: Command) -> Option<String> {
    if command == Command::CopyLink {
        return Some("Ctrl+C".to_string());
    }
    command.chords().first().map(written)
}

/// A chord as Windows' menus write it.
fn written(chord: &Chord) -> String {
    let key = match chord.key {
        Key::A => "A",
        Key::F => "F",
        Key::I => "I",
        Key::N => "N",
        Key::Q => "Q",
        Key::R => "R",
        Key::Comma => ",",
        Key::Period => ".",
        Key::F1 => "F1",
        Key::F3 => "F3",
        Key::Backspace => "Backspace",
        Key::Delete => "Del",
    };
    let mut words = String::new();
    if chord.command {
        words.push_str("Ctrl+");
    }
    if chord.shift {
        words.push_str("Shift+");
    }
    words.push_str(key);
    words
}

/// The row menu for the rows chosen, as the system's menus take it: the groups that apply with
/// a separator between, then, where a row chosen has or will have a file, "Delete file after"
/// with its times, the one the rows share ticked. With `hints`, each item's key after a tab,
/// which Windows' menus show at the item's right.
pub(crate) fn row_menu(view: &Downloads, hints: bool) -> Menu {
    let chosen = view.chosen();
    let resume = dm_desktop_base::rows::resume_word(&chosen);
    let mut items = Vec::new();
    for group in kept(resume, |command| view.enabled(command)) {
        if !items.is_empty() {
            items.push(MenuItem::separator());
        }
        for item in group {
            let words = match hint(item.command).filter(|_| hints) {
                Some(keys) => format!("{}\t{keys}", item.words),
                None => item.words.to_string(),
            };
            items.push(MenuItem::action(words, Run(item.command)).disabled(!item.enabled));
        }
    }
    if chosen.iter().any(|r| has_file(&r.state)) {
        let shared = chosen
            .first()
            .map(|r| r.delete_after)
            .filter(|first| chosen.iter().all(|r| r.delete_after == *first));
        let times: Vec<MenuItem> = dm_desktop_base::file_info::delete_after_times()
            .into_iter()
            .map(|(words, seconds)| {
                MenuItem::action(words, DeleteAfter { seconds }).checked(shared == Some(seconds))
            })
            .collect();
        if !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::submenu(
            Menu::new(t("Delete file after")).items(times),
        ));
    }
    Menu::new(t("Row menu")).items(items)
}

impl Downloads {
    /// The row menu opened at `at` (window points) for the rows chosen. The control port's
    /// click has it held for the port's next click instead, as the system's menu would wait for
    /// a person (`probe::hold_menu`).
    pub(crate) fn show_row_menu(
        &mut self,
        at: Point<Pixels>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.menus.row_menu_opened = Some(at);
        if crate::widgets::popup::scripting() {
            crate::probe::hold_menu(row_menu(self, false).owned());
            return;
        }
        // The test platform has no menus of the system's to show.
        if cfg!(test) {
            return;
        }
        // macOS: dm's own `NSMenu`, its items with their keys; elsewhere gpui-component's.
        #[cfg(target_os = "macos")]
        crate::platform::popmenu::show(row_menu(self, false), at, window, cx);
        #[cfg(not(target_os = "macos"))]
        gpui_component::native_menu::NativeMenu::from(row_menu(self, cfg!(windows)))
            .show(at, window, cx);
    }

    /// "Delete file after" chosen in the row menu: the time set on the rows chosen that have,
    /// or will have, a file.
    pub(crate) fn delete_after_chosen(&mut self, seconds: Option<u64>, cx: &mut Context<Self>) {
        let targets: Vec<dm_core::Id> = self
            .chosen()
            .into_iter()
            .filter(|r| has_file(&r.state))
            .map(|r| r.id)
            .collect();
        // Off the window's thread: each writes the queue.
        self.later_then_read(
            move |backend| {
                for id in targets {
                    backend.set_delete_after(id, seconds);
                }
            },
            cx,
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands(groups: &[Vec<Item>]) -> Vec<Vec<Command>> {
        groups
            .iter()
            .map(|g| g.iter().map(|i| i.command).collect())
            .collect()
    }

    /// Where everything applies, the four groups stand whole, in order.
    #[test]
    fn every_group_stands_where_all_applies() {
        let all = kept("Resume", |_| true);
        assert_eq!(all.len(), 4);
        assert_eq!(all.iter().map(Vec::len).sum::<usize>(), 20);
        assert_eq!(all[0][0].command, Command::OpenFile);
        assert_eq!(all[3][0].command, Command::Properties);
        assert!(all.iter().flatten().all(|i| i.enabled));
    }

    /// A group none of whose items applies is left out; one with any that does stays whole,
    /// its others greyed in their places.
    #[test]
    fn groups_that_do_not_apply_are_left_out() {
        use Command as C;
        let some = kept("Retry", |c| matches!(c, C::CopyLink | C::Properties));
        assert_eq!(
            commands(&some),
            vec![
                vec![
                    C::DownloadNow,
                    C::StopDownload,
                    C::UpdateLink,
                    C::CopyLink,
                    C::CopyDetails,
                    C::Remove,
                    C::DeleteFile,
                ],
                vec![C::Properties],
            ]
        );
        let greyed: Vec<Command> = some[0]
            .iter()
            .filter(|i| !i.enabled)
            .map(|i| i.command)
            .collect();
        assert_eq!(greyed.len(), 6);
        assert!(!greyed.contains(&C::CopyLink));
        // Resume in the words it has for the rows chosen.
        assert_eq!(some[0][0].words, t("Retry"));
        assert!(kept("Resume", |_| false).is_empty());
        let queue = kept("Resume", |c| c == C::MoveDown);
        assert_eq!(
            commands(&queue),
            vec![vec![
                C::MoveToQueue,
                C::RemoveFromQueue,
                C::MoveUp,
                C::MoveDown
            ]]
        );
    }

    /// The keys shown are the window's own, as Windows writes them; an item without one has none.
    #[test]
    fn the_keys_are_written_as_windows_writes_them() {
        assert_eq!(hint(Command::DownloadNow).as_deref(), Some("Ctrl+R"));
        assert_eq!(hint(Command::StopDownload).as_deref(), Some("Ctrl+."));
        assert_eq!(hint(Command::Properties).as_deref(), Some("Ctrl+I"));
        assert_eq!(hint(Command::CopyLink).as_deref(), Some("Ctrl+C"));
        assert_eq!(hint(Command::OpenFolder), None);
        if !cfg!(target_os = "macos") {
            assert_eq!(hint(Command::Remove).as_deref(), Some("Del"));
        }
    }
}
