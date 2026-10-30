//! The menu on a row.

use super::*;

/// The row menu.
pub(super) fn row_menu(app: &mut DmApp, ui: &mut egui::Ui) {
    for command in [
        Command::OpenFile,
        Command::OpenWith,
        Command::Preview,
        Command::OpenFolder,
        Command::ExtractHere,
        Command::ExtractTo,
        Command::MoveRename,
        Command::Redownload,
    ] {
        menu_item(app, ui, command, command.title());
    }
    ui.separator();
    menu_item(
        app,
        ui,
        Command::DownloadNow,
        app.title_of(Command::DownloadNow),
    );
    menu_item(app, ui, Command::StopDownload, t("Stop download"));
    menu_item(app, ui, Command::UpdateLink, t("Update download link"));
    menu_item(app, ui, Command::CopyLink, t("Copy link"));
    menu_item(app, ui, Command::Remove, t("Remove"));
    menu_item(app, ui, Command::DeleteFile, Command::DeleteFile.title());
    delete_after_menu(app, ui);
    ui.separator();
    menu_item(app, ui, Command::MoveToQueue, t("Move to queue"));
    menu_item(
        app,
        ui,
        Command::RemoveFromQueue,
        Command::RemoveFromQueue.title(),
    );
    // Its place in the queue, from the row itself: only the File menu had
    // them.
    menu_item(app, ui, Command::MoveUp, Command::MoveUp.title());
    menu_item(app, ui, Command::MoveDown, Command::MoveDown.title());
    ui.separator();
    menu_item(app, ui, Command::Properties, t("Properties"));
}

/// The times "Delete file after" offers, in seconds, in the language in
/// force; `None` keeps the file.
pub fn delete_after_times() -> [(&'static str, Option<u64>); 4] {
    [
        (t("Never"), None),
        (t("10 minutes"), Some(600)),
        (t("1 hour"), Some(3600)),
        (t("1 day"), Some(86_400)),
    ]
}

/// "Delete file after ▸": the file goes to the Trash, and its row with it,
/// that long after it finished. For rows that are not failed or cancelled,
/// which have no file to delete.
fn delete_after_menu(app: &mut DmApp, ui: &mut egui::Ui) {
    let targets: Vec<Entry> = app
        .targets()
        .into_iter()
        .filter(|r| !matches!(r.state, State::Failed(_) | State::Cancelled))
        .collect();
    ui.add_enabled_ui(!targets.is_empty(), |ui| {
        ui.menu_button(t("Delete file after"), |ui| {
            // The one in force ticked where all the rows share it.
            let shared = targets
                .first()
                .map(|r| r.delete_after)
                .filter(|first| targets.iter().all(|r| r.delete_after == *first));
            for (label, seconds) in delete_after_times() {
                let on = shared == Some(seconds);
                if ui.add(egui::Button::new(label).selected(on)).clicked() {
                    for row in &targets {
                        app.backend.set_delete_after(row.id, seconds);
                    }
                    app.refresh();
                    ui.close();
                }
            }
        });
    });
}

pub(super) fn menu_item(app: &mut DmApp, ui: &mut egui::Ui, command: Command, label: &str) {
    // "…" where it opens a dialog, as in the menu bar, said
    // by its name alone.
    let shown = if command.asks_more() && !label.ends_with('…') {
        format!("{label}…")
    } else {
        label.to_string()
    };
    let enabled = app.enabled(command);
    // 32 tall one against the next, as the menu bar's items are.
    ui.spacing_mut().item_spacing.y = 0.0;
    let response = ui.add_enabled(enabled, egui::Button::new(shown));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if response.clicked() {
        app.queued.push(command);
        ui.close();
    }
}
