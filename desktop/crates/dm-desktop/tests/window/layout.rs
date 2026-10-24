use crate::support::*;

fn last_row_reached(clipboard_bar: bool) {
    let world = world();
    // A maximized window, where it was seen.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    let links: Vec<String> = (0..34)
        .map(|i| world.url(&format!("stalled-{i:02}.bin")))
        .collect();
    harness.state().backend().pause_all();
    harness.state().backend().add(&links.join(" ")).unwrap();
    harness.state().backend().pause_all();
    harness.state_mut().refresh();
    if clipboard_bar {
        harness.state_mut().clipboard.offered = Some(world.url("offered.zip"));
    }
    harness.ctx.set_zoom_factor(3.0);
    harness.run_steps(3);
    let last = harness.state().rows().last().unwrap().id;
    harness.state_mut().scroll_to = Some(last);
    harness.run_steps(4);
    let name = dm_desktop::app::name_of(harness.state().rows().last().unwrap());
    let screen = harness.ctx.content_rect();
    let row = harness
        .query_all_by_label(&name)
        .map(|n| n.rect())
        .find(|r| r.width() > 0.0)
        .unwrap_or_else(|| panic!("{name} was never drawn"));
    assert!(
        row.bottom() <= screen.bottom() + 0.5 && row.top() >= screen.top(),
        "{name} at {row:?} outside the window {screen:?}"
    );
    // And whole above the list's sideways bar, which sits on the status
    // bar: the row ended under it at 300%.
    let status = harness
        .query_all_by_label_contains("Nothing downloading")
        .map(|n| n.rect())
        .next()
        .expect("the status bar");
    let bar = 8.0;
    assert!(
        row.bottom() + bar <= status.top(),
        "{name} at {row:?} runs under the sideways bar above the status bar at {status:?}"
    );
}

/// Every piece of text the last frame painted.
fn painted_text(harness: &Harness<'_, DmApp>) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &harness.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn a_finished_file_dragged_out_of_the_list_is_handed_to_the_systems_drag() {
    let world = world();
    let mut harness = world.harness();
    harness.state_mut().applied.show_complete = false;
    add_url(&mut harness, &world.url("file.zip"), "Start now");
    add_url(&mut harness, &world.url("stalled.bin"), "Add for later");
    until(&mut harness, "the download to complete", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    let done = harness
        .state()
        .rows()
        .iter()
        .find(|r| r.state == State::Completed)
        .unwrap()
        .clone();

    // Pressed on its name and moved: a drag, not a click.
    let drag_at = |harness: &mut Harness<'_, DmApp>, at: egui::Pos2| {
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at));
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        harness.step();
        for step in 1..=4 {
            let to = at + egui::vec2(12.0 * step as f32, 6.0 * step as f32);
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(to));
            harness.step();
        }
        let carried = std::mem::take(&mut harness.state_mut().drag_out);
        let end = at + egui::vec2(48.0, 24.0);
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness.run_steps(2);
        carried
    };
    let drag = |harness: &mut Harness<'_, DmApp>, name: &str| {
        let at = harness.get_by_label(name).rect().left_center() + egui::vec2(8.0, 0.0);
        drag_at(harness, at)
    };
    assert_eq!(
        drag(&mut harness, "file.zip"),
        [std::path::PathBuf::from(&done.path)],
        "the finished file was not carried"
    );
    assert!(
        drag(&mut harness, "stalled.bin").is_empty(),
        "an unfinished download has no file to carry"
    );
    // A quick drag from a column's edge resizes the column; it carried the
    // row's file away.
    let edge = egui::pos2(
        harness.get_by_label("Status").rect().left() - 4.0,
        harness.get_by_label("file.zip").rect().center().y,
    );
    // Pressed and moved far in one frame, as a quick hand does.
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge + egui::vec2(60.0, 4.0)));
    harness.step();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge + egui::vec2(80.0, 6.0)));
    harness.step();
    let carried = std::mem::take(&mut harness.state_mut().drag_out);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge + egui::vec2(80.0, 6.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(2);
    assert!(
        carried.is_empty(),
        "a drag from a column's edge carried the row"
    );
}

/// A column is widened by dragging its heading's right edge, as in any
/// list: in the window it did nothing.
#[test]
fn a_column_is_widened_by_dragging_its_edge() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    // Status's heading starts where its column does (the headings of
    // numbers sit at their columns' right): dragging the edge just left of
    // it widens Size and moves Status along.
    let size_at = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(Role::Button, "Status")
            .rect()
            .left()
    };
    let before = size_at(&harness);
    let edge = egui::pos2(
        before - 5.0,
        harness
            .get_by_role_and_label(Role::Button, "Status")
            .rect()
            .center()
            .y,
    );
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(edge));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for step in 1..=8 {
        harness.input_mut().events.push(egui::Event::PointerMoved(
            edge + egui::vec2(10.0 * step as f32, 0.0),
        ));
        harness.step();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: edge + egui::vec2(80.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(3);
    let after = size_at(&harness);
    assert!(after > before + 40.0, "Size moved from {before} to {after}");

    // And kept at the next launch: the window's memory, written out and
    // read back as eframe does, gives a new window the same widths (they were back to their first ones).
    let saved = harness
        .ctx
        .memory(ron::to_string)
        .expect("the window's memory is written out");
    drop(harness);
    // A new window over the same list, its row listed again.
    let mut again = world.harness();
    again.ctx.memory_mut(|m| {
        *m = ron::from_str(&saved).expect("and read back");
    });
    add_url(&mut again, &world.url("file.zip"), "Add for later");
    again.run_steps(3);
    let relaunched = size_at(&again);
    assert!(
        (relaunched - after).abs() < 1.0,
        "Size was {after} wide when left, {relaunched} at the next launch"
    );
}

/// The series' error is in the language in force, says nothing
/// until From and To are typed, and keeps its room so the buttons stay put.
#[test]
fn the_series_error_waits_for_both_ends_and_speaks_german() {
    use dm_desktop::i18n::t;
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        show_progress: false,
        show_complete: false,
        language: "de".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    harness.run_steps(2);
    harness.state_mut().run(Command::AddSeries);
    harness.run_steps(2);
    let start = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(egui::accesskit::Role::Button, t("Start now"))
            .rect()
    };
    let before = start(&harness);
    type_into(&mut harness, t("Address with *"), &world.url("part.bin"));
    let star = t("The address needs exactly one * to fill in.");
    assert_ne!(star, "The address needs exactly one * to fill in.");
    assert!(
        harness.query_by_label(star).is_none(),
        "said before From/To"
    );
    type_into(&mut harness, t("From"), "5");
    assert!(harness.query_by_label(star).is_none(), "said before To");
    type_into(&mut harness, t("To"), "1");
    assert!(harness.query_by_label(star).is_some(), "not said in German");
    assert_eq!(start(&harness), before, "the dialog jumped");
}

/// A dialog as tall as the room lets it be, give or take: it turned
/// scrolling on and off on alternate frames, and no click on it took
/// (Site files on Windows' fonts, which stand it 28 points taller).
#[test]
fn a_dialog_at_the_edge_of_scrolling_still_takes_clicks() {
    let _file = settings_file();
    let world = world();
    for height in (684..=720).step_by(4) {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, height as f32))
            .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
        harness.run_steps(2);
        click(&mut harness, "Site files");
        type_into(&mut harness, "Start page", &world.url("page.html"));
        click(&mut harness, "Find files");
        let started = harness
            .state()
            .dialogs
            .site_files
            .as_ref()
            .map(|d| !matches!(*dm_core::lock(&d.state), dm_desktop::dialogs::Grab::Idle));
        assert_eq!(
            started,
            Some(true),
            "Find files did not take at {height} points tall"
        );
    }
}

/// View › Text size makes everything larger, smaller, or normal again.
#[test]
fn view_text_size_zooms_the_window() {
    let world = world();
    let mut harness = world.harness();
    press(&mut harness, &["View", "Text size", "Larger"]);
    harness.run_steps(3);
    let zoom = harness.ctx.zoom_factor();
    assert!((zoom - 1.1).abs() < 0.01, "{zoom}");
    press(&mut harness, &["View", "Text size", "Normal size"]);
    harness.run_steps(3);
    assert!((harness.ctx.zoom_factor() - 1.0).abs() < 0.01);
}

/// The pointer says the same thing everywhere, as a desktop app's does: the
/// arrow over buttons, checkboxes, menus and rows; the text cursor over a
/// field; the resize arrows over a column's edge.
#[test]
fn the_pointer_is_the_same_kind_over_the_same_kind_of_thing() {
    use egui::accesskit::Role;
    use egui::CursorIcon;
    let world = world();
    let mut harness = world.harness();
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    let over = |harness: &mut Harness<'_, DmApp>, at: egui::Pos2| {
        harness.hover_at(at);
        harness.run_steps(2);
        harness.output().platform_output.cursor_icon
    };
    let button = harness
        .get_by_role_and_label(Role::Button, "Add URL")
        .rect()
        .center();
    assert_eq!(
        over(&mut harness, button),
        CursorIcon::Default,
        "a toolbar button"
    );
    let row = harness.get_by_label("file.zip").rect().center();
    assert_eq!(over(&mut harness, row), CursorIcon::Default, "a row");
    let status = harness.get_by_role_and_label(Role::Button, "Status").rect();
    let edge = egui::pos2(status.left() - 5.0, status.center().y);
    assert_eq!(
        over(&mut harness, edge),
        CursorIcon::ResizeColumn,
        "a column's edge"
    );
    harness.state_mut().run(Command::AddUrl);
    harness.run_steps(3);
    let field = harness.get_by_label("Address").rect().center();
    assert_eq!(over(&mut harness, field), CursorIcon::Text, "a text field");
    let check = harness.get_by_label("Needs a sign-in").rect().center();
    assert_eq!(over(&mut harness, check), CursorIcon::Default, "a checkbox");
}

#[test]
fn view_language_puts_the_window_in_another_language_and_back() {
    let _file = settings_file();
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(2);
    assert!(harness.query_by_label("Program").is_some());
    // Everything shut stays shut in another language.
    assert!(harness.query_by_label("All (0)").is_some());
    click(&mut harness, "Everything");
    assert!(
        harness.query_by_label("All (0)").is_none(),
        "Everything did not shut"
    );
    click(&mut harness, "View");
    harness.get_by_label_contains("Language").click();
    harness.run_steps(3);
    harness.get_by_label("Deutsch").click();
    harness.run_steps(3);
    assert_eq!(harness.state().backend().settings().language, "de");
    harness.run_steps(10);
    assert!(harness.query_by_label("Alles").is_some());
    assert!(
        harness.query_by_label("Alle (0)").is_none(),
        "the language switch opened Everything again"
    );
    click(&mut harness, "Alles");
    assert!(
        harness.query_by_label("Programm").is_some() && harness.query_by_label("Program").is_none(),
        "the menu is not in German"
    );
    assert!(
        harness.query_by_label("Geschwindigkeit").is_some(),
        "nor the list's headings"
    );
    click(&mut harness, "Ansicht");
    harness.get_by_label_contains("Sprache").click();
    harness.run_steps(3);
    harness.get_by_label("English").click();
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Program").is_some(),
        "not back in English"
    );
    assert_eq!(harness.state().backend().settings().language, "en");
}

#[test]
fn five_hundred_rows_draw_inside_a_frame_budget() {
    let world = world();
    let mut harness = world.harness();
    let links: Vec<String> = (0..500)
        .map(|i| world.url(&format!("stalled-{i}.bin")))
        .collect();
    harness.state().backend().pause_all();
    harness.state().backend().add(&links.join(" ")).unwrap();
    harness.state().backend().pause_all();
    harness.state_mut().refresh();
    harness.run_steps(2);

    // The best of five runs of ten frames: the suite's other tests and
    // whatever else the machine is doing stretch one run, not the window's
    // own work (it read 106 ms beside a Docker build, 38 ms alone).
    let per_frame = (0..5)
        .map(|_| {
            let started = Instant::now();
            for _ in 0..10 {
                harness.step();
            }
            started.elapsed() / 10
        })
        .min()
        .unwrap_or_default();
    println!("500 rows: {per_frame:?} a frame");
    assert_eq!(harness.state().rows().len(), 500);
    assert!(
        per_frame < Duration::from_millis(100),
        "{per_frame:?} a frame"
    );
    // With a filter typed, too: it lowercased every row's name, address
    // and note on every frame, 36 ms a frame at 5,000 rows (release).
    harness.state_mut().filter = "stalled-4".into();
    let filtered = (0..5)
        .map(|_| {
            let started = Instant::now();
            for _ in 0..10 {
                harness.step();
            }
            started.elapsed() / 10
        })
        .min()
        .unwrap_or_default();
    println!("500 rows, filtered: {filtered:?} a frame");
    assert!(
        harness.state().visible().len() < 500,
        "the filter did nothing"
    );
    assert!(
        filtered < Duration::from_millis(100),
        "{filtered:?} a frame"
    );
}

#[test]
fn properties_fit_a_small_window_and_close_from_the_title_bar() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    let long = format!("https://example.com/{}", "very-long-page-name/".repeat(30));
    harness
        .state_mut()
        .add(
            &[world.url("file.zip")],
            dm_core::AddOptions {
                referrer: long,
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(4);
    // Reached, scrolling the dialog's body where it is taller than the room.
    harness.get_by_label("OK").scroll_to_me();
    harness.run_steps(3);
    let ok = harness.get_by_label("OK").rect();
    assert!(
        ok.max.x <= 720.0 && ok.max.y <= 460.0,
        "OK is off the window: {ok:?}"
    );
    harness.get_by_label("Close window").click();
    harness.run_steps(3);
    assert!(
        harness.state().dialogs.properties.is_empty(),
        "no way to close it"
    );
}

/// A long refusal wraps in the progress window, which stays on
/// the screen; the status bar cuts a long sentence and keeps it on hover.
#[test]
fn long_status_text_wraps_and_stays_on_screen() {
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: true,
        show_complete: false,
        language: "en".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 600.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    let long = format!("{}secret.zip", "a-long-path/".repeat(12));
    let url = world.url(&long);
    add_url(&mut harness, &url, "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(4);
    let retry = bounds(&harness, "Retry");
    assert!(retry.x1 <= 720.0 && retry.y1 <= 600.0, "{retry:?}");
    for node in harness.query_all_by_label_contains("needs a sign-in") {
        let b = node.accesskit_node().bounding_box().unwrap();
        assert!(b.x1 <= 720.5, "the status ran off the window: {b:?}");
    }
    harness.state_mut().dialogs = Default::default();
    harness.state_mut().said = format!(
        "this computer could not open {}: xdg-open: {}\nmore",
        "/x".repeat(30),
        "lynx: not found ".repeat(10)
    )
    .into();
    harness.run_steps(3);
    for node in harness.query_all_by_label_contains("this computer could not open") {
        let b = node.accesskit_node().bounding_box().unwrap();
        assert!(b.x1 <= 720.5, "the status bar ran off the window: {b:?}");
    }
}

/// Options › Folders gives each kind's folder field the room its
/// name needs.
#[test]
fn the_kinds_folder_fields_fit_their_names() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    let mut wanted = harness.state().applied.clone();
    wanted.categorize = true;
    harness.state_mut().applied = wanted.clone();
    harness.state_mut().dialogs.open_options(&wanted);
    harness.run_steps(2);
    click(&mut harness, "Folders");
    harness.run_steps(3);
    for kind in ["Videos", "Documents", "Archives", "Other"] {
        let field = harness.get_by_role_and_label(Role::TextInput, kind);
        let b = field.accesskit_node().bounding_box().unwrap();
        assert!(b.width() >= 120.0, "{kind}'s field is {} wide", b.width());
    }
}

/* ------------------------------------------- the eighth hands-on round */

/// An Arabic name is drawn with its words in reading order, and a
/// screen reader is still given it as stored.
#[test]
fn an_arabic_name_is_drawn_in_reading_order() {
    let world = world();
    let mut harness = world.harness();
    // Drawn as a box on the one frame before the computer's font for it is
    // read, or on a computer with none: the order is what is checked here.
    harness
        .ctx
        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
    add_named(&mut harness, &world, "ملف عربي.zip");
    harness.run_steps(4);
    let drawn = painted_text(&harness);
    assert!(
        drawn.iter().any(|t| t == "عربي ملف.zip"),
        "not drawn in reading order: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "ملف عربي.zip"),
        "drawn in stored order"
    );
    assert!(harness.query_by_label("ملف عربي.zip").is_some());
}

/// Download details forgets an earlier opening's warning, and a
/// warning wraps inside a 720 px window rather than pushing Start now off it.
#[test]
fn download_details_starts_afresh_and_its_warning_wraps() {
    let world = world();
    let app = world.app();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 600.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    *world.fake.no_chooser.lock().unwrap() = true;
    let warning = "No folder picker here: type the path.";
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    click(&mut harness, "Browse…");
    harness.run_steps(2);
    assert!(harness.query_by_label(warning).is_some());
    let start = harness.get_by_label("Start now");
    let b = start.accesskit_node().bounding_box().unwrap();
    assert!(
        b.x1 <= 720.0 && b.x0 >= 0.0,
        "Start now is off the window: {b:?}"
    );
    click(&mut harness, "Cancel");
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    harness.run_steps(3);
    assert!(
        harness.query_by_label(warning).is_none(),
        "the last opening's warning stayed"
    );
}

/// In the classic look at 720 px, the progress window's bar keeps
/// its width after an error rather than shrinking to a pill.
#[test]
fn the_progress_bar_keeps_its_width_in_the_classic_look() {
    let world = world();
    let app = world.app_with(Settings {
        download_dir: world.dir.path().join("out"),
        connections: 1,
        show_progress: true,
        show_complete: false,
        language: "en".into(),
        theme: "classic".into(),
        ..Settings::default()
    });
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 600.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), app);
    add_url(&mut harness, &world.url("secret.zip"), "Start now");
    until(&mut harness, "the refusal", |app| {
        app.rows()
            .iter()
            .any(|r| matches!(r.state, State::Failed(_)))
    });
    harness.run_steps(6);
    let bar = harness
        .query_all_by_role(egui::accesskit::Role::ProgressIndicator)
        .map(|n| n.accesskit_node().bounding_box().unwrap())
        .fold(0.0_f64, |w, b| w.max(b.width()));
    assert!(bar >= 300.0, "the bar is {bar} wide");
    // And its track can be seen: at 0% the bar was drawn in the dialog's
    // own colour, all but its words' pill gone.
    let at = harness
        .query_all_by_role(egui::accesskit::Role::ProgressIndicator)
        .map(|n| n.accesskit_node().bounding_box().unwrap())
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .unwrap();
    // Drawn to pixels where the machine has a GPU to draw with; CI's Linux
    // runner has none, and wgpu panics finding no adapter (CI, 0.2.0). The
    // width above holds either way.
    let Ok(Ok(image)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.render()))
    else {
        eprintln!("no GPU to render with here; the track's colour is not checked");
        return;
    };
    let scale = image.width() as f64 / 720.0;
    let pixel = |x: f64, y: f64| *image.get_pixel((x * scale) as u32, (y * scale) as u32);
    let track = pixel(at.x0 + at.width() * 0.8, (at.y0 + at.y1) / 2.0);
    let dialog = pixel(at.x0 + at.width() * 0.8, at.y1 + 4.0);
    assert_ne!(track, dialog, "the bar's track is the dialog's own colour");
}

/// The list's up-and-down bar is at its visible right edge when
/// the columns run past the window, and dragging it scrolls the list.
#[test]
fn the_lists_bar_stays_in_view_when_the_columns_run_past_it() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    harness.state_mut().hidden_columns.clear();
    let links: Vec<String> = (0..40)
        .map(|i| world.url(&format!("gone-{i:02}.bin")))
        .collect();
    harness.state().backend().pause_all();
    for link in &links {
        harness
            .state()
            .backend()
            .add_with(
                link,
                dm_core::AddOptions {
                    later: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    harness.state_mut().refresh();
    harness.run_steps(4);
    let bar = harness.state().list_bar.expect("a bar for 40 rows");
    let clip = harness.state().list_clip.expect("the list was drawn");
    assert!(bar.right() <= 720.0, "the bar {bar:?} is past the window");
    assert!(
        bar.left() >= clip.right() - 1.0,
        "the bar {bar:?} is not beside the list {clip:?}"
    );
    let first = harness.state().visible()[0].id;
    let top = harness.state().row_rects[&first].top();
    // Dragged from its top to its bottom, as a person drags it.
    let from = egui::pos2(bar.center().x, bar.top() + 6.0);
    let to = egui::pos2(bar.center().x, bar.bottom() - 2.0);
    let input = |h: &mut Harness<'_, DmApp>, event: egui::Event| {
        h.input_mut().events.push(event);
        h.run_steps(2);
    };
    input(&mut harness, egui::Event::PointerMoved(from));
    input(
        &mut harness,
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    );
    for step in 1..=8 {
        let at = from + (to - from) * (step as f32 / 8.0);
        input(&mut harness, egui::Event::PointerMoved(at));
    }
    input(
        &mut harness,
        egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    );
    harness.run_steps(4);
    let moved = harness
        .state()
        .row_rects
        .get(&first)
        .map(|r| r.top())
        .unwrap_or(f32::NEG_INFINITY);
    assert!(
        moved < top - 100.0,
        "the list did not scroll: {top} to {moved}"
    );
}

/// Add URL keeps its place when its warning appears, and
/// Download details shows the size once the server says it.
#[test]
fn dialogs_keep_their_place_and_show_the_size() {
    let world = world();
    let mut harness = world.harness();
    click(&mut harness, "Add URL");
    harness.run_steps(4);
    let before = harness.get_by_label("OK").rect();
    let two = format!("{}{}", world.url("a.zip"), world.url("b.zip"));
    type_into(&mut harness, "Address", &two);
    click(&mut harness, "OK");
    harness.run_steps(4);
    assert!(harness.query_by_label_contains("two links").is_some());
    let after = harness.get_by_label("OK").rect();
    assert!(
        (before.top() - after.top()).abs() < 1.0,
        "OK moved from {before:?} to {after:?}"
    );
    harness.state_mut().dialogs.add_url = None;
    harness.run_steps(2);
    harness
        .state_mut()
        .dialogs
        .open_file_info(vec![world.url("file.zip")]);
    until(&mut harness, "the size to be shown", |_| true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.query_by_label("256.0 KB").is_none() {
        assert!(Instant::now() < deadline, "no size in Download details");
        harness.run_steps(2);
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The list's sideways scroll bar is dragged like any other; its
/// thumb did nothing (Shift and the wheel worked).
#[test]
fn the_lists_sideways_bar_is_dragged() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(760.0, 560.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    let name_at =
        |h: &Harness<'_, DmApp>| h.get_by_role_and_label(Role::Button, "Name").rect().left();
    let before = name_at(&harness);
    // The bar runs along the list's foot, a little over the status bar.
    let foot = harness
        .get_by_label_contains("waiting for Start queue")
        .rect()
        .top();
    let mut grabbed = None;
    for up in (4..40).step_by(3).map(|u| u as f32) {
        let at = egui::pos2(300.0, foot - up);
        let events = [
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ];
        harness.input_mut().events.extend(events);
        harness.step();
        for step in 1..=8 {
            harness.input_mut().events.push(egui::Event::PointerMoved(
                at + egui::vec2(25.0 * step as f32, 0.0),
            ));
            harness.step();
        }
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at + egui::vec2(200.0, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness.run_steps(3);
        if name_at(&harness) < before - 20.0 {
            grabbed = Some(up);
            break;
        }
    }
    assert!(
        grabbed.is_some(),
        "dragging along the list's foot did not move the columns (Name stayed at {before})"
    );
}

/// A row waiting in the queue is dragged to another place in it,
/// as Move up and Move down move it a place at a time.
#[test]
fn a_row_is_dragged_to_another_place_in_the_queue() {
    let world = world();
    let mut harness = world.harness();
    for file in ["file.zip", "other.pdf", "x.bin"] {
        add_url(&mut harness, &world.url(file), "Add for later");
    }
    harness.run_steps(3);
    let order = |h: &Harness<'_, DmApp>| -> Vec<String> {
        h.state()
            .rows
            .iter()
            .map(dm_desktop::app::name_of)
            .collect()
    };
    assert_eq!(order(&harness), ["file.zip", "other.pdf", "x.bin"]);
    let rect = |h: &Harness<'_, DmApp>, name: &str| h.get_by_label(name).rect();
    let from = rect(&harness, "x.bin").center();
    let onto = rect(&harness, "file.zip").center();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for step in 1..=10 {
        let at = from + (onto - from) * (step as f32 / 10.0);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at));
        harness.step();
    }
    assert_eq!(
        harness.state().reordering.map(|_| ()),
        Some(()),
        "the drag did not take the row"
    );
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: onto,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(3);
    assert_eq!(order(&harness), ["x.bin", "file.zip", "other.pdf"]);
    assert!(harness.state().reordering.is_none());
}

/// The columns follow the window's width until one is dragged.
/// Kept from the first draw, a list opened narrow and widened left the
/// name at 120 points beside empty room.
#[test]
fn the_columns_follow_the_window_until_one_is_dragged() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    add_url(&mut harness, &world.url("file.zip"), "Add for later");
    harness.run_steps(3);
    let name_width = |h: &Harness<'_, DmApp>| {
        h.get_by_role_and_label(Role::Button, "Queue").rect().left()
            - h.get_by_role_and_label(Role::Button, "Name").rect().left()
    };
    let narrow = name_width(&harness);
    harness.set_size(egui::vec2(1600.0, 800.0));
    harness.run_steps(4);
    let wide = name_width(&harness);
    assert!(
        wide > narrow + 40.0,
        "the name stayed {narrow} wide, now {wide}"
    );
}

/// A progress window opens below the toolbar, not over Add
/// URL; and Download complete waits while the Exit question is asked,
/// rather than opening over it with Open folder under the pointer.
#[test]
fn windows_a_download_opens_keep_clear_of_the_toolbar_and_questions() {
    let world = world();
    // The usual window's size, where a tall progress window was pushed back
    // up over the toolbar to fit.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(960.0, 660.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    for _ in 0..2 {
        harness
            .state()
            .backend()
            .add(&world.url("stalled.bin"))
            .unwrap();
    }
    until(&mut harness, "both to run", |app| {
        app.rows()
            .iter()
            .filter(|r| r.state == State::Running)
            .count()
            == 2
    });
    let ids: Vec<_> = harness.state().rows().iter().map(|r| r.id).collect();
    harness.state_mut().dialogs.progress.extend(ids);
    harness.run_steps(3);
    let add = harness.get_by_label("Add URL").rect();
    let windows: Vec<egui::Rect> = harness
        .query_all_by_label_contains("Download progress")
        .map(|n| n.rect())
        .collect();
    assert_eq!(windows.len(), 2);
    for window in &windows {
        assert!(
            window.top() > add.bottom(),
            "{window:?} over Add URL at {add:?}"
        );
    }
    assert!(
        (windows[0].top() - windows[1].top()).abs() > 10.0,
        "they do not cascade: {windows:?}"
    );

    harness.state_mut().dialogs.progress.clear();
    harness.state_mut().applied.show_complete = true;
    harness.state_mut().dialogs.confirm_exit = true;
    harness
        .state_mut()
        .add(&[world.url("file.zip")], Default::default())
        .unwrap();
    until(&mut harness, "it to finish", |app| {
        app.rows().iter().any(|r| r.state == State::Completed)
    });
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Download complete").is_none(),
        "opened over the Exit question"
    );
    harness.state_mut().dialogs.confirm_exit = false;
    harness.run_steps(3);
    assert!(harness.query_by_label("Download complete").is_some());
}

/// The columns start where they did at the last launch
/// whether or not a row has a queue yet; Queue started narrower in an
/// empty list, so a relaunch moved Size and the rest and cut "Time left".
#[test]
fn the_columns_start_in_one_place_whatever_the_rows_queues() {
    let world = world();
    let mut harness = world.harness();
    harness.run_steps(3);
    let before = harness.get_by_label("Size").rect().left();
    add_url(&mut harness, &world.url("other.pdf"), "Add for later");
    assert!(harness.state().rows().iter().any(|r| !r.queue.is_empty()));
    // A launch: nothing of the list's widths remembered, no column dragged.
    harness.ctx.memory_mut(|m| m.data.clear());
    harness.run_steps(3);
    let after = harness.get_by_label("Size").rect().left();
    assert!(
        (before - after).abs() < 1.0,
        "Size moved from {before} to {after}"
    );
}

/// Where the places give way at large text, a drop-down
/// above the list reaches them; nothing did.
#[test]
fn the_places_are_reachable_when_they_give_way_at_large_text() {
    use egui::accesskit::Role;
    let world = world();
    let mut harness = world.harness();
    harness.ctx.set_zoom_factor(2.0);
    harness.run_steps(3);
    assert!(
        harness.query_by_label("Everything").is_none(),
        "the places did not give way"
    );
    harness
        .get_by_role_and_label(Role::ComboBox, "Place")
        .click();
    harness.run_steps(3);
    // One in view of the drop-down at this size; the rest scroll.
    harness.get_by_label("Videos (0)").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().place,
        dm_desktop::app::Place::Kind(dm_core::Category::Videos)
    );
}

/// At large text, with the Place drop-down above the list,
/// the last rows could never be scrolled into view.
#[test]
fn the_last_row_can_be_reached_at_large_text() {
    last_row_reached(false);
    // And with the clipboard's bar above the list.
    last_row_reached(true);
}

/// At 230% in the smallest window the toolbar wrapped to
/// three rows and no download showed; it gives way and a row shows.
#[test]
fn a_row_shows_at_230_percent_in_the_smallest_window() {
    let world = world();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(720.0, 460.0))
        .build_ui_state(|ui, app: &mut DmApp| app.ui(ui), world.app());
    harness
        .state()
        .backend()
        .add(&world.url("other.pdf"))
        .unwrap();
    harness.state_mut().refresh();
    harness.ctx.set_zoom_factor(2.3);
    harness.run_steps(4);
    let screen = harness.ctx.content_rect();
    // By the name the list gives it, which may be numbered.
    let row = harness
        .query_all_by_label_contains("other")
        .map(|n| n.rect())
        .find(|r| r.height() > 0.0)
        .expect("no row drawn");
    assert!(row.bottom() <= screen.bottom(), "{row:?} below {screen:?}");
}

/// The progress window was once held at the room on the screen, 84% of the
/// window's height for three connections. It is as tall as what it holds,
/// its connections shut until asked for.
#[test]
fn the_progress_window_is_as_tall_as_what_it_holds() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state()
        .backend()
        .add(&world.url("stalled.bin"))
        .unwrap();
    until(&mut harness, "it to run", |app| {
        app.rows().iter().any(|r| r.state == State::Running)
    });
    let id = harness.state().rows()[0].id;
    harness.state_mut().dialogs.progress.insert(id);
    harness.run_steps(4);
    let window = harness
        .query_all_by_label_contains("Download progress")
        .map(|n| n.rect())
        .next()
        .expect("no progress window");
    let screen = harness.ctx.content_rect();
    // Half the window's height, and the row of the download's own speed
    // limit added since: still far from the 84% it once filled.
    assert!(
        window.height() < screen.height() * 0.55,
        "{window:?} is {:.0}% of the window's height",
        100.0 * window.height() / screen.height()
    );
}

/// Properties opens in the middle, as the other dialogs do; it
/// opened in the window's top left corner.
#[test]
fn properties_open_in_the_middle() {
    let world = world();
    let mut harness = world.harness();
    harness
        .state_mut()
        .add(
            &[world.url("file.zip")],
            dm_core::AddOptions {
                later: true,
                ..Default::default()
            },
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.run_steps(2);
    select(&mut harness, "file.zip");
    harness.state_mut().run(Command::Properties);
    harness.run_steps(4);
    let title = harness.get_by_label("File properties").rect();
    // The title is at the dialog's top left; the dialog is about 620 wide.
    assert!(
        title.left() > 150.0,
        "Properties opened at the left: {title:?}"
    );
    assert!(
        title.top() > 100.0,
        "Properties opened at the top: {title:?}"
    );
}
