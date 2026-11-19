// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask help-check`: every sentence a person can see that the help should explain has
//! an entry in it (gaps row W9). The help is `docs/user-guide.md` and the pages of
//! `docs/guide/`; a sentence has its entry where it is written there word for word, with `…`
//! where the window puts a name or a number.
//!
//! The sentences are read from the source, so one added to dm is asked for at once:
//!
//! - each note a download may put on its row (`dm_core::NOTES`);
//! - each failure the engine says: the `#[error("…")]` of `dm_net::Error` and of an archive's
//!   `dm_core::extract::Error`, the plain words `network` and `file_error` put a failure in, and
//!   every sentence a failure is raised with (`Error::Internal`, `Error::Connection`) in
//!   `dm-core` and `dm-net`;
//! - every label, explanation and note of Options' pages (`dm-gpui/src/options/pages.rs` and
//!   the file of each page in `pages/`, the Dial Up / VPN section's among them).
//!
//! The sentence is matched without regard to case, spacing, line breaks, or Markdown's `*` and
//! backticks.

use std::collections::BTreeSet;
use std::path::Path;

/// The help's pages, from the repository's root: the guide and the folder of its pages.
const GUIDE: &str = "docs/user-guide.md";
const PAGES: &str = "docs/guide";

/// The file that holds `dm_core::NOTES`.
const NOTES: &str = "desktop/crates/dm-core/src/download.rs";
/// The files whose error types' `#[error("…")]` a person reads.
const ERROR_TYPES: [&str; 2] = [
    "desktop/crates/dm-net/src/error.rs",
    "desktop/crates/dm-core/src/extract.rs",
];
/// The file whose functions put a failure in plain words, and those functions.
const PLAIN_WORDS: (&str, [&str; 2]) = (
    "desktop/crates/dm-net/src/error.rs",
    ["fn network(", "fn file_error("],
);
/// The engine's folders, whose raised failures are read.
const ENGINE: [&str; 2] = ["desktop/crates/dm-core/src", "desktop/crates/dm-net/src"];
/// What a failure is raised with, as the code before its sentence reads (spaces left out).
const RAISED: [&str; 3] = [
    "Error::Internal(",
    "Error::Internal(format!(",
    "Error::Connection(format!(",
];
/// Options' pages: the file of what they share, and the folder of a file each.
const OPTIONS: &str = "desktop/crates/dm-gpui/src/options/pages.rs";
/// The folder of Options' pages, a file each.
const OPTIONS_PAGES: &str = "desktop/crates/dm-gpui/src/options/pages";

/// A sentence a person can see, and where it is written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Sentence {
    /// As the source writes it, `{…}` and all.
    pub(crate) text: String,
    /// The file and line it is written at.
    pub(crate) at: String,
}

/// `cargo xtask help-check`: fails naming each sentence with no entry.
pub(crate) fn run(root: &Path) -> Result<String, String> {
    let wanted = sentences(root)?;
    let help = help_text(root)?;
    let missing = missing(&wanted, &help);
    if missing.is_empty() {
        return Ok(format!(
            "each of the {} sentences a person can see has an entry in the help",
            wanted.len()
        ));
    }
    let lines: Vec<String> = missing
        .iter()
        .map(|s| format!("  {}  ({})", s.text, s.at))
        .collect();
    Err(format!(
        "{} sentences a person can see have no entry in the help ({GUIDE} and {PAGES}/); \
         write each there word for word, with … for a name or a number:\n{}",
        missing.len(),
        lines.join("\n")
    ))
}

/// The help's text: the guide and every Markdown page of its folder.
pub(crate) fn help_text(root: &Path) -> Result<String, String> {
    let mut text = read(&root.join(GUIDE))?;
    let dir = root.join(PAGES);
    let mut pages: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    pages.sort();
    for page in pages {
        text.push('\n');
        text.push_str(&read(&page)?);
    }
    Ok(text)
}

/// The sentences of `wanted` that `help` has no entry for, each once.
pub(crate) fn missing(wanted: &[Sentence], help: &str) -> Vec<Sentence> {
    let help = normal(&help.replace(['*', '`'], ""));
    let mut seen = BTreeSet::new();
    wanted
        .iter()
        .filter(|s| seen.insert(normal(&s.text)))
        .filter(|s| !help.contains(&normal(&s.text)))
        .cloned()
        .collect()
}

/// Every sentence the help should explain, read from the source under `root`.
pub(crate) fn sentences(root: &Path) -> Result<Vec<Sentence>, String> {
    let mut found = Vec::new();
    // The notes: the literals of the array.
    let text = read(&root.join(NOTES))?;
    let from = text
        .find("pub const NOTES")
        .ok_or(format!("{NOTES} has no `pub const NOTES`"))?;
    let end = text[from..]
        .find("];")
        .map(|e| from + e)
        .ok_or(format!("{NOTES}: NOTES has no end"))?;
    let first = line_of(&text, from);
    for lit in literals(&text[from..end]) {
        found.push(at(lit.text, NOTES, first + lit.line - 1));
    }
    // The error types' words.
    for file in ERROR_TYPES {
        let text = read(&root.join(file))?;
        for lit in literals(&shipped(&text)) {
            if lit.before.ends_with("#[error(") {
                found.push(at(lit.text, file, lit.line));
            }
        }
    }
    // The plain words a failure is put in.
    let (file, functions) = PLAIN_WORDS;
    let text = read(&root.join(file))?;
    for function in functions {
        let (body, first) =
            function_body(&text, function).ok_or(format!("{file} has no `{function}` any more"))?;
        for lit in literals(body) {
            let b = &lit.before;
            if b.ends_with('{') || b.ends_with("=>") || b.ends_with("format!(") {
                found.push(at(lit.text, file, first + lit.line - 1));
            }
        }
    }
    // The failures raised in the engine.
    for dir in ENGINE {
        let mut files = rust_files(&root.join(dir));
        files.sort();
        for path in files {
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = read(&path)?;
            for lit in literals(&shipped(&text)) {
                if RAISED.iter().any(|r| lit.before.ends_with(r)) {
                    found.push(at(lit.text, &shown, lit.line));
                }
            }
        }
    }
    // Options' words: what the pages share, then each page's file.
    let mut pages = rust_files(&root.join(OPTIONS_PAGES));
    pages.sort();
    let mut files = vec![OPTIONS.to_string()];
    files.extend(pages.iter().map(|path| {
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }));
    for file in &files {
        let text = read(&root.join(file))?;
        for lit in literals(&shipped(&text)) {
            if lit.text.contains(' ') || lit.text.chars().any(|c| c.is_uppercase()) {
                let text = lit
                    .text
                    .strip_prefix("key:")
                    .unwrap_or(&lit.text)
                    .to_string();
                found.push(at(text, file, lit.line));
            }
        }
    }
    found.retain(|s| says_something(&s.text));
    Ok(found)
}

fn at(text: String, file: &str, line: usize) -> Sentence {
    Sentence {
        text,
        at: format!("{file}:{line}"),
    }
}

/// Whether `text` has words of its own once its `{…}` are taken out: "{}: {e}" or "{why}" is
/// another sentence passed on, and "{}%" a number.
fn says_something(text: &str) -> bool {
    let mut letters = 0;
    for c in normal(text).chars() {
        if c.is_alphabetic() {
            letters += 1;
            if letters >= 2 {
                return true;
            }
        } else {
            letters = 0;
        }
    }
    false
}

/// `text` as it is compared: each `{…}` an ellipsis, ellipses side by side one, spaces and line
/// breaks one space, curly apostrophes straight, lower case.
pub(crate) fn normal(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                }
                out.push('…');
            }
            '’' => out.push('\''),
            c if c.is_whitespace() => out.push(' '),
            c => out.extend(c.to_lowercase()),
        }
    }
    let mut squeezed = String::new();
    let mut last = None;
    for c in out.chars() {
        if (c == ' ' || c == '…') && last == Some(c) {
            continue;
        }
        // "… …" from "{a} {b}" is one name.
        if c == '…' && squeezed.ends_with("… ") {
            squeezed.pop();
            last = Some('…');
            continue;
        }
        squeezed.push(c);
        last = Some(c);
    }
    squeezed.trim().to_string()
}

/// What of `text` ships: each item under `#[cfg(test)]` (a module, a function, a `use`) blanked
/// out, line for line, so the lines of the rest keep their numbers. An item ends on its first
/// line where that ends in `;`, else at the `}` that rustfmt puts at its own indent.
pub(crate) fn shipped(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() != "#[cfg(test)]" {
            kept.push(lines[i]);
            i += 1;
            continue;
        }
        let indent = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
        let close = format!("{indent}}}");
        kept.push("");
        i += 1;
        // Its other attributes.
        while i < lines.len() && lines[i].trim_start().starts_with("#[") {
            kept.push("");
            i += 1;
        }
        let one_line = lines.get(i).is_some_and(|l| l.trim_end().ends_with(';'));
        while i < lines.len() {
            let end = one_line || lines[i].trim_end() == close;
            kept.push("");
            i += 1;
            if end {
                break;
            }
        }
    }
    kept.join("\n")
}

/// The body of the function that `head` begins (`fn network(`), to the first `}` alone at the
/// start of a line, as rustfmt closes a function; with the line (from 1) it begins at.
fn function_body<'a>(text: &'a str, head: &str) -> Option<(&'a str, usize)> {
    let from = text.find(head)?;
    let end = text[from..].find("\n}\n").map_or(text.len(), |e| from + e);
    Some((&text[from..end], line_of(text, from)))
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The Rust files under `dir`, passing over tests' files (`tests.rs`, `*_tests.rs`, a `tests/`).
fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" {
                found.extend(rust_files(&path));
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            found.push(path);
        }
    }
    found
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/* ---------------------------------------------------------- the literals */

/// A string literal of Rust source: its value, the line (from 1) it begins on, and the code
/// just before it with the spaces left out (a literal before it reads as `""`).
#[derive(Debug, PartialEq)]
pub(crate) struct Literal {
    pub(crate) text: String,
    pub(crate) line: usize,
    pub(crate) before: String,
}

/// How much of the code before a literal is kept.
const BEFORE: usize = 48;

/// The string literals of `source`, in order: comments, character literals and lifetimes passed
/// over, escapes read (a `\` at a line's end joins it to the next line's words), raw strings
/// as they are. Byte strings are not words and are left out.
pub(crate) fn literals(source: &str) -> Vec<Literal> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut code = String::new();
    let mut line = 1;
    let mut i = 0;
    let keep = |code: &mut String| {
        let n = code.chars().count();
        if n > BEFORE * 2 {
            *code = code.chars().skip(n - BEFORE).collect();
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if c == '\'' {
            // A character literal ('"', '\'', 'a') or a lifetime ('a, 'static).
            if next == Some('\\') {
                i += 2;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
            code.push('\'');
        } else if let Some((hashes, start)) = raw_start(&chars, i) {
            let byte = c == 'b';
            let begun = line;
            let mut text = String::new();
            let mut j = start;
            while j < chars.len() {
                if chars[j] == '"' && (0..hashes).all(|h| chars.get(j + 1 + h) == Some(&'#')) {
                    break;
                }
                if chars[j] == '\n' {
                    line += 1;
                }
                text.push(chars[j]);
                j += 1;
            }
            i = j + 1 + hashes;
            if !byte {
                found.push(Literal {
                    text,
                    line: begun,
                    before: code.clone(),
                });
            }
            code.push_str("\"\"");
        } else if c == '"' || (c == 'b' && next == Some('"') && !word_before(&chars, i)) {
            let byte = c == 'b';
            let begun = line;
            i += if byte { 2 } else { 1 };
            let mut text = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    let escaped = chars.get(i + 1).copied().unwrap_or('\\');
                    i += 2;
                    match escaped {
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        'r' => text.push('\r'),
                        '0' => text.push('\0'),
                        // A Windows checkout's `\r\n` after it as well.
                        end @ ('\n' | '\r') => {
                            if end == '\n' {
                                line += 1;
                            }
                            while i < chars.len() && chars[i].is_whitespace() {
                                if chars[i] == '\n' {
                                    line += 1;
                                }
                                i += 1;
                            }
                        }
                        'u' => {
                            let mut hex = String::new();
                            while i < chars.len() && chars[i] != '}' {
                                if chars[i] != '{' {
                                    hex.push(chars[i]);
                                }
                                i += 1;
                            }
                            i += 1;
                            if let Some(ch) =
                                u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                            {
                                text.push(ch);
                            }
                        }
                        'x' => {
                            let hex: String = chars[i..chars.len().min(i + 2)].iter().collect();
                            i += 2;
                            if let Some(ch) =
                                u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                            {
                                text.push(ch);
                            }
                        }
                        other => text.push(other),
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    text.push(chars[i]);
                    i += 1;
                }
            }
            i += 1;
            if !byte {
                found.push(Literal {
                    text,
                    line: begun,
                    before: code.clone(),
                });
            }
            code.push_str("\"\"");
        } else {
            code.push(c);
            i += 1;
        }
        keep(&mut code);
    }
    found
}

/// Whether a letter, digit or `_` stands just before `i`: `ab"` is no byte string's start.
fn word_before(chars: &[char], i: usize) -> bool {
    i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')
}

/// Where a raw string begins at `i` (`r"`, `r#"`, `br#"`): its count of `#` and where its
/// words start.
fn raw_start(chars: &[char], i: usize) -> Option<(usize, usize)> {
    if word_before(chars, i) {
        return None;
    }
    let mut j = i;
    if chars.get(j) == Some(&'b') {
        j += 1;
    }
    if chars.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let mut hashes = 0;
    while chars.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    (chars.get(j) == Some(&'"')).then_some((hashes, j + 1))
}

#[cfg(test)]
mod tests;
