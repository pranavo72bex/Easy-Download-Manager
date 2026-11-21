// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use dm_core::State;

fn row(id: Id, url: &str, name: &str, state: State) -> Arc<Entry> {
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "id": id,
            "url": url,
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": 0, "total": null, "speed": 0.0, "eta": null},
            "filename": name,
        }))
        .unwrap(),
    )
}

#[test]
fn the_number_dm_adds_to_a_second_copy_is_not_the_servers() {
    assert_eq!(without_copy_number("again (2).bin"), "again.bin");
    assert_eq!(without_copy_number("again (12)"), "again");
    assert_eq!(
        without_copy_number("Report (final).pdf"),
        "Report (final).pdf"
    );
    assert_eq!(without_copy_number("a.b (1).tar.gz"), "a.b (1).tar.gz");
    assert_eq!(without_copy_number("plain.bin"), "plain.bin");
}

#[test]
fn what_the_first_read_holds_is_not_asked_about() {
    let mut questions = Questions::default();
    questions.watch(&[
        row(1, "http://e.com/a.bin", "other.bin", State::Running),
        row(
            2,
            "http://e.com/b.bin",
            "b.bin",
            State::Failed("answered 404".into()),
        ),
    ]);
    assert_eq!(questions.next(|_| true), None);
}

#[test]
fn a_server_name_a_page_and_a_404_are_asked_once_each_in_order() {
    let mut questions = Questions::default();
    questions.watch(&[]);
    // Named by the server once it answered.
    questions.watch(&[row(1, "http://e.com/get?id=7", "", State::Queued)]);
    questions.watch(&[row(
        1,
        "http://e.com/get?id=7",
        "report.pdf",
        State::Running,
    )]);
    let page = "the server answered with a web page rather than the file";
    questions.watch(&[
        row(1, "http://e.com/get?id=7", "report.pdf", State::Running),
        row(2, "http://e.com/f.zip", "f.zip", State::Failed(page.into())),
    ]);
    questions.watch(&[
        row(1, "http://e.com/get?id=7", "report.pdf", State::Running),
        row(2, "http://e.com/f.zip", "f.zip", State::Failed(page.into())),
        row(
            3,
            "http://e.com/g.bin",
            "g.bin",
            State::Failed("answered 404".into()),
        ),
    ]);
    assert_eq!(
        questions.next(|_| true),
        Some(Question::Name {
            id: 1,
            address_name: "get".into(),
            server_name: "report.pdf".into(),
        })
    );
    assert_eq!(questions.next(|_| true), Some(Question::Page(2)));
    assert_eq!(questions.next(|_| true), Some(Question::NotFound(3)));
    assert_eq!(questions.next(|_| true), None);
    // The same rows again raise nothing new.
    questions.watch(&[
        row(2, "http://e.com/f.zip", "f.zip", State::Failed(page.into())),
        row(
            3,
            "http://e.com/g.bin",
            "g.bin",
            State::Failed("answered 404".into()),
        ),
    ]);
    assert_eq!(questions.next(|_| true), None);
}

#[test]
fn a_query_address_is_named_by_its_last_segment_and_no_name_sent_asks_nothing() {
    // B39's own example: the address names the file "dl", the server
    // "Report 2026.pdf".
    let mut questions = Questions::default();
    questions.watch(&[]);
    questions.watch(&[row(1, "http://e.com/dl?id=7", "", State::Queued)]);
    questions.watch(&[row(
        1,
        "http://e.com/dl?id=7",
        "Report 2026.pdf",
        State::Running,
    )]);
    assert_eq!(
        questions.next(|_| true),
        Some(Question::Name {
            id: 1,
            address_name: "dl".into(),
            server_name: "Report 2026.pdf".into(),
        })
    );
    // A server that sends no name: the engine names it by the address,
    // and nothing was sent to ask about (it asked "the name was
    // e.com/plain?id=7, but the server sent plain").
    questions.watch(&[row(2, "http://e.com/plain?id=7", "", State::Queued)]);
    questions.watch(&[row(2, "http://e.com/plain?id=7", "plain", State::Running)]);
    assert_eq!(questions.next(|_| true), None);
}

#[test]
fn a_batch_a_copy_number_and_a_gone_row_raise_no_question() {
    let mut questions = Questions::default();
    questions.watch(&[]);
    // Two new at once: Download all links, not asked about names.
    questions.watch(&[
        row(1, "http://e.com/1", "one.bin", State::Running),
        row(2, "http://e.com/2", "two.bin", State::Running),
    ]);
    // The address's own name with dm's copy number.
    questions.watch(&[
        row(1, "http://e.com/1", "one.bin", State::Running),
        row(2, "http://e.com/2", "two.bin", State::Running),
        row(3, "http://e.com/f.bin", "f (2).bin", State::Running),
    ]);
    assert_eq!(questions.next(|_| true), None);
    questions.watch(&[row(
        4,
        "http://e.com/x",
        "",
        State::Failed("answered 404".into()),
    )]);
    assert_eq!(questions.next(|id| id != 4), None, "its row is gone");
}

/// A download stopped as the same file as one already here (gaps row W18) is asked about once,
/// and only that: its name, given for the question, raises no "Name sent by the server"; one a
/// person let through is not asked about again.
#[test]
fn the_same_file_is_asked_about_once_and_not_its_name() {
    let mut questions = Questions::default();
    questions.watch(&[]);
    let same = |id: Id, accepted: bool| {
        let mut entry = (*row(
            id,
            "http://mirror.example/get?id=9",
            "setup.exe",
            State::Failed(dm_core::SAME_FILE.into()),
        ))
        .clone();
        entry.fail_kind = Some(FailKind::SameFile);
        entry.accept_same = accepted;
        Arc::new(entry)
    };
    questions.watch(&[same(1, false)]);
    assert_eq!(questions.next(|_| true), Some(Question::SameFile(1)));
    assert_eq!(questions.next(|_| true), None);
    questions.watch(&[same(1, false)]);
    assert_eq!(questions.next(|_| true), None);
    questions.watch(&[same(1, false), same(2, true)]);
    assert_eq!(questions.next(|_| true), None);
}
