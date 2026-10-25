//! One cell and the header above the columns.

use super::*;

/// One cell: what a column says about a download.
pub(super) fn cell(
    ui: &mut egui::Ui,
    column: Column,
    entry: &Entry,
    held: Option<&str>,
    selected: bool,
    planned: Option<&str>,
    name: &str,
) {
    match column {
        Column::Name => {
            ui.horizontal(|ui| {
                // The file type's picture goes before each name.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
                crate::icons::file(ui.painter(), rect, crate::app::kind_of(entry));
                // Drawn in reading order where it is Arabic or Hebrew.
                let drawn = ui.label(&*crate::bidi::visual(name));
                crate::bidi::said_as(&drawn, name);
                drawn.on_hover_text(format::shown_url(&entry.url));
            });
        }
        // The queue's name, dm's own in the language in force.
        Column::Queue => {
            if !entry.queue.is_empty() {
                let name = crate::i18n::name(&entry.queue);
                ui.label(&name).on_hover_text(name);
            }
        }
        // Numbers against the column's right edge, so their digits line up
        // down the list (a design review: they were ragged on the left).
        Column::Size => {
            right(ui, |ui| ui.label(digits(ui, size_of(entry))));
        }
        Column::Status => status_cell(ui, entry, held, selected),
        Column::TimeLeft => {
            if entry.state == State::Running {
                right(ui, |ui| {
                    let left = entry.progress.eta.map(format::span).unwrap_or_default();
                    ui.label(digits(ui, left))
                });
            }
        }
        Column::Rate => {
            if entry.state == State::Running && entry.progress.speed > 1.0 {
                right(ui, |ui| {
                    ui.label(digits(ui, format::rate(entry.progress.speed)))
                });
            }
        }
        Column::LastTry => {
            ui.label(format::when(entry.finished_at));
        }
        Column::Description => {
            ui.label(&entry.description);
        }
        Column::Referrer => {
            if !entry.referrer.is_empty() {
                ui.label(format::shown_url(&entry.referrer))
                    .on_hover_text(&entry.referrer);
            }
        }
        Column::Folder => {
            let mut folder = crate::app::folder_of(entry);
            if folder.is_empty() {
                folder = planned.unwrap_or_default().to_string();
            }
            if !folder.is_empty() {
                // A cell cut short shows its whole text on hover by itself;
                // a tooltip of our own drew the path twice.
                let drawn = ui.label(&*crate::bidi::visual(&folder));
                crate::bidi::said_as(&drawn, &folder);
            }
        }
    }
}

pub(super) fn header_only(app: &mut DmApp, ui: &mut egui::Ui) {
    // The name as wide as over a list with rows, so the first row moves
    // no heading.
    let shown: Vec<Column> = app
        .column_order
        .iter()
        .copied()
        .filter(|c| !app.hidden_columns.contains(c))
        .collect();
    let name = name_width(&shown, ui.available_width());
    ui.add_space(10.0);
    egui::ScrollArea::horizontal()
        .id_salt("headings-sideways")
        // With no rows, a bar right under the headings looked like a
        // stray line; there is nothing to scroll to.
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Each heading in its column's own width, as over a list
                // with rows: laid end to end, a number heading's right
                // alignment took all the room left and the rest vanished.
                let tall = ui.spacing().interact_size.y;
                for column in app.column_order.clone() {
                    if !app.hidden_columns.contains(&column) {
                        let wide = if column == Column::Name {
                            name
                        } else {
                            initial(column)
                        };
                        ui.allocate_ui_with_layout(
                            egui::vec2(wide, tall),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(wide);
                                heading(app, ui, column);
                            },
                        );
                    }
                }
            });
        });
}

/// A heading that sorts by its column, saying which way.
/// The room before the first column's words in the modern look, where a
/// selected row's indicator is drawn.
pub(super) const LEAD: f32 = 8.0;

pub(super) fn heading(app: &mut DmApp, ui: &mut egui::Ui, column: Column) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let marker = match app.sort {
        Some((c, true)) if c == column => " ↑",
        Some((c, false)) if c == column => " ↓",
        _ => "",
    };
    // Grid headers in the modern look: semibold, in the secondary text colour.
    let text = RichText::new(format!("{}{marker}", column.title()));
    let text = if t.modern {
        text.family(crate::theme::bold(ui.ctx())).color(t.text2)
    } else {
        text.strong()
    };
    // Over the right edge where the numbers under it are.
    let numbers = matches!(column, Column::Size | Column::TimeLeft | Column::Rate);
    let response = if numbers {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Clear of the next column's heading, which it ran into;
            // the numbers under it keep the same gap.
            ui.add_space(NUMBER_GAP);
            ui.add(egui::Button::new(text).frame(false))
        })
        .inner
    } else {
        ui.add(egui::Button::new(text).frame(false))
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, column.title()));
    if response.clicked() {
        app.sort_by(column);
    }
    response.context_menu(|ui| app.columns_menu(ui));
}
