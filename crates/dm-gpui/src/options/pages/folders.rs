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
        |o| o.draft.folders.download_dir.display().to_string(),
        |o, folder| o.draft.folders.download_dir = folder.into(),
    );
    folder(
        o,
        p,
        window,
        cx,
        "parts_dir",
        "Folder for unfinished downloads",
        true,
        |o| o.draft.folders.parts_dir.clone(),
        |o, folder| o.draft.folders.parts_dir = folder,
    );
    p.note(t("A download already begun keeps the folder it began in."));
    let on = o.draft.folders.categorize;
    check(
        p,
        cx,
        "categorize",
        t("Sort finished files into folders by kind"),
        on,
        true,
        |s| s.folders.categorize = !s.folders.categorize,
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
        // Two kinds a line on a computer; one a line on a phone, which has room for one.
        let mut row = div()
            .flex()
            .gap(px(space::XL))
            .when(crate::platform::ON_A_PHONE, |row| {
                row.flex_col().gap(px(native::ROW_GAP))
            });
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
                move |o| kind_folder(o, i),
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
    custom_categories(o, p, window, cx, on, &screen);
    unpacking(o, p, cx);
    archive_passwords(o, p, window, cx);
    let _ = theme;
}

/// Unpack when done (gaps row W14): the kinds whose finished archive is unpacked into a new
/// folder of its name, and what then becomes of the archive.
fn unpacking(o: &Options, p: &mut Page, cx: &mut Context<Options>) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let folders = &o.draft.folders;
    p.section(t("Unpack when done"));
    p.note(t("A finished ZIP, 7z or tar archive of a kind turned on here is unpacked into a new folder of its name beside it. No file is replaced, and nothing is written outside that folder."));
    // A row for each kind, its name then its switch, as every other setting (the maintainer,
    // 2026-11-23: switches, not check boxes, on the phone and the computer alike).
    for (i, kind) in dm_core::Category::ALL.into_iter().enumerate() {
        let name = dm_desktop_base::i18n::name(&dm_desktop_base::format::titled(
            &dm_shell::app::names::category_word(kind),
        ));
        p.says(&name);
        let ticked = folders.unpack_kinds.contains(&kind);
        let id = SharedString::from(format!("unpack-{i}"));
        let check = native::switch(id, name.into(), ticked, theme, &screen).on_click(cx.listener(
            move |o, _, _, cx| {
                let chosen = &mut o.draft.folders.unpack_kinds;
                if chosen.contains(&kind) {
                    chosen.retain(|k| *k != kind);
                } else {
                    chosen.push(kind);
                }
                cx.notify();
            },
        ));
        p.add(check);
    }
    p.words(t("Once it is unpacked:"));
    for (trash, words) in [
        (false, t("Keep the archive")),
        (true, t("Send the archive to the Trash")),
    ] {
        let id = SharedString::from(format!("unpack-trash-{trash}"));
        let on = folders.unpack_trash == trash;
        let radio = native::radio(id, words.into(), on, theme, &screen).on_click(cx.listener(
            move |o, _, _, cx| {
                o.draft.folders.unpack_trash = trash;
                cx.notify();
            },
        ));
        p.says(words);
        p.add(radio);
    }
}

/// The passwords tried on a finished archive that has one (gaps row C2): each shown as dots
/// with Delete, and a field to add one. Kept apart from the settings, sealed, by OK.
fn archive_passwords(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    p.section(t("Archive passwords"));
    p.note(t(
        "Passwords tried on an archive that has one, in turn; the one that opens it goes first.",
    ));
    for i in 0..o.archive_passwords.len() {
        let delete = native::button(
            SharedString::from(format!("archive-password-delete-{i}")),
            t("Delete").into(),
            false,
            theme,
            &screen,
        )
        .on_click(cx.listener(move |o, _, _, cx| {
            if i < o.archive_passwords.len() {
                o.archive_passwords.remove(i);
            }
            cx.notify();
        }));
        p.add(
            div()
                .flex()
                .items_center()
                .gap(px(space::M))
                // Never the password, nor how long it is.
                .child(div().flex_1().child(screen.label("••••••••".to_string())))
                .child(delete),
        );
    }
    let field = o.field("archive-password", |_| String::new(), true, window, cx);
    let add = native::button(
        "archive-password-add",
        t("Add").into(),
        false,
        theme,
        &screen,
    )
    .on_click(cx.listener(move |o, _, window, cx| {
        let Some(field) = o.fields.get("archive-password").cloned() else {
            return;
        };
        let typed = field.read(cx).value().to_string();
        if !typed.is_empty() && !o.archive_passwords.contains(&typed) {
            o.archive_passwords.push(typed);
        }
        field.update(cx, |f, cx| f.set_value("", window, cx));
        cx.notify();
    }));
    p.says(t("Add a password"));
    // The words above, the field and Add on one line under them: room for it on a phone.
    p.add(div().child(screen.label(t("Add a password"))));
    p.add(
        div()
            .flex()
            .items_center()
            .gap(px(space::M))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(320.0))
                    .child(native::field(&field, "Add a password", theme)),
            )
            .child(add),
    );
    p.note(t(
        "An archive none of these passwords opens is left as it is, and its row says so.",
    ));
}

/// Your own categories: each one's name, types and sites, and a button to delete it; and a
/// button to add one.
fn custom_categories(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    on: bool,
    screen: &Screen,
) {
    p.add(native::enabled(
        on,
        // What the list below does, in the small grey words of an explanation.
        native::note(
            t("Your own categories come first. A file of these types, from these sites (any site when empty), goes into a folder named after the category."),
            p.theme.muted,
            screen,
        ),
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
}

/// The folder the draft names for the kind at `i` in `Category::ALL`, empty for none.
fn kind_folder(o: &Options, i: usize) -> String {
    let folders = &o.draft.folders.category_folders;
    folders.get(i).cloned().unwrap_or_default()
}
