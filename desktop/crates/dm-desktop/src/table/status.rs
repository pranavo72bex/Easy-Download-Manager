//! What a row says of its size and state, and the glyph beside it.

use super::*;

pub(super) fn size_of(entry: &Entry) -> String {
    shown_size(entry).map(format::bytes).unwrap_or_default()
}

/// The size the Size column shows: the file's, or what has arrived of one
/// whose length the server did not say. The column sorts by it.
pub fn shown_size(entry: &Entry) -> Option<u64> {
    entry
        .progress
        .total
        .or(Some(entry.progress.downloaded).filter(|n| *n > 0))
}

/// A finished row whose file is no longer where dm saved it.
pub fn gone(entry: &Entry) -> bool {
    !entry.path.is_empty()
        && (!std::path::Path::new(&entry.path).exists()
            || SUPERSEDED.with(|s| s.borrow().contains(&entry.id)))
}

thread_local! {
    /// Finished rows whose path a row finished since also wrote: the
    /// file there is that one's. A row whose file was deleted turned
    /// "Complete" again when the same address was downloaded anew under
    /// its old name.
    pub(super) static SUPERSEDED: std::cell::RefCell<std::collections::HashSet<dm_core::Id>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Which finished rows another, later one has taken the file of, as the
/// rows now stand.
pub fn set_superseded(rows: &[Entry]) {
    let mut latest: std::collections::HashMap<&str, (u64, dm_core::Id)> =
        std::collections::HashMap::new();
    let finished = rows
        .iter()
        .filter(|r| r.state == dm_core::State::Completed && !r.path.is_empty());
    for row in finished.clone() {
        let at = row.finished_at.unwrap_or(0);
        let held = latest.entry(row.path.as_str()).or_insert((at, row.id));
        // Finished in the same second, the later row: the higher id.
        if (at, row.id) > *held {
            *held = (at, row.id);
        }
    }
    let superseded = finished
        .filter(|r| {
            latest
                .get(r.path.as_str())
                .is_some_and(|(_, id)| *id != r.id)
        })
        .map(|r| r.id)
        .collect();
    SUPERSEDED.with(|s| *s.borrow_mut() = superseded);
}

/// Lays `add` out against the cell's right edge.
/// Text with digits of one width, so numbers line up down a column and
/// do not jitter as they change.
pub fn digits(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(crate::theme::numbers(ui.ctx()))
}

/// The room kept right of a number and its heading, before the next column.
pub(super) const NUMBER_GAP: f32 = 6.0;

pub(super) fn right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> egui::Response) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(NUMBER_GAP);
        add(ui);
    });
}

/// The shape before a row's status, so its state is told by a shape as
/// well as by words and colour: a design review found the words alone.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Glyph {
    Running,
    Paused,
    Waiting,
    Done,
    Failed,
    Cancelled,
}

pub(super) fn glyph_of(entry: &Entry) -> Glyph {
    match &entry.state {
        State::Running => Glyph::Running,
        State::Failed(_) => Glyph::Failed,
        // A tick beside "File missing" said all was well.
        State::Completed if gone(entry) => Glyph::Cancelled,
        State::Completed => Glyph::Done,
        State::Cancelled => Glyph::Cancelled,
        State::Queued => Glyph::Waiting,
        // Waiting for the schedule, or added for later and waiting for its
        // queue: a clock, not the pause of one a person stopped.
        State::Paused if entry.paused_by_schedule || crate::app::waits_for_its_queue(entry) => {
            Glyph::Waiting
        }
        _ => Glyph::Paused,
    }
}

/// Draws `glyph` in a 12-point square, in one colour.
pub(super) fn draw_glyph(ui: &mut egui::Ui, glyph: Glyph, colour: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let line = egui::Stroke::new(1.5, colour);
    match glyph {
        Glyph::Running => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(-3.0, -4.5),
                    c + egui::vec2(4.5, 0.0),
                    c + egui::vec2(-3.0, 4.5),
                ],
                egui::Color32::TRANSPARENT,
                line,
            ));
        }
        Glyph::Paused => {
            for dx in [-2.0, 2.0] {
                p.line_segment([c + egui::vec2(dx, -4.0), c + egui::vec2(dx, 4.0)], line);
            }
        }
        Glyph::Waiting => {
            p.circle_stroke(c, 5.0, line);
            p.line_segment([c, c + egui::vec2(0.0, -3.0)], line);
            p.line_segment([c, c + egui::vec2(2.5, 0.0)], line);
        }
        Glyph::Done => {
            p.line_segment([c + egui::vec2(-4.5, 0.0), c + egui::vec2(-1.5, 3.5)], line);
            p.line_segment([c + egui::vec2(-1.5, 3.5), c + egui::vec2(4.5, -3.5)], line);
        }
        Glyph::Failed => {
            p.circle_stroke(c, 5.0, line);
            p.line_segment([c + egui::vec2(-2.0, -2.0), c + egui::vec2(2.0, 2.0)], line);
            p.line_segment([c + egui::vec2(2.0, -2.0), c + egui::vec2(-2.0, 2.0)], line);
        }
        // A circle struck through: stopped for good, not paused (it drew
        // the pause shape).
        Glyph::Cancelled => {
            p.circle_stroke(c, 5.0, line);
            p.line_segment([c + egui::vec2(-3.5, 3.5), c + egui::vec2(3.5, -3.5)], line);
        }
    }
}

pub(super) fn status_cell(ui: &mut egui::Ui, entry: &Entry, held: Option<&str>, selected: bool) {
    let mut text = status_of(entry);
    // A retry says when it tries again, so a wait is not taken for a stall.
    if entry.state == State::Running {
        if let Some(left) = crate::app::next_try(entry.id) {
            text = format!("{text} · {}", tf("next try in {}", &[&format::span(left)]));
        }
    }
    let tokens = crate::theme::Tokens::get(ui.ctx());
    // On the classic look's selected row every word is in the selection's
    // own colour: red, blue or a faded grey would not read on its fill.
    // The modern selection is a subtle fill, and its words keep theirs.
    let on_row = |colour: egui::Color32| {
        if selected && !tokens.modern {
            tokens.row_selected_text
        } else {
            colour
        }
    };
    let glyph = glyph_of(entry);
    let glyph_colour = match glyph {
        Glyph::Running => tokens.progress(),
        Glyph::Done => tokens.success,
        Glyph::Failed => tokens.error,
        Glyph::Paused | Glyph::Waiting | Glyph::Cancelled => tokens.weak,
    };
    // The bar under the words in the modern look: while running, and kept
    // where it stopped for a paused or failed row (it vanished), dimmed or
    // in the error colour. Not for a row that has fetched nothing.
    let fraction = entry.progress.fraction().unwrap_or(0.0) as f32;
    let bar = tokens.modern
        && match &entry.state {
            State::Running => true,
            State::Paused | State::Failed(_) => fraction > 0.0,
            _ => false,
        };
    let bar_colour = match glyph {
        Glyph::Failed => tokens.error,
        // Where a stopped download got to, in the progress colour a little
        // dimmed: in the grey of its track it was the faintest thing in the
        // row, beside the one button (Resume) that makes a person look.
        // A solid blend toward the list, not a see-through colour; High
        // Contrast keeps the colour whole.
        Glyph::Paused if tokens.high_contrast => tokens.progress(),
        Glyph::Paused => tokens.progress().lerp_to_gamma(tokens.list, 0.35),
        Glyph::Waiting | Glyph::Cancelled => tokens.weak,
        _ => tokens.progress(),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        draw_glyph(ui, glyph, on_row(glyph_colour));
        // Words alone sit on the row's line, as the name and size do; only
        // with a bar under them do they stack (they sat 5 px high).
        let layout = if bar {
            egui::Layout::top_down(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        };
        ui.with_layout(layout, |ui| {
            // Words and bar inside the 32 px row: with the window's 8 px
            // between them and 4 above, the bar fell past the row's edge
            // and was never drawn.
            if bar {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add_space(1.0);
            }
            match &entry.state {
                State::Failed(_) => {
                    // The whole reason on hover, with where to sign in when
                    // the server wants it: in plain words, and below them
                    // what the engine said.
                    // The engine's words named as such, under the plain
                    // ones: two lines saying the same, one in lowercase,
                    // read as a mistake.
                    // The reason in the row itself, as far as the column
                    // shows it: "Error" alone sent a person hovering to
                    // learn that the disk was full.
                    // Without "Error: ", which the red mark before it says:
                    // the reason's own words reach the column's edge first
                    // ("Error: the server has no file …" lost the 404). The whole of it, "Error:" too, on hover.
                    let lead = crate::i18n::fill(t("Error: {}"), &[&""]);
                    let short = text.strip_prefix(lead.as_str()).unwrap_or(&text);
                    let short = crate::format::titled(short);
                    let words = egui::RichText::new(short).color(on_row(tokens.error));
                    let hover = match crate::app::failure_as_said(entry) {
                        Some(raw) => format!("{text}\n\n{}", tf("In dm's own words: {}", &[&raw])),
                        None => text.clone(),
                    };
                    // One tooltip: egui's own for cut words stood over this
                    // one, saying the same.
                    ui.add(
                        egui::Label::new(words)
                            .truncate()
                            .show_tooltip_when_elided(false),
                    )
                    .on_hover_text(hover);
                }
                State::Paused if entry.paused_by_schedule => {
                    let label = ui.label(text);
                    if let Some(why) = held {
                        label.on_hover_text(why);
                    }
                }
                // Kept as history, but the file was moved or deleted since:
                // said, rather than offered to Open as if it were there.
                State::Completed if gone(entry) => {
                    let words = egui::RichText::new(t("File missing"));
                    let classic = !crate::theme::Tokens::get(ui.ctx()).modern;
                    let words = if selected && classic {
                        words
                    } else {
                        words.weak()
                    };
                    ui.label(words)
                        .on_hover_text(format!("{} is no longer there", entry.path));
                }
                // When it finished, beside the word, so a file from last week
                // is found by its day and not only by its name.
                State::Completed => {
                    ui.add(egui::Label::new(digits(ui, text)).truncate());
                    let when = format::when(entry.finished_at);
                    if !when.is_empty() {
                        ui.label(RichText::new(when).weak());
                    }
                }
                // Cut with "…" where the column is narrow, the whole on hover:
                // clipped, "Waiting for Sta" ended mid-word.
                _ => {
                    ui.add(egui::Label::new(digits(ui, text)).truncate());
                }
            }
            if bar {
                let width = ui.available_width().max(60.0);
                let (track, response) =
                    ui.allocate_exact_size(egui::vec2(width, 3.0), egui::Sense::hover());
                // A bar a screen reader reads too, with how far it has come
                // (it was paint alone); the row around it names the
                // file.
                response.widget_info(|| {
                    let mut info = egui::WidgetInfo::labeled(
                        egui::accesskit::Role::ProgressIndicator,
                        true,
                        t("Progress"),
                    );
                    info.value = Some((f64::from(fraction.clamp(0.0, 1.0)) * 100.0).floor());
                    info
                });
                // WinUI's progress bar: a 1 px track under a 3 px bar.
                let line =
                    egui::Rect::from_center_size(track.center(), egui::vec2(track.width(), 1.0));
                ui.painter().rect_filled(line, 0.5, tokens.stroke);
                let mut done = track;
                done.set_width(track.width() * fraction.clamp(0.0, 1.0));
                ui.painter().rect_filled(done, 1.5, on_row(bar_colour));
            }
        });
    });
}
