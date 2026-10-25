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
        Command::ShareOnGofile,
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

pub(super) fn menu_item(app: &mut DmApp, ui: &mut egui::Ui, command: Command, label: &str) {
    // "…" where it opens a dialog, as in the menu bar, said
    // by its name alone.
    let shown = if command.asks_more() && !label.ends_with('…') {
        format!("{label}…")
    } else {
        label.to_string()
    };
    let enabled = app.enabled(command);
    let response = ui.add_enabled(enabled, egui::Button::new(shown));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if response.clicked() {
        app.queued.push(command);
        ui.close();
    }
}
