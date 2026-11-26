// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The player's own table of its strings, where it keeps them in one (gaps row 209).

use super::*;

/// The player's own table of its strings, where it keeps them in one (gaps row 209): a player
/// of November 2026 holds most of its words in `g.d='…{alr{…{yes{…'.split("{")` and reads
/// them as `g.d[k^1387]`, `k` a number each function works out as it runs. What dm looks for
/// in a function is then not `set("alr","yes")` in quotes but three readings of the table whose
/// numbers, whatever `k` is, differ from one another as the places of `set`, `alr` and `yes`
/// do: `(1387^k)^(1892^k)` is `1387^1892`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Table {
    /// How the player names it, as written (`g.d`).
    pub(super) name: String,
    /// The places of `set`, `alr` and `yes` in it.
    pub(super) set: u64,
    pub(super) alr: u64,
    pub(super) yes: u64,
}

/// The player's table of its strings ([`Table`]), where it has one with `set`, `alr` and `yes`.
pub(super) fn string_table(player: &str) -> Option<Table> {
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
    pub(super) fn reading(
        &self,
        src: &str,
        st: &[Token],
        k: usize,
    ) -> Option<(String, u64, usize)> {
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
    pub(super) fn sets_alr_yes(
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
    pub(super) fn key_from_global(
        &self,
        src: &str,
        body: &[Token],
        name: &str,
        key: u64,
    ) -> Option<String> {
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
