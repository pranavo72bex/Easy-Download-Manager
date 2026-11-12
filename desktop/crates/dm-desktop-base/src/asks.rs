// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What dm's questions say and offer, in the language in force, so every
//! window asks them in the same words: the title, the sentences, a choice
//! to tick, and the buttons (the one that loses nothing first). How a
//! question is drawn, and where, is the window's.

use dm_core::{Entry, State};

use crate::i18n::{t, tf, tn};

/// The question asked before Remove (B17's neighbour: every way of asking,
/// the Delete key, the toolbar, the menus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveAsk {
    /// The title in English, which the window's tests and the control port
    /// find it by.
    pub title_english: &'static str,
    /// The title, in the language in force.
    pub title: &'static str,
    /// What removing does to them.
    pub sentence: &'static str,
    /// Where finished files are beside unfinished rows: what happens to
    /// the files.
    pub files_sentence: Option<&'static str>,
    /// Which one, by name; or how many.
    pub named: String,
    /// The choice to delete the finished files too, where there are any.
    pub toggle: Option<&'static str>,
    /// The button that removes.
    pub go: &'static str,
    /// The button that keeps them: the default, and Enter's.
    pub keep: &'static str,
}

/// What the rows asked about are: `rows` the rows still listed, `files`
/// how many finished files are still on the disk, `deleting_files` whether
/// the question is Delete file's.
pub fn remove(
    rows: &[&Entry],
    files: usize,
    remove_files: bool,
    deleting_files: bool,
    named: Option<String>,
) -> RemoveAsk {
    // A cancelled row has nothing left to lose: its parts went with the
    // cancel.
    let unfinished = rows
        .iter()
        .any(|r| !matches!(r.state, State::Completed | State::Cancelled));
    // Whether there is anything to throw away: a row added for later and
    // never started has fetched nothing.
    let fetched = rows.iter().any(|r| {
        !matches!(r.state, State::Completed | State::Cancelled) && r.progress.downloaded > 0
    });
    let count = rows.len();
    // Not "unfinished" of a set with finished files in it.
    let title_english = if deleting_files {
        "Delete file"
    } else if unfinished && count == 1 {
        "Remove an unfinished download"
    } else if unfinished && files == 0 {
        "Remove unfinished downloads"
    } else {
        "Remove from the list"
    };
    let title = match title_english {
        "Remove an unfinished download" => t("Remove an unfinished download"),
        "Remove unfinished downloads" => t("Remove unfinished downloads"),
        "Delete file" => t("Delete file"),
        _ => t("Remove from the list"),
    };
    // Whole sentences, one for each number, since languages do not build
    // them from the same pieces.
    let mixed = unfinished && files > 0;
    let sentence = match (unfinished, count) {
        (true, _) if mixed => t("Some downloads aren't finished. They lose what was downloaded."),
        (true, 1) if !fetched => t("Nothing was downloaded yet. Remove it from the list?"),
        (true, _) if !fetched => t("Nothing was downloaded yet. Remove them from the list?"),
        (true, 1) => t("This download isn't finished. Removing it discards what was downloaded."),
        (true, _) => {
            t("Some downloads aren't finished. Removing them discards what was downloaded.")
        }
        (false, 1) if remove_files => t("Its file is deleted too."),
        (false, _) if remove_files => t("Their files are deleted too."),
        // No file on the disk (cancelled, or since deleted): none to stay.
        (false, 1) if files == 0 => t("This removes it from the list."),
        (false, _) if files == 0 => t("This removes them from the list."),
        (false, 1) => t("Its file is kept."),
        (false, _) => t("Their files are kept."),
    };
    let files_sentence = mixed.then(|| match (remove_files, files) {
        (true, 1) => t("The finished file is deleted too."),
        (true, _) => t("The finished files are deleted too."),
        (false, 1) => t("The finished file is kept."),
        (false, _) => t("The finished files are kept."),
    });
    // Which one, by name: "this download" was all it said; and how many,
    // where it is more than one.
    let named = match named {
        Some(name) if count == 1 => name,
        _ => tf("Downloads: {}", &[&count]),
    };
    let (go, keep) = match (unfinished, count) {
        (true, 1) => (t("Yes, remove"), t("Keep it")),
        (true, _) => (t("Yes, remove"), t("Keep them")),
        (false, 1) => (t("Remove it"), t("Keep it")),
        (false, _) => (t("Remove them"), t("Keep them")),
    };
    // A finished file can go with its row, if asked for; never by default.
    let toggle = (files > 0).then(|| {
        if files == 1 {
            t("Also delete the file")
        } else {
            t("Also delete the files")
        }
    });
    let go = if toggle.is_some() && remove_files {
        t("Yes, delete")
    } else {
        go
    };
    RemoveAsk {
        title_english,
        title,
        sentence,
        files_sentence,
        named,
        toggle,
        go,
        keep,
    }
}

/// A question a download raises as it runs (B39 to B41): what it says, and
/// its buttons, the default first (Return's, and the one that changes
/// nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadAsk {
    /// The title in English, which the window's tests and the control port
    /// find it by.
    pub title_english: &'static str,
    /// The title, in the language in force.
    pub title: &'static str,
    /// Its sentences.
    pub lines: Vec<String>,
    /// What the server answered, shown as it came, where it says.
    pub detail: Option<&'static str>,
    /// Its buttons, the default first: verbs that say what each does (gaps row U31).
    pub buttons: Vec<String>,
}

/// B39: the server named the file `server_name`, the address `address_name`.
/// "Use" the server's name (the default) keeps it; "Keep" the address's saves it under that.
pub fn name_sent(address_name: &str, server_name: &str) -> DownloadAsk {
    DownloadAsk {
        title_english: "Name sent by the server",
        title: t("Name sent by the server"),
        lines: vec![tf(
            "The name to save the file was {}, but the server sent {}. Save the file with the name received from the server?",
            &[&address_name, &server_name],
        )],
        detail: None,
        buttons: vec![
            tf("Use {}", &[&server_name]),
            tf("Keep {}", &[&address_name]),
        ],
    }
}

/// B40: a web page answered for a file. "Save as HTML" saves the page as HTML; "Don't
/// download" removes the row and keeps nothing.
pub fn page_for_file() -> DownloadAsk {
    DownloadAsk {
        title_english: "Web page instead of the file",
        title: t("Web page instead of the file"),
        lines: vec![t(
            "The requested file looks like an HTML web page. Would you like to continue downloading it and save it as an HTML page?",
        )
        .to_string()],
        detail: None,
        buttons: vec![t("Save as HTML").into(), t("Don't download").into()],
    }
}

/// B41: the server said the file is not there; told once, OK.
pub fn not_found() -> DownloadAsk {
    DownloadAsk {
        title_english: "The file doesn't exist",
        title: t("The file doesn't exist"),
        lines: vec![
            t("The requested file doesn't exist.").to_string(),
            t("Details:").to_string(),
        ],
        detail: Some("HTTP/1.1 404 Not Found"),
        buttons: vec![t("OK").into()],
    }
}

/// "Exit dm?", asked when Exit is chosen while downloads run: what it says
/// with `busy` downloads running and `waiting` in line, and its buttons.
/// Asked again as the list changes: a download finishing while it is open
/// changes the words and which button is the default, never where each
/// button stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitAsk {
    /// The title in English, which the window's tests and the control port
    /// find it by.
    pub title_english: &'static str,
    /// The title, in the language in force.
    pub title: &'static str,
    /// What it says.
    pub sentence: String,
    /// Nothing runs any more: Exit is the default, and the other button
    /// says Stay.
    pub finished: bool,
    /// The button that exits.
    pub exit: &'static str,
    /// The button that keeps dm open: Keep downloading, or Stay.
    pub keep: &'static str,
}

/// The Exit question with `busy` downloads running and `waiting` in line.
pub fn exit(busy: usize, waiting: usize) -> ExitAsk {
    // Running and waiting counted apart: a queued row is not running, and
    // counting it as one told of downloads that were not.
    let finished = busy + waiting == 0;
    let mut said = Vec::new();
    if finished {
        said.push(t("Nothing is downloading any more. Exit dm?").to_string());
    }
    if busy > 0 {
        said.push(tn(
            "{} download is still running.||{} downloads are still running.",
            busy as u64,
            &[&busy],
        ));
    }
    if waiting > 0 && busy > 0 {
        said.push(tn(
            "{} more is waiting to start.||{} more are waiting to start.",
            waiting as u64,
            &[&waiting],
        ));
    } else if waiting > 0 {
        said.push(tn(
            "{} download is waiting to start.||{} downloads are waiting to start.",
            waiting as u64,
            &[&waiting],
        ));
    }
    if !finished {
        // As plural as the count it speaks of.
        said.push(tn(
            "Exiting pauses it. It picks up where it left off the next time you open dm.||Exiting pauses them. They pick up where they left off the next time you open dm.",
            (busy + waiting) as u64,
            &[],
        ));
    }
    ExitAsk {
        title_english: "Exit dm?",
        title: t("Exit dm?"),
        sentence: said.join(" "),
        finished,
        exit: t("Exit"),
        keep: if finished {
            t("Stay")
        } else {
            t("Keep downloading")
        },
    }
}

/// "Start outside the schedule?", asked when Resume or Start now meets
/// downloads the schedule or the download limit holds: start them now
/// anyway (this time only), or let them wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutsideAsk {
    /// The title in English, which the window's tests and the control port
    /// find it by.
    pub title_english: &'static str,
    /// The title, in the language in force.
    pub title: &'static str,
    /// Which wait: the one by name, or how many.
    pub sentence: String,
    /// What holds them, and until when, where that is known.
    pub why: Option<String>,
    /// The question itself.
    pub question: &'static str,
    /// The button that starts them: the default.
    pub start: &'static str,
    /// The button that lets them wait.
    pub wait: &'static str,
}

/// The question for the downloads named `names` (as the list shows them),
/// held because `why`.
pub fn outside(names: &[String], why: Option<String>) -> OutsideAsk {
    OutsideAsk {
        title_english: "Start outside the schedule?",
        title: t("Start outside the schedule?"),
        sentence: if names.len() == 1 {
            tf(
                "{} waits for the schedule or the download limit.",
                &[&names.join(", ")],
            )
        } else {
            tf(
                "{} downloads wait for the schedule or the download limit.",
                &[&names.len()],
            )
        },
        why: why.filter(|w| !w.is_empty()),
        question: t("Start it now? Only your selection starts."),
        start: t("Start now"),
        wait: t("Keep waiting"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: State, downloaded: u64) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "url": "http://e.com/f",
            "state": serde_json::to_value(state).unwrap(),
            "progress": {"downloaded": downloaded, "total": null, "speed": 0.0, "eta": null},
            "filename": "f",
        }))
        .unwrap()
    }

    #[test]
    fn an_unfinished_download_is_asked_about_by_name_keep_first() {
        let one = row(State::Paused, 10);
        let ask = remove(&[&one], 0, false, false, Some("film.mp4".into()));
        assert_eq!(ask.title_english, "Remove an unfinished download");
        assert!(ask.sentence.starts_with("This download isn't finished"));
        assert_eq!(ask.named, "film.mp4");
        assert_eq!((ask.go, ask.keep), ("Yes, remove", "Keep it"));
        assert_eq!(ask.toggle, None);
    }

    #[test]
    fn finished_files_offer_to_go_too_and_say_what_happens_to_them() {
        let (a, b) = (row(State::Completed, 5), row(State::Completed, 5));
        let kept = remove(&[&a, &b], 2, false, false, None);
        assert_eq!(kept.title_english, "Remove from the list");
        assert_eq!(kept.sentence, "Their files are kept.");
        assert_eq!(kept.toggle, Some("Also delete the files"));
        assert_eq!(kept.named, "Downloads: 2");
        assert_eq!(kept.go, "Remove them");
        let deleted = remove(&[&a, &b], 2, true, false, None);
        assert_eq!(deleted.sentence, "Their files are deleted too.");
        assert_eq!(deleted.go, "Yes, delete");
        let fresh = row(State::Queued, 0);
        let mixed = remove(&[&a, &fresh], 1, false, false, None);
        assert_eq!(mixed.files_sentence, Some("The finished file is kept."));
        assert_eq!(
            remove(&[&a], 1, false, true, None).title_english,
            "Delete file"
        );
    }

    #[test]
    fn the_questions_a_download_raises_say_the_contract_words() {
        let name = name_sent("dl", "Report 2026.pdf");
        assert_eq!(name.title_english, "Name sent by the server");
        assert_eq!(
            name.lines,
            vec!["The name to save the file was dl, but the server sent Report 2026.pdf. Save the file with the name received from the server?".to_string()]
        );
        assert_eq!(name.buttons, vec!["Use Report 2026.pdf", "Keep dl"]);
        let page = page_for_file();
        assert_eq!(page.title_english, "Web page instead of the file");
        assert_eq!(page.buttons, vec!["Save as HTML", "Don't download"]);
        let gone = not_found();
        assert_eq!(gone.title_english, "The file doesn't exist");
        assert_eq!(gone.detail, Some("HTTP/1.1 404 Not Found"));
        assert_eq!(gone.buttons, vec!["OK"]);
    }

    #[test]
    fn exit_says_what_runs_and_what_waits_and_stays_once_all_have_finished() {
        let both = exit(2, 1);
        assert_eq!(
            both.sentence,
            "2 downloads are still running. 1 more is waiting to start. Exiting pauses them. They pick up where they left off the next time you open dm."
        );
        assert!(!both.finished);
        assert_eq!((both.exit, both.keep), ("Exit", "Keep downloading"));
        let one = exit(0, 1);
        assert_eq!(
            one.sentence,
            "1 download is waiting to start. Exiting pauses it. It picks up where it left off the next time you open dm."
        );
        let done = exit(0, 0);
        assert!(done.finished);
        assert_eq!(done.sentence, "Nothing is downloading any more. Exit dm?");
        assert_eq!(done.keep, "Stay");
    }

    #[test]
    fn outside_names_the_one_or_counts_them_and_says_what_holds_them() {
        let one = outside(&["big.zip".into()], Some("Held by the schedule.".into()));
        assert_eq!(one.title_english, "Start outside the schedule?");
        assert_eq!(
            one.sentence,
            "big.zip waits for the schedule or the download limit."
        );
        assert_eq!(one.why.as_deref(), Some("Held by the schedule."));
        assert_eq!((one.start, one.wait), ("Start now", "Keep waiting"));
        let two = outside(&["a".into(), "b".into()], Some(String::new()));
        assert_eq!(
            two.sentence,
            "2 downloads wait for the schedule or the download limit."
        );
        assert_eq!(two.why, None);
    }
}
