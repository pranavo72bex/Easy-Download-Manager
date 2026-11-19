// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn row(id: Id) -> Arc<Entry> {
    let text = format!(
        r#"{{"id":{id},"url":"https://example.com/{id}.bin","state":{{"kind":"queued"}},
        "progress":{{"downloaded":0,"total":null,"speed":0.0,"eta":null}},"filename":""}}"#
    );
    Arc::new(serde_json::from_str(&text).unwrap())
}

fn read(queue_file: &Path) -> Vec<Entry> {
    let text = std::fs::read(queue_file).unwrap();
    let entries: Vec<Entry> = crate::saved::rows_from_slice(&text).unwrap();
    replayed(queue_file, &text, entries)
}

fn ids(entries: &[Entry]) -> Vec<Id> {
    entries.iter().map(|e| e.id).collect()
}

#[test]
fn a_long_list_writes_what_changed_and_reads_back_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("queue.json");
    let mut rows: Vec<Arc<Entry>> = (1..=600).map(row).collect();
    let saved = write(&file, rows.clone(), None).unwrap();
    let whole_bytes = std::fs::metadata(&file).unwrap().len();
    // One row changed, one gone, one added.
    Arc::make_mut(&mut rows[10]).description = "changed".into();
    rows.remove(20);
    rows.push(row(601));
    let saved = write(&file, rows.clone(), Some(saved)).unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().len(),
        whole_bytes,
        "rewritten"
    );
    let journal = std::fs::read_to_string(journal_file(&file)).unwrap();
    assert_eq!(
        journal.lines().count(),
        4,
        "header, gone, changed, added: {journal}"
    );
    let back = read(&file);
    assert_eq!(ids(&back), rows.iter().map(|r| r.id).collect::<Vec<_>>());
    assert_eq!(back[10].description, "changed");
    // Nothing changed: nothing written.
    let saved = write(&file, rows.clone(), Some(saved)).unwrap();
    assert_eq!(
        std::fs::read_to_string(journal_file(&file)).unwrap(),
        journal
    );
    // Another order: written whole, and the journal gone.
    rows.swap(0, 1);
    write(&file, rows.clone(), Some(saved)).unwrap();
    assert!(!journal_file(&file).exists());
    assert_eq!(
        ids(&read(&file)),
        rows.iter().map(|r| r.id).collect::<Vec<_>>()
    );
}

#[test]
fn a_short_list_is_written_whole_every_time() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("queue.json");
    let mut rows: Vec<Arc<Entry>> = (1..=3).map(row).collect();
    let saved = write(&file, rows.clone(), None).unwrap();
    Arc::make_mut(&mut rows[0]).description = "changed".into();
    write(&file, rows.clone(), Some(saved)).unwrap();
    assert!(!journal_file(&file).exists());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"changed\""), "the file says it all");
}

#[test]
fn a_journal_over_an_older_file_or_cut_short_is_not_taken_past_what_holds() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("queue.json");
    let mut rows: Vec<Arc<Entry>> = (1..=600).map(row).collect();
    let saved = write(&file, rows.clone(), None).unwrap();
    Arc::make_mut(&mut rows[0]).description = "first".into();
    let saved = write(&file, rows.clone(), Some(saved)).unwrap();
    Arc::make_mut(&mut rows[1]).description = "second".into();
    write(&file, rows.clone(), Some(saved)).unwrap();
    // A stop in the middle of the last line.
    let journal = journal_file(&file);
    let text = std::fs::read_to_string(&journal).unwrap();
    std::fs::write(&journal, &text[..text.len() - 20]).unwrap();
    let back = read(&file);
    assert_eq!(back[0].description, "first");
    assert_eq!(back[1].description, "", "a cut line was read");
    // A journal over another file: not replayed.
    std::fs::write(&journal, text).unwrap();
    let older: Vec<Entry> = (1..=601).map(|i| Entry::clone(&row(i))).collect();
    std::fs::write(&file, serde_json::to_vec(&older).unwrap()).unwrap();
    let back = read(&file);
    assert_eq!(back[0].description, "", "replayed over another file");
}

#[test]
fn a_secret_stays_out_of_the_journal_and_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("queue.json");
    let mut rows: Vec<Arc<Entry>> = (1..=600).map(row).collect();
    let saved = write(&file, rows.clone(), None).unwrap();
    Arc::make_mut(&mut rows[5]).cookies = "session=abc".into();
    write(&file, rows.clone(), Some(saved)).unwrap();
    let journal = std::fs::read_to_string(journal_file(&file)).unwrap();
    assert!(!journal.contains("session=abc"), "{journal}");
    let kept = std::fs::read_to_string(secrets_file(&file)).unwrap();
    assert!(kept.contains("session=abc"), "{kept}");
}

fn same(read: &[Entry], rows: &[Arc<Entry>]) -> bool {
    let rows: Vec<&Entry> = rows.iter().map(|r| &**r).collect();
    serde_json::to_value(read).unwrap() == serde_json::to_value(rows).unwrap()
}

#[test]
fn the_queue_and_its_secrets_stay_bare_lists_and_read_back_the_same_rows() {
    // A short list (written indented) and a long one (compact, then a journal).
    for count in [3, 600] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("queue.json");
        let mut rows: Vec<Arc<Entry>> = (1..=count).map(row).collect();
        Arc::make_mut(&mut rows[1]).cookies = "session=abc".into();
        let saved = write(&file, rows.clone(), None).unwrap();
        // Bare lists, as an older dm reads them: no number in either.
        for kept in [file.clone(), secrets_file(&file)] {
            let text = std::fs::read(&kept).unwrap();
            assert_eq!(text.first(), Some(&b'['), "{}", kept.display());
            assert!(serde_json::from_slice::<Vec<serde_json::Value>>(&text).is_ok());
        }
        // The secrets come back in the queue's own read (`persist`), not in this one.
        Arc::make_mut(&mut rows[1]).cookies.clear();
        assert!(same(&read(&file), &rows), "{count} rows");
        Arc::make_mut(&mut rows[0]).description = "changed".into();
        write(&file, rows.clone(), Some(saved)).unwrap();
        assert!(same(&read(&file), &rows), "{count} rows, changed");
        if let Ok(journal) = std::fs::read_to_string(journal_file(&file)) {
            let first = journal.lines().next().unwrap();
            let header: Header = serde_json::from_str(first).unwrap();
            assert_eq!(header.format, JOURNAL_FORMAT, "{first}");
        }
    }
}

#[test]
fn a_queue_file_and_journal_from_before_the_numbers_read_as_they_did() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("queue.json");
    let rows: Vec<Arc<Entry>> = (1..=3).map(row).collect();
    // A bare list, as every dm before the numbers wrote it, and a journal whose first line
    // names its base alone.
    let bare: Vec<&Entry> = rows.iter().map(|r| &**r).collect();
    let bytes = serde_json::to_vec_pretty(&bare).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    assert_eq!(crate::saved::format_of(&bytes), 0);
    assert!(same(&read(&file), &rows));
    let mut changed = Entry::clone(&rows[2]);
    changed.description = "from the journal".into();
    let journal = format!(
        "{}\n{}\n",
        serde_json::json!({ "base": base_of(&bytes) }),
        serde_json::to_string(&Change::Row(Box::new(changed))).unwrap()
    );
    std::fs::write(journal_file(&file), journal).unwrap();
    assert_eq!(read(&file)[2].description, "from the journal");

    // The object shape an early build of this change wrote still reads, rows equal.
    std::fs::remove_file(journal_file(&file)).unwrap();
    let object = serde_json::json!({ "format": 1, "rows": bare });
    std::fs::write(&file, object.to_string()).unwrap();
    assert!(same(&read(&file), &rows));
}
