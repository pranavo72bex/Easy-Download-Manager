use super::*;
pub(super) fn progress(app: &mut DmApp, ctx: &egui::Context) {
    let mut ids: Vec<Id> = app.dialogs.progress.iter().copied().collect();
    ids.sort_unstable();
    for (nth, id) in ids.into_iter().enumerate() {
        let Some(mut row) = app.row(id).cloned() else {
            app.dialogs.progress.remove(&id);
            continue;
        };
        // The size a dialog learned, until the row learns its own, as the
        // list shows it ("File size unknown" beside it).
        if row.progress.total.is_none() {
            row.progress.total = app.sizes_told.get(&id).copied();
        }
        let connections = app.backend.connections(id);
        // While its parts are joined the plan they are read from is gone:
        // the last answer stands, and nothing reads as unknown or unstarted
        // with every byte in.
        let joining = row.state == State::Running && crate::app::fetched_all(&row);
        let resumable = match app.backend.resumable(id) {
            Some(answer) => {
                app.dialogs.resumable_seen.insert(id, answer);
                Some(answer)
            }
            None if joining => app.dialogs.resumable_seen.get(&id).copied(),
            None => None,
        };
        let mut open = true;
        let (mut pause, mut resume, mut cancel) = (false, false, false);
        // No wider or taller than the screen leaves room for: a long
        // status (a server's refusal, with where to sign in) wraps, and a
        // window too tall scrolls rather than running off the bottom.
        let screen = ctx.content_rect();
        let room = (screen.width() - 16.0 - sides(ctx)).clamp(300.0, 560.0);
        // Below the menu bar and the toolbar, which it opened over and hid (kept on screen, a tall window was
        // pushed back up over Add URL).
        let mut below_menu = screen;
        below_menu.min.y = app
            .menu_bottom
            .max(app.bars_bottom)
            .max(screen.top())
            .min(screen.bottom() - 120.0);
        let step = 28.0 * (nth % 8) as f32;
        // Behind any dialog that keeps the clicks too: behind Options its
        // Stop download paused a download.
        let asking = app.dialogs.asking() || app.dialogs.modal();
        // The name it will get, as the list says it: two catches of one
        // link were both "big.zip" until they had begun.
        let mut categories = app.applied.categories();
        let named = app.name_it_will_get(&row, &mut categories);
        Dialog::new(tf(
            "Download progress — {}",
            &[&crate::bidi::visual(&named)],
        ))
        .id(egui::Id::unique(("progress", id)))
        .collapsible(false)
        .resizable(false)
        // Below the menu bar, which it opened over and hid.
        .constrain_to(below_menu)
        // Each a step down and across from the one before, as windows
        // cascade: three started together opened exactly over each other.
        // Below the toolbar too: over it, the window covered Add URL and
        // took the click meant for it.
        .default_pos(below_menu.left_top() + egui::Vec2::new(24.0 + step, 8.0 + step))
        // One width: free to grow, it widened a few points as its numbers
        // did. A long status wraps inside it.
        .default_width(room.min(480.0))
        .min_width(room.min(480.0))
        .max_width(room.min(480.0))
        .open(&mut open)
        .show(ctx, |ui| {
            // Under a question, it takes no clicks: Stop download stopped
            // a download behind the Exit question.
            if asking {
                ui.disable();
            }
            // One height whatever it holds: grown and shrunk as
            // connections came and went, the window jumped about
            // the screen, kept on it by being moved.
            // As tall as a tall window allows, so sixteen connections are
            // seen without scrolling in one (held at 400 points
            // in a 900-point window).
            // No taller than the room under the toolbar leaves, less the
            // title, the buttons and its step down, so a window does not
            // have to be pushed up over the toolbar to fit.
            let tall = (below_menu.height() - 130.0 - step)
                .min((screen.height() * 0.7).max(400.0))
                .max(120.0);
            // As tall as what it holds, up to that: held at the room on the
            // screen whatever it held, it filled 84% of the window's height
            // with three connections and a band of nothing.
            // Its height changes only when Connections is
            // opened or shut, which a person does.
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(tall)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(format::shown_url(&row.url)).small().weak())
                            .truncate(),
                    )
                    .on_hover_text(&row.url);
                    let p = &row.progress;
                    // Rows as tall as their text, not as a control: six
                    // lines of words took a third of its height.
                    ui.spacing_mut().interact_size.y = ui.text_style_height(&egui::TextStyle::Body);
                    egui::Grid::new(("progress-grid", id))
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .max_col_width(room - 110.0)
                        .show(ui, |ui| {
                            ui.label(t("Status"));
                            let status = ui.add(egui::Label::new(status_of(&row)).wrap());
                            if let Some(raw) = crate::app::failure_as_said(&row) {
                                status.on_hover_text(raw);
                            }
                            ui.end_row();
                            ui.label(t("File size"));
                            ui.label(
                                p.total
                                    .map(format::bytes)
                                    .unwrap_or_else(|| t("unknown").into()),
                            );
                            ui.end_row();
                            ui.label(t("Downloaded"));
                            // Digits of one width: these change every second.
                            // The percentage is Status's, above: said
                            // twice it read as two numbers.
                            let fetched = format::bytes(p.downloaded);
                            ui.label(crate::table::digits(ui, fetched));
                            ui.end_row();
                            ui.label(t("Speed"));
                            let rate = if row.state == State::Running && !joining {
                                format::rate(p.speed)
                            } else {
                                String::new()
                            };
                            ui.label(crate::table::digits(ui, rate));
                            ui.end_row();
                            // Only while it runs: a stopped download has
                            // no time left to tell.
                            ui.label(t("Time left"));
                            // The span alone: under "Time left" it read
                            // "Time left: 9s left".
                            let left = if row.state == State::Running {
                                p.eta.map(format::span).unwrap_or_default()
                            } else {
                                String::new()
                            };
                            ui.label(crate::table::digits(ui, left));
                            ui.end_row();
                            ui.label(t("Can resume"));
                            // What the server said when the download was planned, not
                            // a guess from the size: a known size says nothing about
                            // whether a server serves ranges.
                            ui.label(match resumable {
                                Some(true) => t("Yes"),
                                Some(false) => t("No — a stop starts it again from the beginning"),
                                // A torrent goes on from the pieces it has;
                                // there is no server to ask,
                                // whether or not it failed.
                                // Not one that failed before it had any:
                                // a .torrent that could not be read has no
                                // pieces to go on from.
                                None if dm_core::is_torrent(&row.url)
                                    && matches!(row.state, State::Failed(_))
                                    && p.downloaded == 0 =>
                                {
                                    t("No — nothing of it was fetched")
                                }
                                None if dm_core::is_torrent(&row.url) => {
                                    t("Yes, from the pieces it already has")
                                }
                                // Failed before the server said: a 401
                                // is an answer, but not to this.
                                // The server answered, with a refusal: a
                                // 404 read "Not yet known" under its own
                                // reason.
                                None if matches!(&row.state, State::Failed(why) if why.contains("the server answered")) => {
                                    t("No — the server refused it")
                                }
                                None if matches!(row.state, State::Failed(_)) => {
                                    t("Not yet known: it failed before the server said")
                                }
                                None => t("Unknown until the server answers"),
                            });
                            ui.end_row();
                        });
                    let fraction = p.fraction().unwrap_or(0.0) as f32;
                    // No words of its own: Status above says the percentage (it was said twice). Laid out as the file
                    // is, a piece for each connection, where the pieces are
                    // known; a plain bar where they are not.
                    match p.total.filter(|_| connections.len() > 1) {
                        Some(total) => {
                            let pieces: Vec<(u64, u64, u64)> = connections
                                .iter()
                                .map(|c| (c.start, (c.end - c.start).saturating_add(1), c.done))
                                .collect();
                            pieces_bar(ui, total, &pieces, 420.0);
                        }
                        None => bar(ui, fraction, 420.0, ""),
                    }
                    // This download's own limit, beside the one for all.
                    if !row.state.settled() {
                        let typed = app.dialogs.own_limits.entry(id).or_insert(if row.speed_limit > 0 {
                            row.speed_limit
                        } else {
                            500
                        });
                        let mut on = row.speed_limit > 0;
                        ui.horizontal(|ui| {
                            let ticked = check(ui, &mut on, t("Limit this download to")).changed();
                            let typed_now = ui
                                .add_enabled(on, number_box(typed).range(1..=10_000_000u64))
                                .changed();
                            ui.label(t("KB/s"));
                            if ticked || (on && typed_now) {
                                app.backend.set_own_limit(id, if on { *typed } else { 0 });
                            }
                        });
                    }
                    // A torrent's pieces come from its peers, not from
                    // connections dm plans: the section said "Nothing fetched
                    // yet." while megabytes arrived.
                    let torrent = dm_core::is_torrent(&row.url);
                    if !torrent {
                        // Shut until asked for: what most want is the bar,
                        // the bytes, the speed and the time left above.
                        egui::CollapsingHeader::new(t("Connections"))
                            .icon(header_chevron)
                            .default_open(false)
                            .id_salt(("connections", id))
                            .show(ui, |ui| {
                                if connections.is_empty() && joining {
                                    ui.label(
                                        RichText::new(t("Every piece is in; putting them together."))
                                            .weak(),
                                    );
                                } else if connections.is_empty() {
                                    ui.label(RichText::new(t("Nothing fetched yet.")).weak());
                                }
                                // The file is cut into more pieces than connections,
                                // and each connection takes the next piece when it is
                                // done: the pieces under way are the connections at
                                // work, and the rest is a count.
                                let span_of =
                                    |c: &dm_core::Connection| (c.end - c.start).saturating_add(1);
                                // Each held to its range and adding up to the
                                // Downloaded above.
                                let held = format::pieces_shown_open(
                                    &connections
                                        .iter()
                                        .map(|c| (span_of(c), c.done))
                                        .collect::<Vec<_>>(),
                                    &connections.iter().map(|c| c.started).collect::<Vec<_>>(),
                                    p.downloaded,
                                );
                                let finished = connections
                                    .iter()
                                    .zip(&held)
                                    .filter(|(c, &done)| done >= span_of(c))
                                    .count();
                                if !connections.is_empty() {
                                    ui.label(
                                        RichText::new(crate::i18n::tn(
                                            "{} of {} piece done||{} of {} pieces done",
                                            connections.len() as u64,
                                            &[&finished, &connections.len()],
                                        ))
                                        .weak(),
                                    );
                                }
                                // Every piece of the plan: those under way with
                                // their bars, the rest said to be waiting or
                                // done, so the list does not grow and shrink as
                                // connections start.
                                // The longest words any piece's bar could have,
                                // so every bar is one length.
                                let widest = connections
                                    .iter()
                                    .map(|c| {
                                        let most = format::bytes(span_of(c));
                                        tf("{} of {}", &[&most, &most])
                                    })
                                    .max_by_key(|w| w.chars().count())
                                    .unwrap_or_default();
                                for (n, (c, &done)) in connections.iter().zip(&held).enumerate() {
                                    let span = span_of(c);
                                    ui.horizontal(|ui| {
                                        // Digits of one width, so the bars start
                                        // in one line (1's was 2 px
                                        // left of the others).
                                        ui.label(crate::table::digits(ui, format!("{:>3}", n + 1)));
                                        if done >= span {
                                            ui.label(
                                                RichText::new(tf(
                                                    "done, {}",
                                                    &[&format::bytes(span)],
                                                ))
                                                .weak(),
                                            );
                                        // Waiting only where no request for it is
                                        // open yet.
                                        } else if done == 0
                                            && !c.started
                                            && row.state == State::Running
                                        {
                                            ui.label(
                                                RichText::new(tf(
                                                    "waiting, {}",
                                                    &[&format::bytes(span)],
                                                ))
                                                .weak(),
                                            );
                                        } else if row.progress.total.is_none() || span >= 1 << 60 {
                                            // Of a length nobody said: what has
                                            // come, not a bar against the range's
                                            // open end, which read "of 16777216.00
                                            // TB".
                                            ui.label(crate::table::digits(
                                                ui,
                                                tf("{} so far", &[&format::bytes(done)]),
                                            ));
                                        } else {
                                            bar_beside(
                                                ui,
                                                done as f32 / span.max(1) as f32,
                                                260.0,
                                                &tf(
                                                    "{} of {}",
                                                    &[&format::bytes(done), &format::bytes(span)],
                                                ),
                                                &widest,
                                            );
                                        }
                                    });
                                }
                            });
                    }
                });
            // The buttons below what scrolls, always in view; a plain row,
            // which keeps the progress window compact.
            ui.horizontal(|ui| {
                if matches!(row.state, State::Running | State::Queued) {
                    pause = ui.button(t("Stop download")).clicked();
                }
                // A failed download is tried again, and the button says so.
                if matches!(row.state, State::Failed(_)) {
                    resume = ui.button(t("Retry")).clicked();
                }
                // Held by the schedule, it is not resumed but started
                // outside it, as the row's own button says.
                if row.state == State::Paused {
                    let word = if row.paused_by_schedule {
                        "Start now"
                    } else {
                        "Resume"
                    };
                    resume = ui.button(t(word)).clicked();
                }
                if !row.state.settled() {
                    cancel = ui.button(t("Cancel download")).clicked();
                }
            });
        });
        if pause {
            app.backend.pause(id);
        }
        if resume {
            match row.state {
                State::Failed(_) => app.backend.retry(id),
                // Held by the schedule: Resume would only put it back to be
                // held again. Asked whether to start it outside the schedule.
                State::Paused if row.paused_by_schedule => {
                    app.dialogs.confirm_outside = Some(vec![id]);
                }
                _ => app.backend.resume(id),
            }
        }
        if cancel {
            app.dialogs.confirm_cancel = Some(id);
        }
        // Its × too takes no click while it is behind.
        if !open && !asking {
            app.dialogs.progress.remove(&id);
            app.dialogs.progress_closed.insert(id);
        }
        if pause || resume || cancel {
            app.refresh();
        }
    }
}
