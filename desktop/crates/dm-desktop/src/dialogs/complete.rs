use super::*;
/// One "Download complete" window for everything that finished, each file
/// with its own Open and Open folder: a window per file stacked them, five
/// for a five-file series, and a click on one half-hidden behind another
/// opened the wrong file.
pub(super) fn complete(app: &mut DmApp, ctx: &egui::Context) {
    let mut rows: Vec<dm_core::Entry> = app
        .dialogs
        .complete
        .iter()
        .filter_map(|id| app.row(*id).cloned())
        .collect();
    // The newest first, where the eye lands: the file just finished sat
    // below the fold, under the older ones.
    rows.sort_by_key(|r| std::cmp::Reverse(r.finished_at));
    app.dialogs
        .complete
        .retain(|id| rows.iter().any(|r| r.id == *id));
    if rows.is_empty() {
        return;
    }
    // Not over a question waiting for its answer: it opened over Exit, its
    // Open folder under the pointer, and took the click. The
    // files wait in the list and the window opens once it is answered.
    // Nor over a dialog that keeps the clicks: behind Scheduler its Close
    // closed it, and over it, it hid the Scheduler's days.
    // It opens once the dialog is done.
    if app.dialogs.asking() || app.dialogs.modal() {
        return;
    }
    let mut done: Vec<Id> = Vec::new();
    let mut close = false;
    let mut never_again = false;
    let mut with: Vec<String> = Vec::new();
    let mut shut = true;
    // In the corner above the status bar, out of the way of the rows a
    // person is clicking while a batch finishes, rather than over them in
    // the middle; resizable, and no wider than the window.
    let screen = ctx.content_rect();
    let width = (screen.width() - 32.0).clamp(280.0, 440.0);
    Dialog::new(t("Download complete"))
        .id(window_id("Download complete"))
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .default_pos(screen.right_bottom() - egui::vec2(16.0, 40.0))
        .default_width(width)
        .max_width(width)
        .min_height(160.0)
        .default_height(360.0_f32.min(screen.height() - 120.0))
        // Never most of the window: at 150% text it filled it and ran off
        // its bottom; the list inside scrolls instead.
        .max_height((screen.height() * 0.6).max(160.0))
        .open(&mut shut)
        .show(ctx, |ui| {
            if rows.len() > 1 {
                ui.label(tf("{} downloads finished.", &[&rows.len()]));
                ui.separator();
            }
            // Every file reachable: the list scrolls in what height the window
            // has, its buttons below it always in view.
            // As tall as the window may be, less its words and buttons: from
            // what was left of a window growing into its size, thirteen
            // files showed one at a time.
            let list = (screen.height() * 0.6 - 140.0).max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("complete-list")
                .auto_shrink([false, true])
                .max_height(list)
                .min_scrolled_height(list)
                .show(ui, |ui| {
                    // Several in two lines each, the name with its size and
                    // the buttons with the folder: in four, thirteen files
                    // showed under three at a time.
                    let several = rows.len() > 1;
                    for row in &rows {
                        let name = name_of(row);
                        let size = crate::table::digits(
                            ui,
                            format::bytes(row.progress.total.unwrap_or(row.progress.downloaded)),
                        );
                        let folder = |ui: &mut egui::Ui| {
                            let drawn = ui.add(
                                egui::Label::new(
                                    RichText::new(&*crate::bidi::visual(&row.path))
                                        .small()
                                        .weak(),
                                )
                                .truncate(),
                            );
                            crate::bidi::said_as(&drawn, &row.path);
                            drawn.on_hover_text(&row.path);
                        };
                        // A long name is cut short, all of it on hover, so the
                        // buttons stay on screen.
                        ui.horizontal(|ui| {
                            if several {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(size.clone().weak());
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                let drawn = ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&*crate::bidi::visual(&name))
                                                            .strong(),
                                                    )
                                                    .truncate(),
                                                );
                                                crate::bidi::said_as(&drawn, &name);
                                                drawn.on_hover_text(&name);
                                            },
                                        );
                                    },
                                );
                            } else {
                                let drawn = ui.add(
                                    egui::Label::new(
                                        RichText::new(&*crate::bidi::visual(&name)).strong(),
                                    )
                                    .truncate(),
                                );
                                crate::bidi::said_as(&drawn, &name);
                                drawn.on_hover_text(&name);
                            }
                        });
                        if !several {
                            ui.label(size);
                            folder(ui);
                        }
                        ui.horizontal(|ui| {
                            // Named for its file where there are several, so a
                            // screen reader says which Open this is.
                            let named = |ui: &mut egui::Ui, word: &str, said: String| {
                                let button = ui.button(word);
                                if rows.len() > 1 {
                                    button.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::Role::Button, true, &said)
                                    });
                                }
                                button.clicked()
                            };
                            if named(ui, t("Open"), tf("Open {}", &[&name])) {
                                app.open_file(&row.path);
                                done.push(row.id);
                            }
                            if named(ui, t("Open with…"), tf("Open {} with…", &[&name])) {
                                with.push(row.path.clone());
                                done.push(row.id);
                            }
                            if named(ui, t("Open folder"), tf("Open the folder of {}", &[&name])) {
                                app.reveal_file(&row.path);
                                done.push(row.id);
                            }
                            if several {
                                folder(ui);
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
            footer(ui, |ui| {
                close = ui.button(t("Close")).clicked();
                // As the window's own choice, not only Options': a
                // "Don't show this dialog again".
                ui.checkbox(&mut never_again, t("Don't show this window again"));
            });
        });
    if never_again {
        let mut wanted = app.applied.clone();
        wanted.show_complete = false;
        if let Err(why) = app.apply_settings(wanted) {
            app.said = why.into();
        } else {
            app.said = crate::i18n::Said::new(
                "This window won't open again. Turn it on in Options › General.",
                &[],
            );
        }
        app.dialogs.complete.clear();
    }
    if close || !shut {
        app.dialogs.complete.clear();
    }
    for id in done {
        app.dialogs.complete.remove(&id);
    }
    for path in with {
        app.open_with_chosen(&path);
    }
}
