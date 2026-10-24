use super::*;

/// The drop target's side, in points: a small card that stays out of the
/// way of the work it floats over.
const SIZE: f32 = 44.0;

impl DmApp {
    /// The floating drop target (View › Drop target): a small window over
    /// every other, moved by dragging it, that takes a link or a file of
    /// links dropped on it as the main window does. A right-click offers to
    /// show dm or put the target away.
    pub fn drop_target(&mut self, ctx: &egui::Context) {
        if !self.applied.drop_target || self.headless {
            return;
        }
        let id = egui::ViewportId::from_hash_of("dm-drop-target");
        // First shown in the main window's bottom-right corner, above its
        // status bar: a window made with no position opens in the middle of
        // the screen, over whatever is being read or clicked, and a corner
        // worked out from the monitor's size alone lands on the wrong
        // display when dm is on a second one. A person moves it by dragging.
        let corner = ctx.input(|i| {
            let view = i.viewport();
            match (view.outer_rect, view.minimized) {
                (Some(window), Some(false) | None) => Some(egui::pos2(
                    (window.max.x - SIZE - 20.0).max(window.min.x),
                    (window.max.y - SIZE - 52.0).max(window.min.y),
                )),
                _ => view.monitor_size.map(|screen| {
                    egui::pos2(
                        (screen.x - SIZE - 20.0).max(0.0),
                        (screen.y - SIZE - 96.0).max(0.0),
                    )
                }),
            }
        });
        let mut builder = egui::ViewportBuilder::default()
            .with_title(crate::drops::DROP_TARGET_TITLE)
            .with_inner_size([SIZE, SIZE])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false);
        if let Some(corner) = corner {
            builder = builder.with_position(corner);
        }
        let mut links = Vec::new();
        let mut dropped_any = false;
        let mut action: Option<Command> = None;
        let mut show = false;
        let tokens = crate::theme::Tokens::get(ctx);
        ctx.show_viewport_immediate(id, builder, |vui, _class| {
            let ctx = vui.ctx().clone();
            crate::drops::attach_floating();
            // Its own window's drops; drawn inside the main window (where
            // there are no separate windows) it shares the main window's,
            // which the main window takes.
            let dropped = if ctx.embed_viewports() {
                Vec::new()
            } else {
                ctx.input(|i| i.raw.dropped_files.clone())
            };
            dropped_any = !dropped.is_empty();
            for file in &dropped {
                links.extend(dropped_links(file.as_ref()));
            }
            let over = ctx.input(|i| !i.raw.hovered_files.is_empty());
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(vui, |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                    // A card, as the rest of the window is: the dialog's
                    // surface, a hairline edge and a Fluent glyph in the
                    // accent. Only a link held over it fills it with the
                    // accent, to say that it will take it.
                    let (fill, edge, ink) = if over {
                        (tokens.brand, tokens.brand, tokens.on_brand)
                    } else if response.hovered() {
                        (tokens.control_hover, tokens.stroke, tokens.accent)
                    } else {
                        (tokens.dialog, tokens.border, tokens.accent)
                    };
                    let card = rect.shrink(3.0);
                    let painter = ui.painter();
                    painter.rect_filled(card, 8.0, fill);
                    painter.rect_stroke(
                        card,
                        8.0,
                        egui::Stroke::new(1.0, edge),
                        egui::StrokeKind::Inside,
                    );
                    let glyph = egui::Rect::from_center_size(card.center(), egui::vec2(18.0, 18.0));
                    if !crate::icons::glyph(
                        painter,
                        glyph,
                        crate::icons::fluent::ARROW_DOWNLOAD,
                        ink,
                    ) {
                        painter.text(
                            card.center(),
                            egui::Align2::CENTER_CENTER,
                            "↓",
                            egui::FontId::proportional(16.0),
                            ink,
                        );
                    }
                    if response.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    let response = response
                        .on_hover_text(t("Drop a link here to download it with dm. Drag to move."));
                    if response.double_clicked() {
                        show = true;
                    }
                    response.context_menu(|ui| {
                        if ui.button(t("Show dm")).clicked() {
                            show = true;
                            ui.close();
                        }
                        if ui.button(t("Hide the drop target")).clicked() {
                            action = Some(Command::ToggleDropTarget);
                            ui.close();
                        }
                    });
                });
        });
        if !links.is_empty() {
            let _ = self.add(&links, Default::default());
        } else if dropped_any {
            self.said = crate::i18n::Said::new(drop_hint(), &[]);
        }
        if let Some(command) = action {
            self.queued.push(command);
        }
        self.show_asked |= show;
    }

    pub(super) fn take_drops(&mut self, ctx: &egui::Context) {
        crate::drops::watch(ctx);
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        // Links dragged in from a browser, where the window system offers
        // them (`drops`).
        let texts = crate::drops::take();
        let mut links = Vec::new();
        for file in &dropped {
            links.extend(dropped_links(file.as_ref()));
        }
        for what in &texts {
            match what {
                crate::drops::Dropped::Text(text) => links.extend(links_in(text)),
                crate::drops::Dropped::File(path) => {
                    links.extend(dropped_links(&crate::drops::DroppedPath(path.clone())));
                }
            }
        }
        if !links.is_empty() {
            let _ = self.add(&links, Default::default());
        } else if !dropped.is_empty() || !texts.is_empty() {
            // A drop with no link in it was ignored without a word.
            self.said = crate::i18n::Said::new(drop_hint(), &[]);
        }
    }
}
