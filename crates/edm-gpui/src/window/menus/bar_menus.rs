// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The menu bar's menus with more to them than a list of commands: File, Downloads, View and
//! Help.

use super::*;
use crate::window::reach;

/// File: Resume first, in the words it has for the rows chosen, and Close last.
pub(super) fn file_menu(ticks: &Ticks) -> Menu {
    use Command as C;
    let mut file = menu("File", reach::FILE);
    // Resume first, in the words it has for the rows chosen (Retry, Start
    // now).
    let resume = if ticks.resume.is_empty() {
        Command::DownloadNow.label()
    } else {
        ticks.resume
    };
    file.items.insert(
        0,
        MenuItem::action(t(resume), Run(C::DownloadNow))
            .disabled(!carried_out(C::DownloadNow) || ticks.off.contains(&C::DownloadNow)),
    );
    // Back up EDM… and Restore… (gaps row W16), before Close.
    file.items.push(MenuItem::separator());
    file.items.extend(items(reach::FILE_LAST));
    file.items.extend([
        MenuItem::separator(),
        MenuItem::action(t("Close"), CloseWindow),
    ]);
    file
}

/// Downloads, with the queues to start or stop, the speed limiter and what is done when the
/// downloads are done.
pub(super) fn downloads_menu(ticks: &Ticks) -> Menu {
    let queues = &ticks.queues;
    // Start Queue and Stop Queue list the queues; the one clicked is started
    // or stopped, whatever the list shows (B64).
    let on_queues = |command: Command| {
        MenuItem::submenu(
            Menu::new(t(command.label())).items(queues.iter().map(|queue| {
                MenuItem::action(
                    edm_desktop_base::i18n::name(queue),
                    RunOnQueue {
                        command,
                        queue: queue.clone(),
                    },
                )
            })),
        )
    };
    let mut downloads = menu("Downloads", reach::DOWNLOADS);
    downloads.items.extend(reach::QUEUES.map(on_queues));
    downloads.items.extend([
        MenuItem::separator(),
        MenuItem::submenu(
            // Limit speed ticked while a limit is in force, and the preset
            // that is.
            Menu::new(t("Speed limiter")).items(speed_limiter(ticks.speed_limit).into_iter().map(
                |choice| match choice {
                    Some((command, ticked)) => item(command).checked(ticked),
                    None => MenuItem::separator(),
                },
            )),
        ),
        // The choice ticked; greyed where this computer gives EDM no way to
        // do it (macOS does not hibernate), rather than failing when the
        // queue runs dry.
        MenuItem::submenu(
            Menu::new(t("When done")).items(reach::WHEN_DONE.into_iter().map(|(c, w)| {
                item(c)
                    .checked(ticks.when_done == w)
                    .disabled(!w.possible(edm_desktop_base::power::available))
            })),
        ),
    ]);
    downloads
}

/// View, each item ticked as it stands; on a phone (`phone`), what it has.
pub(super) fn view_menu(ticks: &Ticks, phone: bool) -> Menu {
    // View: Categories, Toolbar and Drop target
    // ticked while shown; then Columns, Customize toolbar, Text size,
    // Language and Sort by, each ticked as it stands.
    let columns = Menu::new(t("Columns")).items(
        edm_desktop_base::columns::Column::ordered(&ticks.column_order)
            .into_iter()
            // The file name always stays.
            .skip(1)
            .map(|c| {
                MenuItem::action(
                    c.title(),
                    crate::window::viewmenu::ToggleColumn {
                        key: c.key().to_string(),
                    },
                )
                .checked(!ticks.columns_hidden.iter().any(|h| h == c.key()))
            }),
    );
    let style = edm_desktop_base::toolbar::ToolStyle::named(&ticks.toolbar_style);
    let customize = Menu::new(t("Customize toolbar")).items(
        edm_desktop_base::toolbar::TOOL_STYLES
            .iter()
            .map(|(s, words)| {
                MenuItem::action(
                    t(words),
                    crate::window::viewmenu::ToolbarStyle {
                        style: s.name().to_string(),
                    },
                )
                .checked(*s == style)
            })
            .chain([MenuItem::separator()])
            .chain(
                edm_shell::app::commands::toolbar_in(&ticks.toolbar_order)
                    .into_iter()
                    .map(|(c, words)| {
                        MenuItem::action(
                            t(words),
                            crate::window::viewmenu::ToggleToolbarButton {
                                id: c.id().to_string(),
                            },
                        )
                        .checked(!ticks.toolbar_hidden.iter().any(|h| h == c.id()))
                    }),
            ),
    );
    // Text size: how large it is now, then Larger, Smaller and Normal size,
    // with their keys (`bind_keys`).
    let percent = (ticks.text_size * 100.0).round() as u32;
    let text_size = Menu::new(t("Text size")).items([
        MenuItem::action(
            edm_desktop_base::i18n::tf("{}%", &[&percent]),
            crate::window::viewmenu::TextNormal,
        )
        .disabled(true),
        MenuItem::separator(),
        MenuItem::action(t("Larger"), crate::window::viewmenu::TextLarger),
        MenuItem::action(t("Smaller"), crate::window::viewmenu::TextSmaller),
        MenuItem::action(t("Normal size"), crate::window::viewmenu::TextNormal),
    ]);
    let speaks = edm_desktop_base::i18n::chosen(&ticks.language);
    let language = Menu::new(t("Language")).items(edm_desktop_base::i18n::LANGUAGES.iter().map(
        |(code, name)| {
            MenuItem::action(
                *name,
                crate::window::viewmenu::SetLanguage {
                    code: code.to_string(),
                },
            )
            .checked(*code == speaks)
        },
    ));
    let sort = Menu::new(t("Sort by")).items(
        reach::SORT
            .into_iter()
            .map(|c| item(c).checked(ticks.sorted_by == Some(c))),
    );
    // On a phone, none of what draws a computer's window: its side panel, toolbar, floating drop
    // target and the table's columns, which the cards and the narrow bar read nothing of (gaps
    // row N24, `reach::on_phone`).
    let mut items: Vec<MenuItem> = reach::VIEW_SHOWN
        .into_iter()
        .zip([
            ticks.sidebar,
            ticks.toolbar,
            ticks.details,
            ticks.drop_target,
        ])
        .filter(|(c, _)| reach::drawn(reach::Act::Run(*c), phone))
        .map(|(c, ticked)| item(c).checked(ticked))
        .collect();
    let own = [
        (reach::Act::Columns, columns),
        (reach::Act::CustomizeToolbar, customize),
    ]
    .into_iter()
    .filter(|(act, _)| reach::drawn(*act, phone));
    for (n, (_, submenu)) in own.enumerate() {
        if n == 0 && !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::submenu(submenu));
    }
    items.extend([
        MenuItem::submenu(text_size),
        MenuItem::submenu(language),
        MenuItem::submenu(sort),
        // What EDM has done, in figures (gaps row W15).
        MenuItem::separator(),
        MenuItem::action(t("Statistics…"), crate::dialogs::statistics::ShowStatistics),
    ]);
    Menu::new(t("View")).items(items)
}

/// Help, without About; on a phone (`phone`), what it has.
pub(super) fn help_menu(phone: bool) -> Menu {
    use Command as C;
    // Help: EDM's guide and its topics, Tip of the day, the project's home
    // page, support and telling a friend (gaps row 91).
    let page =
        |words: &str, address: String| MenuItem::action(words.to_string(), OpenPage { address });
    let mut help = menu("Help", &[&[C::Help]]);
    help.items.extend(
        edm_desktop_base::project::guide_topics()
            .into_iter()
            .map(|(words, address)| page(words, address)),
    );
    // The step-by-step guides by task, read offline (gaps row C16d); the portable EDM on a USB
    // stick is Windows' alone.
    let guides = edm_desktop_base::project::task_guides()
        .into_iter()
        .filter(|(_, page)| cfg!(windows) || *page != "usb-stick")
        .map(|(words, page)| {
            MenuItem::action(
                words.to_string(),
                OpenGuide {
                    page: page.to_string(),
                },
            )
        });
    help.items
        .push(MenuItem::submenu(Menu::new(t("How to…")).items(guides)));
    help.items.extend([
        MenuItem::action(t("Tip of the day"), TipOfTheDay),
        MenuItem::separator(),
        page(
            t("Home page"),
            edm_desktop_base::project::SOURCE.to_string(),
        ),
        MenuItem::action(t("Copy a report"), CopyReport),
        // A new issue saying this EDM's version, system and language (gaps row C13).
        page(
            t("Contact support"),
            edm_desktop_base::project::report_a_problem(),
        ),
        page(
            t("Tell a friend"),
            edm_desktop_base::project::tell_a_friend(),
        ),
    ]);
    // The phone's app is updated by its store or by its APK, which the check knows nothing of
    // (it offers a `.dmg`, a `-setup.exe` or an `.AppImage`; gaps row N24).
    if reach::drawn(reach::Act::Run(C::CheckForUpdates), phone) {
        help.items
            .extend([MenuItem::separator(), item(C::CheckForUpdates)]);
    }
    help
}
