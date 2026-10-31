use super::*;

impl DmApp {
    /// How wide the places start: room for the longest name in them,
    /// queues included, up to a third of the window. German and Russian
    /// queue names were cut short in a rail that had the room.
    pub(super) fn rail_width(&self, ui: &egui::Ui, least: f32) -> f32 {
        let mut names: Vec<String> = self.queues().iter().map(|q| crate::i18n::name(q)).collect();
        names.extend(
            Category::ALL
                .iter()
                .map(|c| crate::i18n::name(&format::titled(&category_word(*c)))),
        );
        names.extend(["In progress", "Done"].map(|w| t(w).to_string()));
        let widest = names
            .into_iter()
            .map(|n| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap(n, egui::FontId::proportional(14.0), egui::Color32::WHITE)
                        .size()
                        .x
                })
            })
            .fold(0.0_f32, f32::max);
        // The icon and count beside it, the branch's indent and the margins.
        (widest + 36.0 + 34.0 + 18.0 + 16.0).clamp(least, (ui.available_width() / 3.0).max(least))
    }

    pub(super) fn categories(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let mut chosen = None;
        // What a place's right-click menu asked for.
        let mut asked: Option<(Place, TreeAction)> = None;
        let folder = crate::theme::Tokens::get(ui.ctx()).folder;
        let tokens = crate::theme::Tokens::get(ui.ctx());
        let mut place = |ui: &mut egui::Ui, app: &DmApp, place: Place, label: String| {
            let n = app.count(&place);
            let text = format!("{label} ({n})");
            ui.horizontal(|ui| {
                let response = if tokens.modern {
                    nav_item(
                        ui,
                        app.place == place,
                        &label,
                        n,
                        place_icon(&place),
                        &tokens,
                    )
                } else {
                    // The tree is folders.
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                    icons::folder(ui.painter(), rect, folder);
                    ui.selectable_label(app.place == place, text)
                };
                if response.clicked() {
                    chosen = Some(place.clone());
                }
                response.context_menu(|ui| {
                    for (label, action) in tree_menu(&place) {
                        if ui.button(t(label)).clicked() {
                            asked = Some((place.clone(), action));
                            ui.close();
                        }
                    }
                });
            });
        };
        // Kept open or shut by an id of its own, not its words: a language
        // switch opened a shut one again.
        let in_everything = matches!(self.place, Place::All | Place::Kind(_) | Place::Custom(_));
        nav_group(
            ui,
            "tree-everything",
            t("Everything"),
            in_everything,
            &tokens,
            |ui| {
                place(ui, self, Place::All, t("All").into());
                for c in Category::ALL {
                    let label = crate::i18n::name(&format::titled(&category_word(c)));
                    place(ui, self, Place::Kind(c), label);
                }
                for custom in &self.applied.custom_categories {
                    let name = custom.name.trim().to_string();
                    if !name.is_empty() {
                        place(ui, self, Place::Custom(name.clone()), name);
                    }
                }
            },
        );
        place(ui, self, Place::Unfinished, t("In progress").into());
        place(ui, self, Place::Finished, t("Done").into());
        let queues = self.queues();
        let in_queues = matches!(self.place, Place::Queue(_));
        nav_group(ui, "tree-queues", t("Queues"), in_queues, &tokens, |ui| {
            for name in queues {
                let label = crate::i18n::name(&name);
                place(ui, self, Place::Queue(name.clone()), label);
            }
        });
        if let Some(place) = chosen {
            self.place = place;
        }
        if let Some((place, action)) = asked {
            self.tree_action(place, action);
        }
    }

    /// The places as one drop-down above the list, where the window has no
    /// room for them beside it: at large text they went, and All, Done and
    /// the queues could not be reached at all.
    pub(super) fn place_picker(&mut self, ui: &mut egui::Ui) {
        let mut places: Vec<(Place, String)> = vec![(Place::All, t("All").into())];
        for c in Category::ALL {
            places.push((
                Place::Kind(c),
                crate::i18n::name(&format::titled(&category_word(c))),
            ));
        }
        for custom in &self.applied.custom_categories {
            let name = custom.name.trim().to_string();
            if !name.is_empty() {
                places.push((Place::Custom(name.clone()), name));
            }
        }
        places.push((Place::Unfinished, t("In progress").into()));
        places.push((Place::Finished, t("Done").into()));
        for queue in self.queues() {
            let label = crate::i18n::name(&queue);
            places.push((Place::Queue(queue), label));
        }
        let shown = |place: &Place, label: &str| format!("{label} ({})", self.count(place));
        let current = places
            .iter()
            .find(|(p, _)| *p == self.place)
            .map(|(p, l)| shown(p, l))
            .unwrap_or_default();
        let mut chosen = None;
        ui.horizontal(|ui| {
            let name = ui.label(t("Place"));
            crate::dialogs::combo("place-picker")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (place, label) in &places {
                        if crate::dialogs::option(ui, *place == self.place, shown(place, label))
                            .clicked()
                        {
                            chosen = Some(place.clone());
                        }
                    }
                })
                .response
                .labelled_by(name.id);
        });
        if let Some(place) = chosen {
            self.place = place;
        }
    }

    /// Runs what a place's right-click menu asked for. The place is chosen
    /// first, as a right-click selects it.
    pub fn tree_action(&mut self, place: Place, action: TreeAction) {
        self.place = place.clone();
        match action {
            TreeAction::Run(command) => self.run(command),
            TreeAction::EditCategories => {
                self.dialogs.open_options(&self.applied);
                if let Some(options) = self.dialogs.options.as_mut() {
                    options.tab = crate::dialogs::Tab::SaveTo;
                }
            }
            TreeAction::OpenFolder => {
                let root = self.applied.download_dir.clone();
                let categories = self.applied.categories();
                let dir = match &place {
                    Place::Kind(kind) => categories.destination(&root, *kind),
                    Place::Custom(name) => root.join(name),
                    _ => root,
                };
                self.open_file(&dir.to_string_lossy());
            }
        }
    }
}
