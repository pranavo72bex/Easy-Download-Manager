// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The menu bar's menus with more to them than a list of commands: File, Downloads, View and
//! Help.

use super::*;

/// File: Resume first, in the words it has for the rows chosen, and Close last.
pub(super) fn file_menu(ticks: &Ticks) -> Menu {
    use Command as C;
    let file = menu(
        "File",
        &[
            &[C::StopDownload, C::ShowProgress],
            &[
                C::Remove,
                C::DeleteFile,
                C::Redownload,
                C::UpdateLink,
                C::CopyLink,
                C::CopyDetails,
                C::MoveRename,
            ],
            &[
                C::OpenFile,
                C::OpenWith,
                C::Preview,
                C::OpenFolder,
                C::ExtractHere,
                C::ExtractTo,
                C::Properties,
            ],
            &[
                C::MoveToQueue,
                C::RemoveFromQueue,
                C::MoveUp,
                C::MoveDown,
                C::SelectAll,
            ],
        ],
    );
    let mut file = file;
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
    // Back up dm… and Restore… (gaps row W16), before Close.
    file.items
        .extend([MenuItem::separator(), item(C::BackUp), item(C::Restore)]);
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
    use Command as C;
    // Start Queue and Stop Queue list the queues; the one clicked is started
    // or stopped, whatever the list shows (B64).
    let on_queues = |command: Command| {
        MenuItem::submenu(
            Menu::new(t(command.label())).items(queues.iter().map(|queue| {
                MenuItem::action(
                    dm_desktop_base::i18n::name(queue),
                    RunOnQueue {
                        command,
                        queue: queue.clone(),
                    },
                )
            })),
        )
    };
    let mut downloads = menu(
        "Downloads",
        &[
            &[C::ResumeAll, C::StopAll, C::DeleteCompleted],
            &[C::Find, C::FindNext, C::FindPrevious],
            &[C::Scheduler],
        ],
    );
    downloads.items.extend([
        on_queues(C::StartQueue),
        on_queues(C::StopQueue),
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
        // The choice ticked; greyed where this computer gives dm no way to
        // do it (macOS does not hibernate), rather than failing when the
        // queue runs dry.
        MenuItem::submenu(
            Menu::new(t("When done")).items(
                [
                    (C::WhenDoneNothing, WhenDone::Nothing),
                    (C::WhenDoneQuit, WhenDone::Quit),
                    (C::WhenDoneSleep, WhenDone::Sleep),
                    (C::WhenDoneHibernate, WhenDone::Hibernate),
                    (C::WhenDoneShutDown, WhenDone::ShutDown),
                ]
                .into_iter()
                .map(|(c, w)| {
                    item(c)
                        .checked(ticks.when_done == w)
                        .disabled(!w.possible(dm_desktop_base::power::available))
                }),
            ),
        ),
    ]);
    downloads
}

/// View, each item ticked as it stands.
pub(super) fn view_menu(ticks: &Ticks) -> Menu {
    use Command as C;
    // View: Categories, Toolbar and Drop target
    // ticked while shown; then Columns, Customize toolbar, Text size,
    // Language and Sort by, each ticked as it stands.
    let columns = Menu::new(t("Columns")).items(
        dm_desktop_base::columns::Column::ordered(&ticks.column_order)
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
    let style = dm_desktop_base::toolbar::ToolStyle::named(&ticks.toolbar_style);
    let customize = Menu::new(t("Customize toolbar")).items(
        dm_desktop_base::toolbar::TOOL_STYLES
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
                dm_shell::app::commands::toolbar_in(&ticks.toolbar_order)
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
            dm_desktop_base::i18n::tf("{}%", &[&percent]),
            crate::window::viewmenu::TextNormal,
        )
        .disabled(true),
        MenuItem::separator(),
        MenuItem::action(t("Larger"), crate::window::viewmenu::TextLarger),
        MenuItem::action(t("Smaller"), crate::window::viewmenu::TextSmaller),
        MenuItem::action(t("Normal size"), crate::window::viewmenu::TextNormal),
    ]);
    let speaks = dm_desktop_base::i18n::chosen(&ticks.language);
    let language = Menu::new(t("Language")).items(dm_desktop_base::i18n::LANGUAGES.iter().map(
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
        [
            C::SortByName,
            C::SortBySize,
            C::SortByStatus,
            C::SortByTimeLeft,
            C::SortByRate,
            C::SortByDate,
            C::SortByDescription,
        ]
        .into_iter()
        .map(|c| item(c).checked(ticks.sorted_by == Some(c))),
    );
    Menu::new(t("View")).items([
        item(C::ToggleCategories).checked(ticks.sidebar),
        item(C::ToggleToolbar).checked(ticks.toolbar),
        item(C::ToggleDropTarget).checked(ticks.drop_target),
        MenuItem::separator(),
        MenuItem::submenu(columns),
        MenuItem::submenu(customize),
        MenuItem::submenu(text_size),
        MenuItem::submenu(language),
        MenuItem::submenu(sort),
    ])
}

/// Help, without About.
pub(super) fn help_menu() -> Menu {
    use Command as C;
    // Help: dm's guide and its topics, Tip of the day, the project's home
    // page, support and telling a friend (gaps row 91).
    let page =
        |words: &str, address: String| MenuItem::action(words.to_string(), OpenPage { address });
    let mut help = menu("Help", &[&[C::Help]]);
    help.items.extend(
        dm_desktop_base::project::guide_topics()
            .into_iter()
            .map(|(words, address)| page(words, address)),
    );
    help.items.extend([
        MenuItem::action(t("Tip of the day"), TipOfTheDay),
        MenuItem::separator(),
        page(t("Home page"), dm_desktop_base::project::SOURCE.to_string()),
        page(
            t("Contact support"),
            dm_desktop_base::project::SUPPORT.to_string(),
        ),
        page(
            t("Tell a friend"),
            dm_desktop_base::project::tell_a_friend(),
        ),
        MenuItem::separator(),
        item(C::CheckForUpdates),
    ]);
    help
}
