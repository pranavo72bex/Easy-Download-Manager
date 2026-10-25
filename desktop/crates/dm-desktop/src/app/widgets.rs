use super::*;

/// The ◀ ▶ pair that moves a column or a toolbar button a place earlier or
/// later, named for a screen reader and on hover; where the click moves it
/// to, as a function of where it is.
pub(super) fn move_buttons(
    ui: &mut egui::Ui,
    name: &str,
    earlier: bool,
    later: bool,
) -> Option<fn(usize) -> usize> {
    let mut moved: Option<fn(usize) -> usize> = None;
    for (arrow, enabled, said, to) in [
        (
            "◀",
            earlier,
            tf("Move {} left", &[&name]),
            (|at: usize| at - 1) as fn(usize) -> usize,
        ),
        ("▶", later, tf("Move {} right", &[&name]), |at: usize| {
            at + 1
        }),
    ] {
        let button = ui.add_enabled(enabled, egui::Button::new(arrow).small());
        button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &said));
        if button.on_hover_text(said.clone()).clicked() {
            moved = Some(to);
        }
    }
    moved
}

/// How wide a modern toolbar button is: its name beside its picture, or
/// (`stacked`) under it in smaller type.
pub(super) fn modern_width(ui: &egui::Ui, label: &str, stacked: bool) -> f32 {
    // The size the words are drawn at (`modern_tool`): measured at 11.5,
    // a long name stood wider than the room kept for it.
    let size = if stacked { 12.0 } else { 14.0 };
    let text = ui
        .painter()
        .layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(size),
            egui::Color32::WHITE,
        )
        .size()
        .x;
    if stacked {
        (text + 14.0).max(46.0)
    } else {
        12.0 + 20.0 + 8.0 + text + 12.0
    }
}

/// Every text field drawn this frame gets WinUI's bottom line: 1 px in the
/// strong stroke at rest and 2 px in the accent while it has the keyboard,
/// where egui outlines a focused field all round in the text colour. The
/// fields are found among this frame's widgets by the state each one keeps.
pub(super) fn underline_fields(ctx: &egui::Context) {
    let t = crate::theme::Tokens::get(ctx);
    if !t.modern {
        return;
    }
    let focused = ctx.memory(|m| m.focused());
    // Read under the viewport's lock, and only then asked about, which
    // takes the context's lock again.
    let widgets: Vec<egui::WidgetRect> = ctx.viewport(|v| {
        v.this_pass
            .widgets
            .layers()
            .flat_map(|(_, widgets)| widgets.iter())
            // A disabled field keeps its soft edge alone, as WinUI's does.
            // A text field senses clicks and drags; a check box or a label
            // that took a gone field's id does not (rows of
            // Options were underlined).
            .filter(|w| w.enabled && w.visible && w.interact_rect.is_positive())
            .filter(|w| w.sense.senses_click() && w.sense.senses_drag())
            .copied()
            .collect()
    });
    for w in widgets {
        if egui::TextEdit::load_state(ctx, w.id).is_none() {
            continue;
        }
        let (layer, rect, seen, focus) =
            (w.layer_id, w.rect, w.interact_rect, focused == Some(w.id));
        let painter = ctx.layer_painter(layer).with_clip_rect(seen.expand(1.0));
        let radius = egui::CornerRadius::same(t.radius);
        if focus {
            painter.rect_stroke(
                rect,
                radius,
                egui::Stroke::new(1.0, t.edge),
                egui::StrokeKind::Inside,
            );
        }
        let height = if focus { 2.0 } else { 1.0 };
        // Stopping where the corners start to curve: a line run to the
        // ends stood out of the rounded corners as a white notch.
        let inset = f32::from(t.radius) + 0.5;
        let line = egui::Rect::from_min_max(
            egui::pos2(rect.left() + inset, rect.bottom() - height),
            egui::pos2(rect.right() - inset, rect.bottom()),
        );
        painter.rect_filled(line, 0.0, if focus { t.brand } else { t.stroke });
    }
}

/// A modern toolbar button: the picture beside its name, 32 px tall,
/// subtle until hovered; the primary one is filled with the brand colour.
/// `stacked`, the name goes under the picture, where a row is too narrow
/// for them side by side.
pub(super) fn modern_tool(
    ui: &mut egui::Ui,
    icon: Icon,
    (label, widest): (&str, &str),
    enabled: bool,
    primary: bool,
    t: &crate::theme::Tokens,
    stacked: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(if stacked { 12.0 } else { 14.0 });
    let text_color = if !enabled {
        t.disabled
    } else if primary {
        t.on_brand
    } else {
        t.text
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, text_color);
    let size = if stacked {
        egui::vec2(modern_width(ui, widest, true), 44.0)
    } else {
        egui::vec2(modern_width(ui, widest, false), 32.0)
    };
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let pressed = response.is_pointer_button_down_on();
    let fill = match (primary && enabled, response.hovered() && enabled, pressed) {
        (true, _, true) => t.brand_pressed,
        (true, true, _) => t.brand_hover,
        (true, false, _) => t.brand,
        (false, _, true) => t.pressed,
        (false, true, _) => t.hover,
        _ => egui::Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    let picture = if !enabled {
        t.disabled
    } else if primary {
        t.on_brand
    } else {
        t.tool(icon, true)
    };
    let (icon_at, text_at) = if stacked {
        (
            egui::pos2(rect.center().x, rect.top() + 14.0),
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.bottom() - 6.0 - galley.size().y,
            ),
        )
    } else {
        (
            egui::pos2(rect.left() + 12.0 + 10.0, rect.center().y),
            egui::pos2(rect.left() + 40.0, rect.center().y - galley.size().y / 2.0),
        )
    };
    let icon_rect = egui::Rect::from_center_size(icon_at, egui::vec2(18.0, 18.0));
    icons::paint(ui.painter(), icon_rect, icon, picture);
    ui.painter().galley(text_at, galley, text_color);
    response
}

/// A menu's look: egui's, with room at an item's sides. Its two points
/// left a shortcut flush with the menu's right edge ("Ctrl+," touched it) while the names began a few points in.
pub(super) fn menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding.x = 8.0;
}

/// A menu inside a menu. In the modern look its arrow is a thin chevron
/// in the secondary colour, as a menu flyout's sub-item has; egui draws a
/// filled triangle, which the classic look keeps.
pub(super) fn submenu<R>(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    submenu_with(ui, label, false, add)
}

/// [`submenu`] that stays open while what is in it is clicked, for a list
/// of switches: View › Columns closed at every tick, and three columns
/// took three trips through the menu.
pub(super) fn submenu_kept<R>(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    submenu_with(ui, label, true, add)
}

pub(super) fn submenu_with<R>(
    ui: &mut egui::Ui,
    label: &str,
    kept: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let t = crate::theme::Tokens::get(ui.ctx());
    let config = egui::containers::menu::MenuConfig::new()
        .style(menu_style)
        .close_behavior(if kept {
            egui::PopupCloseBehavior::CloseOnClickOutside
        } else {
            egui::PopupCloseBehavior::CloseOnClick
        });
    if !t.modern {
        let (response, inner) = egui::containers::menu::SubMenuButton::new(label)
            .config(config)
            .ui(ui, add);
        return egui::InnerResponse::new(inner.map(|i| i.inner), response);
    }
    let arrow = if crate::theme::fonts_ready(ui.ctx()) {
        egui::RichText::new(icons::fluent::CHEVRON_RIGHT.to_string())
            .family(icons::family())
            .size(12.0)
    } else {
        egui::RichText::new("›")
    };
    let button = egui::Button::new(label).right_text(arrow.color(t.weak));
    let (response, inner) = egui::containers::menu::SubMenuButton::from_button(button)
        .config(config)
        .ui(ui, add);
    egui::InnerResponse::new(inner.map(|i| i.inner), response)
}

/// A group of places: in the modern look a navigation view's expandable
/// item, its name in a 32 px row and a thin chevron at the row's right that
/// turns as it opens, its places indented under it; in the classic look
/// egui's tree header. Open at first; kept open or shut by `id`.
pub(super) fn nav_group(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    holds_selection: bool,
    t: &crate::theme::Tokens,
    add: impl FnOnce(&mut egui::Ui),
) {
    if !t.modern {
        egui::CollapsingHeader::new(label)
            .id_salt(id)
            .default_open(true)
            .show(ui, add);
        return;
    }
    let key = egui::Id::unique(("dm-nav-group", id));
    let mut open = ui.ctx().data_mut(|d| *d.get_persisted_mut_or(key, true));
    let width = ui.available_width().max(120.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.clicked() {
        open = !open;
        ui.ctx().data_mut(|d| d.insert_persisted(key, open));
    }
    // Shut over the place chosen, it carries the selection, as a
    // navigation view's parent does: collapsed, nothing showed it.
    let marked = !open && holds_selection;
    let fill = if response.is_pointer_button_down_on() {
        t.pressed
    } else if response.hovered() || marked {
        t.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius, fill);
    if marked {
        let pill = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 3.0, rect.center().y),
            egui::vec2(3.0, 16.0),
        );
        painter.rect_filled(pill, 1.5, t.brand);
    }
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        t.text,
    );
    // ChevronUp open, ChevronDown shut, as a navigation view turns it.
    let c = egui::pos2(rect.right() - 16.0, rect.center().y);
    let chevron = egui::Rect::from_center_size(c, egui::vec2(12.0, 12.0));
    let which = if open {
        icons::fluent::CHEVRON_UP
    } else {
        icons::fluent::CHEVRON_DOWN
    };
    if !icons::glyph(painter, chevron, which, t.weak) {
        let dy = if open { 2.0 } else { -2.0 };
        painter.line(
            vec![
                c + egui::vec2(-4.5, dy),
                c + egui::vec2(0.0, -dy),
                c + egui::vec2(4.5, dy),
            ],
            egui::Stroke::new(1.0, t.weak),
        );
    }
    if open {
        ui.indent(key, add);
    }
}

/// A place's icon, as a navigation view gives each item its own.
pub(super) fn place_icon(place: &Place) -> char {
    use icons::fluent;
    match place {
        Place::All => fluent::APPS_LIST,
        Place::Kind(kind) => match kind {
            Category::Videos => fluent::VIDEO,
            Category::Audio => fluent::MUSIC,
            Category::Documents => fluent::DOCUMENT,
            Category::Apps => fluent::APPS,
            Category::Archives => fluent::FOLDER_ZIP,
            Category::Pictures => fluent::IMAGE,
            _ => fluent::FOLDER,
        },
        Place::Custom(_) => fluent::FOLDER,
        Place::Unfinished => fluent::ARROW_DOWNLOAD,
        Place::Finished => fluent::CHECKMARK_CIRCLE,
        Place::Queue(name) if name == dm_core::RECHECK_QUEUE => fluent::ARROW_SYNC,
        Place::Queue(_) => fluent::LIST,
    }
}

/// One place in the modern navigation pane: a rounded row, the selected one
/// marked with the brand indicator, its count at the right. Its name for a
/// screen reader is "Label (n)", as the classic tree's was.
pub(super) fn nav_item(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    count: usize,
    icon: char,
    t: &crate::theme::Tokens,
) -> egui::Response {
    let width = ui.available_width().max(120.0);
    // NavigationViewItemOnLeftMinHeight, compact.
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let name = format!("{label} ({count})");
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    let fill = if response.is_pointer_button_down_on() {
        t.pressed
    } else if selected || response.hovered() {
        t.selection
    } else {
        egui::Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius, fill);
    if selected {
        let pill = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 3.0, rect.center().y),
            egui::vec2(3.0, 16.0),
        );
        painter.rect_filled(pill, 2.0, t.brand);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 20.0, rect.center().y),
        egui::vec2(16.0, 14.0),
    );
    // As a navigation view's item: the selection is the fill and the
    // accent pill, the picture and the name stay as they are.
    let icon_rect = egui::Rect::from_center_size(icon_rect.center(), egui::vec2(16.0, 16.0));
    if !icons::glyph(painter, icon_rect, icon, t.folder) {
        icons::folder_outline(painter, icon_rect, t.folder);
    }
    let family = egui::FontFamily::Proportional;
    // A long name is shortened with "…" before the count, never over it.
    let room = (rect.width() - 36.0 - 34.0).max(20.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_string(),
        egui::FontId::new(14.0, family),
        t.text,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    // A name cut short is whole on hover.
    let response = if galley.elided {
        response.on_hover_text(label)
    } else {
        response
    };
    let painter = ui.painter();
    painter.galley(
        egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    painter.text(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(12.0),
        t.weak,
    );
    response
}

/// The toolbar's buttons, in their default order.
pub(super) const TOOLBAR: [(Command, Icon, &str); 11] = [
    (Command::AddUrl, Icon::Add, "Add URL"),
    (Command::DownloadNow, Icon::Resume, "Resume"),
    (Command::StopDownload, Icon::Stop, "Stop"),
    (Command::StopAll, Icon::StopAll, "Stop all"),
    (Command::Remove, Icon::Delete, "Remove"),
    (
        Command::DeleteCompleted,
        Icon::DeleteCompleted,
        "Clear finished",
    ),
    (Command::Options, Icon::Options, "Options"),
    (Command::Scheduler, Icon::Scheduler, "Scheduler"),
    (Command::StartQueue, Icon::StartQueue, "Start queue"),
    (Command::StopQueue, Icon::StopQueue, "Stop queue"),
    (Command::SiteFiles, Icon::SiteFiles, "Site files"),
];

/// The toolbar's buttons in the order a person chose (by command id), with
/// any not named after them in the default order. Unknown ids are skipped.
pub fn toolbar_in(order: &[String]) -> Vec<(Command, Icon, &'static str)> {
    let mut buttons: Vec<(Command, Icon, &'static str)> = Vec::new();
    // A hand-edited settings file can name a button twice; it is drawn once.
    for id in order {
        if let Some(button) = TOOLBAR.iter().find(|(c, _, _)| c.id() == id) {
            if !buttons.iter().any(|(c, _, _)| *c == button.0) {
                buttons.push(*button);
            }
        }
    }
    for button in TOOLBAR {
        if !buttons.iter().any(|(c, _, _)| *c == button.0) {
            buttons.push(button);
        }
    }
    buttons
}

/// The toolbar styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStyle {
    Both,
    Icons,
    Text,
}

impl ToolStyle {
    pub fn named(name: &str) -> ToolStyle {
        match name {
            "icons" => ToolStyle::Icons,
            "text" => ToolStyle::Text,
            _ => ToolStyle::Both,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ToolStyle::Both => "both",
            ToolStyle::Icons => "icons",
            ToolStyle::Text => "text",
        }
    }
}

/// A menu item that is on or off: a plain item, its words where the other
/// items' start, with a tick drawn at its right while it is on. As a
/// checkbox its words started further in than the items around it.
pub(super) fn ticked_item(ui: &mut egui::Ui, title: &str, on: bool) -> egui::Response {
    let item = ui.add(egui::Button::new(title).right_text("    "));
    item.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, title));
    let r = item.rect;
    let at = egui::pos2(r.right() - 12.0, r.center().y);
    let colour = ui.style().interact(&item).fg_stroke.color;
    // A box whether on or off, so the item reads as one that is ticked or
    // not, rather than a tick that is there or is not.
    ui.painter().rect_stroke(
        egui::Rect::from_center_size(at, egui::vec2(12.0, 12.0)),
        2.0,
        egui::Stroke::new(1.0, colour.gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    if on {
        ui.painter().line(
            vec![
                at + egui::vec2(-3.5, 0.0),
                at + egui::vec2(-1.0, 2.5),
                at + egui::vec2(3.5, -3.0),
            ],
            egui::Stroke::new(1.6, colour),
        );
    }
    item
}

/// One toolbar button. Small ones show only the icon, text-only ones only
/// the name; the name stays for a screen reader either way.
pub(super) fn tool(
    ui: &mut egui::Ui,
    icon: Icon,
    (label, widest): (&str, &str),
    enabled: bool,
    style: ToolStyle,
    primary: bool,
) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    if t.modern && style == ToolStyle::Both {
        return modern_tool(ui, icon, (label, widest), enabled, primary, &t, false);
    }
    let small = style == ToolStyle::Icons;
    // Each button as wide as its longest name needs, measured in the font
    // it is drawn in: a fixed 72 points ran German's names into each other
    // in the classic look.
    let fits = |size: f32| {
        [label, widest]
            .iter()
            .map(|text| {
                ui.painter()
                    .layout_no_wrap(
                        text.to_string(),
                        egui::FontId::proportional(size),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max)
    };
    let size = match style {
        ToolStyle::Icons => egui::vec2(34.0, 30.0),
        ToolStyle::Text => egui::vec2(fits(12.0) + 18.0, 28.0),
        ToolStyle::Both => egui::vec2((fits(11.0) + 12.0).max(72.0), 52.0),
    };
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let visuals = if enabled {
        ui.style().interact(&response)
    } else {
        &ui.style().visuals.widgets.noninteractive
    };
    // The one primary action keeps its fill when the names give way to
    // icons alone, as at 200% or in Russian (it lost it).
    let filled = primary && t.modern && enabled;
    if filled {
        ui.painter().rect_filled(rect, f32::from(t.radius), t.brand);
    } else if response.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, 4.0, ui.style().visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if filled {
        t.on_brand
    } else if enabled {
        visuals.fg_stroke.color
    } else {
        ui.style().visuals.weak_text_color()
    };
    if style == ToolStyle::Text {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
        return response;
    }
    let icon_rect = if small {
        egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0))
    } else {
        egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 18.0),
            egui::vec2(22.0, 22.0),
        )
    };
    // The picture in colour, its name in the text colour.
    // On the brand fill, in the colour that shows on it: the "+" was drawn
    // dark on blue.
    let picture = if filled {
        t.on_brand
    } else if enabled {
        crate::theme::Tokens::get(ui.ctx()).tool(icon, true)
    } else {
        color
    };
    icons::paint(ui.painter(), icon_rect, icon, picture);
    if small {
        return response;
    }
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 10.0),
        egui::Align2::CENTER_CENTER,
        label,
        // Nothing under 12 points, the smallest step of the type ramp.
        egui::FontId::proportional(12.0),
        color,
    );
    response
}

#[cfg(test)]
mod toolbar_tests {
    use super::*;

    #[test]
    fn the_toolbar_keeps_a_chosen_order_and_puts_the_rest_after_it() {
        let ids = |order: &[&str]| -> Vec<&'static str> {
            let order: Vec<String> = order.iter().map(|s| s.to_string()).collect();
            toolbar_in(&order).iter().map(|(c, _, _)| c.id()).collect()
        };
        let default = ids(&[]);
        assert_eq!(default.len(), TOOLBAR.len());
        assert_eq!(default[0], Command::AddUrl.id());
        let moved = ids(&[
            "program.site_files",
            "no.such.command",
            "program.site_files",
        ]);
        assert_eq!(moved[0], "program.site_files");
        assert_eq!(
            moved.len(),
            TOOLBAR.len(),
            "each button once, unknown ids dropped"
        );
        assert_eq!(ToolStyle::named("text"), ToolStyle::Text);
        assert_eq!(ToolStyle::named("anything"), ToolStyle::Both);
    }

    #[test]
    fn a_browser_dragged_url_is_read_from_its_path_when_bytes_cannot_be_read() {
        let path = std::env::temp_dir().join(format!("dm-browser-drag-{}.txt", std::process::id()));
        std::fs::write(&path, b"https://example.com/file.bin\n").unwrap();

        #[derive(Debug)]
        struct FakeDroppedFile {
            path: std::path::PathBuf,
            bytes: Result<Vec<u8>, String>,
        }

        impl egui::DroppedFile for FakeDroppedFile {
            fn path(&self) -> &std::path::Path {
                &self.path
            }

            fn bytes(&self) -> Result<Vec<u8>, String> {
                self.bytes.clone()
            }
        }

        let file = FakeDroppedFile {
            path: path.clone(),
            bytes: Err("this source does not expose file bytes".to_string()),
        };

        let found = dropped_links(&file);
        assert_eq!(found, vec!["https://example.com/file.bin"]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_dropped_file_is_read_for_links_only_if_it_is_text_and_only_its_start() {
        #[derive(Debug)]
        struct Dropped(std::path::PathBuf);
        impl egui::DroppedFile for Dropped {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                // What a drop hands over: the whole file.
                std::fs::read(&self.0).map_err(|e| e.to_string())
            }
        }
        let dir = tempfile::tempdir().unwrap();

        // A PDF with an address inside it queues nothing.
        let pdf = dir.path().join("paper.pdf");
        std::fs::write(&pdf, b"%PDF-1.7 https://example.com/cited.zip").unwrap();
        assert!(dropped_links(&Dropped(pdf)).is_empty());

        // A list is read, but only its first 64 KB.
        let list = dir.path().join("links.txt");
        let mut text = String::from("https://example.com/first.bin\n");
        text.push_str(&" ".repeat(DROP_LOOK as usize));
        text.push_str("https://example.com/past-the-start.bin\n");
        std::fs::write(&list, text).unwrap();
        assert_eq!(
            dropped_links(&Dropped(list)),
            vec!["https://example.com/first.bin"]
        );

        // A file with no extension that is not text is not read for links.
        let blob = dir.path().join("blob");
        std::fs::write(&blob, b"\0\x01https://example.com/x.bin").unwrap();
        assert!(dropped_links(&Dropped(blob)).is_empty());

        // A .torrent is the download itself, by its file address.
        let torrent = dir.path().join("a set.torrent");
        std::fs::write(&torrent, b"d4:infodee").unwrap();
        let taken = dropped_links(&Dropped(torrent.clone()));
        assert_eq!(taken, [dm_core::torrent::file_url(&torrent).unwrap()]);
        assert!(dm_core::is_torrent(&taken[0]) && dm_core::queue::accepted(&taken[0]));
    }
}
