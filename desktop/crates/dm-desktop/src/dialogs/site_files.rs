use super::*;
pub(super) fn site_files(app: &mut DmApp, ctx: &egui::Context) {
    // Built before the dialog is borrowed: the client reads the settings.
    let client = app.client();
    let listed = listed_addresses(app);
    let focus = app.dialogs.take_focus("Site files");
    let Some(d) = app.dialogs.site_files.as_mut() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let mut add: Option<Vec<String>> = None;
    let mut look = false;
    let mut save = false;
    let mut run_saved: Option<dm_shell::lists::Search> = None;
    let mut delete_saved: Option<String> = None;
    let saved = app.applied.site_searches.clone();
    window(ctx, "Site files")
        .open(&mut open)
        .show(ctx, |ui| {
            look |= entered(&focused(
                field(ui, t("Start page"), &mut d.page, 420.0),
                focus,
            ));
            field(ui, t("File types"), &mut d.types, 420.0);
            ui.horizontal(|ui| {
                let name = ui.label(t("Page depth (0 for the start page only)"));
                ui.add(number_box(&mut d.depth).range(0..=dm_shell::lists::MAX_DEPTH))
                    .labelled_by(name.id);
            });
            ui.horizontal(|ui| {
                let name = ui.label(t("Stop reading pages after (minutes)"));
                ui.add(
                    number_box(&mut d.stop_after_minutes).range(0..=dm_shell::lists::MOST_MINUTES),
                )
                .labelled_by(name.id);
            });
            field(ui, t("Only paths containing"), &mut d.include, 420.0);
            field(ui, t("Skip paths containing"), &mut d.exclude, 420.0);
            // The dialog's action straight after what it acts on: below the
            // saving lines it was out of sight at 960×660.
            look |= ui.button(t("Find files")).clicked() || enter_free(ui);
            ui.separator();
            // Saving and the saved list come before the results, which can
            // run long enough to push anything below them off the screen.
            // The name on a line of its own, then the timer and the button:
            // wrapped as one line, its words broke as "Run every," / "hours".
            // The words are not wrapped, so the line breaks
            // between its parts if anywhere; in the smallest window Save
            // search goes below rather than off the edge.
            ui.horizontal_wrapped(|ui| {
                field(ui, t("Save this search as"), &mut d.name, 160.0);
            });
            ui.horizontal_wrapped(|ui| {
                let name = ui.add(egui::Label::new(t("Run every (hours, 0 for manual)")).extend());
                ui.add(number_box(&mut d.every_hours).range(0..=24 * 31))
                    .labelled_by(name.id);
                save = ui.button(t("Save search")).clicked();
            });
            if !d.says.is_empty() {
                ui.colored_label(warn(ui), &d.says);
            }
            if !saved.is_empty() {
                ui.label(t("Saved searches"));
                for search in &saved {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} — {}", search.name, search.page));
                        if ui.button(t("Run")).clicked() {
                            run_saved = Some(search.clone());
                        }
                        if ui.button(t("Delete search")).clicked() {
                            delete_saved = Some(search.name.clone());
                        }
                    });
                }
            }
            ui.separator();
            let mut state = dm_core::lock(&d.state);
            match &mut *state {
                Grab::Idle => {}
                Grab::Looking => {
                    ui.label(t("Reading the page…"));
                }
                Grab::Failed(e) => {
                    ui.colored_label(warn(ui), e.as_str());
                }
                Grab::Found(links, _, _) if links.is_empty() => {
                    ui.label(t("No files of those types are linked from that page."));
                }
                Grab::Found(links, pages, stopped) => {
                    // Counted with a colon, which needs no plural: "on 1
                    // pages".
                    ui.label(tf(
                        "Files found: {} · pages read: {}",
                        &[&links.len(), pages],
                    ));
                    if *stopped {
                        ui.label(
                            RichText::new(t(
                                "Its time ran out with pages still to read; these are the files found until then.",
                            ))
                            .weak(),
                        );
                    }
                    pick_list(ui, links, &listed);
                    if ui.button(t("Add for later")).clicked() {
                        add = Some(links.iter().filter(|l| l.1).map(|l| l.0.clone()).collect());
                    }
                }
            }
            // A Close, as every other dialog has.
            footer(ui, |ui| {
                if ui.button(t("Close")).clicked() {
                    close = true;
                }
            });
        });
    if close {
        open = false;
    }
    if look {
        let search = d.search();
        let state = Arc::clone(&d.state);
        *dm_core::lock(&state) = Grab::Looking;
        let ctx = ctx.clone();
        // Those already in the list offered unticked, as the links dialog
        // offers them: all were ticked, and Add for later queued them again
        // without asking.
        let listed = listed.clone();
        let queue = app.backend.queue.clone();
        app.backend.runtime.spawn(async move {
            let options = queue.options().await;
            let found = dm_shell::lists::crawl_with(&client, &search, Some(&options)).await;
            *dm_core::lock(&state) = match found {
                Ok(found) => Grab::Found(
                    found
                        .files
                        .into_iter()
                        .map(|l| {
                            let new = !listed.contains(&bare(&l));
                            (l, new)
                        })
                        .collect(),
                    found.pages,
                    found.stopped,
                ),
                Err(e) => Grab::Failed(e),
            };
            ctx.request_repaint();
        });
    }
    if save {
        let search = d.search();
        if search.name.is_empty() {
            d.says = t("A saved search needs a name.").to_string();
        } else if search.page.is_empty() {
            d.says = t("A saved search needs a start page.").to_string();
        } else {
            d.says.clear();
            let mut settings = app.applied.clone();
            // Saved again under its name: what it found before is kept.
            let mut search = search;
            // Its first run on the timer one interval from now: run at once,
            // a search just saved queued its files with nothing said in the
            // dialog. Find files runs it now.
            search.last_run = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if let Some(before) = settings
                .site_searches
                .iter()
                .find(|s| s.name == search.name)
            {
                search.seen = before.seen.clone();
                search.last_run = before.last_run;
            }
            settings.site_searches.retain(|s| s.name != search.name);
            settings.site_searches.push(search);
            if let Err(e) = app.apply_settings(settings) {
                if let Some(d) = app.dialogs.site_files.as_mut() {
                    d.says = e;
                }
            }
        }
    }
    if let Some(search) = run_saved {
        app.run_search(search);
    }
    if let Some(name) = delete_saved {
        let mut settings = app.applied.clone();
        settings.site_searches.retain(|s| s.name != name);
        if let Err(e) = app.apply_settings(settings) {
            match app.dialogs.site_files.as_mut() {
                Some(d) => d.says = e,
                None => app.problem(e),
            }
        }
    }
    if let Some(chosen) = add {
        let with = AddOptions {
            later: true,
            queue: MAIN_QUEUE.into(),
            ..AddOptions::default()
        };
        if let Ok(n) = app.add(&chosen, with) {
            app.said = crate::i18n::Said::counting_naming(
                "Added {} download to {}.||Added {} downloads to {}.",
                n as u64,
                &[&n, &MAIN_QUEUE],
            );
        }
        open = false;
    }
    if !open {
        app.dialogs.site_files = None;
    }
}
