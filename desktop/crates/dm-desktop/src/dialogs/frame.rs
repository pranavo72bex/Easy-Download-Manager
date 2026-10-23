use super::*;
/// Where the window keeps the menu bar's foot, for the dialogs.
pub fn menu_bottom_id() -> egui::Id {
    egui::Id::unique("dm-menu-bottom")
}

/// A dialog window: not collapsible, placed in the middle, titled in the
/// language in force but known by its English title: its place and size
/// survive a change of language, and two titles that translate alike stay
/// two windows.
pub(super) fn window(ctx: &egui::Context, title: &'static str) -> Dialog<'static> {
    // Below the menu bar: Options and Properties opened over it, while the
    // per-download windows kept clear of it.
    let mut room = ctx.content_rect();
    if let Some(bottom) = ctx.data(|d| d.get_temp::<f32>(menu_bottom_id())) {
        room.min.y = room.min.y.max(bottom).min(room.max.y - 160.0);
    }
    let tall = (room.height() - 56.0).max(160.0);
    let reached = ctx
        .memory(|m| m.area_rect(window_id(title)))
        // With some slack: a scrolled dialog stands about as tall as it may.
        .is_some_and(|was| was.height() >= tall - 8.0);
    // And once scrolled, it stays so while it is open in the same room: a
    // scrolled body is shorter than the room, and turning the scrolling off
    // again for it made the dialog flip between the two every frame, its
    // buttons moving under the pointer so that no click took (Site files on
    // Windows' fonts).
    let held_id = window_id(title).with("scrolled");
    let pass = ctx.cumulative_pass_nr();
    let held = ctx
        .data(|d| d.get_temp::<(f32, u64)>(held_id))
        .is_some_and(|(room_tall, seen)| (room_tall - tall).abs() < 1.0 && seen + 1 >= pass);
    let cramped = reached || held;
    ctx.data_mut(|d| {
        if cramped {
            d.insert_temp(held_id, (tall, pass));
        } else {
            d.remove::<(f32, u64)>(held_id);
        }
    });
    Dialog::new(t(title))
        .id(window_id(title))
        .collapsible(false)
        .resizable(false)
        // Wide enough for its title in the 20 px heading, but never wider
        // than the window: at 150% text in the smallest window 440 points
        // ran off both sides.
        .min_width(440.0_f32.min(room.width() - sides(ctx)).max(200.0))
        // And never taller: a dialog longer than the room scrolls, rather
        // than losing its title and buttons off the top and bottom (Files
        // on a site at 150% in the smallest window). Only one
        // that was that tall on the frame before: a scrolled body sizes
        // itself otherwise, and every other dialog keeps its own size.
        .max_height(tall)
        .vscroll(cramped)
        .constrain_to(room)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
}

/// A dialog laid out as WinUI's are: no title bar, but its title at the
/// top of the body on the left in the Subtitle style (20 px semibold), a
/// close button at the right for the dialogs that have one, and 12 px
/// under them. egui's own title bar centred the title in a grey band.
pub(crate) struct Dialog<'o> {
    window: egui::Window<'static>,
    title: String,
    open: Option<&'o mut bool>,
    /// The least width of its body, as `min_width` set it.
    least: f32,
    /// The width `fixed_size` gave it, frame and all.
    fixed: Option<f32>,
    /// Whether its body scrolls, under a title that stays.
    scrolls: bool,
    /// The most it may stand, as `max_height` set it.
    tallest: Option<f32>,
}

/// Where a dialog keeps, for its title and buttons, how wide its body is:
/// the least width it was given, and the body's own width last frame.
pub(super) fn least_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-least")
}

pub(super) fn body_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-body")
}

/// Whether a dialog's body scrolls this frame, and the room it keeps under
/// it for the buttons: where, and how tall the buttons took last frame.
pub(super) fn scrolls_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-scrolls")
}

pub(super) fn foot_rect_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-foot-rect")
}

pub(super) fn origin_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-origin")
}

pub(super) fn foot_height_id(layer: egui::LayerId) -> egui::Id {
    layer.id.with("dm-dialog-foot-height")
}

impl<'o> Dialog<'o> {
    pub(crate) fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        Dialog {
            window: egui::Window::new(title.clone()).title_bar(false),
            title,
            open: None,
            least: 0.0,
            fixed: None,
            scrolls: false,
            tallest: None,
        }
    }

    pub(super) fn with(
        mut self,
        change: impl FnOnce(egui::Window<'static>) -> egui::Window<'static>,
    ) -> Self {
        self.window = change(self.window);
        self
    }

    pub(crate) fn open(mut self, open: &'o mut bool) -> Self {
        self.open = Some(open);
        self
    }

    pub(crate) fn id(self, id: egui::Id) -> Self {
        self.with(|w| w.id(id))
    }

    pub(crate) fn collapsible(self, on: bool) -> Self {
        self.with(|w| w.collapsible(on))
    }

    pub(crate) fn resizable(self, on: bool) -> Self {
        self.with(|w| w.resizable(on))
    }

    pub(crate) fn constrain(self, on: bool) -> Self {
        self.with(|w| w.constrain(on))
    }

    pub(crate) fn constrain_to(self, rect: egui::Rect) -> Self {
        self.with(|w| w.constrain_to(rect))
    }

    pub(crate) fn pivot(self, pivot: egui::Align2) -> Self {
        self.with(|w| w.pivot(pivot))
    }

    pub(crate) fn anchor(self, align: egui::Align2, offset: impl Into<egui::Vec2>) -> Self {
        let offset = offset.into();
        self.with(|w| w.anchor(align, offset))
    }

    pub(crate) fn default_pos(self, pos: impl Into<egui::Pos2>) -> Self {
        let pos = pos.into();
        self.with(|w| w.default_pos(pos))
    }

    pub(crate) fn default_width(self, width: f32) -> Self {
        self.with(|w| w.default_width(width))
    }

    pub(crate) fn default_height(self, height: f32) -> Self {
        self.with(|w| w.default_height(height))
    }

    pub(crate) fn min_width(mut self, width: f32) -> Self {
        self.least = width;
        self.with(|w| w.min_width(width))
    }

    pub(crate) fn min_height(self, height: f32) -> Self {
        self.with(|w| w.min_height(height))
    }

    pub(crate) fn max_width(self, width: f32) -> Self {
        self.with(|w| w.max_width(width))
    }

    pub(crate) fn max_height(mut self, height: f32) -> Self {
        self.tallest = Some(height);
        self.with(|w| w.max_height(height))
    }

    pub(crate) fn fixed_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        let size = size.into();
        self.fixed = Some(size.x);
        // The least width an earlier call gave it no longer applies.
        self.least = 0.0;
        self.with(|w| w.min_width(0.0).fixed_size(size))
    }

    /// The body scrolls, below the title and its close button, which stay
    /// where they can be reached; egui's own scrolled them away with it.
    pub(crate) fn vscroll(mut self, on: bool) -> Self {
        self.scrolls = on;
        self
    }

    pub(crate) fn show<R>(
        self,
        ctx: &egui::Context,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let Dialog {
            window,
            title,
            open,
            least,
            fixed,
            scrolls,
            tallest,
        } = self;
        if open.as_deref() == Some(&false) {
            return None;
        }
        let closable = open.is_some();
        let mut closed = false;
        let shown = window.show(ctx, |ui| {
            let layer = ui.layer_id();
            // A dialog of fixed size holds its body to the width inside its
            // frame: egui offered the body the width it had grown to the
            // frame before, a scroll area took all of it, and the dialog,
            // placed by its middle, ran off the window (Options at 200%).
            let least = match fixed {
                Some(width) => {
                    let inner = width - ui.spacing().window_margin.sum().x;
                    ui.set_max_width(inner);
                    inner
                }
                None => least,
            }
            .min(ui.max_rect().width());
            ui.ctx().data_mut(|d| d.insert_temp(least_id(layer), least));
            closed = head(ui, &title, closable);
            let origin = ui.max_rect().min;
            // The buttons' room kept under the scrolled part, at what they
            // took last frame; the footer draws them there.
            let kept = ui
                .ctx()
                .data(|d| d.get_temp::<f32>(foot_height_id(layer)))
                .unwrap_or(0.0);
            // From the height the dialog may have, less its title and
            // margins: what egui says is available in a dialog that sizes
            // itself is not that, and the scrolled part ran under the
            // buttons (Options at 150%).
            let spacing = ui.spacing().item_spacing.y;
            let margins = ui.spacing().window_margin.sum().y;
            let used = ui.min_rect().height() + spacing;
            let whole = tallest
                .map_or(ui.available_height(), |most| most - margins - used)
                .min(ui.available_height());
            // Where keeping the buttons in place would leave the body less
            // than a few rows, the page scrolls whole with its buttons at
            // the end, as it did: at 150% in the smallest
            // window Options showed a sliver of its first setting.
            let fixed_foot = scrolls && whole - kept - spacing >= 120.0;
            ui.ctx().data_mut(|d| {
                d.insert_temp(scrolls_id(layer), fixed_foot);
                d.insert_temp(origin_id(layer), origin);
            });
            if scrolls {
                let room = if fixed_foot {
                    whole - kept - spacing
                } else {
                    whole
                }
                .max(48.0);
                let inner = egui::ScrollArea::vertical()
                    .id_salt("dm-dialog-body")
                    .max_height(room)
                    .show(ui, add)
                    .inner;
                if fixed_foot && kept > 0.0 {
                    let (foot, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), kept),
                        egui::Sense::hover(),
                    );
                    // Kept from the dialog's corner, which moves with it.
                    let foot = foot.translate(-origin.to_vec2());
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(foot_rect_id(layer), foot));
                }
                inner
            } else {
                add(ui)
            }
        });
        if let (true, Some(open)) = (closed, open) {
            *open = false;
        }
        shown
    }
}

/// A dialog's title and close button; true when the button was clicked.
pub(super) fn head(ui: &mut egui::Ui, title: &str, closable: bool) -> bool {
    // As wide as the dialog's body was last frame, which is how egui's own
    // title bar finds the far edge of a window that sizes itself to its
    // body; nothing on the first, when egui's own answer is the whole room
    // and a dialog took that as its width (Options at 200%).
    // Measured without the title and the buttons, which would otherwise
    // hold a dialog at any width it once had.
    let layer = ui.layer_id();
    let (least, body) = ui.ctx().data(|d| {
        (
            d.get_temp::<f32>(least_id(layer)).unwrap_or(0.0),
            d.get_temp::<f32>(body_id(layer)).unwrap_or(0.0),
        )
    });
    // A dialog with no footer has no body measured: last frame's width.
    let body = if body > 0.0 {
        body
    } else {
        ui.ctx()
            .memory(|m| m.area_rect(layer.id))
            .map_or(0.0, |r| r.width() - ui.spacing().window_margin.sum().x)
    };
    let wide = least.max(body).min(ui.max_rect().width());
    let mut closed = false;
    ui.horizontal(|ui| {
        // Painted, not a label: the window already gives a screen reader
        // its title, and a second node of the same name was a second
        // "Download details" to every search by name.
        let galley = ui.painter().layout_no_wrap(
            title.to_string(),
            egui::TextStyle::Heading.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(galley.size().x, galley.size().y.max(32.0)),
            egui::Sense::hover(),
        );
        let at = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, ui.visuals().text_color());
        if closable {
            let size = egui::vec2(32.0, 32.0);
            let gap = wide - ui.min_rect().width() - size.x - ui.spacing().item_spacing.x;
            ui.add_space(gap.max(0.0));
            closed = close_button(ui, size).clicked();
        }
    });
    ui.add_space(4.0);
    closed
}

/// A subtle button with a ×, as egui named its own: "Close window".
pub(super) fn close_button(ui: &mut egui::Ui, size: egui::Vec2) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Close window")
    });
    let fill = if response.is_pointer_button_down_on() {
        t.pressed
    } else if response.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    let cross = egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0));
    let line = egui::Stroke::new(1.0, if ui.is_enabled() { t.text } else { t.disabled });
    ui.painter()
        .line_segment([cross.left_top(), cross.right_bottom()], line);
    ui.painter()
        .line_segment([cross.right_top(), cross.left_bottom()], line);
    response
}

/// What a dialog's margins take of the window's width, both sides: a
/// dialog sized to the window less this fills it and no more.
pub(super) fn sides(ctx: &egui::Context) -> f32 {
    ctx.global_style().spacing.window_margin.sum().x
}

/// The id of a dialog's window, by its English title.
pub(super) fn window_id(title: &'static str) -> egui::Id {
    egui::Id::unique(("dm-dialog", title))
}

/// A progress bar: a thin track with the brand fill and its words
/// beside it; the classic look keeps egui's own bar.
pub(super) fn bar(ui: &mut egui::Ui, fraction: f32, width: f32, words: &str) {
    bar_beside(ui, fraction, width, words, words);
}

/// [`bar`], with room kept for `widest` beside it rather than its own
/// words: bars in a column, each with words of its own, are then one
/// length ("1.7 MB" and "1023.9 KB" gave two).
pub(super) fn bar_beside(ui: &mut egui::Ui, fraction: f32, width: f32, words: &str, widest: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    // At most `width`, and short enough that its words fit beside it
    // clear of a scroll bar: "91%" was cut off at the edge.
    let words_width = ui
        .painter()
        .layout_no_wrap(
            widest.to_string(),
            egui::TextStyle::Small.resolve(ui.style()),
            Color32::WHITE,
        )
        .size()
        .x;
    let room = ui.available_width()
        - words_width
        - ui.spacing().item_spacing.x
        - ui.spacing().scroll.allocated_width()
        - 8.0;
    let width = width.min(room).max(60.0);
    if !t.modern {
        // egui's bar, its track in a colour that shows on the dialog: in
        // the classic look the track was the dialog's own colour, and a
        // bar at 0% was a bare pill around its words.
        ui.scope(|ui| {
            ui.visuals_mut().extreme_bg_color = t.border;
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(width)
                    .text(words),
            );
        });
        return;
    }
    ui.horizontal(|ui| {
        // WinUI's progress bar: a 1 px track in the strong stroke under a
        // 3 px bar in the accent, both rounded.
        let (track, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
        let line = egui::Rect::from_center_size(track.center(), egui::vec2(width, 1.0));
        ui.painter().rect_filled(line, 0.5, t.stroke);
        let mut done = egui::Rect::from_center_size(track.center(), egui::vec2(width, 3.0));
        done.set_width(width * fraction);
        ui.painter().rect_filled(done, 1.5, t.brand);
        ui.label(RichText::new(words).small().color(t.weak));
    });
}

/// One bar laid out as the file is: each piece at its place, filled as far
/// as it has come, a thin gap where two pieces meet, so where every
/// connection is working shows at a glance. `pieces` are (start, length,
/// done) in bytes of a file `total` long.
pub fn pieces_bar(ui: &mut egui::Ui, total: u64, pieces: &[(u64, u64, u64)], width: f32) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let width = width.min(ui.available_width() - 8.0).max(60.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 10.0), egui::Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect_filled(rect, 3.0, t.border);
    let total = total.max(1) as f32;
    let at = |bytes: u64| rect.left() + width * (bytes as f32 / total).min(1.0);
    for &(start, length, done) in pieces {
        let (from, to) = (at(start), at(start.saturating_add(length)));
        let filled = at(start.saturating_add(done.min(length)));
        if filled > from {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(from, rect.top()),
                    egui::pos2(filled, rect.bottom()),
                ),
                0.0,
                t.brand,
            );
        }
        // Where this piece begins, unless it is the file's start.
        if start > 0 && to > from {
            painter.vline(from, rect.y_range(), egui::Stroke::new(1.0, t.dialog));
        }
    }
    let downloaded: u64 = pieces
        .iter()
        .map(|&(_, length, done)| done.min(length))
        .sum();
    let percent = (downloaded as f32 / total * 100.0).round() as u32;
    let said = crate::i18n::tn(
        "{}% downloaded, in {} piece||{}% downloaded, in {} pieces",
        pieces.len() as u64,
        &[&percent, &pieces.len()],
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, &said));
}

/// A dialog's buttons as WinUI's ContentDialog has them: on a band in the
/// window's base colour under a hairline, across the whole dialog to its
/// bottom edge, below the body; the buttons at the right, the default
/// first. In a dialog whose body scrolls, the band is drawn in the room
/// the dialog keeps for it under the scrolled part, so the buttons stay in
/// reach, as a ContentDialog's do. Classic keeps a plain row.
pub(crate) fn footer<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let t = crate::theme::Tokens::get(ui.ctx());
    if !t.modern {
        return ui.horizontal(add);
    }
    let layer = ui.layer_id();
    let margin = ui.spacing().window_margin;
    let (scrolled, kept) = ui.ctx().data(|d| {
        let origin = d
            .get_temp::<egui::Pos2>(origin_id(layer))
            .unwrap_or_default();
        (
            d.get_temp::<bool>(scrolls_id(layer)).unwrap_or(false),
            d.get_temp::<egui::Rect>(foot_rect_id(layer))
                .map(|r| r.translate(origin.to_vec2())),
        )
    });
    if let (true, Some(foot)) = (scrolled, kept) {
        // The room kept under the scrolled body: the band at its top, the
        // row 24 below, outside the scroll area's clip.
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(foot)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(
            foot.expand2(egui::vec2(f32::from(margin.left), f32::from(margin.bottom))),
        );
        child.add_space(f32::from(margin.top));
        let shown = row_on_band(&mut child, foot.left(), foot.right(), add);
        let height = shown.response.rect.bottom() - foot.top();
        ui.ctx()
            .data_mut(|d| d.insert_temp(foot_height_id(layer), height));
        return shown;
    }
    // The body's width as laid out so far this frame, or the dialog's least
    // width: the row reaches that edge and never past it, so it cannot
    // widen the dialog it is measured against.
    let least = ui
        .ctx()
        .data(|d| d.get_temp::<f32>(least_id(layer)))
        .unwrap_or(0.0);
    let left = ui.max_rect().left();
    let body = (ui.min_rect().right() - left).max(least);
    ui.ctx().data_mut(|d| d.insert_temp(body_id(layer), body));
    let right = (left + body).min(ui.max_rect().right());
    // The band 24 below the body's last line, as a ContentDialog pads its
    // content: the row is 24 into the band, so 40 above it with the 8 the
    // layout puts after a widget. The gap goes above the row, which in a
    // layout built from the foot up means after it. 20 left the band 4
    // under the last line.
    let upward = ui.layout().main_dir() == egui::Direction::BottomUp;
    let gap = 40.0;
    if !upward {
        ui.add_space(gap);
    }
    let shown = row_on_band(ui, left, right, add);
    if upward {
        ui.add_space(gap);
    }
    if scrolled {
        let height = shown.response.rect.height() + f32::from(margin.top);
        ui.ctx()
            .data_mut(|d| d.insert_temp(foot_height_id(layer), height));
    }
    shown
}

/// The buttons' row, set at `right`, on the band that runs from 24 above it
/// to the dialog's foot, `left` to `right` and out over the margins.
pub(super) fn row_on_band<R>(
    ui: &mut egui::Ui,
    left: f32,
    right: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let t = crate::theme::Tokens::get(ui.ctx());
    let margin = ui.spacing().window_margin;
    let band = ui.painter().add(egui::Shape::Noop);
    let group = ui.scope_id().with("dm-footer-group");
    let group_width = ui.ctx().data(|d| d.get_temp::<f32>(group));
    let shown = ui.horizontal(|ui| {
        if let Some(width) = group_width {
            let free = right - ui.cursor().left() - width;
            ui.add_space(free.max(0.0).floor());
        }
        let start = ui.cursor().left();
        let inner = add(ui);
        let width = ui.min_rect().right() - start;
        ui.ctx().data_mut(|d| d.insert_temp(group, width));
        inner
    });
    let row = shown.response.rect;
    let rect = egui::Rect::from_min_max(
        egui::pos2(
            left - f32::from(margin.left),
            row.top() - f32::from(margin.top),
        ),
        egui::pos2(
            right.max(row.right()) + f32::from(margin.right),
            row.bottom() + f32::from(margin.bottom),
        ),
    )
    // Inside the dialog's hairline edge.
    .shrink2(egui::vec2(1.0, 0.0))
    .with_max_y(row.bottom() + f32::from(margin.bottom) - 1.0);
    let radius = egui::CornerRadius {
        nw: 0,
        ne: 0,
        sw: t.card_radius.saturating_sub(1),
        se: t.card_radius.saturating_sub(1),
    };
    let painter = ui.painter().with_clip_rect(rect.expand(1.0));
    painter.set(
        band,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, radius, t.frame),
            egui::Shape::hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, t.border)),
        ]),
    );
    shown
}
