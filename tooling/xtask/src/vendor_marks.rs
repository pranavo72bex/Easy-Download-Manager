// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask layers`' check of the crates dm builds with a change of its own (gaps rows A17
//! and X5): each change in `vendor/` is marked `dm(<tag>):` in its code, and each tag is
//! a row of the one table in `vendor/README.md` (the change, its copy, why the other
//! systems do not need it, whether upstream has been told). A mark with no tag (`dm:`), a tag
//! with no row and a row no mark names are each a problem. A text scan of the copies' Rust and
//! TOML files and of the patches; the README itself is the table, and is not scanned for marks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The vendored crates and their patches, from the repository's root.
const VENDOR: &str = "vendor";

/// The file whose table names every change, from the repository's root.
const TABLE: &str = "vendor/README.md";

/// The kinds of file a mark is looked for in.
const EXTENSIONS: &[&str] = &["rs", "toml", "patch"];

/// What is wrong with the marks of the vendored crates under `root`, a line each.
pub(crate) fn problems(root: &Path) -> Vec<String> {
    let mut wrong = Vec::new();
    let table_path = root.join(TABLE);
    let table = std::fs::read_to_string(&table_path).unwrap_or_default();
    let rows = table_tags(&table);
    if rows.is_empty() {
        wrong.push(format!(
            "{TABLE} has no table of dm's changes (gaps rows A17, X5)"
        ));
    }
    let mut named = BTreeSet::new();
    for file in files(&root.join(VENDOR)) {
        if file == table_path {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let shown = file.strip_prefix(root).unwrap_or(&file).display();
        for (line, tag) in marks(&text) {
            match tag {
                None => wrong.push(format!(
                    "{shown}:{line} has a `dm:` mark with no tag: write `dm(<tag>):`, the tag \
                     its change's row in {TABLE} (gaps rows A17, X5)"
                )),
                Some(tag) => {
                    if !rows.contains(&tag) {
                        wrong.push(format!(
                            "{shown}:{line} is marked `dm({tag})`, which no row of {TABLE}'s \
                             table names: add the change's row (gaps rows A17, X5)"
                        ));
                    }
                    named.insert(tag);
                }
            }
        }
    }
    for tag in rows.difference(&named) {
        wrong.push(format!(
            "{TABLE}: the row `{tag}` names a change no `dm({tag}):` mark is on: mark it or \
             remove the row (gaps rows A17, X5)"
        ));
    }
    wrong
}

/// The files under `dir` a mark is looked for in. A build's `target/` and hidden folders are
/// not read.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                found.extend(files(&path));
            }
        } else if path
            .extension()
            .is_some_and(|e| EXTENSIONS.iter().any(|x| e == *x))
        {
            found.push(path);
        }
    }
    found
}

/// Whether `c` may be part of a tag: lowercase letters, digits and `-`.
fn is_tag_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

/// The tags of `text`'s table rows: a row whose first cell is a backticked tag.
fn table_tags(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix('|'))
        .filter_map(|row| row.split('|').next())
        .filter_map(|cell| cell.trim().strip_prefix('`')?.strip_suffix('`'))
        .filter(|tag| !tag.is_empty() && tag.chars().all(is_tag_char))
        .map(str::to_string)
        .collect()
}

/// The marks in `text`, each with its line (from 1) and its tag, `None` for an untagged `dm:`.
/// A mark is `dm(<tag>):` or `dm:` not inside a longer word; `dm::` (a path) is not one.
fn marks(text: &str) -> Vec<(usize, Option<String>)> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut from = 0;
        while let Some(at) = line[from..].find("dm").map(|i| from + i) {
            from = at + 2;
            let before = line[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let rest = &line[at + 2..];
            if rest.starts_with(':') && !rest.starts_with("::") {
                found.push((n + 1, None));
            } else if let Some(inner) = rest.strip_prefix('(') {
                let tag: String = inner.chars().take_while(|c| is_tag_char(*c)).collect();
                if !tag.is_empty() && inner[tag.len()..].starts_with("):") {
                    found.push((n + 1, Some(tag)));
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_and_untagged_marks_are_found_with_their_lines() {
        let text = "fn a() {}\n\
                    // dm(x11-a11y): AccessKit is told where the window is.\n\
                    # dm: a change with no tag\n\
                    /// dm(win-drag-out): files dragged out; dm(theme): two on a line\n";
        assert_eq!(
            marks(text),
            vec![
                (2, Some("x11-a11y".to_string())),
                (3, None),
                (4, Some("win-drag-out".to_string())),
                (4, Some("theme".to_string())),
            ]
        );
    }

    #[test]
    fn words_paths_and_placeholders_are_not_marks() {
        let text = "let admin: u8 = 0; // adm: not dm's\n\
                    use dm::core;\n\
                    handle_dm(x); fn dm(x: u8) {}\n\
                    each change is marked `dm(<tag>):` in its code\n\
                    const DM_POINTERHITTEST: u32 = 0;\n";
        assert_eq!(marks(text), Vec::new());
    }

    #[test]
    fn the_table_gives_the_tag_of_each_row() {
        let text = "Some prose with `mse` in it.\n\
                    | Tag | The change | Copy |\n\
                    | --- | --- | --- |\n\
                    | `x11-a11y` | AccessKit | gpui-pre-linux |\n\
                    | `mse` | Encryption | librqbit |\n\
                    | Not a tag | `theme` | elsewhere |\n";
        let tags: Vec<String> = table_tags(text).into_iter().collect();
        assert_eq!(tags, vec!["mse".to_string(), "x11-a11y".to_string()]);
    }

    #[test]
    fn a_patchs_added_line_is_a_mark() {
        let text = "+    // dm(names): every file's name checked first.\n \
                    unchanged context\n";
        assert_eq!(marks(text), vec![(1, Some("names".to_string()))]);
    }
}
