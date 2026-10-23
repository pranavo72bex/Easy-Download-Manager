use crate::support::*;

#[test]
fn the_category_tree_has_right_click_menus() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(2);

    // A queue: Start queue runs the synchronization check on it.
    harness
        .get_by_label_contains("Recheck queue")
        .click_secondary();
    harness.run_steps(2);
    // The toolbar has a Start queue too; the menu's is the later one.
    harness
        .query_all_by_label("Start queue")
        .last()
        .unwrap()
        .click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().place,
        dm_desktop::app::Place::Queue(dm_core::RECHECK_QUEUE.into()),
        "a right-click selects the place"
    );
    assert!(
        harness.state().said.english().contains("Checked"),
        "{}",
        harness.state().said.english()
    );

    // Done: Clear finished, from the menu (the toolbar has one too).
    harness.get_by_label_contains("Done (").click_secondary();
    harness.run_steps(2);
    harness
        .query_all_by_label("Clear finished")
        .last()
        .expect("the menu's Clear finished")
        .click();
    harness.run_steps(3);
    harness.state_mut().refresh();
    assert!(harness.state().rows().is_empty());

    // A kind: categories are edited in Options › Save to.
    harness.get_by_label_contains("Videos (").click_secondary();
    harness.run_steps(2);
    click(&mut harness, "Add or edit categories…");
    let tab = harness.state().dialogs.options.as_ref().map(|o| o.tab);
    assert_eq!(tab, Some(dm_desktop::dialogs::Tab::SaveTo));
}

/// Typing over the list narrows it to the rows whose name, address or
/// note hold every word, whatever their case; Clear shows them all again.
#[test]
fn the_list_narrows_as_a_filter_is_typed() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    type_into(&mut harness, "Address with *", &world.url("part*.bin"));
    type_into(&mut harness, "From", "1");
    type_into(&mut harness, "To", "12");
    click(&mut harness, "Add for later");
    harness.run_steps(3);
    assert_eq!(harness.state().visible().len(), 12);
    type_role(&mut harness, "Filter", "PART1");
    let shown: Vec<String> = harness
        .state()
        .visible()
        .iter()
        .map(|r| r.url.rsplit('/').next().unwrap_or_default().to_string())
        .collect();
    let mut shown = shown;
    shown.sort();
    assert_eq!(
        shown,
        ["part1.bin", "part10.bin", "part11.bin", "part12.bin"]
    );
    assert!(harness.query_by_label("4 shown").is_some());
    type_role(&mut harness, "Filter", " nothing-like-it");
    assert!(harness.state().visible().is_empty());
    assert!(harness
        .query_by_label("No download here matches the filter.")
        .is_some());
    click(&mut harness, "Clear");
    assert_eq!(harness.state().visible().len(), 12);
}

#[test]
fn escape_closes_the_menu_or_the_dialog_in_front_and_nothing_behind_it() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Options");

    // The menu bar takes no clicks behind it (its menus
    // opened and their items did nothing).
    harness.get_by_label("View").click();
    harness.run_steps(3);
    assert!(
        !harness.ctx.any_popup_open(),
        "a menu opened behind Options"
    );
    assert!(harness.state().dialogs.options.is_some());

    // A combo box dropped down in the dialog, the same.
    click(&mut harness, "Proxy");
    click(&mut harness, "Connect through a proxy");
    harness
        .get_by_role_and_label(Role::ComboBox, "Proxy type")
        .click();
    harness.run_steps(3);
    assert!(harness.ctx.any_popup_open(), "the combo box did not open");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(!harness.ctx.any_popup_open());
    assert!(
        harness.state().dialogs.options.is_some(),
        "the Escape that put the combo box away closed Options"
    );
    // Typing in a field: the same Escape leaves the field and closes.
    type_into(&mut harness, "Host", "proxy.example");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.options.is_none(),
        "Escape did not close Options"
    );
    assert!(
        !harness.state().backend().settings().proxy.enabled,
        "Escape kept what was typed"
    );

    // A dialog over a dialog: Find over Download details. Escape closes
    // Find; Download details, and what was typed into it, stay.
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &world.url("file.zip"));
    click(&mut harness, "OK");
    type_role(&mut harness, "Note", "keep me");
    harness.state_mut().run(Command::Find);
    harness.run_steps(2);
    assert!(harness.state().dialogs.find_open);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(!harness.state().dialogs.find_open, "Find stayed open");
    let details = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .expect("Download details was closed under Find");
    assert_eq!(details.description, "keep me");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().dialogs.file_info.is_none());
}

/// A dialog opened from the keyboard gives the keyboard back to what
/// opened it when it closes.
#[test]
fn closing_a_dialog_gives_the_keyboard_back_to_its_opener() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    harness
        .get_by_role_and_label(Role::Button, "Add URL")
        .focus();
    harness.run_steps(2);
    harness.key_press(egui::Key::Space);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.add_url.is_some(),
        "Space did not open Add URL"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(4);
    assert!(harness.state().dialogs.add_url.is_none());
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Add URL")
            .accesskit_node()
            .is_focused(),
        "the keyboard was not given back to Add URL"
    );
}

/// F10 opens the menu bar's first menu, and Shift+F10 the menu of the row
/// with the keyboard's place, as on any desktop.
#[test]
fn menus_open_from_the_keyboard() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    // Menu items carry their keys after their names.
    assert!(harness
        .query_by_label_contains("Add numbered series")
        .is_none());
    harness.key_press(egui::Key::F10);
    harness.run_steps(4);
    assert!(
        harness
            .query_by_label_contains("Add numbered series")
            .is_some(),
        "F10 opened no menu"
    );
    // Right goes on to the next menu, Left back, round the bar.
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(4);
    assert!(
        harness.query_by_label_contains("Move to queue").is_some(),
        "Right did not open File"
    );
    assert!(harness
        .query_by_label_contains("Add numbered series")
        .is_none());
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(4);
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(4);
    assert!(
        harness
            .query_by_label_contains("Check for updates")
            .is_some(),
        "Left from the first did not reach Help"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    select(&mut harness, "file.zip");
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    harness.run_steps(4);
    let props = harness
        .query_by_label_contains("Properties")
        .expect("Shift+F10 opened no row menu")
        .rect();
    // Under the row, not over the places beside the list.
    let row = harness.get_by_label("file.zip").rect();
    assert!(
        props.left() >= row.left() - 30.0 && props.top() >= row.top(),
        "the menu at {props:?} is not under the row at {row:?}"
    );
}

#[test]
fn the_lists_keys_do_not_reach_it_from_behind_a_dialog() {
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::Options);
    harness.run_steps(2);
    harness.key_press(egui::Key::Delete);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert_eq!(
        harness.state().rows().len(),
        1,
        "Delete behind Options removed it"
    );
    assert!(
        harness.state().dialogs.progress.is_empty(),
        "Enter reached the list"
    );
    assert!(harness.state().dialogs.confirm_remove.is_none());
    // With the dialog gone, Delete is the list's again.
    harness.state_mut().dialogs.options = None;
    harness.run_steps(2);
    harness.key_press(egui::Key::Delete);
    until(&mut harness, "Delete to act on the list", |app| {
        app.rows().is_empty() || app.dialogs.confirm_remove.is_some()
    });
}

#[test]
fn f3_finds_the_next_one_while_find_is_open() {
    let world = world();
    let mut harness = world.harness();
    for name in ["alpha.bin", "beta.bin", "alphabet.bin"] {
        harness
            .state()
            .backend()
            .add_with(
                &world.url(name),
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.state_mut().run(Command::Find);
    harness.run_steps(2);
    type_into(&mut harness, "Find what", "alpha");
    let picked = |h: &Harness<'_, DmApp>| {
        h.state()
            .targets()
            .iter()
            .map(dm_desktop::app::name_of)
            .collect::<Vec<_>>()
    };
    // From Find's own field.
    harness.key_press(egui::Key::F3);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alpha.bin"], "F3 in the field");
    // Enter finds too, and the field keeps the keyboard: the
    // next Enter finds the next, and typing goes on into the field.
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alphabet.bin"]);
    assert!(
        harness.ctx.egui_wants_keyboard_input(),
        "Find's field let go of the keyboard after Enter"
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alpha.bin"], "a second Enter");
    harness.event(egui::Event::Text("bet".into()));
    harness.run_steps(2);
    assert_eq!(harness.state().dialogs.find_text, "alphabet");
    harness.key_press(egui::Key::F3);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["alphabet.bin"], "F3 with Find open");
    assert!(harness.state().dialogs.find_open);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(
        !harness.state().dialogs.find_open,
        "Escape still closes Find"
    );
}

/// A dialog opened to be typed in takes the keyboard at once, and
/// Enter presses its default button.
#[test]
fn a_dialog_is_typed_in_at_once_and_enter_presses_its_default_button() {
    let world = world();
    let mut harness = world.harness();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    harness.run_steps(3);
    assert!(harness.state().dialogs.add_url.is_some());
    // No click in the field first.
    harness.event(egui::Event::Text(world.url("file.zip")));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.add_url.is_none(),
        "Enter did not press OK"
    );
    let urls = harness
        .state()
        .dialogs
        .file_info
        .as_ref()
        .map(|d| d.urls.clone());
    assert_eq!(urls, Some(vec![world.url("file.zip")]));
    harness.state_mut().dialogs = Default::default();
    harness.run_steps(2);

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    harness.run_steps(3);
    harness.event(egui::Event::Text("file".into()));
    harness.run_steps(2);
    assert_eq!(harness.state().dialogs.find_text, "file", "Find's field");

    harness.state_mut().dialogs = Default::default();
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(3);
    harness.event(egui::Event::Text(world.url("img*.jpg")));
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .dialogs
            .series
            .as_ref()
            .map(|d| d.template.clone()),
        Some(world.url("img*.jpg")),
        "the series' address field"
    );
}

/// At the narrowest window and in German, every toolbar button
/// still shows its name, rather than a picture to guess at.
#[test]
fn the_toolbar_keeps_its_names_narrow_and_in_german() {
    // And in the classic look, whose buttons were a fixed 72 points and
    // ran German's names into each other when the toolbar wrapped.
    // And at 200% text, where even its icons alone ran past the right
    // edge: they go on to a second row.
    for (look, zoom) in [("light", 1.0), ("classic", 1.0), ("light", 2.0)] {
        let world = world();
        let app = world.app_with(Settings {
            download_dir: world.dir.path().join("out"),
            show_progress: false,
            show_complete: false,
            language: "de".into(),
            theme: look.into(),
            ..Settings::default()
        });
        let mut harness = Harness::builder()
            .with_size(egui::vec2(720.0, 460.0))
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
        harness.ctx.set_zoom_factor(zoom);
        harness.run_steps(4);
        let mut seen: Vec<(String, egui::accesskit::Rect)> = Vec::new();
        for (_, _, label) in dm_desktop::app::toolbar_in(&[]) {
            let name = dm_desktop::i18n::t(label);
            let node = harness.get_by_role_and_label(egui::accesskit::Role::Button, name);
            let bounds = node.accesskit_node().bounding_box().unwrap();
            let shown = harness
                .ctx
                .fonts_mut(|f| {
                    f.layout_no_wrap(
                        name.into(),
                        egui::FontId::proportional(11.5),
                        egui::Color32::WHITE,
                    )
                })
                .size()
                .x;
            // Where names are drawn, each fits its button: the modern look
            // shows icons alone when its names would not fit on one row.
            if look == "classic" {
                assert!(
                    bounds.width() as f32 >= shown,
                    "{name}: {} wide, its name {shown}",
                    bounds.width()
                );
            }
            assert!(
                // In the window's pixels, whatever the text size.
                bounds.x1 <= 720.0,
                "{look} {zoom}x: {name} is past the window's edge"
            );
            for (other, at) in &seen {
                let meet = bounds.x0 < at.x1 - 0.5
                    && at.x0 < bounds.x1 - 0.5
                    && bounds.y0 < at.y1 - 0.5
                    && at.y0 < bounds.y1 - 0.5;
                assert!(!meet, "{look}: {name} runs into {other}");
            }
            seen.push((name.to_string(), bounds));
        }
    }
    dm_desktop::i18n::set("en");
}

/// The arrow keys, Home and End move the list's selection, Shift
/// extends it, and the keyboard's place is ringed.
#[test]
fn the_arrow_keys_move_the_selection_and_the_focus_shows() {
    let world = world();
    let mut harness = world.harness();
    for name in ["a.bin", "b.bin", "c.bin"] {
        harness
            .state()
            .backend()
            .add_with(
                &world.url(name),
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.run_steps(2);
    let picked = |h: &Harness<'_, DmApp>| {
        h.state()
            .targets()
            .iter()
            .map(dm_desktop::app::name_of)
            .collect::<Vec<_>>()
    };
    harness.key_press(egui::Key::ArrowDown);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["a.bin"]);
    assert!(
        harness.state().focus_ring.is_some(),
        "no ring where the keys are"
    );
    harness.key_press(egui::Key::ArrowDown);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowDown);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["b.bin", "c.bin"]);
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["a.bin"]);
    harness.key_press(egui::Key::End);
    harness.run_steps(2);
    assert_eq!(picked(&harness), ["c.bin"]);
    // A click puts the ring away; Tab brings it to the control focused.
    click(&mut harness, "a.bin");
    assert!(harness.state().focus_ring.is_none(), "a ring after a click");
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    assert!(harness.state().focus_ring.is_some(), "no ring after Tab");
    // With the keyboard's focus on a control, not a text field, the list's
    // keys still work: Delete asks about the selected row.
    harness.key_press(egui::Key::Delete);
    harness.run_steps(2);
    assert!(
        harness.state().dialogs.confirm_remove.is_some(),
        "Delete did nothing while a control had the focus"
    );
}

/// "Download complete" reaches every file (its list scrolls), cuts
/// a long name so Close stays in view, sits in the corner rather than over
/// the rows, and Escape closes all of it.
#[test]
fn the_complete_window_scrolls_cuts_long_names_and_closes_on_escape() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    harness.run_steps(2);
    let long = format!("{}.zip", "very-long-name-".repeat(20));
    harness
        .state_mut()
        .add(
            &[world.url("file.zip")],
            dm_core::AddOptions {
                save_name: Some(long.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let urls: Vec<String> = (0..11).map(|_| world.url("file.zip")).collect();
    harness.state_mut().add(&urls, Default::default()).unwrap();
    until(&mut harness, "all twelve to finish", |app| {
        app.rows().len() == 12 && app.rows().iter().all(|r| r.state == State::Completed)
    });
    harness.run_steps(4);
    assert!(harness.query_by_label("12 downloads finished.").is_some());
    // Several at a time, not one.
    let seen = harness
        .query_all_by_label_contains("Open the folder of")
        .filter(|n| {
            n.accesskit_node()
                .bounding_box()
                .is_some_and(|b| b.y1 <= 720.0)
        })
        .count();
    assert!(seen >= 4, "only {seen} of twelve finished files in view");
    let close = bounds(&harness, "Close");
    assert!(close.x1 <= 1100.0, "Close is off the window: {close:?}");
    // In the corner, out of the middle of the list.
    assert!(close.x0 > 1100.0 / 2.0, "not in the corner: {close:?}");
    // The entry lowest in the window: it lists the newest first, so which
    // row that is depends on the order they finished in, and under load the
    // list's last row could finish last and stand at the top.
    let open_last = harness
        .query_all_by_label_contains("Open the folder of")
        .max_by(|a, b| {
            let y = |n: &egui_kittest::Node<'_>| {
                n.accesskit_node().bounding_box().map_or(0.0, |r| r.y0)
            };
            y(a).total_cmp(&y(b))
        })
        .and_then(|n| n.accesskit_node().label().map(|l| l.to_string()))
        .expect("no Open the folder of");
    let before = bounds(&harness, &open_last);
    assert!(
        before.y0 > close.y0,
        "the premise: the last file is below the fold"
    );
    // The wheel over the list scrolls it.
    let list = bounds(&harness, "12 downloads finished.");
    harness.hover_at(egui::pos2(list.x0 as f32 + 20.0, list.y1 as f32 + 60.0));
    for _ in 0..20 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -200.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(4);
    let after = bounds(&harness, &open_last);
    assert!(
        after.y1 < close.y0,
        "the last file is not reachable: {after:?} under {close:?}"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(
        harness.state().dialogs.complete.is_empty(),
        "Escape closed one file of the window, not the window"
    );
}

/// Tab goes round a dialog's own controls, never to the menu bar
/// behind it, and Enter with none of them focused presses the default.
#[test]
fn tab_stays_in_the_dialog_and_enter_presses_its_default() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    let dialog = harness
        .query_by_label("Start now")
        .expect("Download details is open");
    let dialog = dialog.accesskit_node().bounding_box().unwrap();
    let _ = dialog;
    for _ in 0..12 {
        harness.key_press(egui::Key::Tab);
        harness.run_steps(1);
        let focused = harness.ctx.memory(|m| m.focused());
        let layer = focused.and_then(|id| harness.ctx.read_response(id).map(|r| r.layer_id));
        if let Some(layer) = layer {
            assert_eq!(
                layer.order,
                egui::Order::Middle,
                "Tab left the dialog for {layer:?}"
            );
        }
    }
    assert!(!harness.ctx.any_popup_open(), "Tab opened a menu behind");
    // Nothing focused: Enter is Start now.
    harness.ctx.memory_mut(|m| m.stop_text_input());
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.file_info.is_none(),
        "Enter did not press Start now"
    );
    assert_eq!(harness.state().rows().len(), 1);
}

/// The Delete key asks as the toolbar's Delete does, for a
/// finished row too; the speed limiter's field has the keyboard and Enter
/// applies it.
#[test]
fn the_delete_key_asks_and_the_limiter_takes_enter() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    select(&mut harness, "file.zip");
    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    assert_eq!(harness.state().rows().len(), 1, "Delete removed it at once");
    assert!(harness.query_by_label("Remove from the list").is_some());
    click(&mut harness, "Keep it");
    click(&mut harness, "Remove");
    assert!(
        harness.state().dialogs.confirm_remove.is_some(),
        "the toolbar's Delete asks the same"
    );
    click(&mut harness, "Remove it");
    assert!(harness.state().rows().is_empty());

    harness.state_mut().run(Command::SpeedLimiterSettings);
    harness.run_steps(3);
    assert!(
        harness.ctx.egui_wants_keyboard_input(),
        "the limit's field does not have the keyboard"
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("321".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.limiter.is_none(),
        "Enter did not apply"
    );
    assert_eq!(harness.state().applied.speed_limit, 321);
    let mut off = harness.state().applied.clone();
    off.speed_limit = 0;
    harness.state_mut().apply_settings(off).unwrap();
}

/// A right click moves the keyboard's place to the row clicked.
#[test]
fn a_right_click_moves_the_keyboards_place() {
    let world = world();
    let mut harness = world.harness();
    for name in ["one.zip", "two.zip", "three.zip"] {
        add_named(&mut harness, &world, name);
    }
    harness.run_steps(3);
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    let first = harness.state().cursor;
    harness.get_by_label("three.zip").click_secondary();
    harness.run_steps(2);
    let three = harness
        .state()
        .rows()
        .iter()
        .find(|r| dm_desktop::app::name_of(r) == "three.zip")
        .map(|r| r.id);
    assert_ne!(first, three);
    assert_eq!(harness.state().cursor, three, "the cursor stayed behind");
    assert_eq!(
        harness.state().anchor,
        three,
        "Shift would extend from elsewhere"
    );
}

/// Enter presses Start now in "Links on the clipboard" and in
/// "Download links".
#[test]
fn enter_starts_the_links_dialogs() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().dialogs.clip_links = Some(vec![(world.url("file.zip"), true)]);
    harness.run_steps(3);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.clip_links.is_none(),
        "Enter did nothing"
    );
    assert_eq!(harness.state().rows().len(), 1);
    harness.state_mut().dialogs.links = Some(dm_desktop::dialogs::LinksImport {
        page: world.url("page.html"),
        sign_in: None,
        cookies: String::new(),
        items: vec![(world.url("other.pdf"), true)],
        listed: Default::default(),
        refused: Default::default(),
        hide_html: true,
        types: String::new(),
        found: Default::default(),
    });
    harness.run_steps(3);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(harness.state().dialogs.links.is_none(), "Enter did nothing");
    assert_eq!(harness.state().rows().len(), 2);
}

/// Page Down and Page Up move a screenful; End brings the
/// last row wholly into view, clear of the sideways scroll bar; the focus
/// ring stays inside the list.
#[test]
fn page_keys_move_a_page_and_end_shows_the_last_row_whole() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    let links: Vec<String> = (0..40)
        .map(|i| world.url(&format!("stalled-{i:02}.bin")))
        .collect();
    harness.state().backend().pause_all();
    harness.state().backend().add(&links.join(" ")).unwrap();
    harness.state().backend().pause_all();
    harness.state_mut().refresh();
    harness.run_steps(3);
    let at = |h: &Harness<'_, DmApp>| {
        let ids: Vec<_> = h.state().visible().iter().map(|r| r.id).collect();
        h.state()
            .cursor
            .and_then(|c| ids.iter().position(|i| *i == c))
    };
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    assert_eq!(at(&harness), Some(0));
    let page = harness.state().list_page;
    assert!(page > 2, "a page of {page} rows");
    harness.key_press(egui::Key::PageDown);
    harness.run_steps(3);
    assert_eq!(at(&harness), Some(page), "Page Down did not move a page");
    harness.key_press(egui::Key::PageUp);
    harness.run_steps(3);
    assert_eq!(at(&harness), Some(0), "Page Up did not move back");

    harness.key_press(egui::Key::End);
    harness.run_steps(4);
    assert_eq!(at(&harness), Some(39));
    let clip = harness.state().list_clip.expect("the list was drawn");
    let last = harness.state().visible()[39].id;
    let row = harness.state().row_rects[&last];
    assert!(
        row.bottom() <= clip.bottom() + 0.5,
        "the last row {row:?} is under the list's edge {clip:?}"
    );
    let ring = harness
        .state()
        .focus_ring
        .expect("a ring where the keys are");
    assert!(
        clip.expand(0.5).contains_rect(ring),
        "the ring {ring:?} runs past the list {clip:?}"
    );
}

/// The Remove dialog's buttons agree with its sentence, Enter
/// keeps, and after a removal the next row is selected.
#[test]
fn remove_says_one_thing_enter_keeps_and_the_next_row_is_selected() {
    let world = world();
    let mut harness = world.harness();
    for name in ["a.bin", "b.bin", "c.bin"] {
        harness
            .state()
            .backend()
            .add_with(
                &world.url(&format!("stalled-{name}")),
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.run_steps(3);
    let ids: Vec<_> = harness.state().visible().iter().map(|r| r.id).collect();
    harness.state_mut().selected = [ids[1]].into();
    harness.run_steps(1);
    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    assert!(harness.query_by_label("Keep it").is_some());
    assert!(harness.query_by_label("Keep them").is_none());
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(harness.state().dialogs.confirm_remove.is_none());
    assert_eq!(harness.state().rows().len(), 3, "Enter deleted it");

    harness.key_press(egui::Key::Delete);
    harness.run_steps(3);
    click(&mut harness, "Yes, remove");
    harness.state_mut().refresh();
    assert_eq!(harness.state().rows().len(), 2);
    assert_eq!(
        harness.state().selected,
        [ids[2]].into(),
        "the selection was lost"
    );
}

/// Enter answers "Already in the list", with the
/// answer that changes nothing — Show it — since an Enter meant for the
/// dialog before it fetched the file again.
#[test]
fn enter_shows_the_row_when_already_listed() {
    let world = world();
    let mut harness = world.harness();
    let url = world.url("file.zip");
    add_url(&mut harness, &url, "Add for later");
    click(&mut harness, "Add URL");
    type_into(&mut harness, "Address", &url);
    click(&mut harness, "OK");
    assert!(harness.query_by_label("Already in the list").is_some());
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(harness.query_by_label("Already in the list").is_none());
    assert!(harness.query_by_label("Download details").is_none());
    assert_eq!(harness.state().rows().len(), 1);
    assert_eq!(harness.state().selected.len(), 1, "the row is not shown");
}

/// A right-click in a text field offers Cut, Copy and Paste,
/// and Paste puts the clipboard in the field.
#[test]
fn a_text_field_has_cut_copy_and_paste_on_a_right_click() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    let link = world.url("file.zip");
    *world.fake.clipboard.lock().unwrap() = Some(link.clone());
    let field = harness
        .get_by_role_and_label(Role::TextInput, "Address")
        .rect();
    harness.hover_at(field.center());
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: field.center(),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        harness.step();
    }
    harness.run_steps(2);
    assert!(
        harness.query_by_label("Cut").is_some(),
        "no menu on a right-click"
    );
    assert!(harness.query_by_label("Copy").is_some());
    click(&mut harness, "Paste");
    harness.run_steps(3);
    let typed = harness
        .state()
        .dialogs
        .add_url
        .as_ref()
        .map(|d| d.address.clone());
    assert_eq!(typed.as_deref(), Some(link.as_str()));
    assert!(harness.query_by_label("Cut").is_none(), "the menu stayed");

    // Cut acts on what was selected before the right-click moved the
    // caret (Cut and Copy did nothing).
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: field.center(),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        harness.step();
    }
    harness.run_steps(2);
    click(&mut harness, "Cut");
    harness.run_steps(3);
    let left = harness
        .state()
        .dialogs
        .add_url
        .as_ref()
        .map(|d| d.address.clone());
    assert_eq!(left.as_deref(), Some(""), "Cut left the text");

    // Escape closes the menu, and leaves the dialog open.
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: field.center(),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        harness.step();
    }
    harness.run_steps(2);
    assert!(harness.query_by_label("Paste").is_some());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Paste").is_none(),
        "Escape left the menu"
    );
    assert!(
        harness.state().dialogs.add_url.is_some(),
        "Escape closed the dialog too"
    );
}

/// Escape cancels the When-done countdown, even with Download
/// complete open behind it, and the keyboard starts on Cancel.
#[test]
fn escape_cancels_the_when_done_countdown() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = true;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.state_mut().countdown = Some((
        dm_desktop::power::WhenDone::Sleep,
        std::time::Instant::now() + std::time::Duration::from_secs(30),
    ));
    harness.run_steps(3);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .accesskit_node()
            .is_focused(),
        "the keyboard is not on Cancel"
    );
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(
        harness.state().countdown.is_none(),
        "Escape did not cancel it"
    );
    assert!(
        !world
            .fake
            .opened
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.contains("sleep")),
        "it slept"
    );
}

/// Escape closes the window in front, not one behind it. Download
/// complete and a progress window stay open side by side; Escape closed
/// Download complete whichever was in front.
#[test]
fn escape_closes_the_window_in_front() {
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(3);
    harness
        .state()
        .backend()
        .add_with(
            &world.url("stalled.bin"),
            dm_core::AddOptions {
                later: true,
                queue: dm_core::MAIN_QUEUE.into(),
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(3);
    let id = harness.state().rows()[0].id;
    let progress = egui::LayerId::new(egui::Order::Middle, egui::Id::unique(("progress", id)));
    let complete = egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::unique(("dm-dialog", "Download complete")),
    );
    for front in [progress, complete] {
        harness.state_mut().dialogs.progress.insert(id);
        harness.state_mut().dialogs.complete.insert(id);
        harness.run_steps(3);
        harness.ctx.move_to_top(front);
        harness.run_steps(2);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        let dialogs = &harness.state().dialogs;
        let (progress_open, complete_open) =
            (dialogs.progress.contains(&id), !dialogs.complete.is_empty());
        if front == progress {
            assert_eq!(
                (progress_open, complete_open),
                (false, true),
                "progress was in front"
            );
        } else {
            assert_eq!(
                (progress_open, complete_open),
                (true, false),
                "complete was in front"
            );
        }
        harness.state_mut().dialogs = Default::default();
        harness.run_steps(2);
    }
}
