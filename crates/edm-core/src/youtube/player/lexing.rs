// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The player script cut into tokens: names, numbers, strings, templates, regular expressions and
//! marks, each with where it is.

use super::*;

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
pub(super) fn tokens(src: &str, from: usize) -> Result<(Vec<Token>, usize), String> {
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
