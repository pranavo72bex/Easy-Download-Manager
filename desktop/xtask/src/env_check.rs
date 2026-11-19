// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask layers`' check of dm's `DM_` switches (gaps row A11): each is read through
//! `dm_core::env`, whose table names it and says what it is for. A text scan of the crates'
//! sources: no `DM_` variable read with `std::env` (or `env!`) outside that module, and no `DM_`
//! name written in a product file that the table does not hold.

use std::path::Path;

/// The table's file, from the repository's root.
const TABLE: &str = "desktop/crates/dm-core/src/env.rs";

/// The crates' source folders read, from the repository's root.
const SOURCES: &[&str] = &[
    "desktop/crates/dm-assets/src",
    "desktop/crates/dm-js/src",
    "desktop/crates/dm-media/src",
    "desktop/crates/dm-torrent/src",
    "desktop/crates/dm-net/src",
    "desktop/crates/dm-protocol/src",
    "desktop/crates/dm-safari/src",
    "desktop/crates/dm-core/src",
    "desktop/crates/dm-shell/src",
    "desktop/crates/dm-test-support/src",
    "desktop/crates/dm-desktop-base/src",
    "desktop/crates/dm-gpui/src",
    "browser/src",
];

/// What reads a `DM_` variable without the table: `std::env::var`, `var_os`, `env!` and
/// `option_env!`, with or without their paths. Each is a whole word, so a test's `set_var` and
/// `remove_var` are not reads.
const DIRECT_READS: &[&str] = &[
    "var(\"DM_",
    "var_os(\"DM_",
    "env!(\"DM_",
    "option_env!(\"DM_",
];

/// What is wrong with the `DM_` switches under `root`, a line each.
pub(crate) fn problems(root: &Path) -> Vec<String> {
    let mut wrong = Vec::new();
    let table = std::fs::read_to_string(root.join(TABLE)).unwrap_or_default();
    let names = table_names(&table);
    if names.is_empty() {
        wrong.push(format!("{TABLE} lists no `DM_` switch (gaps row A11)"));
    }
    for dir in SOURCES {
        for file in crate::walk(&root.join(dir)) {
            if file.extension().is_none_or(|e| e != "rs") || is_table(&file) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let relative = file.strip_prefix(root).unwrap_or(&file);
            let shown = relative.display();
            for (line, what) in direct_reads(&text) {
                wrong.push(format!(
                    "{shown}:{line} reads a `DM_` variable itself ({what}…): read it with \
                     `dm_core::env::get` or `get_os` (gaps row A11)"
                ));
            }
            if is_test(relative) {
                continue;
            }
            for (line, name) in switch_names(&text) {
                if !names.contains(&name) {
                    wrong.push(format!(
                        "{shown}:{line} names {name}, which `dm_core::env::SWITCHES` does not \
                         list: add it there with what it is for (gaps row A11)"
                    ));
                }
            }
        }
    }
    wrong
}

/// Whether `file` is the table's own.
fn is_table(file: &Path) -> bool {
    file.to_string_lossy().replace('\\', "/").ends_with(TABLE)
}

/// Whether `file` holds tests alone: `tests.rs`, `<what>_tests.rs`, or a file in a folder
/// called `tests` or `<what>_tests` (`file` is from the repository's root, so the folders above
/// it do not count).
fn is_test(file: &Path) -> bool {
    file.iter().filter_map(|part| part.to_str()).any(|part| {
        let part = part.strip_suffix(".rs").unwrap_or(part);
        part == "tests" || part.ends_with("_tests")
    })
}

/// The names the table's `name: "DM_…"` lines give.
fn table_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("name: \"DM_"))
        .filter_map(|rest| rest.split_once('"'))
        .map(|(rest, _)| format!("DM_{rest}"))
        .collect()
}

/// The lines of `text` (from 1) that read a `DM_` variable without the table, with the read's
/// start. Comments are passed over.
fn direct_reads(text: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let read = DIRECT_READS.iter().find(|r| {
            line.match_indices(**r).any(|(at, _)| {
                !line[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
            })
        });
        if let Some(what) = read {
            found.push((i + 1, *what));
        }
    }
    found
}

/// The `"DM_…"` string literals of `text` that are a name alone (capitals, digits and `_`),
/// each with its line (from 1). Comments are passed over, and so is a literal that only starts
/// with a name (`"DM_HOME is not set"`, `"DM_TEST_{}"`).
fn switch_names(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("\"DM_") {
            let after = &rest[at + 1..];
            let len = after
                .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
                .unwrap_or(after.len());
            if after[len..].starts_with('"') {
                found.push((i + 1, after[..len].to_string()));
            }
            rest = &after[len..];
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dm_variable_read_without_the_table_is_found() {
        let text = "let a = std::env::var(\"DM_HOME\");\n\
                    let b = std::env::var_os(\"DM_NO_SWARM\");\n\
                    // std::env::var(\"DM_HOME\") in a comment\n\
                    let c = option_env!(\"DM_BUILT\");\n\
                    let d = dm_core::env::get_os(\"DM_HOME\");\n\
                    let e = std::env::var_os(\"HOME\");\n\
                    std::env::set_var(\"DM_HOME\", dir); std::env::remove_var(\"DM_HOME\");\n\
                    let f = env!(\"DM_BUILT\");\n";
        let lines: Vec<usize> = direct_reads(text).iter().map(|(n, _)| *n).collect();
        assert_eq!(lines, [1, 2, 4, 8]);
    }

    #[test]
    fn a_name_alone_is_a_switch_and_a_sentence_or_a_pattern_is_not() {
        let text = "get(\"DM_HOME\"); set_var(\"DM_NO_SECRET_STORE\", \"1\");\n\
                    eprintln!(\"DM_JS_PLAYER not set: not run\");\n\
                    let key = format!(\"DM_TEST_{}\", name);\n\
                    // get(\"DM_IN_A_COMMENT\")\n\
                    let unknown = \"DM_2X\";\n";
        let found = switch_names(text);
        let names: Vec<(usize, &str)> = found.iter().map(|(n, s)| (*n, s.as_str())).collect();
        assert_eq!(
            names,
            [(1, "DM_HOME"), (1, "DM_NO_SECRET_STORE"), (5, "DM_2X")]
        );
    }

    #[test]
    fn the_table_gives_its_names_from_its_name_lines() {
        let text = "    Switch {\n        name: \"DM_HOME\",\n        purpose: \"A \\\n\
                    \"DM_ELSE\" is not a name.\",\n    },\n    name: \"DM_TEST_<NAME>\",\n";
        assert_eq!(table_names(text), ["DM_HOME", "DM_TEST_<NAME>"]);
    }

    #[test]
    fn a_test_file_or_folder_is_told_from_a_product_file() {
        assert!(is_test(Path::new("dm-core/src/ftp/tls_tests.rs")));
        assert!(is_test(Path::new("dm-core/src/secretstore/tools/tests.rs")));
        assert!(is_test(Path::new("dm-gpui/src/window_tests/fuzz.rs")));
        assert!(!is_test(Path::new("dm-shell/src/settings.rs")));
        assert!(!is_test(Path::new("dm-core/src/contest.rs")));
        assert!(is_table(Path::new(
            r"C:\dm\desktop\crates\dm-core\src\env.rs"
        )));
        assert!(!is_table(Path::new(
            "desktop/crates/dm-core/src/envelope.rs"
        )));
    }

    /// The tree as it is: every `DM_` read goes through the table.
    #[test]
    fn the_crates_read_every_dm_switch_through_the_table() {
        let wrong = problems(&crate::root());
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
