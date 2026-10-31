//! Stop, Resume and Remove on the row under the pointer, where a person is
//! looking, instead of up in the toolbar or down a right-click menu.

use super::*;
use crate::icons::{self, Icon};

/// Draws the buttons over the end of the hovered row. They act on that row,
/// which they select, through the same commands the toolbar runs.
pub(super) fn actions(
    app: &mut DmApp,
    ui: &mut egui::Ui,
    rows: &[std::sync::Arc<Entry>],
    clip: Option<egui::Rect>,
) {
    let Some(clip) = clip else { return };
    // Not over a question, a drag or a row being moved.
    if app.dialogs.asking() || app.dialogs.modal() || app.reordering.is_some() {
        return;
    }
    let ctx = ui.ctx().clone();
    if ctx.dragged_id().is_some() {
        return;
    }
    let Some(pos) = ctx.pointer_hover_pos().filter(|p| clip.contains(*p)) else {
        return;
    };
    // Only where the list itself is under the pointer: a window in front of
    // it (Download complete, Properties) is not the row behind it, and the
    // buttons drew over its words. The buttons' own area counts as the list.
    let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("row-actions"));
    if ctx
        .layer_id_at(pos)
        .is_some_and(|layer| layer != ui.layer_id() && layer != own)
    {
        return;
    }
    let Some((entry, rect)) = rows.iter().find_map(|r| {
        app.row_rects
            .get(&r.id)
            .filter(|rect| rect.contains(pos))
            .map(|rect| (r, *rect))
    }) else {
        return;
    };

    // Named for the row's file, so a screen reader says which Stop this is
    // and it is not the toolbar's.
    let file = crate::app::name_of(entry);
    let mut buttons: Vec<(Icon, Command, String)> = Vec::new();
    match &entry.state {
        State::Running | State::Queued => buttons.push((
            Icon::Stop,
            Command::StopDownload,
            format!("{} {file}", t("Stop")),
        )),
        State::Paused => buttons.push((
            Icon::Resume,
            Command::DownloadNow,
            format!("{} {file}", t("Resume")),
        )),
        State::Failed(_) => buttons.push((
            Icon::Resume,
            Command::DownloadNow,
            format!("{} {file}", t("Retry")),
        )),
        // What a person wants of a finished download: to play it, or to
        // see where it is.
        State::Completed => {
            buttons.push((
                Icon::Resume,
                Command::OpenFile,
                format!("{} {file}", t("Open")),
            ));
            buttons.push((
                Icon::Folder,
                Command::OpenFolder,
                format!("{} {file}", t("Open folder")),
            ));
        }
        State::Cancelled => {}
    }
    // The details window a download no longer opens by itself.
    if !matches!(entry.state, State::Completed | State::Cancelled) {
        buttons.push((
            Icon::Details,
            Command::ShowProgress,
            format!("{} {file}", t("Details")),
        ));
    }
    buttons.push((
        Icon::Delete,
        Command::Remove,
        format!("{} {file}", t("Remove")),
    ));

    let tokens = crate::theme::Tokens::get(&ctx);
    let size = egui::vec2(28.0, 24.0);
    let gap = 2.0;
    let margin = 3.0;
    let width = buttons.len() as f32 * size.x + (buttons.len() as f32 - 1.0) * gap + 2.0 * margin;
    let height = size.y + 2.0 * margin;
    let right = rect.right().min(clip.right()) - 10.0;
    let at = egui::pos2(
        (right - width).max(clip.left()),
        rect.center().y - height / 2.0,
    );
    let mut chosen: Option<Command> = None;
    egui::Area::new(egui::Id::unique("row-actions"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(tokens.control)
                .stroke(egui::Stroke::new(1.0, tokens.border))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::same(margin as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.horizontal(|ui| {
                        for (icon, command, name) in &buttons {
                            let (rect, response) =
                                ui.allocate_exact_size(size, egui::Sense::click());
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(3),
                                    tokens.hover,
                                );
                            }
                            let glyph =
                                egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
                            icons::paint(ui.painter(), glyph, *icon, tokens.tool(*icon, true));
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, name)
                            });
                            if response.on_hover_text(name).clicked() {
                                chosen = Some(*command);
                            }
                        }
                    });
                });
        });
    if let Some(command) = chosen {
        // Acts on this row alone, as a click on it would have selected it.
        if !(app.selected.len() > 1
            && app.selected.contains(&entry.id)
            && command != Command::Remove)
        {
            app.selected = BTreeSet::from([entry.id]);
        }
        app.queued.push(command);
    }
}
