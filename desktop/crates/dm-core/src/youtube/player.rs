// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! YouTube's player script cut down to what answers its two checks: the `n` value every
//! stream's address carries, and the signature some streams are locked behind.
//!
//! This is how reference A answers them (`docs/reference-behavior.md`, "How reference A answers
//! YouTube's checks"): pieces cut out of the player, put in a small program of its own, and run
//! in the computer's own JavaScript engine (`crate::javascript`), with no engine or script of
//! anyone else's shipped or fetched.
//!
//! The player is one function, `(function(g){…})(_yt_player)` (the TV's, `(function(){…})
//! .call(this)`), whose statements define its functions, classes and tables and then start it. Run whole, it would reach for the browser
//! at once; cut down to its definitions it reaches for nothing: every statement is kept but the
//! calls it makes while loading (an expression statement that is not one assignment). Its
//! function that builds a stream's address, the one whose own statements include
//! `x.set("alr","yes")`, is then called the way the player calls it, with the signature to
//! decipher, and the method its address class adds to `set`, `get` and `clone` turns `n` into
//! the value the server accepts. That is the player's shape, not its names, which change with
//! every version: the function and the method are found, never named.

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use std::fmt::Write as _;
use std::ops::Range;

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A name or a keyword.
    Word,
    /// A number.
    Number,
    /// A quoted string.
    Text,
    /// A piece of a template: from a backtick or a `}` to a backtick or a `${`.
    Template,
    /// A regular expression.
    Pattern,
    /// A punctuator.
    Mark,
}

/// One token: its kind and where it is in the script, in bytes.
#[derive(Clone, Copy, Debug)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

/// Punctuators of more than one character, the longest first.
const MARKS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "<<", ">>", "**",
];
/// Punctuators of one character.
const SINGLE: &[u8] = b"{}()[];,<>+-*/%&|^!~?:=.@#";
/// The words after which a `/` starts a regular expression rather than dividing.
const BEFORE_PATTERN: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
];
/// Words that start a statement which may end at its closing brace, with no `;`.
const BLOCK_WORDS: &[&str] = &[
    "function", "if", "for", "while", "try", "switch", "do", "with", "class",
];
/// Words that carry a statement on past its closing brace.
const GOES_ON: &[&str] = &["else", "catch", "finally", "while"];
/// Words that start a statement that is not an expression.
const STATEMENT_WORDS: &[&str] = &[
    "var", "let", "const", "return", "throw", "break", "continue", "else", "catch", "finally",
    "debugger",
];
/// The assignment operators.
const ASSIGNS: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "**=", "&&=", "||=",
    "??=",
];

/// A token's text.
fn text<'a>(src: &'a str, token: &Token) -> &'a str {
    src.get(token.start..token.end).unwrap_or("")
}

/// Whether a byte goes on a name (ASCII letters, digits, `_`, `$`, and every byte of a
/// character outside ASCII).
fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

/// The length of the space at `i` that is not ASCII (no-break space, the byte-order mark, the
/// line and paragraph separators), or 0.
fn wide_space(b: &[u8], i: usize) -> usize {
    let at = |k: usize| b.get(i + k).copied().unwrap_or(0);
    match (at(0), at(1), at(2)) {
        (0xC2, 0xA0, _) => 2,
        (0xEF, 0xBB, 0xBF) | (0xE2, 0x80, 0xA8) | (0xE2, 0x80, 0xA9) => 3,
        _ => 0,
    }
}

/// Whether a `/` after `prev` starts a regular expression.
fn pattern_allowed(src: &str, prev: Option<&Token>) -> bool {
    let Some(prev) = prev else { return true };
    let said = text(src, prev);
    match prev.kind {
        Kind::Number | Kind::Text | Kind::Pattern => false,
        Kind::Template => said.ends_with("${"),
        Kind::Word => BEFORE_PATTERN.contains(&said),
        Kind::Mark => !matches!(said, ")" | "]" | "}" | "++" | "--"),
    }
}

/// The tokens of `src` from `from` to the brace that closes the block `from` is inside, and
/// where that brace is. A script that ends before that brace, or holds what no script holds,
/// is an error.
fn tokens(src: &str, from: usize) -> Result<(Vec<Token>, usize), String> {
    let b = src.as_bytes();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut out: Vec<Token> = Vec::with_capacity(b.len().saturating_sub(from) / 4);
    // For each brace open: whether it opened a template's `${`.
    let mut braces: Vec<bool> = Vec::new();
    let mut i = from;
    while i < b.len() {
        let c = at(i);
        if c.is_ascii_whitespace() || c == 0x0B {
            i += 1;
            continue;
        }
        let wide = wide_space(b, i);
        if wide > 0 {
            i += wide;
            continue;
        }
        if c == b'/' && at(i + 1) == b'/' {
            i = src
                .get(i..)
                .and_then(|rest| rest.find('\n'))
                .map_or(b.len(), |n| i + n);
            continue;
        }
        if c == b'/' && at(i + 1) == b'*' {
            let end = src
                .get(i + 2..)
                .and_then(|rest| rest.find("*/"))
                .ok_or_else(|| format!("a comment that never ends, at byte {i}"))?;
            i += 2 + end + 2;
            continue;
        }
        let start = i;
        let kind;
        if c == b'\'' || c == b'"' {
            let mut j = i + 1;
            loop {
                match at(j) {
                    0 if j >= b.len() => {
                        return Err(format!("a string that never ends, at byte {i}"))
                    }
                    b'\\' => j += 2,
                    q if q == c => break,
                    _ => j += 1,
                }
            }
            kind = Kind::Text;
            i = j + 1;
        } else if c == b'`' || (c == b'}' && braces.last() == Some(&true)) {
            if c == b'}' {
                braces.pop();
            }
            let mut j = i + 1;
            loop {
                match at(j) {
                    0 if j >= b.len() => {
                        return Err(format!("a template that never ends, at byte {i}"))
                    }
                    b'\\' => j += 2,
                    b'`' => {
                        j += 1;
                        break;
                    }
                    b'$' if at(j + 1) == b'{' => {
                        braces.push(true);
                        j += 2;
                        break;
                    }
                    _ => j += 1,
                }
            }
            kind = Kind::Template;
            i = j;
        } else if c == b'/' && pattern_allowed(src, out.last()) {
            i = scan::pattern_end(b, i)?;
            kind = Kind::Pattern;
        } else if c.is_ascii_digit() || (c == b'.' && at(i + 1).is_ascii_digit()) {
            i = scan::number_end(b, i);
            kind = Kind::Number;
        } else if name_byte(c) || c == b'\\' {
            let mut j = i;
            while name_byte(at(j)) || at(j) == b'\\' {
                j += if at(j) == b'\\' { 2 } else { 1 };
            }
            kind = Kind::Word;
            i = j;
        } else {
            let rest = src.get(i..).unwrap_or("");
            let long = MARKS.iter().find(|mark| {
                rest.starts_with(**mark) && !(**mark == "?." && at(i + 2).is_ascii_digit())
            });
            let len = match long {
                Some(mark) => mark.len(),
                None if SINGLE.contains(&c) => 1,
                None => {
                    return Err(format!(
                        "a character no script holds, {:?}, at byte {i}",
                        rest.chars().next().unwrap_or('?')
                    ))
                }
            };
            if len == 1 && c == b'{' {
                braces.push(false);
            } else if len == 1 && c == b'}' && braces.pop().is_none() {
                // The brace that closes the block this started in.
                return Ok((out, i));
            }
            kind = Kind::Mark;
            i += len;
        }
        out.push(Token {
            kind,
            start,
            end: i.min(b.len()),
        });
    }
    Err("the script ends inside its function".into())
}

/// Whether a token is the punctuator `mark`.
fn is(src: &str, token: Option<&Token>, mark: &str) -> bool {
    token.is_some_and(|t| t.kind == Kind::Mark && text(src, t) == mark)
}

/// Whether a token is the word `word`.
fn is_word(src: &str, token: Option<&Token>, word: &str) -> bool {
    token.is_some_and(|t| t.kind == Kind::Word && text(src, t) == word)
}

/// How a punctuator changes the depth of brackets: `(`, `[` and `{` open, their mates close.
fn depth_change(src: &str, token: &Token) -> i32 {
    if token.kind != Kind::Mark {
        return 0;
    }
    match text(src, token) {
        "(" | "[" | "{" => 1,
        ")" | "]" | "}" => -1,
        _ => 0,
    }
}

/// The statements a run of tokens holds, each as the range of its tokens: a statement ends at
/// a `;` outside any bracket, or, where it starts with a word like `function` or `if`, at a
/// closing brace outside any bracket with no `else`, `catch`, `finally` or `while` after it.
fn statements(src: &str, toks: &[Token]) -> Result<Vec<Range<usize>>, String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    for (i, token) in toks.iter().enumerate() {
        depth += depth_change(src, token);
        if depth < 0 {
            return Err(format!(
                "a bracket closed that was never open, at byte {}",
                token.start
            ));
        }
        if depth != 0 || token.kind != Kind::Mark {
            continue;
        }
        let said = text(src, token);
        let ends = match said {
            ";" => true,
            "}" => {
                // The statement's own first word, past any labels (`name:`).
                let mut own = start;
                while toks.get(own).is_some_and(|t| t.kind == Kind::Word)
                    && is(src, toks.get(own + 1), ":")
                {
                    own += 2;
                }
                let first = toks.get(own);
                let block = is(src, first, "{")
                    || first.is_some_and(|f| {
                        f.kind == Kind::Word && BLOCK_WORDS.contains(&text(src, f))
                    });
                let next = toks.get(i + 1);
                let goes_on = next.is_some_and(|n| {
                    (n.kind == Kind::Word && GOES_ON.contains(&text(src, n)))
                        || is(src, Some(n), ";")
                        || is(src, Some(n), ",")
                });
                block && !goes_on
            }
            _ => false,
        };
        if ends {
            out.push(start..i + 1);
            start = i + 1;
        }
    }
    if depth != 0 {
        return Err("a bracket is never closed".into());
    }
    if start < toks.len() {
        out.push(start..toks.len());
    }
    Ok(out)
}

/// Whether a statement is a call made while the player loads, which is left out: an expression
/// statement that is not one assignment (a call, `new`, a sequence, a test with a side). Every
/// other statement (a declaration, an assignment, a block, a string alone) is kept.
fn left_out(src: &str, st: &[Token]) -> bool {
    let Some(first) = st.first() else {
        return false;
    };
    let said = text(src, first);
    match first.kind {
        Kind::Word if BLOCK_WORDS.contains(&said) || STATEMENT_WORDS.contains(&said) => {
            return false
        }
        // A label.
        Kind::Word if is(src, st.get(1), ":") => return false,
        Kind::Mark if said == "{" || said == ";" => return false,
        // A directive (`'use strict'`) or another literal alone.
        Kind::Text | Kind::Number if st.len() <= 2 => return false,
        _ => {}
    }
    // A chain of names, members and calls, then one assignment operator, then no comma outside
    // any bracket (which would make it a sequence).
    let mut depth = 0i32;
    let mut assigned = false;
    for (j, token) in st.iter().enumerate() {
        let change = depth_change(src, token);
        if change != 0 {
            depth += change;
            continue;
        }
        if depth != 0 {
            continue;
        }
        let said = text(src, token);
        if token.kind == Kind::Mark && said == ";" {
            break;
        }
        if assigned {
            if token.kind == Kind::Mark && said == "," {
                return true;
            }
            continue;
        }
        if token.kind == Kind::Mark && ASSIGNS.contains(&said) {
            if j == 0 {
                return true;
            }
            assigned = true;
        } else if !(token.kind == Kind::Word || (token.kind == Kind::Mark && said == ".")) {
            return true;
        }
    }
    !assigned
}

/// The index of the token past the bracket group that opens at `open`.
fn past_group(src: &str, toks: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (k, token) in toks.iter().enumerate().skip(open) {
        depth += depth_change(src, token);
        if depth == 0 {
            return Some(k + 1);
        }
    }
    None
}

/// The functions a statement defines, each with the name it is reached by and the index of
/// its `function` word: `name = function(…){…}` (the name a chain of members), `function
/// name(…){…}`, and each `name = function(…){…}` a `var` declares.
fn defined(src: &str, st: &[Token]) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let word = |k: usize| st.get(k).filter(|t| t.kind == Kind::Word);
    if is_word(src, st.first(), "function") {
        if let Some(name) = word(1) {
            out.push((text(src, name).to_string(), 0));
        }
        return out;
    }
    if st
        .first()
        .is_some_and(|t| t.kind == Kind::Word && matches!(text(src, t), "var" | "let" | "const"))
    {
        let mut depth = 0i32;
        let mut k = 1;
        let mut at_declarator = true;
        while let Some(token) = st.get(k) {
            if depth == 0 && at_declarator {
                if let (Some(name), true, true) = (
                    word(k),
                    is(src, st.get(k + 1), "="),
                    is_word(src, st.get(k + 2), "function"),
                ) {
                    out.push((text(src, name).to_string(), k + 2));
                }
            }
            depth += depth_change(src, token);
            at_declarator = depth == 0 && is(src, Some(token), ",");
            k += 1;
        }
        return out;
    }
    // A chain of names and dots, then `=`, then `function`.
    let mut k = 0;
    while st
        .get(k)
        .is_some_and(|t| t.kind == Kind::Word || (t.kind == Kind::Mark && text(src, t) == "."))
    {
        k += 1;
    }
    if k > 0 && is(src, st.get(k), "=") && is_word(src, st.get(k + 1), "function") {
        let name: String = st
            .get(..k)
            .unwrap_or(&[])
            .iter()
            .map(|t| text(src, t))
            .collect();
        out.push((name, k + 1));
    }
    out
}

/// Whether a quoted string's words are `word`.
fn says(src: &str, token: Option<&Token>, word: &str) -> bool {
    token.is_some_and(|t| {
        let said = text(src, t);
        t.kind == Kind::Text
            && said.len() == word.len() + 2
            && said.get(1..=word.len()) == Some(word)
    })
}

/// Whether the function whose `function` word is at `at` in `st` has, among its own statements
/// (not those of a function or block inside it), `x.y("alr","yes")` or `x[…]("alr","yes")`: the
/// mark of the player's function that builds a stream's address.
/// `Some` where it does: with what has to be set before it is called, where its key comes from
/// a global its callers set (`g.B[25]=2314109;Yp(…)`, gaps row 209), else nothing.
fn sets_alr_yes(src: &str, st: &[Token], at: usize, table: Option<&Table>) -> Option<String> {
    // Past the word, the name if any, and the parameters, to the body's brace.
    let mut k = at + 1;
    if st.get(k).is_some_and(|t| t.kind == Kind::Word) {
        k += 1;
    }
    if !is(src, st.get(k), "(") {
        return None;
    }
    let open = past_group(src, st, k)?;
    if !is(src, st.get(open), "{") {
        return None;
    }
    let end = past_group(src, st, open)?;
    let mut depth = 0i32;
    for j in open + 1..end.saturating_sub(1) {
        let Some(token) = st.get(j) else { break };
        let starts = depth == 0
            && (is(src, st.get(j.wrapping_sub(1)), ";")
                || is(src, st.get(j.wrapping_sub(1)), "{")
                || is(src, st.get(j.wrapping_sub(1)), "}"));
        if starts && token.kind == Kind::Word {
            // The callee: a name, then `.name` or a bracket group.
            let call = if is(src, st.get(j + 1), ".")
                && st.get(j + 2).is_some_and(|t| t.kind == Kind::Word)
            {
                Some(j + 3)
            } else if is(src, st.get(j + 1), "[") {
                past_group(src, st, j + 1)
            } else {
                None
            };
            if let Some(c) = call {
                if is(src, st.get(c), "(")
                    && says(src, st.get(c + 1), "alr")
                    && is(src, st.get(c + 2), ",")
                    && says(src, st.get(c + 3), "yes")
                    && is(src, st.get(c + 4), ")")
                    && (is(src, st.get(c + 5), ";") || is(src, st.get(c + 5), "}"))
                {
                    return Some(String::new());
                }
                if let Some(table) = table {
                    if is(src, st.get(j + 1), "[") {
                        if let Some((key_name, key)) = table.sets_alr_yes(src, st, j + 2, c) {
                            return Some(
                                table
                                    .key_from_global(
                                        src,
                                        st.get(open..end).unwrap_or(&[]),
                                        &key_name,
                                        key,
                                    )
                                    .unwrap_or_default(),
                            );
                        }
                    }
                }
            }
        }
        depth += depth_change(src, token);
    }
    None
}

/// The player's own table of its strings, where it keeps them in one (gaps row 209): a player
/// of November 2026 holds most of its words in `g.d='…{alr{…{yes{…'.split("{")` and reads
/// them as `g.d[k^1387]`, `k` a number each function works out as it runs. What dm looks for
/// in a function is then not `set("alr","yes")` in quotes but three readings of the table whose
/// numbers, whatever `k` is, differ from one another as the places of `set`, `alr` and `yes`
/// do: `(1387^k)^(1892^k)` is `1387^1892`.
#[derive(Debug, Clone, PartialEq)]
struct Table {
    /// How the player names it, as written (`g.d`).
    name: String,
    /// The places of `set`, `alr` and `yes` in it.
    set: u64,
    alr: u64,
    yes: u64,
}

/// The player's table of its strings ([`Table`]), where it has one with `set`, `alr` and `yes`.
fn string_table(player: &str) -> Option<Table> {
    let bytes = player.as_bytes();
    for form in [".split(\"{\")", ".split('{')"] {
        let mut from = 0;
        while let Some(found) = player.get(from..).and_then(|rest| rest.find(form)) {
            let split = from + found;
            from = split + form.len();
            // The quoted text before it, back to its opening quote (not one escaped).
            let Some(&quote) = split.checked_sub(1).and_then(|i| bytes.get(i)) else {
                continue;
            };
            if quote != b'"' && quote != b'\'' {
                continue;
            }
            let mut open = split - 1;
            while let Some(before) = open.checked_sub(1) {
                open = before;
                if bytes.get(open) == Some(&quote) {
                    let escapes = bytes
                        .get(..open)
                        .unwrap_or(&[])
                        .iter()
                        .rev()
                        .take_while(|&&b| b == b'\\')
                        .count();
                    if escapes % 2 == 0 {
                        break;
                    }
                }
            }
            if bytes.get(open) != Some(&quote) || open + 1 >= split {
                continue;
            }
            // `name =` before the quote.
            let head = player.get(..open).unwrap_or("").trim_end();
            let Some(head) = head.strip_suffix('=') else {
                continue;
            };
            let head = head.trim_end();
            let name_from = head
                .bytes()
                .rposition(|b| !(name_byte(b) || b == b'.'))
                .map_or(0, |i| i + 1);
            let name = head.get(name_from..).unwrap_or("");
            if name.is_empty() || name.starts_with('.') || name.ends_with('.') {
                continue;
            }
            let words: Vec<&str> = player
                .get(open + 1..split - 1)
                .unwrap_or("")
                .split('{')
                .collect();
            let place = |word: &str| words.iter().position(|w| *w == word).map(|p| p as u64);
            if let (Some(set), Some(alr), Some(yes)) = (place("set"), place("alr"), place("yes")) {
                return Some(Table {
                    name: name.to_string(),
                    set,
                    alr,
                    yes,
                });
            }
        }
    }
    None
}

impl Table {
    /// A reading of the table at `k` in `st`, `name[v^n]`: the name `v` and the number `n`, and
    /// where the reading ends.
    fn reading(&self, src: &str, st: &[Token], k: usize) -> Option<(String, u64, usize)> {
        let mut spelled = String::new();
        let mut at = k;
        while spelled.len() < self.name.len() {
            let token = st.get(at)?;
            if token.kind != Kind::Word && !is(src, Some(token), ".") {
                return None;
            }
            spelled.push_str(text(src, token));
            at += 1;
        }
        if spelled != self.name || !is(src, st.get(at), "[") {
            return None;
        }
        let key = st.get(at + 1).filter(|t| t.kind == Kind::Word)?;
        if !is(src, st.get(at + 2), "^") {
            return None;
        }
        let number = st.get(at + 3).filter(|t| t.kind == Kind::Number)?;
        let said = text(src, number);
        let n = match said.strip_prefix("0x").or_else(|| said.strip_prefix("0X")) {
            Some(hex) => u64::from_str_radix(hex, 16).ok()?,
            None => said.parse().ok()?,
        };
        if !is(src, st.get(at + 4), "]") {
            return None;
        }
        Some((text(src, key).to_string(), n, at + 5))
    }

    /// Where `x[T[v^a]](T[v^b],T[v^c])` stands at `k` (the reading inside the callee's
    /// bracket) with its call's `(` at `call`, a set of `alr` to `yes` whatever `v` is: the name
    /// `v` and the number it has to be for the table to say `set`, `alr` and `yes` there.
    fn sets_alr_yes(
        &self,
        src: &str,
        st: &[Token],
        k: usize,
        call: usize,
    ) -> Option<(String, u64)> {
        let (v, a, after) = self.reading(src, st, k)?;
        if !is(src, st.get(after), "]") || after + 1 != call || !is(src, st.get(call), "(") {
            return None;
        }
        let (v1, b, comma) = self.reading(src, st, call + 1)?;
        if !is(src, st.get(comma), ",") {
            return None;
        }
        let (v2, c, close) = self.reading(src, st, comma + 1)?;
        let sets = v == v1
            && v1 == v2
            && is(src, st.get(close), ")")
            && (is(src, st.get(close + 1), ";") || is(src, st.get(close + 1), "}"))
            && a ^ b == self.set ^ self.alr
            && b ^ c == self.alr ^ self.yes;
        sets.then_some((v, a ^ self.set))
    }

    /// What a caller sets before calling a function whose key `name` it works out from a global
    /// as its first thing, `var e=g.B[25]%53723;`: `g.B[25]=<key>;`, the key being what the
    /// table needs (`Table::sets_alr_yes`). `None` where the key comes from anything else (its
    /// arguments, which dm's own call gives) or could not be that remainder.
    fn key_from_global(&self, src: &str, body: &[Token], name: &str, key: u64) -> Option<String> {
        for k in 0..body.len() {
            if !(is_word(src, body.get(k), name) && is(src, body.get(k + 1), "=")) {
                continue;
            }
            // The global: a chain of names, then `[N]`, then `%M`.
            let mut at = k + 2;
            let mut global = String::new();
            while body
                .get(at)
                .is_some_and(|t| t.kind == Kind::Word || is(src, Some(t), "."))
            {
                if let Some(token) = body.get(at) {
                    global.push_str(text(src, token));
                }
                at += 1;
            }
            let index = body.get(at + 1).filter(|t| t.kind == Kind::Number)?;
            let modulus = body.get(at + 4).filter(|t| t.kind == Kind::Number)?;
            if global.is_empty()
                || !is(src, body.get(at), "[")
                || !is(src, body.get(at + 2), "]")
                || !is(src, body.get(at + 3), "%")
            {
                return None;
            }
            let modulus: u64 = text(src, modulus).parse().ok()?;
            if key >= modulus {
                return None;
            }
            return Some(format!("{global}[{}]={key};", text(src, index)));
        }
        None
    }
}

/// Whether the function whose `function` word is at `at` in `st` takes a text apart letter by
/// letter and puts it back (`.split("")` and `.join("")` in its body): the shape of the player's
/// own `n` function, a second way to it where the address class's method is not found or gives
/// nothing (gaps row Y3).
fn splits_and_joins(src: &str, st: &[Token], at: usize) -> bool {
    let mut k = at + 1;
    if st.get(k).is_some_and(|t| t.kind == Kind::Word) {
        k += 1;
    }
    if !is(src, st.get(k), "(") {
        return false;
    }
    let Some(open) = past_group(src, st, k) else {
        return false;
    };
    let (Some(first), Some(end)) = (st.get(open), past_group(src, st, open)) else {
        return false;
    };
    let Some(last) = st.get(end.saturating_sub(1)) else {
        return false;
    };
    let body = src.get(first.start..last.end).unwrap_or("");
    let squeezed: String = body.split_whitespace().collect();
    let any = |call: &str| {
        [format!("{call}(\"\")"), format!("{call}('')")]
            .iter()
            .any(|form| squeezed.contains(form.as_str()))
    };
    any(".split") && any(".join")
}

/// The most functions of the `n` function's shape the program tries (gaps row Y3).
const MOST_N_FUNCTIONS: usize = 8;

/// What the player's code finds in place of the browser: a window that is the program's own
/// global object, and a page with nothing in it.
const BEFORE: &str = r#"var globalThis = this, window = this, self = this;
var XMLHttpRequest = { prototype: {} };
var location = { hash: "", host: "www.youtube.com", hostname: "www.youtube.com",
  href: "https://www.youtube.com/watch?v=dm", origin: "https://www.youtube.com", password: "",
  pathname: "/watch", port: "", protocol: "https:", search: "?v=dm", username: "" };
var document = Object.create(null), navigator = Object.create(null);
var __dm = { makers: [], nfuncs: [] };
var _yt_player = {};
"#;

/// The program's own part: each challenge answered by every function found, which must agree.
/// For `n`, an address is built with no signature and the address class's own method run; for
/// a signature, the address is built with it, which deciphers it.
const AFTER: &str = r#"
__dm.solve = function (input) {
  function made(make, sig) {
    return make("https://www.youtube.com/watch?v=dm", "s", sig === undefined ? undefined : encodeURIComponent(sig));
  }
  function n(make, value) {
    try {
      return byMethod(make, value);
    } catch (e) {
      // A second way to it: a function of the `n` function's own shape, called on the value.
      for (var f = 0; f < __dm.nfuncs.length; f++) {
        try {
          var got = __dm.nfuncs[f](value);
          if (typeof got === "string" && got !== "" && got !== value) return got;
        } catch (ignored) {}
      }
      throw e;
    }
  }
  function byMethod(make, value) {
    var url = made(make);
    url.set("n", value);
    var proto = Object.getPrototypeOf(url);
    var keys = Object.keys(proto).concat(Object.getOwnPropertyNames(proto));
    for (var i = 0; i < keys.length; i++) {
      var key = keys[i];
      if (key !== "constructor" && key !== "set" && key !== "get" && key !== "clone" && typeof url[key] === "function") {
        url[key]();
        break;
      }
    }
    var got = url.get("n");
    if (typeof got !== "string" || got === "") throw new Error("no n came back");
    return got;
  }
  function sig(make, value) {
    var got = made(make, value).get("s");
    if (typeof got !== "string" || got === "") throw new Error("no signature came back");
    return decodeURIComponent(got);
  }
  var answer = { n: {}, sig: {} }, kinds = { n: n, sig: sig };
  for (var kind in kinds) {
    var asked = input[kind] || [];
    for (var c = 0; c < asked.length; c++) {
      var found = null, errors = [];
      // With no address function found, the `n` function's own shape is the one way (gaps row Y3).
      var makers = __dm.makers.length ? __dm.makers : [null];
      for (var m = 0; m < makers.length; m++) {
        try {
          var got = kinds[kind](makers[m], asked[c]);
          if (found !== null && found !== got) throw new Error("the functions found disagree");
          found = got;
        } catch (e) {
          errors.push(String(e));
        }
      }
      if (found === null) throw new Error("no answer for " + kind + " " + asked[c] + ": " + errors.join("; "));
      answer[kind][asked[c]] = found;
    }
  }
  return JSON.stringify(answer);
};
"#;

/// The program that answers the checks of `player` (the player script's text), once run: it
/// defines `__dm.solve`, which [`call`] calls. An error says, in words, what in the player was
/// not as dm knows it, which means the player changed and dm needs an update.
pub(crate) fn program(player: &str) -> Result<String, String> {
    let changed = |what: &str| format!("YouTube's player is not as dm knows it ({what})");
    // `(function(g){`, the player's own function, whose parameter is what it hangs its parts on
    // (`_yt_player`); or `(function(){`, as the TV's player is, called on the window.
    let opened = player
        .find("(function(")
        .ok_or_else(|| changed("no function holds it"))?;
    let rest = player.get(opened + "(function(".len()..).unwrap_or("");
    let close = rest
        .find(')')
        .ok_or_else(|| changed("its function has no parameters"))?;
    let param = rest.get(..close).unwrap_or("").trim();
    if !param.bytes().all(name_byte) {
        return Err(changed("its function takes more than one name"));
    }
    let brace = rest
        .get(close..)
        .and_then(|r| r.find('{'))
        .map(|b| opened + "(function(".len() + close + b + 1)
        .ok_or_else(|| changed("its function has no body"))?;
    let (toks, _end) = tokens(player, brace).map_err(|e| changed(&e))?;
    let all = statements(player, &toks).map_err(|e| changed(&e))?;
    // Its first statement, `var window=this;`, is kept as it is: called from the program's
    // global code, `this` is the program's own window (`BEFORE`), and the player's later
    // `'use strict'` stays a plain string, as it is in a page.
    let kept: Vec<&[Token]> = all
        .iter()
        .map(|range| toks.get(range.clone()).unwrap_or(&[]))
        .filter(|st| !left_out(player, st))
        .collect();
    let table = string_table(player);
    let mut makers: Vec<String> = Vec::new();
    let mut nfuncs: Vec<String> = Vec::new();
    for st in &kept {
        for (name, at) in defined(player, st) {
            if let Some(setup) = sets_alr_yes(player, st, at, table.as_ref()) {
                // Called as its callers call it: what they set first, set first.
                let maker = if setup.is_empty() {
                    name
                } else {
                    format!("function (a, b, c) {{ {setup} return {name}(a, b, c); }}")
                };
                if !makers.contains(&maker) {
                    makers.push(maker);
                }
            } else if !name.contains('.')
                && nfuncs.len() < MOST_N_FUNCTIONS
                && splits_and_joins(player, st, at)
                && !nfuncs.contains(&name)
            {
                nfuncs.push(name);
            }
        }
    }
    if makers.is_empty() && nfuncs.is_empty() {
        return Err(changed("no function in it builds a stream's address"));
    }
    let mut out = String::with_capacity(player.len());
    out.push_str(BEFORE);
    let _ = writeln!(out, "(function ({param}) {{");
    for st in &kept {
        let (Some(first), Some(last)) = (st.first(), st.last()) else {
            continue;
        };
        out.push_str(player.get(first.start..last.end).unwrap_or(""));
        out.push('\n');
    }
    let _ = writeln!(out, ";__dm.makers = [{}];", makers.join(", "));
    let _ = writeln!(out, "__dm.nfuncs = [{}];", nfuncs.join(", "));
    // Called on the program's window, as a page calls it (`this` is the window in either shape).
    out.push_str("}).call(this, _yt_player);\n");
    out.push_str(AFTER);
    Ok(out)
}

/// The call that answers `ns` (`n` values) and `sigs` (signatures) once [`program`] has run:
/// its value is the JSON `{"n": {asked: answer}, "sig": {asked: answer}}`.
pub(crate) fn call(ns: &[&str], sigs: &[&str]) -> String {
    format!(
        "__dm.solve({})",
        serde_json::json!({ "n": ns, "sig": sigs })
    )
}

mod scan;

#[cfg(test)]
mod tests;
