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
            let mut j = i + 1;
            let mut class = false;
            loop {
                if j >= b.len() || at(j) == b'\n' {
                    return Err(format!("a regular expression that never ends, at byte {i}"));
                }
                match at(j) {
                    b'\\' => j += 2,
                    b'[' => {
                        class = true;
                        j += 1;
                    }
                    b']' => {
                        class = false;
                        j += 1;
                    }
                    b'/' if !class => break,
                    _ => j += 1,
                }
            }
            j += 1;
            while name_byte(at(j)) {
                j += 1;
            }
            kind = Kind::Pattern;
            i = j;
        } else if c.is_ascii_digit() || (c == b'.' && at(i + 1).is_ascii_digit()) {
            let hex = c == b'0' && (at(i + 1) | 0x20) == b'x';
            let mut j = i + 1;
            loop {
                let d = at(j);
                let part = d.is_ascii_alphanumeric() || d == b'_' || d == b'.';
                // The sign of an exponent (`1E-6`), which a hex digit `e` is not.
                let sign = (d == b'+' || d == b'-') && !hex && (at(j - 1) | 0x20) == b'e';
                if !(part || sign) {
                    break;
                }
                j += 1;
            }
            kind = Kind::Number;
            i = j;
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
fn sets_alr_yes(src: &str, st: &[Token], at: usize) -> bool {
    // Past the word, the name if any, and the parameters, to the body's brace.
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
    if !is(src, st.get(open), "{") {
        return false;
    }
    let Some(end) = past_group(src, st, open) else {
        return false;
    };
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
                    return true;
                }
            }
        }
        depth += depth_change(src, token);
    }
    false
}

/// What the player's code finds in place of the browser: a window that is the program's own
/// global object, and a page with nothing in it.
const BEFORE: &str = r#"var globalThis = this, window = this, self = this;
var XMLHttpRequest = { prototype: {} };
var location = { hash: "", host: "www.youtube.com", hostname: "www.youtube.com",
  href: "https://www.youtube.com/watch?v=dm", origin: "https://www.youtube.com", password: "",
  pathname: "/watch", port: "", protocol: "https:", search: "?v=dm", username: "" };
var document = Object.create(null), navigator = Object.create(null);
var __dm = { makers: [] };
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
      for (var m = 0; m < __dm.makers.length; m++) {
        try {
          var got = kinds[kind](__dm.makers[m], asked[c]);
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
    let mut makers: Vec<String> = Vec::new();
    for st in &kept {
        for (name, at) in defined(player, st) {
            if sets_alr_yes(player, st, at) && !makers.contains(&name) {
                makers.push(name);
            }
        }
    }
    if makers.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn words(src: &str) -> Vec<String> {
        let wrapped = format!("{src}}}");
        let (toks, end) = tokens(&wrapped, 0).unwrap();
        assert_eq!(end, wrapped.len() - 1);
        toks.iter().map(|t| text(&wrapped, t).to_string()).collect()
    }

    #[test]
    fn a_slash_divides_after_a_value_and_starts_a_pattern_elsewhere() {
        assert_eq!(words("a=b/c/d"), ["a", "=", "b", "/", "c", "/", "d"]);
        assert_eq!(
            words("a=/c\\/[/]d/gi.test(x)"),
            ["a", "=", "/c\\/[/]d/gi", ".", "test", "(", "x", ")"]
        );
        assert_eq!(
            words("return/x/.test(y)"),
            ["return", "/x/", ".", "test", "(", "y", ")"]
        );
        assert_eq!(words("(a)/2"), ["(", "a", ")", "/", "2"]);
        assert_eq!(words("x/=2"), ["x", "/=", "2"]);
    }

    #[test]
    fn strings_comments_and_templates_hide_their_brackets() {
        assert_eq!(
            words("a='}{\\''+\"]\" // }\n/* { */b"),
            ["a", "=", "'}{\\''", "+", "\"]\"", "b"]
        );
        assert_eq!(
            words("t=`x${ {a:1}.a + `in${2}` }y`;"),
            [
                "t", "=", "`x${", "{", "a", ":", "1", "}", ".", "a", "+", "`in${", "2", "}`",
                "}y`", ";"
            ]
        );
        assert_eq!(
            words("n=1E-6+0x1F+.5+a?.b+c?.5:1"),
            [
                "n", "=", "1E-6", "+", "0x1F", "+", ".5", "+", "a", "?.", "b", "+", "c", "?", ".5",
                ":", "1"
            ]
        );
    }

    #[test]
    fn an_unended_string_or_a_stray_bracket_is_an_error() {
        assert!(tokens("a='x", 0).is_err());
        assert!(tokens("a=/x", 0).is_err());
        assert!(tokens("a=(1", 0).is_err());
        let src = "a=1;b=(2;}";
        let (toks, _) = tokens(src, 0).unwrap();
        assert!(statements(src, &toks).is_err());
    }

    fn split(src: &str) -> Vec<String> {
        let wrapped = format!("{src}}}");
        let (toks, _) = tokens(&wrapped, 0).unwrap();
        statements(&wrapped, &toks)
            .unwrap()
            .into_iter()
            .map(|r| {
                let st = &toks[r];
                wrapped[st[0].start..st[st.len() - 1].end].to_string()
            })
            .collect()
    }

    #[test]
    fn statements_end_at_a_semicolon_or_a_blocks_brace() {
        assert_eq!(
            split("var a=1,b=function(){return 2};function f(){}g.x=function(){if(a){}};if(a){b()}else{c()}try{}catch(e){}do{}while(a);x()"),
            [
                "var a=1,b=function(){return 2};",
                "function f(){}",
                "g.x=function(){if(a){}};",
                "if(a){b()}else{c()}",
                "try{}catch(e){}",
                "do{}while(a);",
                "x()",
            ]
        );
    }

    #[test]
    fn calls_made_while_loading_are_left_out_and_definitions_kept() {
        let wrapped = "'use strict';var a=f();g.b=function(){};c.prototype.d=1;e[f]=2;h(1);new g.Xo;(0,g.y)();a=1,b();x&&y();!function(){}();lbl:for(;;){}g.z(1).w=3;}";
        let (toks, _) = tokens(wrapped, 0).unwrap();
        let kept: Vec<&str> = statements(wrapped, &toks)
            .unwrap()
            .into_iter()
            .filter(|r| !left_out(wrapped, &toks[r.clone()]))
            .map(|r| {
                let st = &toks[r];
                &wrapped[st[0].start..st[st.len() - 1].end]
            })
            .collect();
        assert_eq!(
            kept,
            [
                "'use strict';",
                "var a=f();",
                "g.b=function(){};",
                "c.prototype.d=1;",
                "e[f]=2;",
                "lbl:for(;;){}",
                "g.z(1).w=3;"
            ]
        );
    }

    /// A player in the real one's shape, small: its names behind a string table, its address
    /// class, its signature steps in a dispatcher picked by number (as `a5c2aafe`'s
    /// `gm(18,2147,$g(6,5915,c))`), its `n` method, and calls at load that would fail where
    /// there is no page.
    pub(crate) const PLAYER: &str = r#"var _yt_player={};(function(g){var window=this;/*
 a licence */
'use strict';var ca="set;get;n;split;reverse;join".split(";"),Xa;
g.ch=function(a,b){this.url=a;this.p={}};
g.ch.prototype.set=function(a,b){this.p[a]=b};
g.ch.prototype.get=function(a){return this.p[a]};
g.ch.prototype.tN=function(){var a=this.get(ca[2]);a&&this.set(ca[2],a.split("").reverse().join("")+"_"+Xa.length)};
g.ch.prototype.clone=function(){return new g.ch(this.url)};
Xa=[1,2,3];
var Ys=function(a,b,c){switch(a){case 6:return c[ca[3]]("");case 18:return c[ca[4]]();case 13:return c[ca[5]]("").toUpperCase()}return c};
om=function(a,b,c){b=b===void 0?"":b;c=c===void 0?"":c;a=new g.ch(a,!0);a.set("alr","yes");c&&(c=Ys(18,2147,Ys(6,5915,decodeURIComponent(c))),a[ca[0]](b,encodeURIComponent(Ys(13,2385,c))));return a};
pm=function(a){a.set("alr","yes")||a.get("x")};
window.document.createElement("div").appendChild(null);
g.Vx(om);
})(_yt_player);
"#;

    #[test]
    fn the_player_is_cut_to_its_definitions_and_its_address_function_found() {
        let program = program(PLAYER).unwrap();
        assert!(program.contains(";__dm.makers = [om];"), "{program}");
        assert!(
            !program.contains("createElement"),
            "a call at load was kept"
        );
        assert!(!program.contains("g.Vx(om)"), "a call at load was kept");
        assert!(program.contains("(function (g) {\nvar window=this;\n'use strict';"));
        assert!(program.contains("Xa=[1,2,3];"));
        assert_eq!(
            call(&["abc"], &["x%2Fy"]),
            r#"__dm.solve({"n":["abc"],"sig":["x%2Fy"]})"#
        );
    }

    /// Whether an engine is here to run a program in: the computer's own (Windows', macOS's), or
    /// an installed one.
    fn an_engine_is_here() -> bool {
        crate::javascript::system_engine_here()
            || crate::videopage::deno().is_some()
            || crate::videopage::quickjs().is_some()
    }

    #[tokio::test]
    async fn the_cut_player_answers_both_checks_in_the_computers_own_engine() {
        if !an_engine_is_here() {
            eprintln!("no JavaScript engine here: not run");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let program = program(PLAYER).unwrap();
        let got = crate::javascript::run(
            &program,
            &call(&["abc", "xyz"], &["a/b+c"]),
            std::time::Duration::from_secs(30),
            dir.path(),
        )
        .await
        .unwrap();
        let answer: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(answer["n"]["abc"], "cba_3");
        assert_eq!(answer["n"]["xyz"], "zyx_3");
        assert_eq!(answer["sig"]["a/b+c"], "C+B/A");
        // A player whose `n` method fails says so, rather than answering with what it was given.
        let failing = PLAYER.replace("var a=this.get(ca[2]);", "throw new Error('changed');");
        let why = crate::javascript::run(
            &super::program(&failing).unwrap(),
            &call(&["abc"], &[]),
            std::time::Duration::from_secs(30),
            dir.path(),
        )
        .await
        .unwrap_err();
        assert!(
            why.contains("no answer for n abc") && why.contains("changed"),
            "{why}"
        );
    }

    /// The TV's player, in its own shape: a function with no parameter called on the window,
    /// strict, its parts declared as functions.
    const TV_PLAYER: &str = r#"(function(){'use strict';var h="set;get;n".split(";");
function Ba(a,b){this.url=a;this.p={}}
Ba.prototype.set=function(a,b){this.p[a]=b};Ba.prototype.get=function(a){return this.p[a]};
Ba.prototype.vN=function(){var a=this.get(h[2]);a&&this.set(h[2],a+"!")};
Ba.prototype.clone=function(){return new Ba(this.url)};
function Oa(a,b,c){return a===17?c.slice(1):c}
function JS(a,b,c){b=b===void 0?"":b;c=c===void 0?"":c;a=new Ba(a,!0);a.set("alr","yes");c&&(c=Oa(17,2144,c),a[h[0]](b,c));return a}
document.body.appendChild(null);}).call(this);
"#;

    #[tokio::test]
    async fn the_tvs_player_is_cut_and_answers_too() {
        let program = program(TV_PLAYER).unwrap();
        assert!(program.contains(";__dm.makers = [JS];"), "{program}");
        assert!(!program.contains("appendChild"));
        if !an_engine_is_here() {
            eprintln!("no JavaScript engine here: not run");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let got = crate::javascript::run(
            &program,
            &call(&["abc"], &["xyz"]),
            std::time::Duration::from_secs(30),
            dir.path(),
        )
        .await
        .unwrap();
        assert_eq!(got, r#"{"n":{"abc":"abc!"},"sig":{"xyz":"yz"}}"#);
    }

    #[test]
    fn a_player_with_no_address_function_or_not_one_function_is_said_to_have_changed() {
        let none = PLAYER.replace(r#"a.set("alr","yes");"#, "");
        let why = program(&none).unwrap_err();
        assert!(
            why.contains("no function in it builds a stream's address"),
            "{why}"
        );
        let why = program("var a = 1;").unwrap_err();
        assert!(
            why.contains("YouTube's player is not as dm knows it"),
            "{why}"
        );
        let why = program("(function(g){var a='x;").unwrap_err();
        assert!(why.contains("never ends"), "{why}");
    }
}
