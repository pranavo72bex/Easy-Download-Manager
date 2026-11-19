// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' Folders page.

use super::*;

pub(super) fn folders(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    folder(
        o,
        p,
        window,
        cx,
        "download_dir",
        "Download folder",
        true,
        |o| o.draft.download_dir.display().to_string(),
        |o, folder| o.draft.download_dir = folder.into(),
    );
    folder(
        o,
        p,
        window,
        cx,
        "parts_dir",
        "Folder for unfinished downloads",
        true,
        |o| o.draft.parts_dir.clone(),
        |o, folder| o.draft.parts_dir = folder,
    );
    p.note(t("A download already begun keeps the folder it began in."));
    let on = o.draft.categorize;
    check(
        p,
        cx,
        "categorize",
        t("Sort finished files into folders by kind"),
        on,
        true,
        |s| s.categorize = !s.categorize,
    );
    // Why what follows is greyed.
    if !on {
        p.note(t(
            "Turn on sorting to set the kinds' folders and your own categories.",
        ));
    }
    // Each kind's folder: the kind in the window's language, the folder a
    // name on disk, as it is.
    let kinds = dm_core::Category::ALL;
    for pair in kinds.chunks(2) {
        let mut row = div().flex().gap(px(space::XL));
        for kind in pair {
            let i = kinds.iter().position(|k| k == kind).unwrap_or(0);
            let name = dm_desktop_base::i18n::name(&dm_desktop_base::format::titled(
                &dm_shell::app::names::category_word(*kind),
            ));
            let field = input(
                o,
                window,
                cx,
                &format!("category_folders.{i}"),
                "Category",
                170.0,
                on,
                false,
                move |o| o.draft.category_folders.get(i).cloned().unwrap_or_default(),
            );
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(native::LABEL_GAP))
                    .child(
                        div()
                            .w(px(90.0))
                            .flex()
                            .justify_end()
                            .child(screen.label(name)),
                    )
                    .child(field),
            );
        }
        p.add(native::enabled(on, row));
    }
    p.add(native::enabled(
        on,
        div().child(screen.label(t("Your own categories come first. A file of these types, from these sites (any site when empty), goes into a folder named after the category."))),
    ));
    for i in 0..o.typed.custom.len() {
        let name = input(
            o,
            window,
            cx,
            &format!("custom.{i}.0"),
            "Category",
            130.0,
            on,
            false,
            move |o| o.typed.custom[i][0].clone(),
        );
        let types = input(
            o,
            window,
            cx,
            &format!("custom.{i}.1"),
            "Types",
            150.0,
            on,
            false,
            move |o| o.typed.custom[i][1].clone(),
        );
        let sites = input(
            o,
            window,
            cx,
            &format!("custom.{i}.2"),
            "Sites",
            150.0,
            on,
            false,
            move |o| o.typed.custom[i][2].clone(),
        );
        let delete = button(p, ("delete-category", i), t("Delete category")).on_click(cx.listener(
            move |o, _, _, cx| {
                if i < o.typed.custom.len() {
                    o.typed.custom.remove(i);
                    o.forget("custom");
                }
                cx.notify();
            },
        ));
        p.add(native::enabled(
            on,
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Category")))
                .child(name)
                .child(screen.label(t("Types")))
                .child(types)
                .child(screen.label(t("Sites")))
                .child(sites)
                .child(delete),
        ));
    }
    let add =
        button(p, "add-category", t("Add a category")).on_click(cx.listener(|o, _, _, cx| {
            o.typed.custom.push(Default::default());
            cx.notify();
        }));
    p.add(native::enabled(on, div().flex().child(add)));
    let _ = theme;
}
