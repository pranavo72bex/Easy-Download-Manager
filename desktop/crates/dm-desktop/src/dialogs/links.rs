use super::*;
/// The "Download links" dialog.
pub(super) fn links(app: &mut DmApp, ctx: &egui::Context) {
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
            let hidden = d.items.iter().filter(|(u, _)| !d.shown(u)).count();
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
            ui.horizontal(|ui| {
                check(ui, &mut d.hide_html, t("Hide HTML files"));
                // Check all and Uncheck all act on what is listed.
                let listed: Vec<bool> = d.items.iter().map(|(u, _)| d.shown(u)).collect();
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
            let found = dm_core::lock(&d.found).clone();
            for (url, on) in d.items.iter_mut() {
                if found.get(url.as_str()).is_some_and(|f| f.failed.is_some())
                    && d.refused.insert(url.clone())
                {
                    *on = false;
                }
            }
            let listed: Vec<bool> = d.items.iter().map(|(u, _)| d.shown(u)).collect();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    egui::Grid::new("links")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong(t("File name"));
                            ui.strong(t("Type"));
                            ui.strong(t("Size"));
                            ui.end_row();
                            for ((url, on), shown) in d.items.iter_mut().zip(listed) {
                                if !shown {
                                    continue;
                                }
                                let what = found.get(url.as_str());
                                let name = what
                                    .map(|f| f.name.clone())
                                    .filter(|n| !n.is_empty())
                                    .unwrap_or_else(|| format::provisional(url));
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
                                ui.end_row();
                            }
                        });
                });
            footer(ui, |ui| {
                if primary(ui, "Start now").clicked() || enter_free(ui) {
                    chosen = Some(false);
                }
                if ui.button(t("Add for later")).clicked() {
                    chosen = Some(true);
                }
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    if let Some(later) = chosen {
        let chosen = d.chosen();
        let with = AddOptions {
            later,
            queue: if later {
                MAIN_QUEUE.into()
            } else {
                String::new()
            },
            referrer: d.page.clone(),
            ..AddOptions::default()
        };
        // The sizes the servers already told, kept on the rows until each
        // learns its own.
        let sizes: std::collections::HashMap<String, u64> = dm_core::lock(&d.found)
            .iter()
            .filter_map(|(url, found)| found.size.map(|size| (url.clone(), size)))
            .collect();
        // The page's sign-in goes with the links on its own server, and with
        // no others.
        let (signed, plain): (Vec<String>, Vec<String>) = chosen.into_iter().partition(|link| {
            d.sign_in
                .as_ref()
                .is_some_and(|s| s.for_url(link).is_some())
        });
        let signed_with = AddOptions {
            cookies: d.cookies.clone(),
            ..with.clone()
        };
        app.dialogs.links = None;
        let added = match (
            app.add_sized(&signed, signed_with, &sizes),
            app.add_sized(&plain, with, &sizes),
        ) {
            (Ok(a), Ok(b)) => Ok(a + b),
            (Err(e), _) | (_, Err(e)) => Err(e),
        };
        match added {
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
        return;
    }
    if cancel || !open {
        app.dialogs.links = None;
    }
}

pub(super) fn clip_links(app: &mut DmApp, ctx: &egui::Context) {
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
