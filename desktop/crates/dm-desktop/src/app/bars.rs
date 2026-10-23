use super::*;

impl DmApp {
    /// Until a browser has connected, how to connect one: a person who
    /// installed only the app sees why clicked downloads still go to the
    /// browser, and gets to the extension in one click.
    pub(super) fn browser_bar(&mut self, ui: &mut egui::Ui) {
        let home = dm_shell::settings::home();
        let stale = self
            .browser_hint
            .0
            .is_none_or(|at| at.elapsed() > Duration::from_secs(3));
        if stale {
            self.browser_hint = (
                Some(Instant::now()),
                !dm_shell::host::browser_seen(&home) && !home.join(BROWSER_HINT_HIDDEN).exists(),
            );
        }
        // Not where the list would be left with no room for its rows: the
        // last of them could no longer be scrolled to at 300%.
        let room = ui.available_height() / ui.text_style_height(&egui::TextStyle::Body);
        if !self.browser_hint.1 || room < 16.0 {
            return;
        }
        let mut open: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(t("Add dm to your browser:")).on_hover_text(t(
                "Downloads you click in your browser come to dm once its extension is added.",
            ));
            if ui.button("Microsoft Edge").clicked() {
                open = Some(EXTENSION_EDGE);
            }
            if ui.button("Firefox").clicked() {
                open = Some(EXTENSION_FIREFOX);
            }
            if ui.button(t("Chrome and others")).clicked() {
                open = Some(EXTENSION_CHROME);
            }
            if ui.button(t("Not now")).clicked() {
                let _ = std::fs::create_dir_all(&home);
                let _ = std::fs::write(home.join(BROWSER_HINT_HIDDEN), b"");
                self.browser_hint = (Some(Instant::now()), false);
            }
        });
        if let Some(page) = open {
            self.open_file(page);
        }
        ui.separator();
    }

    /// A download that paused by itself for want of room or of its drive:
    /// what happened, and the two ways on — another folder (its unfinished
    /// parts moved there too), or Resume once there is room again.
    pub(super) fn stalled_bar(&mut self, ui: &mut egui::Ui) {
        let Some(row) = self
            .rows
            .iter()
            .find(|r| r.state == State::Paused && !r.stalled.is_empty())
            .cloned()
        else {
            return;
        };
        let room = ui.available_height() / ui.text_style_height(&egui::TextStyle::Body);
        if room < 12.0 {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(tf(
                    "{} is paused: {}.",
                    &[&crate::bidi::visual(&name_of(&row)), &stalled_said(&row)],
                ))
                .color(ui.visuals().warn_fg_color),
            );
            if crate::dialogs::primary(ui, "Choose another folder…").clicked() {
                if let Some(dir) = self.backend.platform.choose_folder() {
                    match self.backend.relocate(row.id, dir) {
                        Ok(()) => self.refresh(),
                        Err(e) => self.said = e.into(),
                    }
                }
            }
            if ui.button(t("Resume")).clicked() {
                self.backend.resume(row.id);
                self.refresh();
            }
        });
        ui.separator();
    }

    /// dm's update, downloaded and checked against its release's digest:
    /// installed from here, dm closing so the installer can replace it. An
    /// AppImage is not installed but put in place of this one by the person,
    /// so its folder is shown instead.
    pub(super) fn update_bar(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.update_row else {
            return;
        };
        // Not in the list yet, the moment it is added: waited for.
        let Some(row) = self.rows.iter().find(|r| r.id == id) else {
            return;
        };
        match row.state {
            State::Completed if row.verified && !row.path.is_empty() => {}
            // Its own row says why it failed; nothing to offer.
            State::Failed(_) | State::Cancelled => {
                self.update_row = None;
                return;
            }
            _ => return,
        }
        let path = row.path.clone();
        let appimage = cfg!(target_os = "linux");
        ui.horizontal_wrapped(|ui| {
            ui.label(t("The dm update is downloaded and checked."));
            if appimage {
                if crate::dialogs::primary(ui, "Open folder").clicked() {
                    self.reveal_file(&path);
                    self.update_row = None;
                }
            } else if crate::dialogs::primary(ui, "Install now").clicked() {
                self.open_file(&path);
                self.update_row = None;
                self.ask_quit();
            }
            if ui.button(t("Later")).clicked() {
                self.update_row = None;
            }
        });
        ui.separator();
    }

    /// A field over the list that narrows it as one types. Shown once
    /// there is a list to narrow; Escape in it, or ×, shows every row again.
    pub(super) fn filter_bar(&mut self, ui: &mut egui::Ui) {
        if self.rows.is_empty() && self.filter.is_empty() {
            return;
        }
        // Left out where the list itself has too little room, as at 300 %
        // text in a short window, where it pushed the last row under the
        // status bar; never while it holds words, which hide rows.
        if self.filter.is_empty() && ui.available_height() < 200.0 {
            return;
        }
        ui.horizontal(|ui| {
            let name = ui.label(t("Filter"));
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text(t("Name, address or note"))
                        .desired_width(ui.available_width().min(320.0)),
                )
                .labelled_by(name.id);
            if field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.filter.clear();
            }
            if !self.filter.is_empty() {
                if ui.button(t("Clear")).clicked() {
                    self.filter.clear();
                }
                let shown = self.rows.iter().filter(|r| self.shows(r)).count();
                ui.label(
                    egui::RichText::new(crate::i18n::tn(
                        "{} shown||{} shown",
                        shown as u64,
                        &[&shown],
                    ))
                    .weak(),
                );
            }
        });
        ui.add_space(2.0);
    }

    pub(super) fn clipboard_bar(&mut self, ui: &mut egui::Ui) {
        let Some(url) = self.clipboard.offered.clone() else {
            return;
        };
        let all = self.clipboard.offered_all.clone();
        if all.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(tf("Links on your clipboard: {}", &[&all.len()]));
                // The dialog that lists them, each to tick or not.
                if ui.button(format!("{}…", t("Download"))).clicked() {
                    self.clipboard.dismissed.extend(all.iter().cloned());
                    self.clipboard.offered = None;
                    self.clipboard.offered_all.clear();
                    self.queued.push(Command::AddFromClipboard);
                }
                if ui.button(t("Dismiss")).clicked() {
                    self.clipboard.dismissed.extend(all.iter().cloned());
                    self.clipboard.offered = None;
                    self.clipboard.offered_all.clear();
                }
            });
            ui.separator();
            return;
        }
        // The buttons first, from the right, and the link in what is left,
        // cut short with the whole of it on hover: a long one pushed
        // Download and Dismiss past the window's edge.
        ui.horizontal(|ui| {
            ui.label(t("Link on your clipboard:"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t("Dismiss")).clicked() {
                    self.clipboard.dismissed.insert(url.clone());
                    self.clipboard.offered = None;
                }
                if ui.button(t("Download")).clicked() {
                    self.clipboard.dismissed.insert(url.clone());
                    self.clipboard.offered = None;
                    self.dialogs.open_file_info(vec![url.clone()]);
                }
                let shown = format::shown_url(&url);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&shown).weak()).truncate())
                        .on_hover_text(&shown);
                });
            });
        });
        ui.separator();
    }

    /// What the tray's tooltip says while downloads run: how many and how
    /// fast, as the status bar says it; `None` when nothing is downloading.
    /// It said only "downloads keep running here".
    pub fn tray_activity(&self) -> Option<String> {
        let running: Vec<&Entry> = self
            .rows
            .iter()
            .filter(|r| r.state == State::Running && !fetched_all(r))
            .collect();
        if running.is_empty() {
            return None;
        }
        let rate: f64 = running.iter().map(|r| r.progress.speed).sum();
        Some(crate::i18n::tn(
            "{} downloading at {}||{} downloading at {}",
            running.len() as u64,
            &[&running.len(), &format::rate(rate)],
        ))
    }

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        // A row with every byte is only putting its file together: said
        // to be finishing, not counted as downloading at 0 B/s.
        let (finishing, running): (Vec<&Entry>, Vec<&Entry>) = self
            .rows
            .iter()
            .filter(|r| r.state == State::Running)
            .partition(|r| fetched_all(r));
        let rate: f64 = running.iter().map(|r| r.progress.speed).sum();
        let (finishing, running) = (finishing.len(), running.len());
        let later = self.rows.iter().filter(|r| waits_for_its_queue(r)).count();
        // The limiter's own sentence is not said again beside the
        // standing "limited to" above. Nor is one left standing long
        // after: it named rows that may be gone since.
        if self
            .said_at
            .as_ref()
            .is_none_or(|(said, _)| *said != self.said)
        {
            self.said_at = Some((self.said.clone(), Instant::now()));
        }
        let fresh = self
            .said_at
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < SAID_FOR);
        // "Waiting for the schedule" only while something waits for it:
        // left standing, it described a download running since.
        let stale = self.said.is_schedule_wait()
            && !self
                .rows
                .iter()
                .any(|r| r.state == State::Paused && r.paused_by_schedule);
        let showing = fresh && !stale && !self.said.is_empty() && !self.said.is_limiter();
        ui.horizontal(|ui| {
            // Each "· …" part starts with its own space: the usual gap as
            // well read as two spaces before the dot.
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(if running == 0 && finishing == 0 {
                t("Nothing downloading").to_string()
            } else if running == 0 {
                crate::i18n::tn(
                    "{} finishing||{} finishing",
                    finishing as u64,
                    &[&finishing],
                )
            } else {
                crate::i18n::tn(
                    "{} downloading at {}||{} downloading at {}",
                    running as u64,
                    &[&running, &format::rate(rate)],
                )
            });
            // A sentence being said takes the room of the counts below,
            // whose words cut off its end, which says what to do.
            if running > 0 && finishing > 0 && !showing {
                ui.label(crate::i18n::tn(
                    "· {} finishing||· {} finishing",
                    finishing as u64,
                    &[&finishing],
                ));
            }
            // Rows added for later are counted, and said to wait for Start
            // queue, so a list of them is not taken for a stuck one.
            if later > 0 && !showing {
                ui.label(crate::i18n::tn(
                    "· {} waiting for Start queue||· {} waiting for Start queue",
                    later as u64,
                    &[&later],
                ));
            }
            if self.applied.speed_limit > 0 {
                ui.label(tf("· limited to {} KB/s", &[&self.applied.speed_limit]));
            }
            if let Some(wait) = self.backend.queue.allowance_spent() {
                ui.label(tf(
                    "· download limit of {} MB reached, held for {}",
                    &[&self.applied.limit_mb, &format::span(wait)],
                ));
            } else if let Some(at) = self
                .schedule_holds()
                // Not beside a sentence that says so already: it was said
                // twice and the second cut off.
                .filter(|_| !(showing && self.said.is_schedule_wait()))
            {
                // Said while the schedule is what downloads wait for, so a
                // queue that does nothing is not taken for a broken one.
                ui.label(match at {
                    Some(at) => tf("· waiting for the schedule (opens at {})", &[&at]),
                    None => t("· waiting for the schedule").to_string(),
                });
            }
            if showing {
                // Its first line, cut short to the room there is with the
                // whole of it on hover: an opener's error output ran off the
                // window's edge.
                let whole = self.said.shown();
                let short = whole.lines().next().unwrap_or_default().to_string();
                let room = (ui.available_width() - 8.0).max(80.0);
                ui.scope(|ui| {
                    ui.set_max_width(room);
                    let drawn = ui.add(
                        egui::Label::new(format!("· {}", crate::bidi::visual(&short)))
                            .truncate()
                            // The whole is on hover below; egui's own cut-off
                            // tooltip made two.
                            .show_tooltip_when_elided(false),
                    );
                    crate::bidi::said_as(&drawn, &format!("· {short}"));
                    // Read out when it changes, without moving the screen
                    // reader's place: what dm says here was seen only.
                    ui.ctx().accesskit_node_builder(drawn.id, |node| {
                        node.set_live(egui::accesskit::Live::Polite);
                    });
                    drawn.on_hover_text(whole);
                });
                ui.ctx().request_repaint_after(SAID_FOR);
            }
            // The folder in what room is left, cut short with an ellipsis
            // (its whole path on hover) rather than drawn over the words.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Not while a sentence is said, whose end it cut off in the
                // smallest window.
                if ui.available_width() > 40.0 && !showing {
                    let folder = self.applied.download_dir.display().to_string();
                    let drawn = ui.add(
                        egui::Label::new(
                            RichText::new(&*crate::bidi::visual(&folder)).small().weak(),
                        )
                        .truncate(),
                    );
                    crate::bidi::said_as(&drawn, &folder);
                    drawn.on_hover_text(folder);
                }
            });
        });
    }

    pub(super) fn problem_list(&mut self, ui: &mut egui::Ui) {
        let mut dismissed = None;
        for (i, problem) in self.problems.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(crate::theme::Tokens::get(ui.ctx()).warn, as_shown(problem));
                if ui.button(t("Dismiss")).clicked() {
                    dismissed = Some(i);
                }
            });
        }
        if let Some(i) = dismissed {
            self.problems.remove(i);
        }
    }
}
