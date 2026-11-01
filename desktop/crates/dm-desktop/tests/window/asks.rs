//! What reference A asks about, and dm now asks (maintainer, 2026-11-01): the
//! name the server sent, a web page where a file was asked for, a 404.

use crate::support::*;

fn finished(app: &DmApp) -> bool {
    app.rows().iter().any(|r| r.state == State::Completed)
}

#[test]
fn a_name_the_server_sent_is_asked_about_and_yes_keeps_it() {
    let world = world();
    let mut harness = world.quick_harness();
    add_url(&mut harness, &world.url("named"), "Start now");
    until(&mut harness, "the question", |app| {
        app.dialogs.name_ask.is_some()
    });
    harness.run_steps(3);
    assert!(harness.query_by_label("Name sent by the server").is_some());
    click(&mut harness, "Yes");
    until(&mut harness, "it to finish", finished);
    harness.run_steps(3);
    assert_eq!(harness.state().rows()[0].filename, "report-2026.pdf");
    assert!(harness.query_by_label("Name sent by the server").is_none());
}

#[test]
fn a_name_the_server_sent_is_given_up_for_the_addresss_on_no() {
    let world = world();
    let mut harness = world.quick_harness();
    add_url(&mut harness, &world.url("named"), "Start now");
    until(&mut harness, "the question", |app| {
        app.dialogs.name_ask.is_some()
    });
    harness.run_steps(3);
    click(&mut harness, "No");
    until(
        &mut harness,
        "it to finish under the address's name",
        |app| {
            app.rows()
                .iter()
                .any(|r| r.state == State::Completed && r.filename == "named")
        },
    );
    assert!(std::path::Path::new(&harness.state().rows()[0].path).is_file());
}

#[test]
fn a_web_page_for_a_file_is_saved_as_html_on_yes() {
    let world = world();
    let mut harness = world.quick_harness();
    add_url(&mut harness, &world.url("pagefile.zip"), "Start now");
    until(&mut harness, "the question", |app| {
        app.dialogs.page_ask.is_some()
    });
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Web page instead of the file")
        .is_some());
    click(&mut harness, "Yes");
    until(&mut harness, "the page to be saved", finished);
    assert_eq!(harness.state().rows()[0].filename, "pagefile.html");
}

#[test]
fn a_web_page_for_a_file_leaves_nothing_on_no() {
    let world = world();
    let mut harness = world.quick_harness();
    add_url(&mut harness, &world.url("pagefile.zip"), "Start now");
    until(&mut harness, "the question", |app| {
        app.dialogs.page_ask.is_some()
    });
    harness.run_steps(3);
    click(&mut harness, "No");
    until(&mut harness, "the row to go", |app| app.rows().is_empty());
}

#[test]
fn a_404_is_told_once_in_a_window_of_its_own() {
    let world = world();
    let mut harness = world.quick_harness();
    add_url(&mut harness, &world.url("missing.bin"), "Start now");
    until(&mut harness, "the window", |app| {
        app.dialogs.not_found.is_some()
    });
    harness.run_steps(3);
    assert!(harness.query_by_label("The file doesn't exist").is_some());
    assert!(harness
        .query_by_label("The requested file doesn't exist.")
        .is_some());
    assert!(harness.query_by_label("HTTP/1.1 404 Not Found").is_some());
    click(&mut harness, "OK");
    // The row stays failed, and the window does not come back for it.
    for _ in 0..20 {
        harness.state_mut().refresh();
        harness.run_steps(2);
    }
    assert!(harness.state().dialogs.not_found.is_none());
    assert!(matches!(harness.state().rows()[0].state, State::Failed(_)));
}
