use super::*;
/// Asked before Remove, however it was asked for — the Delete key, the
/// toolbar, the menus: the Delete key took finished rows off at once while
/// the toolbar asked. Unfinished rows lose what was fetched; finished ones
/// only leave the list.
pub(super) fn confirm_remove(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ids) = app.dialogs.confirm_remove.clone() else {
        return;
    };
    // A cancelled row has nothing left to lose: its parts went with the
    // cancel (it was warned about as unfinished).
    let unfinished = ids
        .iter()
        .filter_map(|id| app.row(*id))
        .any(|r| !matches!(r.state, State::Completed | State::Cancelled));
    // Whether there is anything to throw away: a row added for later and
    // never started has fetched nothing, which the warning said it would
    // lose.
    let fetched = ids.iter().filter_map(|id| app.row(*id)).any(|r| {
        !matches!(r.state, State::Completed | State::Cancelled) && r.progress.downloaded > 0
    });
    // The finished files still on disk that could go with their rows.
    let files: Vec<String> = ids
        .iter()
        .filter_map(|id| app.row(*id))
        .filter(|r| r.state == State::Completed && std::path::Path::new(&r.path).is_file())
        .map(|r| r.path.clone())
        .collect();
    let (mut yes, mut no) = (false, false);
    // Not "unfinished" of a set with finished files in it.
    let english = if unfinished && ids.len() == 1 {
        "Remove an unfinished download"
    } else if unfinished && files.is_empty() {
        "Remove unfinished downloads"
    } else {
        "Remove from the list"
    };
    app.dialogs.remove_title = english;
    let title = match english {
        "Remove an unfinished download" => t("Remove an unfinished download"),
        "Remove unfinished downloads" => t("Remove unfinished downloads"),
        _ => t("Remove from the list"),
    };
    Dialog::new(title)
        .id(window_id("Delete unfinished downloads"))
        .collapsible(false)
        .resizable(false)
        .min_width(440.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            // Whole sentences, one for each number, since languages do not
            // build them from the same pieces.
            // Finished files beside unfinished rows: said apart, with what
            // happens to the files; "have not fetched anything yet" was said
            // of two finished files the box would delete.
            let mixed = unfinished && !files.is_empty();
            ui.label(match (unfinished, ids.len()) {
                (true, _) if mixed => t(
                    "Some of these are finished and some are not. Unfinished ones lose what they fetched.",
                ),
                (true, 1) if !fetched => {
                    t("This download has not fetched anything yet. Delete it from the list?")
                }
                (true, _) if !fetched => {
                    t("These downloads have not fetched anything yet. Delete them from the list?")
                }
                (true, 1) => t(
                    "This download is not finished. Removing it throws away what has been fetched.",
                ),
                (true, _) => t(
                    "Some of these are not finished. Removing them throws away what has been fetched.",
                ),
                // What the box below says happens to the files, said here
                // too: the sentence promised the file stayed while the box
                // was deleting it.
                (false, 1) if app.dialogs.remove_files => {
                    t("Take this download off the list and delete its file from the disk?")
                }
                (false, _) if app.dialogs.remove_files => {
                    t("Take these downloads off the list and delete their files from the disk?")
                }
                // No file on the disk (cancelled, or since deleted): none to
                // stay anywhere.
                (false, 1) if files.is_empty() => t("Take this download off the list?"),
                (false, _) if files.is_empty() => t("Take these downloads off the list?"),
                (false, 1) => t("Take this download off the list? Its file stays where it is."),
                (false, _) => {
                    t("Take these downloads off the list? Their files stay where they are.")
                }
            });
            if mixed {
                // In its number: "the finished ones'" of one.
                ui.label(match (app.dialogs.remove_files, files.len()) {
                    (true, 1) => t("The finished one's file is deleted from the disk too."),
                    (true, _) => t("The finished ones' files are deleted from the disk too."),
                    (false, 1) => t("The finished one's file stays where it is."),
                    (false, _) => t("The finished ones' files stay where they are."),
                });
            }
            // Which one, by name: "this download" was all it said.
            if let [only] = ids.as_slice() {
                if let Some(row) = app.row(*only) {
                    let named = app.name_it_will_get(row, &mut app.applied.categories());
                    ui.strong(&*crate::bidi::visual(&named));
                }
            } else {
                // And how many, where it is more than one: 507 went with
                // no number said.
                ui.strong(tf("Downloads: {}", &[&ids.len()]));
            }
            // The buttons say what the sentence asked, in its number, and
            // Enter is the one that loses nothing.
            let (go, keep) = match (unfinished, ids.len()) {
                (true, 1) => (t("Yes, remove"), t("Keep it")),
                (true, _) => (t("Yes, remove"), t("Keep them")),
                (false, 1) => (t("Remove it"), t("Keep it")),
                (false, _) => (t("Remove them"), t("Keep them")),
            };
            // A finished file can go with its row, if asked for; never by
            // default, and never by Enter.
            let files = files.len();
            let go = if files > 0 {
                check(ui,
                    &mut app.dialogs.remove_files,
                    if files == 1 {
                        t("Also delete the file from the disk")
                    } else {
                        t("Also delete their files from the disk")
                    },
                );
                if app.dialogs.remove_files {
                    t("Yes, delete")
                } else {
                    go
                }
            } else {
                go
            };
            // The default first, on the left, as every dialog has it (here alone it was on the right).
            footer(ui, |ui| {
                no = primary_worded(ui, keep).clicked() || enter_free(ui);
                yes = ui.button(go).clicked();
            });
        });
    if yes {
        app.dialogs.confirm_remove = None;
        let delete = std::mem::take(&mut app.dialogs.remove_files);
        if delete {
            let failed: Vec<String> = files
                .iter()
                .filter_map(|path| {
                    std::fs::remove_file(path)
                        .err()
                        .map(|e| format!("{path}: {e}"))
                })
                .collect();
            if !failed.is_empty() {
                app.said = crate::i18n::Said::new("Could not delete {}", &[&failed.join("; ")]);
            }
        }
        // The selection goes to the row after those removed, or the one
        // before where they were the last: it was lost, and the keys had
        // nowhere to go on from.
        let shown: Vec<Id> = app.visible().iter().map(|r| r.id).collect();
        let next = shown
            .iter()
            .rposition(|id| ids.contains(id))
            .and_then(|last| {
                shown
                    .get(last + 1..)
                    .and_then(|after| after.iter().find(|id| !ids.contains(id)))
                    .or_else(|| {
                        shown
                            .get(..last)
                            .and_then(|before| before.iter().rev().find(|id| !ids.contains(id)))
                    })
            })
            .copied();
        app.remove_now(&ids);
        if let Some(id) = next {
            app.selected.insert(id);
            app.cursor = Some(id);
            app.anchor = Some(id);
        }
    } else if no {
        app.dialogs.confirm_remove = None;
        app.dialogs.remove_files = false;
    }
}

/// Asked before the progress window's Cancel download throws away what
/// was fetched. Enter keeps the download.
pub(super) fn confirm_cancel(app: &mut DmApp, ctx: &egui::Context) {
    let Some(id) = app.dialogs.confirm_cancel else {
        return;
    };
    // Gone, or finished (or stopped) while it was asked: nothing to cancel,
    // and the question about a finished file stood beside Download
    // complete.
    let fetched = app.row(id).is_some_and(|r| r.progress.downloaded > 0);
    let Some(name) = app
        .row(id)
        .filter(|r| matches!(r.state, State::Running | State::Queued | State::Paused))
        .map(|r| app.name_it_will_get(r, &mut app.applied.categories()))
    else {
        app.dialogs.confirm_cancel = None;
        return;
    };
    let (mut cancel, mut keep) = (false, false);
    window(ctx, "Cancel this download?").show(ctx, |ui| {
        ui.strong(&*crate::bidi::visual(&name));
        if fetched {
            ui.label(t("Cancelling it throws away what has been fetched."));
        }
        // Apart from the words, as in the other questions: the buttons sat
        // against the file's name.
        ui.add_space(8.0);
        // The default first, as in every dialog.
        footer(ui, |ui| {
            keep = primary_worded(ui, t("Keep downloading")).clicked() || enter_free(ui);
            cancel = ui.button(t("Yes, cancel")).clicked();
        });
    });
    if cancel {
        app.dialogs.confirm_cancel = None;
        app.backend.cancel(id);
        app.dialogs.progress.remove(&id);
        app.dialogs.progress_closed.insert(id);
        app.refresh();
    } else if keep {
        app.dialogs.confirm_cancel = None;
    }
}

/// Asked when a download the schedule or the download limit holds is
/// started (Resume, Start now): start it now anyway, for this one only, or
/// let it wait.
pub(super) fn confirm_outside(app: &mut DmApp, ctx: &egui::Context) {
    let Some(ids) = app.dialogs.confirm_outside.clone() else {
        return;
    };
    let (mut start, mut wait) = (false, false);
    // The name the list shows: "big.zip" was asked about while the row
    // read "big (1).zip".
    let mut categories = app.applied.categories();
    let names: Vec<String> = ids
        .iter()
        .filter_map(|id| app.row(*id))
        .map(|r| app.name_it_will_get(r, &mut categories))
        .collect();
    let why = app.held_because().unwrap_or_default();
    window(ctx, "Start outside the schedule?").show(ctx, |ui| {
        ui.label(if names.len() == 1 {
            tf(
                "{} waits for the schedule or the download limit.",
                &[&names.join(", ")],
            )
        } else {
            tf(
                "{} downloads wait for the schedule or the download limit.",
                &[&names.len()],
            )
        });
        if !why.is_empty() {
            ui.label(RichText::new(why).small().weak());
        }
        ui.label(t(
            "Start it now anyway? Only what you picked starts; the rest keep waiting.",
        ));
        footer(ui, |ui| {
            start = primary(ui, "Start now").clicked() || enter_free(ui);
            wait = ui.button(t("Keep waiting")).clicked();
        });
    });
    if start {
        app.dialogs.confirm_outside = None;
        for id in &ids {
            if app.backend.start_outside_schedule(*id) && app.applied.show_progress {
                app.dialogs.progress.insert(*id);
            }
        }
        app.refresh();
    } else if wait {
        app.dialogs.confirm_outside = None;
    }
}

/// Asked when Exit is chosen while downloads run: quitting stops them, and
/// they go on from where they were the next time dm opens.
pub(super) fn confirm_exit(app: &mut DmApp, ctx: &egui::Context) {
    if !app.dialogs.confirm_exit {
        return;
    }
    let (mut exit, mut stay) = (false, false);
    // Running and waiting counted apart: a queued row is not running, and
    // counting it as one told of downloads that were not.
    let (busy, waiting) = app.running_and_waiting();
    let mut said = Vec::new();
    // Finished meanwhile: the question changes, and dm still waits for the
    // person's answer. It used to quit by itself in the first frame that
    // counted nothing running, so a person reading the question, or
    // reaching for "Keep downloading", saw dm vanish. Only Exit
    // quits.
    let finished = busy + waiting == 0;
    if finished {
        said.push(t("Nothing is downloading any more. Exit dm?").to_string());
    }
    if busy > 0 {
        said.push(crate::i18n::tn(
            "{} download is still running.||{} downloads are still running.",
            busy as u64,
            &[&busy],
        ));
    }
    if waiting > 0 && busy > 0 {
        said.push(crate::i18n::tn(
            "{} more is waiting to start.||{} more are waiting to start.",
            waiting as u64,
            &[&waiting],
        ));
    } else if waiting > 0 {
        said.push(crate::i18n::tn(
            "{} download is waiting to start.||{} downloads are waiting to start.",
            waiting as u64,
            &[&waiting],
        ));
    }
    if !finished {
        // As plural as the count it speaks of.
        said.push(crate::i18n::tn(
            "Exiting stops it; it goes on from where it was the next time dm opens.||Exiting stops them; each goes on from where it was the next time dm opens.",
            (busy + waiting) as u64,
            &[],
        ));
    }
    // Narrower on a screen that has not the room, as the other dialogs.
    let width = EXIT_WIDTH.min(ctx.content_rect().width() - 48.0).max(220.0);
    window(ctx, "Exit dm?").show(ctx, |ui| {
        // One width and room for three lines whatever the words say: a
        // download finishing while the question is open changes them, and
        // a window that shrank around shorter words moved its buttons —
        // Exit slid under a pointer on its way to "Keep downloading".
        ui.set_width(width);
        let lines =
            ui.text_style_height(&egui::TextStyle::Body) * 3.0 + ui.spacing().item_spacing.y * 2.0;
        ui.allocate_ui_with_layout(
            egui::vec2(width, lines),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_height(lines);
                ui.add(egui::Label::new(said.join(" ")).wrap());
            },
        );
        footer(ui, |ui| {
            // With downloads running, the highlighted answer is the one that
            // loses nothing, as Remove's is; Exit only by its own click.
            // With nothing running, Exit is what was asked for.
            // Each button keeps its place whatever the question says;
            // only which one is highlighted changes.
            // The second as wide as its longer words, so that the row,
            // set at the right, keeps Exit where it was either way.
            let font = egui::TextStyle::Button.resolve(ui.style());
            let words = |w: &str| {
                ui.painter()
                    .layout_no_wrap(w.to_string(), font.clone(), Color32::WHITE)
                    .size()
                    .x
            };
            let second = words(t("Stay")).max(words(t("Keep downloading")))
                + ui.spacing().button_padding.x * 2.0;
            if finished {
                exit = primary(ui, "Exit").clicked();
                stay = ui
                    .add(egui::Button::new(t("Stay")).min_size(egui::vec2(second, 0.0)))
                    .clicked();
            } else {
                exit = ui.button(t("Exit")).clicked();
                stay = primary_sized(ui, t("Keep downloading"), second).clicked();
            }
        });
    });
    if exit {
        app.dialogs.confirm_exit = false;
        app.quit_asked = true;
    } else if stay {
        app.dialogs.confirm_exit = false;
    }
}

/// The Exit question's width, held whatever its words say, where the
/// screen has room for it.
pub(super) const EXIT_WIDTH: f32 = 440.0;

/// Asked after two handed-over downloads from one site were cancelled in a
/// row: whether to leave that site's downloads to the browser from now on,
/// which puts it on Options › Browser's list of sites never taken.
pub(super) fn skip_offer(app: &mut DmApp, ctx: &egui::Context) {
    let Some(site) = app.dialogs.skip_offer.clone() else {
        return;
    };
    let (mut leave, mut keep) = (false, false);
    window(ctx, "Leave this site to the browser?").show(ctx, |ui| {
        ui.label(tf(
            "You cancelled two downloads handed over from {}. Leave its downloads to the browser from now on?",
            &[&site],
        ));
        footer(ui, |ui| {
            leave = primary(ui, "Leave them to the browser").clicked();
            keep = ui.button(t("Keep handing them over")).clicked();
        });
    });
    if leave {
        app.dialogs.skip_offer = None;
        let mut settings = app.applied.clone();
        if !settings.catch.skip_hosts.iter().any(|h| h == &site) {
            settings.catch.skip_hosts.push(site);
        }
        if let Err(e) = app.apply_settings(settings) {
            app.problems.push(e);
        }
    } else if keep {
        app.dialogs.skip_offer = None;
    }
}

/// The countdown before the computer is turned off: time to say no.
pub(super) fn countdown(app: &mut DmApp, ctx: &egui::Context) {
    let Some((action, at)) = app.countdown else {
        return;
    };
    let left = at
        .saturating_duration_since(std::time::Instant::now())
        .as_secs();
    // Escape cancels it, taken before any window behind it sees the key:
    // it closed Download complete instead, and the countdown ran out.
    // It is drawn first of the dialogs for that.
    let mut cancel = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let mut now = false;
    // The keyboard on Cancel as it appears, so Enter or Space stops it.
    let shown_id = egui::Id::unique("countdown-shown");
    let first = ctx.data(|d| d.get_temp::<std::time::Instant>(shown_id)) != Some(at);
    window(ctx, "Downloads are done").show(ctx, |ui| {
        ui.label(tf("dm will {} in {} seconds.", &[&t(action.verb()), &left]));
        footer(ui, |ui| {
            now = ui.button(t("Now")).clicked();
            let stop = ui.button(t("Cancel"));
            if first {
                stop.request_focus();
            }
            cancel |= stop.clicked();
        });
    });
    ctx.data_mut(|d| d.insert_temp(shown_id, at));
    if cancel {
        app.countdown = None;
    } else if now {
        app.countdown = Some((action, std::time::Instant::now()));
        app.countdown_tick();
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}
