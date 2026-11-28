// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
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

mod lexing;
/// The player's own table of its strings (gaps row 209).
mod table;

use lexing::tokens;
use table::{string_table, Table};

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

/// Whether the function whose `function` word is at `at` in `st` is the player's `inherits`:
/// two parameters, `a` and `b`, its body starting `a.prototype=` and reading `b.prototype`
/// (`function(a,b){a.prototype=ta(b.prototype);a.prototype.constructor=a;…}`).
fn makes_a_subclass(src: &str, st: &[Token], at: usize) -> bool {
    let at_ = |k: usize| st.get(at + k);
    let named = |k: usize| {
        at_(k)
            .filter(|t| t.kind == Kind::Word)
            .map(|t| text(src, t))
    };
    let (Some(child), Some(parent)) = (named(2), named(4)) else {
        return false;
    };
    if !(is(src, at_(1), "(")
        && is(src, at_(3), ",")
        && is(src, at_(5), ")")
        && is(src, at_(6), "{")
        && named(7) == Some(child)
        && is(src, at_(8), ".")
        && is_word(src, at_(9), "prototype")
        && is(src, at_(10), "="))
    {
        return false;
    }
    let end = past_group(src, st, at + 6).unwrap_or(st.len());
    (at + 11..end.saturating_sub(2)).any(|k| {
        st.get(k)
            .is_some_and(|t| t.kind == Kind::Word && text(src, t) == parent)
            && is(src, st.get(k + 1), ".")
            && is_word(src, st.get(k + 2), "prototype")
    })
}

/// Whether a statement is a call to one of `inherits` with two names for its classes, and
/// nothing else: `g.z(jp,g.Q);`.
fn inherits_call(src: &str, st: &[Token], inherits: &[String]) -> bool {
    // A chain of names and dots from `k`: its text and the index past it.
    let chain = |mut k: usize| {
        let mut name = String::new();
        while let Some(t) = st
            .get(k)
            .filter(|t| t.kind == Kind::Word || (t.kind == Kind::Mark && text(src, t) == "."))
        {
            name.push_str(text(src, t));
            k += 1;
        }
        (name, k)
    };
    let (callee, k) = chain(0);
    if callee.is_empty() || !inherits.contains(&callee) || !is(src, st.get(k), "(") {
        return false;
    }
    let (child, k) = chain(k + 1);
    if child.is_empty() || !is(src, st.get(k), ",") {
        return false;
    }
    let (parent, k) = chain(k + 1);
    if parent.is_empty() || !is(src, st.get(k), ")") {
        return false;
    }
    match st.get(k + 1) {
        None => true,
        Some(_) => is(src, st.get(k + 1), ";") && st.len() == k + 2,
    }
}

/// The most functions of the `n` function's shape the program tries (gaps row Y3).
const MOST_N_FUNCTIONS: usize = 8;

/// What the player's code finds in place of the browser: a window that is the program's own
/// global object, and a page with nothing in it.
const BEFORE: &str = r#"var globalThis = this, window = this, self = this;
var XMLHttpRequest = { prototype: {} };
var location = { hash: "", host: "www.youtube.com", hostname: "www.youtube.com",
  href: "https://www.youtube.com/watch?v=EDM", origin: "https://www.youtube.com", password: "",
  pathname: "/watch", port: "", protocol: "https:", search: "?v=EDM", username: "" };
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
    return make("https://www.youtube.com/watch?v=EDM", "s", sig === undefined ? undefined : encodeURIComponent(sig));
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
/// not as EDM knows it, which means the player changed and EDM needs an update.
pub(crate) fn program(player: &str) -> Result<String, String> {
    let changed = |what: &str| format!("YouTube's player is not as EDM knows it ({what})");
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
    let every: Vec<&[Token]> = all
        .iter()
        .map(|range| toks.get(range.clone()).unwrap_or(&[]))
        .collect();
    // The player's own `inherits` (`g.z=function(a,b){a.prototype=ta(b.prototype);…}`): a
    // player in the older JavaScript (`player_ias`) makes its classes with a call to it as it
    // loads, `g.z(jp,g.Q);`, and without it `jp` has none of `g.Q`'s methods.
    let inherits: Vec<String> = every
        .iter()
        .filter(|st| !left_out(player, st))
        .flat_map(|st| {
            defined(player, st)
                .into_iter()
                .filter(|(_, at)| makes_a_subclass(player, st, *at))
                .map(|(name, _)| name)
        })
        .collect();
    // Each statement kept, and whether it is such a call, run where it fails without stopping
    // the rest.
    let kept: Vec<(&[Token], bool)> = every
        .iter()
        .filter_map(|st| {
            if !left_out(player, st) {
                Some((*st, false))
            } else if inherits_call(player, st, &inherits) {
                Some((*st, true))
            } else {
                None
            }
        })
        .collect();
    let table = string_table(player);
    let mut makers: Vec<String> = Vec::new();
    let mut nfuncs: Vec<String> = Vec::new();
    for (st, _) in &kept {
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
    for (st, guarded) in &kept {
        let (Some(first), Some(last)) = (st.first(), st.last()) else {
            continue;
        };
        let said = player.get(first.start..last.end).unwrap_or("");
        if *guarded {
            let _ = writeln!(out, "try {{ {said} }} catch (__edm_e) {{}}");
        } else {
            out.push_str(said);
            out.push('\n');
        }
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
