use super::*;

impl DmApp {
    /// A menu item for a command: its label, its key, and greyed out when
    /// it has nothing to act on.
    pub(super) fn item(&mut self, ui: &mut egui::Ui, command: Command) {
        let title = self.title_of(command);
        let shown = if command.asks_more() && !title.ends_with('…') {
            format!("{title}…")
        } else {
            title.to_string()
        };
        let mut button = egui::Button::new(shown);
        if let Some(shortcut) = command.shortcut() {
            button = button.shortcut_text(format::shortcut(ui.ctx(), &shortcut));
        }
        let enabled = self.enabled(command);
        let response = ui.add_enabled(enabled, button);
        // Said by its name alone: the "…" is for the eye.
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, title));
        if response.clicked() {
            self.queued.push(command);
            ui.close();
        }
    }

    /// Whether a question or a dialog that keeps the clicks is open: the
    /// menu bar, the toolbar and the places then take none (behind Options, Add URL opened over it and Done was picked).
    pub(super) fn behind_a_dialog(&self) -> bool {
        self.dialogs.asking() || self.dialogs.modal()
    }

    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        // Greyed while a question waits, as the list and toolbar take no
        // clicks then: it opened bright over the dimmed window.
        // And while any dialog keeps the clicks: behind Options its menus
        // opened and their items did nothing.
        let behind = self.behind_a_dialog();
        if behind {
            self.open_menu_by_key = false;
        }
        ui.add_enabled_ui(!behind, |ui| self.menu_bar_items(ui));
    }

    pub(super) fn menu_bar_items(&mut self, ui: &mut egui::Ui) {
        // F10 opens the first menu, as on any desktop: the menus could be
        // reached only by Tabbing to them. Opened before the bar
        // draws, by the id it had last frame: egui closes a popup opened
        // after its frame's drawing.
        if std::mem::take(&mut self.open_menu_by_key) {
            if let Some(id) = self.menus.first() {
                egui::Popup::open_id(ui.ctx(), *id);
            }
        }
        // Left and Right go from one open menu to the next, round the bar,
        // as on any desktop (only Up and Down walked a menu).
        let ctx = ui.ctx().clone();
        if let Some(at) = self
            .menus
            .iter()
            .position(|id| egui::Popup::is_id_open(&ctx, *id))
        {
            let step = ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                    Some(1)
                } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                    Some(self.menus.len() - 1)
                } else {
                    None
                }
            });
            if let Some(step) = step {
                let next = self.menus[(at + step) % self.menus.len()];
                egui::Popup::open_id(&ctx, next);
            }
        }
        egui::MenuBar::new()
            .config(egui::containers::menu::MenuConfig::new().style(menu_style))
            .ui(ui, |ui| {
                let program = ui.menu_button(t("Program"), |ui| {
                    for c in [
                        Command::AddUrl,
                        Command::AddSeries,
                        Command::AddFromClipboard,
                    ] {
                        self.item(ui, c);
                    }
                    ui.separator();
                    self.item(ui, Command::Export);
                    self.item(ui, Command::Import);
                    ui.separator();
                    self.item(ui, Command::SiteFiles);
                    ui.separator();
                    self.item(ui, Command::Quit);
                });
                let file = ui.menu_button(t("File"), |ui| {
                    for c in [
                        Command::DownloadNow,
                        Command::StopDownload,
                        Command::ShowProgress,
                    ] {
                        self.item(ui, c);
                    }
                    ui.separator();
                    for c in [
                        Command::Remove,
                        Command::Redownload,
                        Command::UpdateLink,
                        Command::CopyLink,
                        Command::MoveRename,
                    ] {
                        self.item(ui, c);
                    }
                    ui.separator();
                    for c in [
                        Command::OpenFile,
                        Command::OpenWith,
                        Command::Preview,
                        Command::OpenFolder,
                        Command::ExtractHere,
                        Command::ExtractTo,
                        Command::Properties,
                    ] {
                        self.item(ui, c);
                    }
                    ui.separator();
                    for c in [
                        Command::MoveToQueue,
                        Command::RemoveFromQueue,
                        Command::MoveUp,
                        Command::MoveDown,
                        Command::SelectAll,
                    ] {
                        self.item(ui, c);
                    }
                });
                let downloads = ui.menu_button(t("Downloads"), |ui| {
                    for c in [
                        Command::ResumeAll,
                        Command::StopAll,
                        Command::DeleteCompleted,
                    ] {
                        self.item(ui, c);
                    }
                    ui.separator();
                    self.item(ui, Command::Find);
                    self.item(ui, Command::FindNext);
                    ui.separator();
                    for c in [Command::Scheduler, Command::StartQueue, Command::StopQueue] {
                        self.item(ui, c);
                    }
                    submenu(ui, t("Speed limiter"), |ui| {
                        // Ticked while a limit is in force: the item says
                        // whether the limiter is on, not only that it toggles.
                        // A menu item like the one under it, its tick at the
                        // right: as a checkbox its words started further in
                        // than the item below.
                        let on = self.applied.speed_limit > 0;
                        let item = ticked_item(ui, Command::SpeedLimiterToggle.title(), on);
                        if item.clicked() {
                            self.queued.push(Command::SpeedLimiterToggle);
                            ui.close();
                        }
                        ui.separator();
                        // One click to a speed, as the tray offers too.
                        for preset in [
                            Command::SpeedLow,
                            Command::SpeedMedium,
                            Command::SpeedHigh,
                            Command::SpeedUnlimited,
                        ] {
                            let now = preset.speed_preset() == Some(self.applied.speed_limit);
                            if ticked_item(ui, preset.title(), now).clicked() {
                                self.queued.push(preset);
                                ui.close();
                            }
                        }
                        ui.separator();
                        self.item(ui, Command::SpeedLimiterSettings);
                    });
                    submenu(ui, t("When done"), |ui| {
                        use crate::power::WhenDone;
                        for (c, w) in [
                            (Command::WhenDoneNothing, WhenDone::Nothing),
                            (Command::WhenDoneQuit, WhenDone::Quit),
                            (Command::WhenDoneSleep, WhenDone::Sleep),
                            (Command::WhenDoneHibernate, WhenDone::Hibernate),
                            (Command::WhenDoneShutDown, WhenDone::ShutDown),
                        ] {
                            // Greyed where this computer has no such command
                            // (macOS does not hibernate), rather than failing
                            // when the queue runs dry.
                            let can = matches!(w, WhenDone::Nothing | WhenDone::Quit)
                                || crate::power::available(w);
                            if ui
                                .add_enabled_ui(can, |ui| {
                                    crate::dialogs::radio_flag(ui, self.when_done == w, c.title())
                                })
                                .inner
                                // Why, where it is greyed.
                                .on_disabled_hover_text(t(
                                    "This computer gives dm no way to do this.",
                                ))
                                .clicked()
                            {
                                self.queued.push(c);
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    self.item(ui, Command::Options);
                });
                let view = ui.menu_button(t("View"), |ui| {
                    // Menu items with their tick at the right, so their words
                    // start where the items under them do.
                    if ticked_item(ui, t("Categories"), self.show_categories).clicked() {
                        self.queued.push(Command::ToggleCategories);
                        ui.close();
                    }
                    if ticked_item(ui, t("Toolbar"), self.show_toolbar).clicked() {
                        self.queued.push(Command::ToggleToolbar);
                        ui.close();
                    }
                    if ticked_item(ui, t("Drop target"), self.applied.drop_target).clicked() {
                        self.queued.push(Command::ToggleDropTarget);
                        ui.close();
                    }
                    // "Columns": each column on or off; the file
                    // name stays, since a row without one is no row.
                    submenu_kept(ui, t("Columns"), |ui| self.columns_menu(ui));
                    submenu(ui, t("Customize toolbar"), |ui| self.toolbar_menu(ui));
                    // View › Text size: the window's zoom, kept across launches,
                    // which the keyboard's Ctrl (Cmd) with + and − also changes.
                    submenu(ui, t("Text size"), |ui| {
                        let ctx = ui.ctx().clone();
                        let zoom = ctx.zoom_factor();
                        ui.label(
                            RichText::new(tf("{}%", &[&((zoom * 100.0).round() as u32)])).weak(),
                        );
                        if ui.button(t("Larger")).clicked() {
                            ctx.set_zoom_factor((zoom + 0.1).min(3.0));
                        }
                        if ui.button(t("Smaller")).clicked() {
                            ctx.set_zoom_factor((zoom - 0.1).max(0.5));
                        }
                        if ui.button(t("Normal size")).clicked() {
                            ctx.set_zoom_factor(1.0);
                        }
                    });
                    // View › Language.
                    submenu(ui, t("Language"), |ui| {
                        let now = crate::i18n::chosen(&self.applied.language);
                        if let Some(code) = crate::i18n::pick(ui, now) {
                            let mut wanted = self.applied.clone();
                            wanted.language = code.to_string();
                            if let Err(e) = self.apply_settings(wanted) {
                                self.said = e.into();
                            }
                            ui.close();
                        }
                    });
                    submenu(ui, t("Sort by"), |ui| {
                        for c in [
                            Command::SortByName,
                            Command::SortBySize,
                            Command::SortByStatus,
                            Command::SortByTimeLeft,
                            Command::SortByRate,
                            Command::SortByDate,
                            Command::SortByDescription,
                        ] {
                            self.item(ui, c);
                        }
                    });
                });
                let help = ui.menu_button(t("Help"), |ui| {
                    self.item(ui, Command::Help);
                    self.item(ui, Command::CheckForUpdates);
                    self.item(ui, Command::About);
                });
                self.menus = [program, file, downloads, view, help]
                    .iter()
                    .map(|menu| egui::Popup::default_response_id(&menu.response))
                    .collect();
            });
    }

    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let chosen = ToolStyle::named(&self.applied.toolbar_style);
        let buttons: Vec<_> = toolbar_in(&self.applied.toolbar_order)
            .into_iter()
            .filter(|(command, _, _)| {
                !self
                    .applied
                    .toolbar_hidden
                    .iter()
                    .any(|h| h == command.id())
            })
            .collect();
        let failed_only = self.failed_only();
        let held_only = self.held_only();
        // Each button with the name it shows and the widest it can show,
        // which is the room it takes: Resume becoming Retry moved every
        // button after it.
        let buttons: Vec<(Command, Icon, &'static str, &'static str)> = buttons
            .into_iter()
            .map(|(command, icon, label)| {
                let names = if command == Command::DownloadNow {
                    vec![
                        crate::i18n::t(label),
                        crate::i18n::t("Retry"),
                        crate::i18n::t("Start now"),
                    ]
                } else {
                    vec![crate::i18n::t(label)]
                };
                let shown = if command == Command::DownloadNow && failed_only {
                    crate::i18n::t("Retry")
                } else if command == Command::DownloadNow && held_only {
                    // As the row menu says.
                    crate::i18n::t("Start now")
                } else {
                    crate::i18n::t(label)
                };
                let widest = names
                    .into_iter()
                    .max_by(|a, b| {
                        modern_width(ui, a, false).total_cmp(&modern_width(ui, b, false))
                    })
                    .unwrap_or(shown);
                (command, icon, shown, widest)
            })
            .collect();
        // Every button keeps its name. The modern toolbar puts each name
        // beside its picture where one row has room for that, under it
        // where it has not (a narrow window, or longer words, as German's),
        // and goes on to a second row only where even that does not fit.
        // Pictures alone, at those widths, left most of it to be guessed.
        let t = crate::theme::Tokens::get(ui.ctx());
        let spacing = ui.spacing().item_spacing.x;
        let room = ui.available_width();
        let width = |stacked: bool| -> f32 {
            buttons
                .iter()
                .map(|(_, _, _, widest)| modern_width(ui, widest, stacked) + spacing)
                .sum()
        };
        let modern = t.modern && chosen == ToolStyle::Both;
        let stacked = modern && width(false) > room;
        // Where even stacked words do not fit on one row, icons alone, their
        // names on hover: wrapped to three rows at 150% text in a small
        // window, the toolbar left one row of the list.
        let icons_only = stacked && width(true) > room;
        // And icons alone go on to a second row where even they do not fit:
        // at 200% text in the smallest window they ran past the right edge.
        // The classic look's buttons always wrap.
        let wrap = !modern || icons_only;
        let in_front = self.dialogs.in_front();
        let mut draw = |ui: &mut egui::Ui| {
            for (command, icon, name, widest) in &buttons {
                // The modern toolbar leads with one primary action.
                let primary = *command == Command::AddUrl;
                // Greyed behind a dialog too: Add URL stayed bright blue
                // under the dim.
                let enabled = self.enabled(*command) && ui.is_enabled();
                let response = if icons_only {
                    tool(
                        ui,
                        *icon,
                        (name, widest),
                        enabled,
                        ToolStyle::Icons,
                        primary,
                    )
                } else if stacked {
                    modern_tool(ui, *icon, (name, widest), enabled, primary, &t, true)
                } else {
                    tool(ui, *icon, (name, widest), enabled, chosen, primary)
                };
                // What the button does in full, with its key: only where that
                // says more than the button's own word (every
                // tooltip repeated its label).
                let title = self.title_of(*command);
                let key = command
                    .shortcut()
                    .map(|k| format!(" ({})", format::shortcut(ui.ctx(), &k)))
                    .unwrap_or_default();
                let said = format!("{title}{key}");
                // None while a dialog is in front: one drawn over its title
                // read as part of it.
                let response =
                    if (said != *name || chosen == ToolStyle::Icons || icons_only) && !in_front {
                        response.on_hover_text(said)
                    } else {
                        response
                    };
                if response.clicked() {
                    self.queued.push(*command);
                }
            }
        };
        if wrap {
            ui.horizontal_wrapped(|ui| draw(ui));
        } else {
            ui.horizontal(|ui| draw(ui));
        }
        ui.add_space(2.0);
    }

    /// Customising the toolbar: the style, which buttons show, and their
    /// order. Saved with the settings.
    pub(super) fn toolbar_menu(&mut self, ui: &mut egui::Ui) {
        let mut wanted = self.applied.clone();
        let current = ToolStyle::named(&wanted.toolbar_style);
        for (style, name) in TOOL_STYLES {
            if crate::dialogs::radio_flag(ui, current == style, t(name)).clicked() {
                wanted.toolbar_style = style.name().into();
            }
        }
        ui.separator();
        let buttons = toolbar_in(&wanted.toolbar_order);
        let mut order: Vec<String> = buttons.iter().map(|(c, _, _)| c.id().to_string()).collect();
        for (i, (command, _, label)) in buttons.iter().enumerate() {
            let id = command.id().to_string();
            ui.horizontal(|ui| {
                let mut on = !wanted.toolbar_hidden.contains(&id);
                if crate::dialogs::check(ui, &mut on, t(label)).changed() {
                    if on {
                        wanted.toolbar_hidden.retain(|h| *h != id);
                    } else {
                        wanted.toolbar_hidden.push(id.clone());
                    }
                }
                if let Some(to) =
                    move_buttons(ui, crate::i18n::t(label), i > 0, i + 1 < buttons.len())
                {
                    order.swap(i, to(i));
                }
            });
        }
        if order
            != buttons
                .iter()
                .map(|(c, _, _)| c.id().to_string())
                .collect::<Vec<_>>()
        {
            wanted.toolbar_order = order;
        }
        if wanted.toolbar_style != self.applied.toolbar_style
            || wanted.toolbar_hidden != self.applied.toolbar_hidden
            || wanted.toolbar_order != self.applied.toolbar_order
        {
            if let Err(e) = self.apply_settings(wanted) {
                self.said = e.into();
            }
        }
    }
}
