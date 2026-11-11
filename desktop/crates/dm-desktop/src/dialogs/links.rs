// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
/// The "Download links" dialog.
pub(super) fn links(app: &mut DmApp, ctx: &egui::Context) {
    if app.dialogs.links.is_none() {
        return;
    }
    // Where each link goes, for the Save to column and the choice of one
    // category: the settings' folders and categories.
    let root = app.applied.download_dir.clone();
    let categories = app.applied.categories();
    let customs: Vec<String> = app
        .applied
        .custom_categories
        .iter()
        .map(|c| c.name.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    let platform = &app.backend.platform;
    let queues = app.queues();
    let Some(d) = app.dialogs.links.as_mut() else {
        return;
    };
    let mut open = true;
    // Some(later): Start now, or Add for later.
    let mut chosen: Option<bool> = None;
    let mut cancel = false;
    // Titled for either list the browser hands over: a page's every link,
    // or the links in what was selected on it.
    window(ctx, "Download links")
        .open(&mut open)
        .show(ctx, |ui| {
            // The links listed, and the pages left out of the list said
            // so: "4 links" over three lines read as one lost.
            let pages = if d.hide_html {
                d.items.iter().filter(|(u, _)| d.is_html(u)).count()
            } else {
                0
            };
            let hidden = d.visible().iter().filter(|listed| !**listed).count();
            ui.label(tf(
                "{} links on {}",
                &[&(d.items.len() - hidden), &format::shown_url(&d.page)],
            ));
            if pages > 0 {
                ui.label(
                    RichText::new(crate::i18n::tn(
                        "{} web page hidden||{} web pages hidden",
                        pages as u64,
                        &[&pages],
                    ))
                    .weak(),
                );
            }
            ui.horizontal(|ui| {
                let name = ui.label(t("Only these types"));
                ui.add(
                    egui::TextEdit::singleline(&mut d.types)
                        .hint_text(t("zip pdf mp4"))
                        .desired_width(180.0),
                )
                .labelled_by(name.id);
            });
            // What is left out, as reference A offers it.
            ui.horizontal_wrapped(|ui| {
                check(ui, &mut d.hide_html, t("Hide HTML files"));
                check(ui, &mut d.hide_images, t("Hide images"));
                check(ui, &mut d.hide_repeats, t("Hide repeated files"));
            });
            ui.horizontal(|ui| {
                // Check all and Uncheck all act on what is listed.
                let listed = d.visible();
                if ui.button(t("Check all")).clicked() {
                    for (l, shown) in d.items.iter_mut().zip(&listed) {
                        if *shown {
                            l.1 = true;
                        }
                    }
                }
                if ui.button(t("Uncheck all")).clicked() {
                    for (l, shown) in d.items.iter_mut().zip(&listed) {
                        if *shown {
                            l.1 = false;
                        }
                    }
                }
            });
            d.untick_refused();
            let found = dm_core::lock(&d.found).clone();
            let listed = d.visible();
            let mut edit_asked: Option<usize> = None;
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    egui::Grid::new("links")
                        .num_columns(6)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong(t("File name"));
                            ui.strong(t("Type"));
                            ui.strong(t("Size"));
                            ui.strong(t("Save to"));
                            ui.strong(t("Link text"));
                            ui.label("");
                            ui.end_row();
                            let names: Vec<String> =
                                d.items.iter().map(|(url, _)| d.name_of(url)).collect();
                            let texts = d.texts.clone();
                            let folders: Vec<String> = d
                                .items
                                .iter()
                                .map(|(url, _)| d.folder_of(url, &root, &categories))
                                .collect();
                            for (n, (((url, on), shown), name)) in
                                d.items.iter_mut().zip(listed).zip(names).enumerate()
                            {
                                if !shown {
                                    continue;
                                }
                                let what = found.get(url.as_str());
                                let name = if d.listed.contains(&bare(url)) {
                                    tf("{} (already in the list)", &[&name])
                                } else {
                                    name
                                };
                                // The name is the checkbox's, so a screen reader says it.
                                check(ui, on, &name).on_hover_text(format::shown_url(url));
                                match what {
                                    Some(Found {
                                        failed: Some(why), ..
                                    }) => {
                                        let plain =
                                            format::reason(why).map(t).unwrap_or(why.as_str());
                                        ui.colored_label(warn(ui), t("Error"))
                                            .on_hover_text(format::titled(plain));
                                    }
                                    // The kind as the tree names it, in the
                                    // language in force.
                                    Some(f) => {
                                        ui.label(crate::i18n::name(&format::titled(&f.kind)));
                                    }
                                    None => {
                                        ui.label("…");
                                    }
                                }
                                let size = what
                                    .and_then(|f| f.size)
                                    .map(format::bytes)
                                    .unwrap_or_default();
                                ui.label(crate::table::digits(ui, size));
                                // The folder's own name, its whole path on hover:
                                // cut to the room, a path said only its drive.
                                let folder = folders.get(n).cloned().unwrap_or_default();
                                let last = std::path::Path::new(&folder)
                                    .file_name()
                                    .map(|f| f.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| folder.clone());
                                ui.add(egui::Label::new(RichText::new(last).weak()).truncate())
                                    .on_hover_text(&folder);
                                // Its words on the page, as reference A's Link Text.
                                let words = texts.get(url.as_str()).cloned().unwrap_or_default();
                                ui.add(egui::Label::new(RichText::new(&words).weak()).truncate())
                                    .on_hover_text(&words);
                                // Its name and address changed before it is added.
                                if ui.small_button(t("Edit")).clicked() {
                                    edit_asked = Some(n);
                                }
                                ui.end_row();
                            }
                        });
                });
            if let Some(n) = edit_asked {
                d.editing = Some(n);
            }
            // The row being edited: its name and its address.
            if let Some(n) = d.editing.filter(|n| *n < d.items.len()) {
                let url = d.items[n].0.clone();
                // As typed, spaces and all; trimmed where it is used.
                let mut name = d
                    .names
                    .get(&url)
                    .cloned()
                    .unwrap_or_else(|| d.name_of(&url));
                let mut address = url.clone();
                ui.horizontal_wrapped(|ui| {
                    field(ui, t("File name"), &mut name, 160.0);
                    field(ui, t("Address"), &mut address, 200.0);
                    if ui.button(t("Done")).clicked() {
                        d.editing = None;
                    }
                });
                let address = address.trim().to_string();
                if !address.is_empty() && address != url {
                    if let Some(given) = d.names.remove(&url) {
                        d.names.insert(address.clone(), given);
                    }
                    d.items[n].0 = address.clone();
                }
                let key = if address.is_empty() { url } else { address };
                // Kept once it differs from the name it would have, and while
                // it is being typed after that.
                if d.names.contains_key(&key) || name.trim() != d.name_of(&key) {
                    d.names.insert(key, name);
                }
            }
            // Each link's words on the page kept as its download's description.
            check(ui, &mut d.use_texts, t("Use link texts as descriptions"));
            // Where the ticked links go, as reference A offers it.
            ui.label(RichText::new(t("Save to")).strong());
            if radio_flag(
                ui,
                d.save.save_to == SaveTo::ByCategory,
                t("Each file to the folder of its category"),
            )
            .clicked()
            {
                d.save.save_to = SaveTo::ByCategory;
            }
            ui.horizontal(|ui| {
                if radio_flag(
                    ui,
                    d.save.save_to == SaveTo::OneCategory,
                    t("Every file to this category"),
                )
                .clicked()
                {
                    d.save.save_to = SaveTo::OneCategory;
                }
                ui.add_enabled_ui(d.save.save_to == SaveTo::OneCategory, |ui| {
                    let shown = |word: &str| crate::i18n::name(&format::titled(word));
                    combo("links-category")
                        .selected_text(shown(&d.save.category))
                        .show_ui(ui, |ui| {
                            for kind in dm_core::Category::ALL {
                                let word = crate::app::category_word(kind);
                                pick(ui, &mut d.save.category, word.clone(), shown(&word));
                            }
                            for own in &customs {
                                pick(ui, &mut d.save.category, own.clone(), own.clone());
                            }
                        });
                });
            });
            ui.horizontal(|ui| {
                if radio_flag(
                    ui,
                    d.save.save_to == SaveTo::OneFolder,
                    t("Every file to this folder"),
                )
                .clicked()
                {
                    d.save.save_to = SaveTo::OneFolder;
                }
                ui.add_enabled_ui(d.save.save_to == SaveTo::OneFolder, |ui| {
                    let wide = (ui.available_width() - 120.0).clamp(120.0, 320.0);
                    let name = t("Every file to this folder");
                    let typed =
                        ui.add(egui::TextEdit::singleline(&mut d.save.folder).desired_width(wide));
                    typed.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::TextInput, true, name)
                    });
                    browse(ui, &**platform, &mut d.save.folder);
                });
            });
            footer(ui, |ui| {
                if primary(ui, "Start now").clicked() || enter_free(ui) {
                    chosen = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    chosen = Some(true);
                }
                // The queue they wait in, as reference A asks on closing a
                // batch dialog and as Download details offers (gaps row 85).
                let to = ui.label(t("queue:to").trim_start_matches("queue:"));
                combo("links-queue")
                    .selected_text(crate::i18n::name(&d.queue))
                    .show_ui(ui, |ui| {
                        for q in queues.iter() {
                            pick(ui, &mut d.queue, q.clone(), crate::i18n::name(q));
                        }
                    })
                    .response
                    .labelled_by(to.id);
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    if let Some(later) = chosen {
        // What is added is dm's (`LinksImport::adds`), as every window's.
        let (items, sizes) = match d.adds(later, &root, &categories) {
            Ok(adds) => adds,
            Err(words) => {
                app.said = crate::i18n::Said::new(words, &[]);
                return;
            }
        };
        let queue = d.queue_or_main();
        app.dialogs.links = None;
        let added = app.add_items(items, &sizes);
        match added {
            Ok(n) if later => {
                app.said = crate::i18n::Said::counting_naming(
                    "Added {} download to {}.||Added {} downloads to {}.",
                    n as u64,
                    &[&n, &crate::i18n::name(&queue)],
                )
            }
            Ok(n) => {
                app.said = crate::i18n::Said::counting(
                    "Started {} download.||Started {} downloads.",
                    n as u64,
                    &[&n],
                )
            }
            Err(e) => app.said = e.into(),
        }
        return;
    }
    if cancel || !open {
        app.dialogs.links = None;
    }
}

pub(super) fn clip_links(app: &mut DmApp, ctx: &egui::Context) {
    // Before the addresses are worked out: every row's was parsed on every
    // frame for a dialog that is almost never open (a fifth of a frame's
    // time with 5,000 rows).
    if app.dialogs.clip_links.is_none() {
        return;
    }
    let listed = listed_addresses(app);
    let Some(links) = app.dialogs.clip_links.as_mut() else {
        return;
    };
    let mut open = true;
    // Some(later): Start now, or Add for later.
    let mut add: Option<bool> = None;
    let mut cancel = false;
    window(ctx, "Links on the clipboard")
        .open(&mut open)
        .show(ctx, |ui| {
            // How many were found, and how many are ticked.
            let ticked = links.iter().filter(|l| l.1).count();
            ui.label(crate::i18n::tn(
                "Found {} link; {} ticked.||Found {} links; {} ticked.",
                links.len() as u64,
                &[&links.len(), &ticked],
            ));
            pick_list(ui, links, &listed);
            footer(ui, |ui| {
                if primary(ui, "Start now").clicked() || enter_free(ui) {
                    add = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    add = Some(true);
                }
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    if cancel {
        open = false;
    }
    if let Some(later) = add {
        let chosen: Vec<String> = links.iter().filter(|l| l.1).map(|l| l.0.clone()).collect();
        let with = AddOptions {
            later,
            queue: if later {
                MAIN_QUEUE.into()
            } else {
                String::new()
            },
            ..AddOptions::default()
        };
        match app.add(&chosen, with) {
            Ok(n) if later => {
                app.said = crate::i18n::Said::counting_naming(
                    "Added {} download to {}.||Added {} downloads to {}.",
                    n as u64,
                    &[&n, &MAIN_QUEUE],
                )
            }
            Ok(n) => {
                app.said = crate::i18n::Said::counting(
                    "Started {} download.||Started {} downloads.",
                    n as u64,
                    &[&n],
                )
            }
            Err(e) => app.said = e.into(),
        }
        open = false;
    }
    if !open {
        app.dialogs.clip_links = None;
    }
}
