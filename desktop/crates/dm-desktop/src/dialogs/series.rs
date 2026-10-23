use super::*;
pub(super) fn series(app: &mut DmApp, ctx: &egui::Context) {
    let focus = app.dialogs.take_focus("Add numbered series");
    let Some(d) = app.dialogs.series.as_mut() else {
        return;
    };
    let mut open = true;
    let mut add: Option<bool> = None;
    let before = (d.template.clone(), d.from.clone(), d.to.clone(), d.width);
    let mut cancel = false;
    window(ctx, "Add numbered series")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(t(
                "Put * where the number or letter changes, as in example.com/photo*.jpg",
            ));
            let mut enter = entered(&focused(
                field(ui, t("Address with *"), &mut d.template, 420.0),
                focus,
            ));
            ui.horizontal(|ui| {
                enter |= entered(&field(ui, t("From"), &mut d.from, 60.0));
                enter |= entered(&field(ui, t("To"), &mut d.to, 60.0));
                // "Wildcard size" said nothing of what it does.
                let name = ui.label(t("Digits"));
                ui.add(number_box(&mut d.width).range(0..=9))
                    .labelled_by(name.id)
                    .on_hover_text(t(
                        "How many digits each number has: 3 makes 7 into 007; 0 leaves it as it is",
                    ));
            });
            // One line, its room kept whether or not it says anything, so
            // the dialog does not jump as it starts to; and nothing said
            // about the range until both its ends are typed.
            let typed = !d.template.trim().is_empty()
                && !d.from.trim().is_empty()
                && !d.to.trim().is_empty();
            // What a press of Start said stands only until the address or
            // the range is changed: kept, it hid the count and the live
            // error of what was typed since.
            if (&d.template, &d.from, &d.to, d.width) != (&before.0, &before.1, &before.2, before.3)
            {
                d.says.clear();
            }
            let mut warn = d.says.clone();
            let mut count = String::new();
            let mut listed = Vec::new();
            match dm_shell::lists::series(&d.template, &d.from, &d.to, d.width) {
                Ok(urls) => {
                    listed = urls.clone();
                    if let Some(first) = urls.first() {
                        count = crate::i18n::tn(
                            "{} address: {}||{} addresses, the first {}",
                            urls.len() as u64,
                            &[&urls.len(), first],
                        );
                    }
                }
                Err(e) if typed && warn.is_empty() => warn = series_error(e).to_string(),
                Err(_) => {}
            }
            if warn.is_empty() {
                kept_line(ui, RichText::new(count));
            } else {
                warning_line(ui, &warn);
            }
            // Every address the series makes, so a wrong padding or end is
            // seen before anything is fetched; its room is kept whether or
            // not there is a list yet, and only the rows in view are drawn.
            let row = ui.text_style_height(&egui::TextStyle::Small) + 2.0;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if listed.is_empty() {
                    ui.label(
                        RichText::new(t("Each address the series makes is listed here."))
                            .small()
                            .weak(),
                    );
                }
                egui::ScrollArea::vertical()
                    .id_salt("series-addresses")
                    .min_scrolled_height(row * 6.0)
                    .max_height(row * 6.0)
                    .auto_shrink([false, false])
                    .show_rows(ui, row, listed.len(), |ui, rows| {
                        for url in listed.get(rows).unwrap_or_default() {
                            ui.label(RichText::new(url).small().monospace());
                        }
                    });
            });
            // Greyed until there is an address to make a series of; with
            // one, a press says what is wrong with the range instead.
            let addressed = !d.template.trim().is_empty();
            footer(ui, |ui| {
                ui.add_enabled_ui(addressed, |ui| {
                    if primary(ui, "Start now").clicked()
                        || (addressed && (enter || enter_free(ui)))
                    {
                        add = Some(false);
                    }
                    if ui.button(t("Add for later")).clicked() {
                        add = Some(true);
                    }
                });
                if ui.button(t("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    if !open || cancel {
        app.dialogs.series = None;
        return;
    }
    let Some(later) = add else {
        return;
    };
    match dm_shell::lists::series(&d.template, &d.from, &d.to, d.width) {
        Ok(urls) => {
            let with = AddOptions {
                later,
                queue: if later {
                    MAIN_QUEUE.into()
                } else {
                    String::new()
                },
                ..AddOptions::default()
            };
            match app.add(&urls, with) {
                Ok(n) => {
                    app.said = crate::i18n::Said::counting(
                        "Added {} download.||Added {} downloads.",
                        n as u64,
                        &[&n],
                    );
                    app.dialogs.series = None;
                }
                Err(e) => d_says(&mut app.dialogs.series, e),
            }
        }
        Err(e) => d.says = series_error(e).to_string(),
    }
}

/// The key a file's kind is remembered under in `last_folders`.
pub(super) fn kind_key(name: &str) -> &'static str {
    dm_core::category::by_name(name)
        .unwrap_or(dm_core::Category::Other)
        .folder()
}

/// Why a series cannot be made, in the language in force.
pub(super) fn series_error(e: dm_shell::lists::SeriesError) -> &'static str {
    use dm_shell::lists::SeriesError;
    match e {
        SeriesError::Star => t("The address needs exactly one * to fill in."),
        SeriesError::Backwards => t("The range runs backwards."),
        SeriesError::TooMany => t("Ten thousand addresses is more than a batch."),
        SeriesError::Ends => t("From and To must both be numbers, or both letters of one case."),
    }
}

pub(super) fn d_says(series: &mut Option<Series>, e: String) {
    if let Some(d) = series.as_mut() {
        d.says = e;
    }
}

/// A list of links to tick before adding: the clipboard's, or a site's.
/// The addresses already in dm's list, bare (without a login), for a pick
/// list to offer unticked and say so.
pub(crate) fn listed_addresses(app: &DmApp) -> std::collections::HashSet<String> {
    app.rows()
        .iter()
        .filter(|r| r.state != State::Cancelled)
        .map(|r| bare(&r.url))
        .collect()
}

pub(super) fn pick_list(
    ui: &mut egui::Ui,
    links: &mut [(String, bool)],
    listed: &std::collections::HashSet<String>,
) {
    ui.horizontal(|ui| {
        if ui.button(t("Check all")).clicked() {
            links.iter_mut().for_each(|l| l.1 = true);
        }
        if ui.button(t("Uncheck all")).clicked() {
            links.iter_mut().for_each(|l| l.1 = false);
        }
    });
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .show(ui, |ui| {
            for (link, on) in links.iter_mut() {
                let shown = format::shown_url(link);
                if listed.contains(&bare(link)) {
                    check(ui, on, tf("{} (already in the list)", &[&shown]));
                } else {
                    check(ui, on, shown);
                }
            }
        });
}
