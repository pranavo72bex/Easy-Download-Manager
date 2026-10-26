use super::*;
/// A number typed into a box, as a desktop's number fields are: not
/// changed by dragging across it, with the text cursor over it. The drag
/// to change, and the resize arrow that announced it, surprised people.
/// Its range is said on hover: 35 in Files at once became 10 without a
/// word.
pub(crate) struct NumberBox<'a>(egui::DragValue<'a>, Option<String>);

pub(crate) fn number_box<Num: egui::emath::Numeric>(value: &mut Num) -> NumberBox<'_> {
    NumberBox(egui::DragValue::new(value).speed(0.0), None)
}

impl NumberBox<'_> {
    pub(crate) fn range<Num: egui::emath::Numeric>(
        self,
        range: std::ops::RangeInclusive<Num>,
    ) -> Self {
        let said = tf(
            "From {} to {}",
            &[&range.start().to_f64(), &range.end().to_f64()],
        );
        NumberBox(self.0.range(range), Some(said))
    }
}

impl egui::Widget for NumberBox<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        /* Text that is not a number leaves the number as it was, which
         * egui does without a word: "abc" in a size floor left 0 there and
         * looked taken. The parser notes the refusal, and
         * leaving the box says so beside it until it is typed in again. */
        let refused = std::rc::Rc::new(std::cell::Cell::new(false));
        let noted = std::rc::Rc::clone(&refused);
        let parser = move |text: &str| {
            let text: String = text
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| if c == '−' { '-' } else { c })
                .collect();
            let parsed = text.parse::<f64>().ok();
            noted.set(parsed.is_none() && !text.is_empty());
            parsed
        };
        let response = ui
            .add(self.0.custom_parser(parser))
            .on_hover_cursor(egui::CursorIcon::Text);
        let said = response.id.with("not a number");
        if response.gained_focus() || response.changed() {
            ui.ctx().data_mut(|d| d.remove::<bool>(said));
        }
        if response.lost_focus() && refused.get() {
            ui.ctx().data_mut(|d| d.insert_temp(said, true));
        }
        if ui.ctx().data(|d| d.get_temp::<bool>(said)).unwrap_or(false) {
            let t = crate::theme::Tokens::get(ui.ctx());
            ui.label(
                RichText::new(crate::i18n::t("Numbers only: the one before was kept"))
                    .small()
                    .color(t.warn),
            );
        }
        // The bottom line a text field has, at rest too: the fields'
        // pass finds a number box only while it is being typed in, so at
        // rest it had none.
        let t = crate::theme::Tokens::get(ui.ctx());
        if t.modern && ui.is_enabled() {
            let focus = response.has_focus();
            let height = if focus { 2.0 } else { 1.0 };
            let rect = response.rect;
            // Stopping where the corners start to curve, as the other
            // fields' line does.
            let inset = f32::from(t.radius) + 0.5;
            let line = egui::Rect::from_min_max(
                egui::pos2(rect.left() + inset, rect.bottom() - height),
                egui::pos2(rect.right() - inset, rect.bottom()),
            );
            ui.painter()
                .rect_filled(line, 0.0, if focus { t.brand } else { t.stroke });
        }
        match self.1 {
            Some(range) => response.on_hover_text(range),
            None => response,
        }
    }
}

/// A setting's words and its number box: the words wrap in the room the
/// box leaves them, and the box stays beside them, centred on them. Laid
/// out as one wrapped line, a long setting (Russian's recheck interval)
/// wrapped its words and left the box hanging after their second line.
pub(super) fn number_row(
    ui: &mut egui::Ui,
    words: &str,
    add: impl FnOnce(&mut egui::Ui, egui::Id),
) {
    setting_row(ui, words, add);
}

/// A heading over a group of settings: a little room above, in the
/// heading's weight, on the 4-point grid.
pub(super) fn section(ui: &mut egui::Ui, words: &str) {
    ui.add_space(12.0);
    // The semibold face, as the window's headings are: strong() only
    // changed the colour, and the groups read as more options.
    ui.label(RichText::new(words).family(crate::theme::bold(ui.ctx())));
    ui.add_space(4.0);
}

/// A dialog's main action: the primary button, filled with the brand
/// colour (the classic look draws it like any other).
pub(crate) fn primary(ui: &mut egui::Ui, text: &'static str) -> egui::Response {
    // The button's words in the language in force.
    primary_worded(ui, crate::i18n::t(text))
}

/// [`primary`], with words already in the language in force.
pub(super) fn primary_worded(ui: &mut egui::Ui, text: &str) -> egui::Response {
    primary_sized(ui, text, 0.0)
}

/// [`primary_worded`], at least `width` wide.
pub(super) fn primary_sized(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let min = egui::vec2(width, 0.0);
    if !t.modern {
        return ui.add(egui::Button::new(text).min_size(min));
    }
    // An accent button: its fill a shade lighter under the pointer and
    // lighter again pressed, as WinUI's AccentButton is.
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for (state, fill) in [
            (&mut w.inactive, t.brand),
            (&mut w.hovered, t.brand_hover),
            (&mut w.active, t.brand_pressed),
        ] {
            state.weak_bg_fill = fill;
            state.bg_fill = fill;
            state.bg_stroke = egui::Stroke::new(1.0, fill);
            state.fg_stroke = egui::Stroke::new(1.0, t.on_brand);
        }
        ui.add(egui::Button::new(RichText::new(text).color(t.on_brand)).min_size(min))
    })
    .inner
}

/// A section's opener: a thin chevron pointing at its name, turning down
/// as it opens, as an expander's does; egui's filled triangle in the
/// classic look.
pub(super) fn header_chevron(ui: &mut egui::Ui, openness: f32, response: &egui::Response) {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        egui::collapsing_header::paint_default_icon(ui, openness, response);
        return;
    }
    let c = response.rect.center();
    let turn = egui::emath::Rot2::from_angle(openness * std::f32::consts::FRAC_PI_2);
    let points = [
        egui::vec2(-2.0, -4.5),
        egui::vec2(2.5, 0.0),
        egui::vec2(-2.0, 4.5),
    ]
    .map(|p| c + turn * p)
    .to_vec();
    ui.painter().line(points, egui::Stroke::new(1.0, t.weak));
}

/// A drop-down list with WinUI's chevron, in the secondary colour, where
/// egui draws a filled triangle.
pub(crate) fn combo(salt: impl std::hash::Hash + std::fmt::Debug) -> egui::ComboBox<'static> {
    egui::ComboBox::from_id_salt(salt).icon(chevron)
}

/// One choice in a drop-down's list: egui's selectable item, and the
/// picked one marked with the accent pill at its left, as WinUI's list
/// items are. It had only the subtle fill.
pub(crate) fn option(
    ui: &mut egui::Ui,
    selected: bool,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    let response = ui.selectable_label(selected, label);
    let t = crate::theme::Tokens::get(ui.ctx());
    if selected && t.modern {
        let tall = (response.rect.height() - 8.0).clamp(8.0, 16.0);
        let pill = egui::Rect::from_center_size(
            egui::pos2(response.rect.left() + 1.5, response.rect.center().y),
            egui::vec2(3.0, tall),
        );
        ui.painter().rect_filled(pill, 1.5, t.brand);
    }
    response
}

/// [`option`] for a value, as egui's `selectable_value`: a click picks
/// `value`, and the response says it changed when it did.
pub(crate) fn pick<V: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut V,
    value: V,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    let mut response = option(ui, *current == value, label);
    if response.clicked() && *current != value {
        *current = value;
        response.mark_changed();
    }
    response
}

pub(super) fn chevron(
    ui: &egui::Ui,
    rect: egui::Rect,
    visuals: &egui::style::WidgetVisuals,
    _open: bool,
) {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        // egui's own triangle, for the classic look.
        let rect = egui::Rect::from_center_size(
            rect.center(),
            egui::vec2(rect.width() * 0.7, rect.height() * 0.45),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![rect.left_top(), rect.right_top(), rect.center_bottom()],
            visuals.fg_stroke.color,
            visuals.fg_stroke,
        ));
        return;
    }
    let c = rect.center();
    let colour = if ui.is_enabled() { t.weak } else { t.disabled };
    let glyph = egui::Rect::from_center_size(c, egui::vec2(12.0, 12.0));
    if crate::icons::glyph(
        ui.painter(),
        glyph,
        crate::icons::fluent::CHEVRON_DOWN,
        colour,
    ) {
        return;
    }
    let line = egui::Stroke::new(1.0, colour);
    ui.painter().line(
        vec![
            c + egui::vec2(-4.5, -2.0),
            c + egui::vec2(0.0, 2.5),
            c + egui::vec2(4.5, -2.0),
        ],
        line,
    );
}

/// How much of its 24 points' padding each of these tabs keeps so that
/// all of them fit in `room`: all of it where they fit, else down to 12.
/// At 150% text Options' tabs stood 19 points past the dialog's edge.
pub(super) fn tabs_padding(ui: &egui::Ui, labels: &[&str], room: f32) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let words: f32 = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap((*l).to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .sum();
    let gaps = ui.spacing().item_spacing.x * labels.len().saturating_sub(1) as f32;
    let each = (room - words - gaps) / labels.len().max(1) as f32;
    each.clamp(12.0, 24.0)
}

/// One item of a row of pages, as WinUI's selector bar draws it: words in
/// the secondary colour, the picked one in the primary colour with the
/// accent pill under it, the subtle fill under the pointer. Classic keeps
/// egui's selectable label.
/// `padding` is the room beside its words, both sides together.
pub(super) fn selector(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    padding: f32,
) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return ui.selectable_label(selected, label);
    }
    // Laid out in the colour it is drawn in: a colour given here wins over
    // the one given when painting, and every word came out primary.
    let words = if selected { t.text } else { t.weak };
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::TextStyle::Button.resolve(ui.style()),
        words,
    );
    let size = egui::vec2(galley.size().x + padding, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label)
    });
    let fill = if response.is_pointer_button_down_on() {
        t.pressed
    } else if response.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect.shrink2(egui::vec2(0.0, 4.0)), t.radius, fill);
    painter.galley(
        rect.center() - galley.size() / 2.0 - egui::vec2(0.0, 1.0),
        galley,
        words,
    );
    if selected {
        let pill = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - 3.5),
            egui::vec2(16.0, 3.0),
        );
        painter.rect_filled(pill, 1.5, t.brand);
    }
    response
}

/// A check box as WinUI draws one: a 20 px box outlined in the strong
/// stroke, filled with the accent and a check in the colour on it once
/// ticked; lighter under the pointer.
pub(crate) fn check(
    ui: &mut egui::Ui,
    on: &mut bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let checked = *on;
    let text = choice_words(ui, checked, text.into());
    ui.scope(|ui| {
        choice_look(ui, checked);
        ui.checkbox(on, text)
    })
    .inner
}

/// A radio button as WinUI draws one, in [`check`]'s colours.
pub(crate) fn radio<V: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut V,
    value: V,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let checked = *current == value;
    let text = choice_words(ui, checked, text.into());
    ui.scope(|ui| {
        choice_look(ui, checked);
        ui.radio_value(current, value, text)
    })
    .inner
}

/// [`radio`] for a choice that is not one value of a field: picked when
/// `checked`, its response clicked when chosen.
pub(crate) fn radio_flag(
    ui: &mut egui::Ui,
    checked: bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let text = choice_words(ui, checked, text.into());
    ui.scope(|ui| {
        choice_look(ui, checked);
        ui.radio(checked, text)
    })
    .inner
}

/// A choice's words in the text colour: egui takes them from the colour
/// the check mark is drawn in, which is the one on the accent.
pub(super) fn choice_words(
    ui: &egui::Ui,
    checked: bool,
    text: egui::WidgetText,
) -> egui::WidgetText {
    let t = crate::theme::Tokens::get(ui.ctx());
    if checked && t.modern {
        text.color(t.text)
    } else {
        text
    }
}

pub(super) fn choice_look(ui: &mut egui::Ui, checked: bool) {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return;
    }
    // WinUI's 20 px box; the window's own icon width is a drop-down's.
    ui.spacing_mut().icon_width = 20.0;
    // ControlAltFillColorSecondary, Tertiary and Quarternary behind an
    // empty box.
    let alt = |light: u8, dark: u8| {
        if t.dark {
            egui::Color32::from_white_alpha(dark)
        } else {
            egui::Color32::from_black_alpha(light)
        }
    };
    // Ticked but disabled, AccentFillColorDisabled: grey, not a pale
    // accent that read as ticked and live.
    let enabled = ui.is_enabled();
    let disabled = if t.dark {
        egui::Color32::from_white_alpha(0x28)
    } else {
        egui::Color32::from_black_alpha(0x37)
    };
    let accent = |live: egui::Color32| if enabled { live } else { disabled };
    let w = &mut ui.visuals_mut().widgets;
    let states = [
        (&mut w.inactive, accent(t.brand), alt(0x06, 0x00)),
        (&mut w.hovered, accent(t.brand_hover), alt(0x0f, 0x0b)),
        (&mut w.active, accent(t.brand_pressed), alt(0x18, 0x12)),
    ];
    for (state, accent, empty) in states {
        if checked {
            state.bg_fill = accent;
            state.bg_stroke = egui::Stroke::new(1.0, accent);
            state.fg_stroke = egui::Stroke::new(1.5, t.on_brand);
        } else {
            state.bg_fill = empty;
            state.bg_stroke = egui::Stroke::new(1.0, t.stroke);
        }
    }
}

/// A text field with a name a screen reader says.
pub(super) fn field(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    width: f32,
) -> egui::Response {
    let name = ui.label(label);
    // The field's own frame is left off and drawn here: egui draws a
    // focused one in the selection's stroke, which `selection_seen` turns
    // white (the colour selected words read in), and a white frame showed
    // at the rounded corners.
    let corner = ui.visuals().widgets.inactive.corner_radius;
    let fill = ui.visuals().extreme_bg_color;
    let edge = ui.visuals().widgets.inactive.bg_stroke;
    let frame = ui.painter().add(egui::Shape::Noop);
    let field = ui
        .scope(|ui| {
            selection_seen(ui);
            // No wider than the room there is: at 200% text a fixed width
            // pushed Options off the window.
            let width = width.min(ui.available_width()).max(40.0);
            ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(width)
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2))),
            )
        })
        .inner
        .labelled_by(name.id);
    // Focused, the window's own pass draws the frame and the accent line
    // (`underline_fields`); a second outline under it blurred the corners.
    let stroke = if field.has_focus() {
        egui::Stroke::NONE
    } else {
        edge
    };
    ui.painter().set(
        frame,
        egui::epaint::RectShape::new(field.rect, corner, fill, stroke, egui::StrokeKind::Inside),
    );
    field
}

/// Selected text drawn in the list's selection colours, which read on a
/// field: the looks' own selection fill is a pale grey for the tree, and a
/// link selected in Add URL could not be told from one that was not.
pub(super) fn selection_seen(ui: &mut egui::Ui) {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let selection = &mut ui.visuals_mut().selection;
    selection.bg_fill = tokens.text_selection;
    selection.stroke.color = egui::Color32::WHITE;
}

/// A dialog's warning, under its fields, with its room kept while there is
/// nothing to say: appearing, it grew the dialog, which is placed by its
/// middle, and the whole dialog jumped. Two lines, which is what
/// the dialogs' warnings take at their widths.
pub(super) fn warning_line(ui: &mut egui::Ui, says: &str) {
    kept_line(ui, RichText::new(says).color(warn(ui)));
}

/// [`warning_line`]'s room, holding any words.
pub(super) fn kept_line(ui: &mut egui::Ui, says: RichText) {
    let line = ui.text_style_height(&egui::TextStyle::Body);
    let room = line * 2.0 + ui.spacing().item_spacing.y;
    // As wide as what is above it: the room it could have would widen
    // a dialog that sizes itself to its contents.
    let width = ui.min_rect().width().max(160.0);
    ui.allocate_ui(egui::vec2(width, room), |ui| {
        ui.set_min_size(egui::vec2(width, room));
        if !says.text().is_empty() {
            ui.add(egui::Label::new(says).wrap());
        }
    });
}

/// Enter pressed in a field: the dialog's default button, as in any
/// dialog. A one-line field lets the keyboard go on Enter.
pub(super) fn entered(field: &egui::Response) -> bool {
    field.lost_focus() && field.ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

/// Enter pressed in the dialog in front with none of its controls holding
/// the keyboard: its default button, as a field's Enter is. A button of its
/// own that has the keyboard takes Enter for itself instead.
pub(super) fn enter_free(ui: &egui::Ui) -> bool {
    let ctx = ui.ctx();
    let in_front = ctx.memory(|m| m.top_modal_layer()) == Some(ui.layer_id());
    let own = ctx
        .memory(|m| m.focused())
        .and_then(|id| ctx.read_response(id))
        .is_some_and(|r| r.layer_id == ui.layer_id());
    in_front && !own && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// A number's field has all of its digits selected as it takes the
/// keyboard, so what is typed replaces them: "500" typed into the limit's
/// 0 made 0500.
pub(super) fn select_on_focus(ui: &egui::Ui, field: &egui::Response, text: &str) {
    let key = field.id.with("dm-selected-on-focus");
    let done = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    if field.has_focus() && !done {
        let mut state = egui::TextEdit::load_state(ui.ctx(), field.id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::select_all_str(text)));
        state.store(ui.ctx(), field.id);
        ui.data_mut(|d| d.insert_temp(key, true));
    } else if !field.has_focus() && done {
        ui.data_mut(|d| d.remove::<bool>(key));
    }
}

/// Gives a field the keyboard when `now` says, and passes it on.
pub(super) fn focused(field: egui::Response, now: bool) -> egui::Response {
    if now {
        field.request_focus();
    }
    field
}

/// A folder's field: its name above, the path, and Browse… beside it.
pub(super) fn folder_field(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    platform: &dyn crate::backend::Platform,
) {
    let name = ui.label(label);
    ui.horizontal(|ui| {
        // Browse keeps its room beside it, however narrow the dialog.
        let wide = (ui.available_width() - 120.0).clamp(120.0, 380.0);
        ui.add(egui::TextEdit::singleline(text).desired_width(wide))
            .labelled_by(name.id);
        browse(ui, platform, text);
    });
}

/// A Browse… button beside a folder's field: the folder picked goes in it.
/// Where the desktop has no folder chooser (a Linux desktop with neither
/// the portal's nor zenity), says so beside the button and to type the
/// path: the chooser's absence looked like a Cancel, and the button did
/// nothing.
pub(super) fn browse(
    ui: &mut egui::Ui,
    platform: &dyn crate::backend::Platform,
    folder: &mut String,
) {
    let missing = ui.scope_id().with("no-folder-chooser");
    if let Some(chose) = browse_button(ui, platform, folder) {
        if chose {
            ui.data_mut(|d| d.remove::<bool>(missing));
        } else {
            ui.data_mut(|d| d.insert_temp(missing, true));
        }
    }
    if ui.data(|d| d.get_temp::<bool>(missing)).unwrap_or(false) {
        ui.colored_label(warn(ui), t("No folder picker here: type the path."));
    }
}

/// The Browse… button itself: `None` when not pressed, else whether the
/// desktop had a folder chooser to ask.
pub(super) fn browse_button(
    ui: &mut egui::Ui,
    platform: &dyn crate::backend::Platform,
    folder: &mut String,
) -> Option<bool> {
    if !ui.button(t("Browse…")).clicked() {
        return None;
    }
    if !platform.can_choose_folder() {
        return Some(false);
    }
    if let Some(picked) = platform.choose_folder() {
        *folder = picked.display().to_string();
    }
    Some(true)
}

/// A row of settings as WinUI's toggle switch draws it: the words at the
/// left, a 40 by 20 pill at the right that is the accent when on and an
/// outline when off, the thumb sliding to the side that is chosen. The
/// classic look keeps its check box.
pub(crate) fn toggle_row(
    ui: &mut egui::Ui,
    on: &mut bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return check(ui, on, text);
    }
    let text: egui::WidgetText = text.into();
    let words = text.text().to_string();
    let galley = text.into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let size = egui::vec2(ui.available_width().max(120.0), 36.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, enabled, *on, &words));
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, t.radius, t.hover);
    }
    painter.galley(
        egui::pos2(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    let pill = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 4.0 - 20.0, rect.center().y),
        egui::vec2(40.0, 20.0),
    );
    let round = 10.0;
    if *on {
        painter.rect_filled(pill, round, if enabled { t.brand } else { t.disabled });
    } else {
        painter.rect_stroke(
            pill,
            round,
            egui::Stroke::new(1.0, t.text2),
            egui::StrokeKind::Inside,
        );
    }
    let grow = if response.hovered() || response.is_pointer_button_down_on() {
        1.0
    } else {
        0.0
    };
    let (x, radius, colour) = if *on {
        (pill.right() - 10.0, 6.0 + grow, t.on_brand)
    } else {
        (pill.left() + 10.0, 5.0 + grow, t.text2)
    };
    painter.circle_filled(egui::pos2(x, pill.center().y), radius, colour);
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            t.radius,
            egui::Stroke::new(2.0, t.focus_outer),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// The sizes of text the window offers, by name and as the window's zoom:
/// medium is the design.
pub(crate) const TEXT_SIZES: [(&str, f32); 3] = [("Small", 0.85), ("Medium", 1.0), ("Large", 1.2)];

/// The name of the text size nearest to a zoom.
pub(crate) fn text_size_named(zoom: f32) -> &'static str {
    TEXT_SIZES
        .iter()
        .min_by(|a, b| (a.1 - zoom).abs().total_cmp(&(b.1 - zoom).abs()))
        .map_or("Medium", |s| s.0)
}

/// One row of an Appearance-style list: the name at the left, the control
/// at the right, and a hairline under it, as the settings of a modern app
/// are laid out. The control is drawn right to left, so a drop-down
/// stands against the edge.
pub(crate) fn setting_row(
    ui: &mut egui::Ui,
    name: &str,
    control: impl FnOnce(&mut egui::Ui, egui::Id),
) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, 40.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(40.0);
            // The words wrap in the room the control leaves them, and the
            // control stays beside them, centred on them.
            let room = (width - 120.0).max(120.0);
            let label = ui
                .allocate_ui_with_layout(
                    egui::vec2(room, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                    |ui| ui.add(egui::Label::new(name).wrap()),
                )
                .inner;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Clear of the scroll bar, which floats over the right edge.
                ui.add_space(8.0);
                control(ui, label.id)
            });
        },
    );
    let y = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, t.border));
    ui.add_space(1.0);
}

/// The look's choice as a row of icon buttons in one rounded tray, the
/// chosen one raised: a monitor for "as the computer is", a sun, a moon and
/// a window for the classic look. Each says its name when the pointer rests
/// on it, and to a screen reader.
pub(crate) fn theme_segments(ui: &mut egui::Ui, current: &mut String) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let side = 32.0;
    let pad = 2.0;
    let count = crate::dialogs::THEMES.len() as f32;
    let (tray, mut whole) = ui.allocate_exact_size(
        egui::vec2(count * side + (count + 1.0) * pad, side + 2.0 * pad),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(tray, t.radius.saturating_add(2), t.control);
    ui.painter().rect_stroke(
        tray,
        t.radius.saturating_add(2),
        egui::Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    for (n, (word, label)) in crate::dialogs::THEMES.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            tray.min + egui::vec2(pad + n as f32 * (side + pad), pad),
            egui::vec2(side, side),
        );
        let name = crate::i18n::t(label);
        let mut response = ui
            .interact(
                cell,
                ui.scope_id().with(("theme", *word)),
                egui::Sense::click(),
            )
            .on_hover_text(name);
        let chosen = *current == *word;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, true, chosen, name)
        });
        if response.clicked() && !chosen {
            *current = (*word).to_string();
            response.mark_changed();
            whole.mark_changed();
        }
        let painter = ui.painter();
        if chosen {
            painter.rect_filled(cell, t.radius, t.control_pressed);
            painter.rect_stroke(
                cell,
                t.radius,
                egui::Stroke::new(1.0, t.stroke),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(cell, t.radius, t.hover);
        }
        let ink = if chosen { t.text } else { t.weak };
        theme_icon(painter, cell.center(), word, egui::Stroke::new(1.5, ink));
        if response.has_focus() {
            painter.rect_stroke(
                cell,
                t.radius,
                egui::Stroke::new(2.0, t.focus_outer),
                egui::StrokeKind::Inside,
            );
        }
    }
    whole
}

/// A 16 px line drawing for a look, centred on `c`.
fn theme_icon(painter: &egui::Painter, c: egui::Pos2, word: &str, stroke: egui::Stroke) {
    let ink = stroke.color;
    match word {
        // A monitor on its stand.
        "system" => {
            let screen =
                egui::Rect::from_center_size(c - egui::vec2(0.0, 1.5), egui::vec2(15.0, 10.0));
            painter.rect_stroke(screen, 2.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment(
                [c + egui::vec2(-3.5, 6.0), c + egui::vec2(3.5, 6.0)],
                stroke,
            );
            painter.line_segment([c + egui::vec2(0.0, 3.5), c + egui::vec2(0.0, 6.0)], stroke);
        }
        // A sun: a disc and eight rays.
        "light" => {
            painter.circle_stroke(c, 3.5, stroke);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let d = egui::vec2(a.cos(), a.sin());
                painter.line_segment([c + d * 6.0, c + d * 8.0], stroke);
            }
        }
        // A moon: the part of a disc outside a second, nearer one corner.
        "dark" => {
            let (r1, r2) = (7.0_f32, 6.0_f32);
            let bite = c + egui::vec2(3.5, -2.5);
            let ring = |centre: egui::Pos2, r: f32| {
                (0..=48)
                    .map(|k| {
                        let a = k as f32 / 48.0 * std::f32::consts::TAU;
                        centre + egui::vec2(a.cos(), a.sin()) * r
                    })
                    .collect::<Vec<_>>()
            };
            let mut edge: Vec<egui::Pos2> = ring(c, r1)
                .into_iter()
                .filter(|p| p.distance(bite) >= r2)
                .collect();
            // The inner arc, walked back from where the outer one ends.
            let mut inner: Vec<egui::Pos2> = ring(bite, r2)
                .into_iter()
                .filter(|p| p.distance(c) <= r1)
                .collect();
            inner.reverse();
            edge.extend(inner);
            if let Some(first) = edge.first().copied() {
                edge.push(first);
            }
            painter.add(egui::Shape::line(edge, stroke));
        }
        // A window with its title bar: the classic look.
        _ => {
            let frame = egui::Rect::from_center_size(c, egui::vec2(15.0, 12.0));
            painter.rect_stroke(frame, 1.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment(
                [
                    frame.left_top() + egui::vec2(0.0, 3.5),
                    frame.right_top() + egui::vec2(0.0, 3.5),
                ],
                stroke,
            );
            painter.circle_filled(frame.left_top() + egui::vec2(3.0, 1.8), 0.8, ink);
        }
    }
}
