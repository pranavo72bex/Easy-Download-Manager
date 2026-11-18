// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;
use crate::tests::Scratch;

fn texts(found: &[Literal]) -> Vec<&str> {
    found.iter().map(|l| l.text.as_str()).collect()
}

#[test]
fn literals_are_read_past_comments_characters_and_lifetimes() {
    let source = "// \"not this\"\n/* nor \"this\" /* nested */ */\nfn f<'a>(x: &'a str) -> char {\n    let q = '\"';\n    let e = '\\'';\n    g(\"one\", r#\"two \"quoted\"\"#, b\"bytes\", br\"raw bytes\");\n    '}'\n}\n";
    let found = literals(source);
    assert_eq!(texts(&found), ["one", "two \"quoted\""]);
    assert_eq!(found[0].line, 6);
    assert!(found[0].before.ends_with("g("), "{:?}", found[0].before);
    assert!(
        found[1].before.ends_with("g(\"\","),
        "{:?}",
        found[1].before
    );
}

#[test]
fn escapes_and_a_line_joined_by_a_backslash_are_read_as_the_compiler_reads_them() {
    let source = "x(\"a \\\"b\\\" c\\n\\u{2026}\\x41\");\ny(\"how \\\n        long\");\nz(\"after\");\nw(\"crlf \\\r\n    too\");\n";
    let found = literals(source);
    assert_eq!(
        texts(&found),
        ["a \"b\" c\n…A", "how long", "after", "crlf too"]
    );
    assert_eq!(found[2].line, 4);
    assert_eq!(found[3].line, 5);
}

#[test]
fn what_is_under_cfg_test_does_not_ship_and_the_rest_keeps_its_lines() {
    let text = "a
#[cfg(test)]
use x::y;
b
    #[cfg(test)]
    #[allow(dead_code)]
    fn t(
        z: u8,
    ) {
        q(\"{\");
    }
    c
#[cfg(test)]
mod tests {
    fn d() {}
}
e
";
    let kept = shipped(text);
    let lines: Vec<&str> = kept.lines().collect();
    assert_eq!(lines.len(), 17);
    assert_eq!(lines[0], "a");
    assert_eq!(lines[3], "b");
    assert_eq!(lines[11], "    c");
    assert_eq!(lines[16], "e");
    assert!(!kept.contains("fn t") && !kept.contains("fn d") && !kept.contains("x::y"));
}

#[test]
fn a_sentence_is_compared_with_its_names_as_an_ellipsis_whatever_its_case_and_spacing() {
    assert_eq!(
        normal("The server answered {} for {url}{}"),
        "the server answered … for …"
    );
    assert_eq!(normal("{a} {b} left\n  out"), "… left out");
    assert_eq!(normal("dm’s  own"), "dm's own");
    assert!(says_something("not enough room: {}"));
    assert!(!says_something("{}: {e}"));
    assert!(!says_something("{why}"));
    assert!(!says_something("{}%"));
}

#[test]
fn a_sentence_has_its_entry_where_the_help_writes_it_word_for_word() {
    let wanted = |text: &str| Sentence {
        text: text.to_string(),
        at: "x.rs:1".to_string(),
    };
    let help =
        "| `The server answered … for …` | It\nrefused. |\n**Connections per\nfile** sets it.";
    let all = [
        wanted("the server answered {} for {url}{}"),
        wanted("Connections per file"),
        wanted("Connections per file"),
        wanted("the disk is full"),
        wanted("the disk is full"),
    ];
    let missing = missing(&all, help);
    assert_eq!(missing, [wanted("the disk is full")]);
}

/// A repository of the files the sentences are read from, small.
fn fixture() -> Scratch {
    let dir = Scratch::new("help");
    let root = &dir.0;
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        NOTES,
        "/// \"a comment\"\npub const NOTES: [&str; 2] = [\n    \"a note\",\n    \"{} pieces were gone\",\n];\nconst OTHER: &str = \"not a note\";\n",
    );
    write(
        ERROR_TYPES[0],
        "pub enum Error {\n    #[error(\"the server answered {} for {url}\", x)]\n    Refused,\n    #[error(\"{0}\")]\n    Internal(String),\n}\n\nfn network(e: &E) -> String {\n    let plain = if all.contains(\"dns error\") {\n        \"the name was not found\"\n    } else {\n        return format!(\"network: {}\", e);\n    };\n    plain.into()\n}\n\npub fn file_error(e: &E) -> String {\n    match e {\n        NotFound => \"it is not there\".into(),\n        _ => said[..at].to_string(),\n    }\n}\n",
    );
    write(
        ERROR_TYPES[1],
        "pub enum Error {\n    #[error(\"the archive was stopped\")]\n    Cancelled,\n}\n",
    );
    write(
        "desktop/crates/dm-core/src/a.rs",
        "fn f() {\n    Err(Error::Internal(\"the page took too long\".into()))?;\n    Err(Error::Internal(\n        format!(\"{} did not answer\", host),\n    ))?;\n    Err(Error::Connection(format!(\"the FTP server said {code}\")))?;\n    Err(Error::Internal(why))?;\n    log(\"not raised\");\n}\n#[cfg(test)]\nmod tests { fn t() { Error::Internal(\"a test's\".into()); } }\n",
    );
    write(
        "desktop/crates/dm-core/src/a/tests.rs",
        "Error::Internal(\"in a test file\".into());\n",
    );
    write(
        "desktop/crates/dm-net/src/b.rs",
        "Error::Internal(format!(\"{why}\"))\n",
    );
    write(
        OPTIONS[0],
        "fn general() {\n    check(p, cx, \"start-on-login\", t(\"Start dm when I sign in\"), on);\n    p.hint(t(\"Lets other programs add downloads.\"));\n    let k = t(\"key:Shift\");\n    let id = format!(\"browse-{key}\");\n    let pct = tf(\"{}%\", &[&n]);\n}\n",
    );
    write(
        OPTIONS[1],
        "fn dialup() {\n    p.section(t(\"Dial Up / VPN\"));\n}\n",
    );
    dir
}

#[test]
fn the_sentences_are_read_from_the_notes_the_errors_the_engine_and_options() {
    let dir = fixture();
    let found = sentences(&dir.0).unwrap();
    let said: Vec<&str> = found.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        said,
        [
            "a note",
            "{} pieces were gone",
            "the server answered {} for {url}",
            "the archive was stopped",
            "the name was not found",
            "network: {}",
            "it is not there",
            "the page took too long",
            "{} did not answer",
            "the FTP server said {code}",
            "Start dm when I sign in",
            "Lets other programs add downloads.",
            "Shift",
            "Dial Up / VPN",
        ]
    );
    // Where each is written, to find it from the failure.
    assert_eq!(found[0].at, format!("{NOTES}:3"));
    assert_eq!(found[8].at, "desktop/crates/dm-core/src/a.rs:4");
}

#[test]
fn the_check_names_each_sentence_the_help_lacks_and_passes_once_it_has_them() {
    let dir = fixture();
    let root = &dir.0;
    std::fs::create_dir_all(root.join(PAGES)).unwrap();
    std::fs::write(root.join(GUIDE), "# Using dm\n\n`a note`\n").unwrap();
    let why = run(root).unwrap_err();
    assert!(why.contains("13 sentences"), "{why}");
    assert!(
        why.contains("the page took too long  (desktop/crates/dm-core/src/a.rs:2)"),
        "{why}"
    );
    assert!(!why.contains("  a note "), "{why}");
    let all = sentences(root)
        .unwrap()
        .iter()
        .map(|s| format!("- {}\n", s.text.replace("{}", "…")))
        .collect::<String>()
        .replace("{url}", "…")
        .replace("{code}", "…");
    std::fs::write(root.join(PAGES).join("failures.md"), all).unwrap();
    // A page that is not Markdown is not the help.
    std::fs::write(
        root.join(PAGES).join("notes.txt"),
        "the archive was stopped",
    )
    .unwrap();
    assert!(run(root).is_ok(), "{:?}", run(root));
}

/// The real help, read by `cargo test` and so by `cargo xtask check`: every sentence a person
/// can see that the help should explain has its entry there (gaps row W9).
#[test]
fn every_sentence_a_person_can_see_has_an_entry_in_the_help() {
    if let Err(why) = run(&crate::root()) {
        panic!("{why}");
    }
}
