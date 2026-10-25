use super::*;

impl DmApp {
    /// The window, drawn once.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.welcome_checked {
            self.welcome_checked = true;
            self.welcome_if_new();
        }
        // Copy link's addresses, onto the system's clipboard.
        if let Some(text) = self.copied.take() {
            ctx.copy_text(text);
        }
        // What a text field's menu chose on the frame before, given to the
        // field as the keys would give it.
        // Escape closes a field's menu first, not the dialog under it,
        // which took the key and left the menu standing.
        if self.field_menu.is_some()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.field_menu = None;
        }
        if !self.field_edits.is_empty() {
            let edits = std::mem::take(&mut self.field_edits);
            ctx.input_mut(|i| i.events.extend(edits));
        }
        // The smallest window is the same on screen at any text size: given
        // in points, it grew with the zoom, and at 200% a relaunched window
        // opened past the bottom of the screen and could not be made
        // smaller.
        let zoom = ctx.zoom_factor();
        if self.min_size_zoom != Some(zoom) {
            self.min_size_zoom = Some(zoom);
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(
                egui::vec2(MIN_WINDOW.0, MIN_WINDOW.1) / zoom.max(0.5),
            ));
        }
        if self.searches_running > 0 {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        // The window itself is what a screen reader names first; egui leaves
        // its root node without a name, which Orca read as a nameless frame.
        ctx.accesskit_node_builder(egui::accesskit_root_id(), |root| root.set_label("dm"));
        // "system" follows the computer's light or dark setting.
        // On Linux the window library cannot say, so the desktop is asked.
        let system_dark = crate::system_theme::prefers_dark()
            .or_else(|| ctx.system_theme().map(|t| t == egui::Theme::Dark))
            .unwrap_or(false);
        // The look chosen in an open Options is shown as it is chosen, and
        // goes back with Cancel; OK puts it in force.
        let wanted = self
            .dialogs
            .options
            .as_ref()
            .map_or(self.applied.theme.as_str(), |d| d.draft.theme.as_str());
        let theme = crate::theme::Theme::resolve(wanted, system_dark);
        // Applied again once the embedded fonts are in force (a frame after
        // they are set), so headings take the semibold face.
        let ready = crate::theme::fonts_ready(&ctx);
        self.dark = theme.tokens().dark;
        // The computer's accent, for the modern looks' accent brushes.
        let accent = crate::accent::system();
        if self.theme_in_force != Some(theme)
            || ready != self.fonts_ready
            || self.accent_in_force != Some(accent)
        {
            crate::theme::apply_with_accent(&ctx, theme, accent);
            self.accent_in_force = Some(accent);
            self.theme_in_force = Some(theme);
            self.fonts_ready = ready;
            self.motion_in_force = None;
        }
        // Less motion where the computer asks for it.
        let reduced = crate::motion::reduced();
        if self.motion_in_force != Some(reduced) {
            crate::motion::apply(&ctx, reduced);
            self.motion_in_force = Some(reduced);
        }
        self.logic(&ctx);
        // A name in a script dm's own typeface lacks (Chinese, Korean,
        // Arabic…) has the computer's font for it found, rather than
        // drawn as blanks.
        crate::theme::cover(
            &ctx,
            self.rows
                .iter()
                .flat_map(|r| [&r.filename, &r.path, &r.description])
                .map(String::as_str)
                // The window's own words in Chinese, Japanese or Korean, and
                // each language's name in its own script before View ›
                // Language first lists them.
                .chain(crate::i18n::LANGUAGES.iter().map(|(_, name)| *name)),
        );
        self.keys(&ctx);
        self.take_drops(&ctx);
        self.drop_target(&ctx);

        let t = crate::theme::Tokens::get(&ctx);
        // The modern look: surfaces meet without lines; the classic look keeps them.
        let lines = !t.modern;
        let flat = |fill: egui::Color32, x: i8, y: i8| {
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(x, y))
        };
        let behind = self.behind_a_dialog();
        if behind && !self.was_behind {
            self.focus_before = ui.ctx().memory(|m| m.focused());
        } else if !behind && self.was_behind {
            if let Some(id) = self.focus_before.take() {
                ui.ctx().memory_mut(|m| m.request_focus(id));
            }
        }
        self.was_behind = behind;
        // Decided afresh each frame, by the top row that draws the box; a
        // frame without that row leaves the list its own filter.
        self.title_search = false;
        let menu = egui::Panel::top("menu")
            .show_separator_line(lines)
            .frame(flat(t.frame, 6, 2))
            .show(ui, |ui| self.menu_bar(ui));
        self.menu_bottom = menu.response.rect.bottom();
        // For the dialogs, which keep below it.
        ui.ctx()
            .data_mut(|d| d.insert_temp(crate::dialogs::menu_bottom_id(), self.menu_bottom));
        self.bars_bottom = self.menu_bottom;
        // Left out where the window is too short for it and a row of the
        // list: at 230% in the smallest window it wrapped to three rows and
        // no download showed at all. Its commands are all in
        // the menus.
        let tall_enough = ui.ctx().content_rect().height() >= 215.0;
        if self.show_toolbar && tall_enough {
            let bar = egui::Panel::top("toolbar")
                .show_separator_line(lines)
                .frame(flat(t.frame, 8, if t.modern { 4 } else { 0 }))
                .show(ui, |ui| {
                    let behind = self.behind_a_dialog();
                    ui.add_enabled_ui(!behind, |ui| self.toolbar(ui));
                });
            self.bars_bottom = bar.response.rect.bottom();
        }
        egui::Panel::bottom("status")
            .show_separator_line(lines)
            .frame(flat(t.frame, 12, 4))
            .show(ui, |ui| self.status_bar(ui));
        if !self.problems.is_empty() {
            egui::Panel::bottom("problems").show(ui, |ui| self.problem_list(ui));
        }
        // The rail gives way where the list would have too little room
        // left: at 200% text in the usual window it left two columns.
        // View › Categories still turns it off and on.
        let room = ui.available_width();
        let rail_gone = self.show_categories && room < 640.0;
        if self.show_categories && room >= 640.0 {
            // Fitted afresh in another language: kept from a German run,
            // the places stayed wide in English and cut the list's last
            // column.
            let language = self.applied.language.clone();
            if self.rail_language.as_ref().is_some_and(|l| *l != language) {
                let id = ui.scope_id().with_salt(egui::IdSalt::new("categories"));
                ui.ctx().data_mut(|d| d.remove::<egui::PanelState>(id));
            }
            self.rail_language = Some(language);
            egui::Panel::left("categories")
                .resizable(true)
                .show_separator_line(lines)
                .default_size(self.rail_width(ui, if t.modern { 230.0 } else { 200.0 }))
                // And never narrower than its longest name needs: a width
                // kept from English cut German names after a switch and a
                // relaunch.
                .min_size(self.rail_width(ui, 120.0))
                .frame(flat(t.frame, 8, 4))
                .show(ui, |ui| {
                    // More places than fit scroll, rather than being cut off,
                    // with a bar that shows it: hidden until hovered, a tree
                    // cut by a two-row toolbar looked as though it ended there.
                    ui.spacing_mut().scroll = crate::theme::steady_scroll();
                    // Places 4 apart, as a navigation view's items stand.
                    if t.modern {
                        ui.spacing_mut().item_spacing.y = 4.0;
                    }
                    let behind = self.behind_a_dialog();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_enabled_ui(!behind, |ui| self.categories(ui))
                        });
                });
        }
        // The list on its own surface: in the classic look, a card rounded at
        // every corner. In the modern look, as a navigation view's content: the list on
        // the layer, reaching the window's right edge, its top-left corner
        // alone rounded, a hairline where it meets the pane and the bars.
        let card = egui::Frame::new()
            .fill(t.list)
            .stroke(egui::Stroke::new(
                1.0,
                if t.modern { t.border } else { t.list },
            ))
            .corner_radius(if t.modern {
                egui::CornerRadius {
                    nw: t.card_radius,
                    ..Default::default()
                }
            } else {
                egui::CornerRadius::same(t.card_radius)
            })
            // Its right hairline past the window's edge.
            .outer_margin(egui::Margin {
                right: if t.modern { -1 } else { 0 },
                ..Default::default()
            })
            .inner_margin(egui::Margin::same(if t.modern { 6 } else { 2 }));
        let central = if t.modern {
            egui::Frame::new().fill(t.frame).inner_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 4,
                bottom: 0,
            })
        } else {
            flat(t.frame, 0, 0)
        };
        egui::CentralPanel::default_margins()
            .frame(central)
            .show(ui, |ui| {
                card.show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    if rail_gone {
                        self.place_picker(ui);
                    }
                    self.awaiting_bar(ui);
                    self.update_bar(ui);
                    self.stalled_bar(ui);
                    self.browser_bar(ui);
                    self.clipboard_bar(ui);
                    self.filter_bar(ui);
                    crate::table::show(self, ui);
                });
            });

        crate::dialogs::show(self, &ctx);
        underline_fields(&ctx);
        self.focus_ring(&ctx);
        self.field_menu(&ctx);

        for command in std::mem::take(&mut self.queued) {
            self.run(command);
        }
    }

    pub(super) fn keys(&mut self, ctx: &egui::Context) {
        // F3 types nothing: it finds the next one even from Find's own field.
        if self.dialogs.find_open && ctx.egui_wants_keyboard_input() {
            let f3 = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F3);
            if ctx.input_mut(|i| i.consume_shortcut(&f3)) {
                self.queued.push(Command::FindNext);
            }
        }
        // Not while typing: Delete in a text field must delete a letter, not
        // a download. Only a text field: a row or a button with the
        // keyboard's focus counted as typing, and Delete did nothing for a
        // person using the keyboard alone.
        if ctx.text_edit_focused() {
            return;
        }
        // A focused button takes Enter for itself.
        let focused = ctx.egui_wants_keyboard_input();
        // F10: the menu bar; Shift+F10 or the Menu key: the row menu of the
        // row with the keyboard's place, as a right-click opens it.
        // Shift+F10 first: egui matches a key given no Shift with Shift held
        // too, so plain F10 took it for itself.
        let row_menu = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Menu)
        });
        let f10 = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F10));
        if f10 && !self.dialogs.asking() {
            self.open_menu_by_key = true;
            ctx.request_repaint();
        }
        if row_menu && !self.dialogs.in_front() && self.cursor.is_some() {
            self.row_menu_by_key = true;
            ctx.request_repaint();
        }
        // A dialog in front has the keys: Delete or Enter pressed in Options
        // must not remove or open the downloads selected behind it. The
        // windows each download opens on its own are not in front of the list
        // that way, and leave it its keys.
        let behind = self.dialogs.in_front();
        for command in ctx.input_mut(Command::pressed) {
            // F3 is what Find is open for: it finds behind its own dialog.
            let finding = command == Command::FindNext && self.dialogs.find_open;
            if !(behind && command.acts_on_the_list()) || finding {
                self.queued.push(command);
            }
        }
        if !behind
            && !focused
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
            && !self.selected.is_empty()
        {
            self.queued.push(Command::ShowProgress);
        }
        // Up, Down, Home and End move the selection; with Shift they extend
        // it from where it was started. Not while a menu is open: its
        // keys are its own.
        if !behind && !self.popup_was_open {
            let pressed = ctx.input(|i| {
                [
                    (egui::Key::ArrowUp, crate::table::Step::Up),
                    (egui::Key::ArrowDown, crate::table::Step::Down),
                    (egui::Key::Home, crate::table::Step::First),
                    (egui::Key::End, crate::table::Step::Last),
                    (
                        egui::Key::PageUp,
                        crate::table::Step::PageUp(self.list_page),
                    ),
                    (
                        egui::Key::PageDown,
                        crate::table::Step::PageDown(self.list_page),
                    ),
                ]
                .into_iter()
                .find(|(key, _)| i.key_pressed(*key))
                .map(|(_, step)| (step, i.modifiers.shift))
            });
            if let Some((step, shift)) = pressed {
                let ids: Vec<Id> = self.visible().iter().map(|r| r.id).collect();
                let (selected, anchor, cursor) = crate::table::stepped(
                    &ids,
                    &self.selected,
                    self.anchor,
                    self.cursor,
                    step,
                    shift,
                );
                (self.selected, self.anchor, self.cursor) = (selected, anchor, cursor);
                self.scroll_to = cursor;
                // The list has the keys now: egui's own move of the focus
                // to a control above or below is not wanted beside it.
                ctx.memory_mut(|m| m.stop_text_input());
            }
        }
    }

    /// Whether the keyboard is in use, and a ring around what has its
    /// focus — a control, or the list's row the keys are on — so it can be
    /// seen where Tab and the arrows went. Not drawn after a click.
    pub(super) fn focus_ring(&mut self, ctx: &egui::Context) {
        let (pointer, keys) = ctx.input(|i| {
            let keys = i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::Tab
                            | egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                            | egui::Key::Home
                            | egui::Key::End
                            | egui::Key::PageUp
                            | egui::Key::PageDown,
                        pressed: true,
                        ..
                    }
                )
            });
            (i.pointer.any_pressed(), keys)
        });
        if pointer {
            self.keyboard = false;
        } else if keys {
            self.keyboard = true;
        }
        self.focus_ring = None;
        if !self.keyboard {
            return;
        }
        let t = crate::theme::Tokens::get(ctx);
        let stroke = egui::Stroke::new(2.0, t.focus_outer);
        let focused = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id));
        // A field with the keyboard shows it by its accent line, as WinUI's
        // fields do; the ring around it as well was one too many.
        if t.modern
            && focused
                .as_ref()
                .is_some_and(|r| egui::TextEdit::load_state(ctx, r.id).is_some())
        {
            return;
        }
        let (rect, layer, row) = match focused {
            Some(r) => (r.interact_rect, r.layer_id, false),
            // Kept inside the list's frame: a row is as wide as every
            // column, and its ring ran past the frame's edge.
            None => match self.cursor.and_then(|id| self.row_rects.get(&id)) {
                Some(rect) => (
                    self.list_clip.map_or(*rect, |clip| rect.intersect(clip)),
                    egui::LayerId::background(),
                    true,
                ),
                None => return,
            },
        };
        if rect.is_positive() {
            // Two lines, as WinUI draws its focus: an outer one in the
            // text colour and an inner one in the background's, so one of
            // them shows on whatever the control is filled with.
            let painter = ctx.layer_painter(layer);
            let radius = egui::CornerRadius::same(t.radius);
            let inner = egui::Stroke::new(1.0, t.focus_inner);
            if row {
                // A row's inside its frame, where the list ends.
                painter.rect_stroke(rect.shrink(1.0), radius, stroke, egui::StrokeKind::Inside);
                painter.rect_stroke(rect.shrink(3.0), radius, inner, egui::StrokeKind::Inside);
            } else {
                // Round a control, not over it: inside, it crossed a
                // checkbox's box and its last letter.
                painter.rect_stroke(rect, radius, inner, egui::StrokeKind::Outside);
                painter.rect_stroke(rect.expand(1.0), radius, stroke, egui::StrokeKind::Outside);
            }
            self.focus_ring = Some(rect);
        }
    }

    /// Cut, Copy and Paste on a right-click in a text field, as in any
    /// program: the keys worked, the menu was not there.
    pub(super) fn field_menu(&mut self, ctx: &egui::Context) {
        let (clicked, right_down, at) = ctx.input(|i| {
            (
                i.pointer.secondary_clicked(),
                i.pointer.secondary_down() || i.pointer.secondary_pressed(),
                i.pointer.interact_pos(),
            )
        });
        let focused = ctx
            .memory(|m| m.focused())
            .filter(|_| ctx.egui_wants_keyboard_input());
        // The field's selection, kept from before the right-click: the
        // press puts the caret where it lands, and Cut and Copy then had
        // nothing to act on.
        if !right_down && !clicked && self.field_menu.is_none() {
            self.field_selection = focused.and_then(|id| {
                egui::text_edit::TextEditState::load(ctx, id)
                    .and_then(|state| state.cursor.char_range())
                    .filter(|range| !range.is_empty())
                    .map(|range| (id, range))
            });
        }
        if clicked {
            // The field the right-click put the caret in.
            let field = focused
                .and_then(|id| ctx.read_response(id).map(|r| (id, r.rect)))
                .filter(|(_, rect)| at.is_some_and(|at| rect.contains(at)));
            self.field_menu = field.zip(at).map(|((id, _), at)| (id, at));
            return;
        }
        let Some((id, at)) = self.field_menu else {
            return;
        };
        // Gone with its dialog, it does not stay over the list.
        if ctx.read_response(id).is_none() {
            self.field_menu = None;
            return;
        }
        let selected = self
            .field_selection
            .filter(|(field, _)| *field == id)
            .map(|(_, range)| range);
        let mut chosen: Option<u8> = None;
        let shown = egui::Area::new(egui::Id::unique("dm-field-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    // Items of one width and no frame, as in every menu.
                    ui.set_min_width(140.0);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        for (n, word) in [t("Cut"), t("Copy"), t("Paste")].into_iter().enumerate() {
                            let enabled = n == 2 || selected.is_some();
                            let item = egui::Button::new(word).frame(false);
                            if ui.add_enabled(enabled, item).clicked() {
                                chosen = Some(n as u8);
                            }
                        }
                    });
                })
            });
        // The clipboard read only for a paste.
        let chosen = chosen.map(|n| match n {
            0 => egui::Event::Cut,
            1 => egui::Event::Copy,
            _ => egui::Event::Paste(self.backend.platform.clipboard().unwrap_or_default()),
        });
        let away = ctx.input(|i| i.pointer.any_pressed()) && !at_inside(ctx, shown.response.rect);
        if let Some(edit) = chosen {
            // The selection put back, for Cut and Copy to act on.
            if let (Some(range), Some(mut state)) =
                (selected, egui::text_edit::TextEditState::load(ctx, id))
            {
                state.cursor.set_char_range(Some(range));
                state.store(ctx, id);
            }
            self.field_edits.push(edit);
            ctx.memory_mut(|m| m.request_focus(id));
            self.field_menu = None;
        } else if away {
            self.field_menu = None;
        }
    }

    /// A new install's first window: "Add dm to your browser", with a
    /// button for each browser on the computer, as the installer's last
    /// step (which starts dm) leads into. Once, and only while no browser
    /// has spoken to dm; not for a dm run on a home of its own (tests and
    /// checks set `DM_HOME`), which opens it with [`Self::welcome`].
    pub(super) fn welcome_if_new(&mut self) {
        let own_home = std::env::var_os("DM_HOME").is_some_and(|h| !h.is_empty());
        let home = dm_shell::settings::home();
        if self.headless
            || own_home
            || dm_shell::host::browser_seen(&home)
            || home.join(WELCOMED).exists()
            || home.join(BROWSER_HINT_HIDDEN).exists()
        {
            return;
        }
        self.welcome();
    }

    /// Opens the welcome with the browsers found on this computer.
    pub fn welcome(&mut self) {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        self.dialogs.welcome = Some(dm_shell::register::installed_browsers(&home));
        let dm_home = dm_shell::settings::home();
        let _ = std::fs::create_dir_all(&dm_home);
        let _ = std::fs::write(dm_home.join(WELCOMED), b"");
    }

    /// While the main window is hidden in the tray: the floating drop
    /// target, and what is dropped on it, are still taken.
    pub fn while_hidden(&mut self, ctx: &egui::Context) {
        if self.applied.drop_target {
            self.take_drops(ctx);
            self.drop_target(ctx);
        }
    }
}
