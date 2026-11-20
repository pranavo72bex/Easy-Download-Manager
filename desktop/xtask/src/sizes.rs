// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The size gate (gaps row A12), part of `cargo xtask layers`: a file that grows into a
//! monolith, or a function that grows into a page, is split before it is changed again.
//!
//! - A product file may have [`FILE_LIMIT`] lines, and a function [`FN_LIMIT`], counted from
//!   the line of its `fn` to the line of its closing brace.
//! - Those longer when the gate came in are named, with their lengths then, in two baselines
//!   (`desktop/xtask/size-baseline.txt` and `fn-baseline.txt`). One named there may not grow
//!   past its entry; when it shrinks the check says so, and `cargo xtask size-baseline` lowers
//!   the entries to the lengths now. Nothing ever raises an entry or adds one.
//! - No file passes [`FILE_CEILING`], named in the baseline or not.
//!
//! Lines are counted as `str::lines` counts them. A function's end is found by matching its
//! braces in the code with its comments, strings and character literals blanked, so a brace
//! inside one of those is not counted; a lifetime (`'a`) is code.

use std::collections::BTreeMap;
use std::path::Path;

/// The source folders of dm's product crates, whose `.rs` files the gate measures; a test
/// file of its own (`tests.rs`, `*_tests.rs`) is not measured.
pub(crate) const PRODUCT_SOURCES: [&str; 15] = [
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
    "desktop/crates/dm-view/src",
    "desktop/crates/dm-desktop-base/src",
    "desktop/crates/dm-gpui/src",
    "desktop/crates/dm-android/src",
    "browser/src",
];

/// The most lines a product file the file baseline does not name may have.
pub(crate) const FILE_LIMIT: usize = 800;

/// The most lines any product file may have, named in the baseline or not.
pub(crate) const FILE_CEILING: usize = 2500;

/// The most lines a function the function baseline does not name may have.
pub(crate) const FN_LIMIT: usize = 150;

/// The files longer than [`FILE_LIMIT`] when the gate came in, from the repository's root.
const FILE_BASELINE: &str = "desktop/xtask/size-baseline.txt";

/// The functions longer than [`FN_LIMIT`] when the gate came in, from the repository's root.
const FN_BASELINE: &str = "desktop/xtask/fn-baseline.txt";

/// What heads [`FILE_BASELINE`]: what it is and how it changes.
const FILE_HEADER: &str = "\
# The size gate's file baseline (gaps row A12; `cargo xtask layers`, xtask/src/sizes.rs).
# Each product file that was over 800 lines when the gate came in, with its lines then:
# `<path> <lines>`. A file named here may not grow past its entry, any other may not pass
# 800 lines, and none may pass 2,500. An entry is only ever lowered or removed:
# `cargo xtask size-baseline` does both from the files as they are now.
";

/// What heads [`FN_BASELINE`]: what it is and how it changes.
const FN_HEADER: &str = "\
# The size gate's function baseline (gaps row A12; `cargo xtask layers`, xtask/src/sizes.rs).
# Each product function that was over 150 lines when the gate came in, from the line of its
# `fn` to the line of its closing brace, with its lines then: `<path> <name> <lines>`. A
# function named here may not grow past its entry, any other may not pass 150 lines. An
# entry is only ever lowered or removed: `cargo xtask size-baseline` does both.
";

/// Lengths in lines: of files, and of the functions longer than [`FN_LIMIT`]. What was
/// measured, or what a baseline allows.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Sizes {
    /// Each file's lines, by its path from the repository's root, with `/` between folders.
    pub(crate) files: BTreeMap<String, usize>,
    /// The lengths of the functions of one name in one file, by path and name.
    pub(crate) functions: BTreeMap<(String, String), Vec<usize>>,
}

impl Sizes {
    /// Records one file: its lines, and those of its functions longer than [`FN_LIMIT`].
    pub(crate) fn add(&mut self, shown: &str, text: &str) {
        for (name, lines) in functions(text) {
            if lines > FN_LIMIT {
                self.functions
                    .entry((shown.to_string(), name))
                    .or_default()
                    .push(lines);
            }
        }
        self.files.insert(shown.to_string(), text.lines().count());
    }
}

/// What the gate finds: what fails it, and what only asks for a baseline to be lowered.
#[derive(Default, Debug)]
pub(crate) struct Verdict {
    /// A file or function too long: each fails `cargo xtask layers`.
    pub(crate) wrong: Vec<String>,
    /// An entry of a baseline that can be lowered or removed; nothing fails for it.
    pub(crate) notes: Vec<String>,
}

/// The gate as `cargo xtask layers` runs it: what is too long, each a sentence. What only asks
/// for a baseline to be lowered is printed to the error stream and fails nothing.
pub(crate) fn check(root: &Path) -> Result<Vec<String>, String> {
    let baseline = baselines(root)?;
    let verdict = judge(&measure(root), &baseline);
    for note in &verdict.notes {
        eprintln!("note: {note}");
    }
    Ok(verdict.wrong)
}

/// `cargo xtask size-baseline`: each entry of the baselines lowered to the length now, and
/// those no longer over the limit removed. Nothing is raised or added.
pub(crate) fn lower(root: &Path) -> Result<String, String> {
    let baseline = baselines(root)?;
    let lowered = lowered(&measure(root), &baseline);
    let (files, functions) = render(&lowered);
    for (name, text) in [(FILE_BASELINE, files), (FN_BASELINE, functions)] {
        let path = root.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let count = |s: &Sizes| {
        (
            s.files.len(),
            s.functions.values().map(Vec::len).sum::<usize>(),
        )
    };
    let (files_then, functions_then) = count(&baseline);
    let (files_now, functions_now) = count(&lowered);
    Ok(format!(
        "{FILE_BASELINE} names {files_now} files (was {files_then}), {FN_BASELINE} \
         {functions_now} functions (was {functions_then}); no entry was raised"
    ))
}

/// Every product file measured: the `.rs` files under [`PRODUCT_SOURCES`] but test files.
fn measure(root: &Path) -> Sizes {
    let mut measured = Sizes::default();
    for dir in PRODUCT_SOURCES {
        for file in crate::walk(&root.join(dir)) {
            if file.extension().is_none_or(|e| e != "rs") || crate::is_test_file(&file) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let shown = file.strip_prefix(root).unwrap_or(&file).to_string_lossy();
            measured.add(&shown.replace('\\', "/"), &text);
        }
    }
    measured
}

/// The two baselines, read from the repository.
fn baselines(root: &Path) -> Result<Sizes, String> {
    let files = crate::read(&root.join(FILE_BASELINE))?;
    let functions = crate::read(&root.join(FN_BASELINE))?;
    parse(&files, &functions)
}

/// The baselines from their texts: `<path> <lines>` and `<path> <name> <lines>` a line, with
/// blank lines and those starting with `#` passed over.
pub(crate) fn parse(files: &str, functions: &str) -> Result<Sizes, String> {
    let mut sizes = Sizes::default();
    for (mut words, lines) in entries(files, 1, FILE_BASELINE)? {
        sizes.files.insert(words.remove(0), lines);
    }
    for (mut words, lines) in entries(functions, 2, FN_BASELINE)? {
        let name = words.remove(1);
        let path = words.remove(0);
        sizes.functions.entry((path, name)).or_default().push(lines);
    }
    Ok(sizes)
}

/// Each entry of a baseline: its `words` words and the count after them.
fn entries(text: &str, words: usize, shown: &str) -> Result<Vec<(Vec<String>, usize)>, String> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let count = parts.last().and_then(|c| c.parse::<usize>().ok());
        match count {
            Some(count) if parts.len() == words + 1 => {
                found.push((
                    parts[..words].iter().map(|w| w.to_string()).collect(),
                    count,
                ));
            }
            _ => {
                return Err(format!(
                    "{shown}:{}: `{line}` is not {words} words and a count of lines",
                    n + 1
                ));
            }
        }
    }
    Ok(found)
}

/// The baselines' texts, each its header and then its entries in order.
pub(crate) fn render(sizes: &Sizes) -> (String, String) {
    let mut files = FILE_HEADER.to_string();
    for (path, lines) in &sizes.files {
        files.push_str(&format!("{path} {lines}\n"));
    }
    let mut functions = FN_HEADER.to_string();
    for ((path, name), lengths) in &sizes.functions {
        let mut lengths = lengths.clone();
        lengths.sort_unstable();
        for lines in lengths {
            functions.push_str(&format!("{path} {name} {lines}\n"));
        }
    }
    (files, functions)
}

/// The lengths of `measured`, longest first, each beside the longest entry of `allowed` not yet
/// paired, so that two functions of one name in one file each have an entry of their own.
fn paired(measured: &[usize], allowed: Option<&Vec<usize>>) -> Vec<(usize, Option<usize>)> {
    let mut measured = measured.to_vec();
    measured.sort_unstable();
    measured.reverse();
    let mut allowed = allowed.cloned().unwrap_or_default();
    allowed.sort_unstable();
    allowed.reverse();
    measured
        .iter()
        .enumerate()
        .map(|(i, &lines)| (lines, allowed.get(i).copied()))
        .collect()
}

/// What is too long in `measured` against `baseline`, and which entries can be lowered.
pub(crate) fn judge(measured: &Sizes, baseline: &Sizes) -> Verdict {
    let mut verdict = Verdict::default();
    let lower = "`cargo xtask size-baseline` lowers it";
    for (path, &lines) in &measured.files {
        let allowed = baseline.files.get(path).copied();
        if lines > FILE_CEILING {
            verdict.wrong.push(format!(
                "{path} is {lines} lines: split it into modules (no file may pass {FILE_CEILING})"
            ));
            continue;
        }
        match allowed {
            None if lines > FILE_LIMIT => verdict.wrong.push(format!(
                "{path} is {lines} lines: split it into modules (the limit is {FILE_LIMIT})"
            )),
            Some(was) if lines > was => verdict.wrong.push(format!(
                "{path} is {lines} lines, more than its {was} in {FILE_BASELINE}: a file over \
                 {FILE_LIMIT} lines may only shrink, so split it into modules"
            )),
            Some(was) if lines < was => verdict.notes.push(format!(
                "{path} is down to {lines} lines from {was}: lower its entry in \
                 {FILE_BASELINE} ({lower})"
            )),
            _ => {}
        }
    }
    for path in baseline.files.keys() {
        if !measured.files.contains_key(path) {
            verdict.notes.push(format!(
                "{path} is gone: remove its entry from {FILE_BASELINE} ({lower})"
            ));
        }
    }
    for ((path, name), lengths) in &measured.functions {
        let allowed = baseline.functions.get(&(path.clone(), name.clone()));
        for (lines, was) in paired(lengths, allowed) {
            match was {
                None => verdict.wrong.push(format!(
                    "{path}: fn {name} is {lines} lines: split it (the limit is {FN_LIMIT})"
                )),
                Some(was) if lines > was => verdict.wrong.push(format!(
                    "{path}: fn {name} is {lines} lines, more than its {was} in {FN_BASELINE}: \
                     a function over {FN_LIMIT} lines may only shrink, so split it"
                )),
                Some(was) if lines < was => verdict.notes.push(format!(
                    "{path}: fn {name} is down to {lines} lines from {was}: lower its entry in \
                     {FN_BASELINE} ({lower})"
                )),
                Some(_) => {}
            }
        }
    }
    for ((path, name), entries) in &baseline.functions {
        let now = measured
            .functions
            .get(&(path.clone(), name.clone()))
            .map_or(0, Vec::len);
        if entries.len() > now {
            verdict.notes.push(format!(
                "{path}: fn {name} is no longer over {FN_LIMIT} lines, or gone: remove its \
                 entry from {FN_BASELINE} ({lower})"
            ));
        }
    }
    verdict
}

/// `baseline` with each entry lowered to what `measured` has now, and those no longer over the
/// limit removed. No entry is raised and none is added.
pub(crate) fn lowered(measured: &Sizes, baseline: &Sizes) -> Sizes {
    let mut sizes = Sizes::default();
    for (path, &was) in &baseline.files {
        if let Some(&lines) = measured.files.get(path).filter(|&&l| l > FILE_LIMIT) {
            sizes.files.insert(path.clone(), lines.min(was));
        }
    }
    for (key, entries) in &baseline.functions {
        let Some(lengths) = measured.functions.get(key) else {
            continue;
        };
        let kept: Vec<usize> = paired(lengths, Some(entries))
            .into_iter()
            .filter_map(|(lines, was)| was.map(|was| lines.min(was)))
            .collect();
        if !kept.is_empty() {
            sizes.functions.insert(key.clone(), kept);
        }
    }
    sizes
}

/* ------------------------------------------------------- function lengths */

/// The characters [`functions`] passes over between `fn` and the function's name.
const WHITESPACE: [char; 5] = [' ', '\t', '\n', '\r', '\u{c}'];

/// Each function of `text` with its length in lines, from the line its `fn` is on to the line
/// of its closing brace, in the order they start. A declaration without a body (a trait's) is
/// not one, nor a function pointer's type (`fn(u8)`); a function inside another is counted on
/// its own, and in the other's lines too.
pub(crate) fn functions(text: &str) -> Vec<(String, usize)> {
    let code = code_only(text);
    let mut line_at = Vec::with_capacity(code.len());
    let mut line = 1;
    for &c in &code {
        line_at.push(line);
        if c == '\n' {
            line += 1;
        }
    }
    let mut found = Vec::new();
    let mut i = 0;
    while i < code.len() {
        if !is_ident(code[i]) {
            i += 1;
            continue;
        }
        let end = word_end(&code, i);
        if code[i..end] == ['f', 'n'] {
            found.extend(function_at(&code, &line_at, i, end));
        }
        i = end;
    }
    found
}

/// The function whose `fn` is `code[start..after]`, with its length in lines, or `None` when
/// what follows is not a function with a body.
fn function_at(
    code: &[char],
    line_at: &[usize],
    start: usize,
    after: usize,
) -> Option<(String, usize)> {
    let name_start = after
        + code[after..]
            .iter()
            .take_while(|c| WHITESPACE.contains(*c))
            .count();
    let name_end = word_end(code, name_start);
    if name_end == name_start {
        return None;
    }
    let name: String = code[name_start..name_end].iter().collect();
    // The body is the first `{` outside the parameters' brackets; a `;` there first (a trait's
    // declaration) means there is none. `[u8; 4]` holds a `;` inside its brackets.
    let mut depth = 0usize;
    let mut open = name_end;
    loop {
        match *code.get(open)? {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => return None,
            '{' if depth == 0 => break,
            _ => {}
        }
        open += 1;
    }
    let mut braces = 0usize;
    for (k, &c) in code.iter().enumerate().skip(open) {
        if c == '{' {
            braces += 1;
        } else if c == '}' {
            braces = braces.saturating_sub(1);
            if braces == 0 {
                return Some((name, line_at[k] - line_at[start] + 1));
            }
        }
    }
    None
}

/// Whether `c` is part of a word: an identifier, a keyword or a number.
fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Where the word starting at `i` ends (`i` itself when none starts there).
fn word_end(src: &[char], i: usize) -> usize {
    i + src
        .get(i..)
        .unwrap_or_default()
        .iter()
        .take_while(|c| is_ident(**c))
        .count()
}

/// `text` with its comments, strings and character literals blanked: each of their characters a
/// space but a newline, which stays, so each line of the result is that line of `text`.
pub(crate) fn code_only(text: &str) -> Vec<char> {
    let src: Vec<char> = text.chars().collect();
    let mut out = Vec::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        let (end, keep) = piece_at(&src, i);
        if keep {
            out.extend_from_slice(&src[i..end]);
        } else {
            out.extend(
                src[i..end]
                    .iter()
                    .map(|&c| if c == '\n' { '\n' } else { ' ' }),
            );
        }
        i = end;
    }
    out
}

/// Where the piece of `src` starting at `i` ends, and whether it is code (kept) rather than a
/// comment, a string or a character literal (blanked).
fn piece_at(src: &[char], i: usize) -> (usize, bool) {
    let next = src.get(i + 1).copied();
    match src[i] {
        '/' if next == Some('/') => (line_end(src, i), false),
        '/' if next == Some('*') => (block_comment_end(src, i), false),
        '"' => (string_end(src, i + 1), false),
        '\'' => char_end(src, i).map_or((i + 1, true), |end| (end, false)),
        c if is_ident(c) => {
            let end = word_end(src, i);
            let raw_prefix = matches!(&src[i..end], ['r'] | ['b', 'r'] | ['c', 'r']);
            match raw_end(src, end) {
                Some(raw) if raw_prefix => (raw, false),
                _ => (end, true),
            }
        }
        _ => (i + 1, true),
    }
}

/// Where the line comment starting at `i` ends: at its newline, which is not part of it.
fn line_end(src: &[char], i: usize) -> usize {
    i + src[i..].iter().take_while(|&&c| c != '\n').count()
}

/// Where the block comment starting at `i` ends, after its `*/`; block comments nest.
fn block_comment_end(src: &[char], mut i: usize) -> usize {
    let mut depth = 0usize;
    while i < src.len() {
        match (src[i], src.get(i + 1).copied()) {
            ('/', Some('*')) => {
                depth += 1;
                i += 2;
            }
            ('*', Some('/')) => {
                depth = depth.saturating_sub(1);
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    src.len()
}

/// Where the string whose text starts at `i` ends, after its closing quote; `\` escapes the
/// character after it.
fn string_end(src: &[char], mut i: usize) -> usize {
    while i < src.len() {
        match src[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    src.len()
}

/// Where the character literal starting with the quote at `i` ends, after its closing quote, or
/// `None` when the quote starts a lifetime or a label (`'a`, `'static`, `'outer:`) instead.
fn char_end(src: &[char], i: usize) -> Option<usize> {
    match (src.get(i + 1).copied(), src.get(i + 2).copied()) {
        // An escape (`'\n'`, `'\''`, `'\u{7b}'`): the next quote after the escaped character.
        (Some('\\'), _) => {
            let rest = src.get(i + 3..).unwrap_or_default();
            let close = i + 3 + rest.iter().take_while(|&&c| c != '\'').count();
            Some((close + 1).min(src.len()))
        }
        (Some(_), Some('\'')) => Some(i + 3),
        _ => None,
    }
}

/// Where the raw string whose `#`s or opening quote start at `i` (after its `r`, `br` or `cr`)
/// ends, after its closing quote and as many `#`s; `None` when no quote follows the `#`s, as
/// for a raw identifier (`r#type`).
fn raw_end(src: &[char], i: usize) -> Option<usize> {
    let hashes = src
        .get(i..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&c| c == '#')
        .count();
    if src.get(i + hashes) != Some(&'"') {
        return None;
    }
    let mut j = i + hashes + 1;
    while j < src.len() {
        if src[j] == '"' && (1..=hashes).all(|k| src.get(j + k) == Some(&'#')) {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(src.len())
}

#[cfg(test)]
mod tests;
