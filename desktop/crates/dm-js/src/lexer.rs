// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The program's text cut into tokens: names, numbers, strings, punctuation, a pattern, a
//! piece of a template. The parser asks for one token at a time. Two tokens depend on where the
//! parser is, and it says so: a `/` that starts a pattern (where a value is expected) is read
//! again as one ([`Lexer::pattern_at`]), and a template's next piece after `${…}` is read from
//! its `}` ([`Lexer::template_after`]).

use std::rc::Rc;

/// A JavaScript string: UTF-16 code units, as the language counts and indexes them.
pub type Text = Rc<[u16]>;

/// The UTF-16 units of `s`.
pub fn text(s: &str) -> Text {
    s.encode_utf16().collect::<Vec<u16>>().into()
}

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A name or a keyword, as written (escapes read).
    Name(Rc<str>),
    /// A number.
    Number(f64),
    /// A BigInt literal's digits, in base ten.
    BigInt(Rc<str>),
    /// A string literal's value.
    String(Text),
    /// A piece of a template literal: its value (`None` where an escape is not valid, which a
    /// tagged template allows), its raw text, and whether it ends the template.
    Template {
        /// The piece's value.
        cooked: Option<Text>,
        /// The piece as written.
        raw: Text,
        /// The template's last piece (it ended with a backtick, not `${`).
        tail: bool,
    },
    /// A pattern literal: its source and its flags.
    Pattern {
        /// Between the slashes.
        source: Rc<str>,
        /// After them.
        flags: Rc<str>,
    },
    /// Punctuation.
    Punct(&'static str),
    /// The end of the text.
    End,
}

/// A token, where it is, and whether a line ended before it (for automatic semicolons and the
/// few places a line break is not allowed).
#[derive(Clone, Debug)]
pub struct Token {
    /// What it is.
    pub kind: Kind,
    /// Its first byte.
    pub start: usize,
    /// The byte after it.
    pub end: usize,
    /// A line ended between the token before and this one.
    pub newline_before: bool,
}

/// Punctuation, longest first, so the longest that matches is taken.
const PUNCTS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "<<", ">>", "**", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%",
    "&", "|", "^", "!", "~", "?", ":", "=", ".", "@", "#",
];

/// Reads tokens from a program's text.
pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    at: usize,
}

/// A lexing error: what and where.
#[derive(Debug, Clone)]
pub struct LexError {
    /// What was wrong.
    pub message: String,
    /// The byte it was at.
    pub at: usize,
}

type Lexed<T> = Result<T, LexError>;

impl<'a> Lexer<'a> {
    /// A lexer at the start of `src`.
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src,
            bytes: src.as_bytes(),
            at: 0,
        }
    }

    /// The whole text.
    pub fn source(&self) -> &'a str {
        self.src
    }

    fn error<T>(&self, message: impl Into<String>, at: usize) -> Lexed<T> {
        Err(LexError {
            message: message.into(),
            at,
        })
    }

    fn peek(&self, ahead: usize) -> u8 {
        self.bytes.get(self.at + ahead).copied().unwrap_or(0)
    }

    /// The character at the cursor (a whole one, where it is not ASCII).
    fn char_here(&self) -> Option<char> {
        self.src.get(self.at..)?.chars().next()
    }

    /// Skips spaces, line ends and comments; whether a line ended among them.
    fn skip_space(&mut self) -> Lexed<bool> {
        let mut newline = false;
        loop {
            match self.peek(0) {
                b' ' | b'\t' | 0x0b | 0x0c => self.at += 1,
                b'\n' | b'\r' => {
                    newline = true;
                    self.at += 1;
                }
                b'/' if self.peek(1) == b'/' => {
                    while self.at < self.bytes.len() && !matches!(self.peek(0), b'\n' | b'\r') {
                        self.at += 1;
                    }
                }
                b'/' if self.peek(1) == b'*' => {
                    let from = self.at;
                    self.at += 2;
                    loop {
                        if self.at >= self.bytes.len() {
                            return self.error("a comment is not closed", from);
                        }
                        if self.peek(0) == b'*' && self.peek(1) == b'/' {
                            self.at += 2;
                            break;
                        }
                        if matches!(self.peek(0), b'\n' | b'\r') {
                            newline = true;
                        }
                        self.at += 1;
                    }
                }
                b if b >= 0x80 => match self.char_here() {
                    // Line and paragraph separators end a line.
                    Some('\u{2028}' | '\u{2029}') => {
                        newline = true;
                        self.at += 3;
                    }
                    Some(c) if c.is_whitespace() || c == '\u{feff}' => self.at += c.len_utf8(),
                    _ => return Ok(newline),
                },
                _ => return Ok(newline),
            }
        }
    }

    /// The next token, a `/` read as punctuation.
    pub fn next_token(&mut self) -> Lexed<Token> {
        let newline_before = self.skip_space()?;
        let start = self.at;
        let kind = self.read(start)?;
        Ok(Token {
            kind,
            start,
            end: self.at,
            newline_before,
        })
    }

    fn read(&mut self, start: usize) -> Lexed<Kind> {
        let b = self.peek(0);
        if self.at >= self.bytes.len() {
            return Ok(Kind::End);
        }
        if b.is_ascii_digit() || (b == b'.' && self.peek(1).is_ascii_digit()) {
            return self.number(start);
        }
        if b == b'"' || b == b'\'' {
            return self.string(b);
        }
        if b == b'`' {
            self.at += 1;
            return self.template_piece();
        }
        if is_name_start_byte(b) || b == b'\\' || b >= 0x80 {
            return self.name(start);
        }
        for p in PUNCTS {
            if self.src[self.at..].starts_with(p) {
                // `?.` before a digit is `?` and a number (`a?.5:b`).
                if *p == "?." && self.peek(2).is_ascii_digit() {
                    continue;
                }
                self.at += p.len();
                return Ok(Kind::Punct(p));
            }
        }
        self.error(
            format!("a character not in the language: {:?}", self.char_here()),
            start,
        )
    }

    fn name(&mut self, start: usize) -> Lexed<Kind> {
        let mut out = String::new();
        let mut plain = true;
        loop {
            let b = self.peek(0);
            if is_name_byte(b) {
                self.at += 1;
                continue;
            }
            if b == b'\\' {
                // `A` or `\u{41}` in a name.
                if plain {
                    out.push_str(&self.src[start..self.at]);
                    plain = false;
                }
                if self.peek(1) != b'u' {
                    return self.error("a name holds a backslash", self.at);
                }
                self.at += 2;
                let c = self.unicode_escape()?;
                out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                continue;
            }
            if b >= 0x80 {
                match self.char_here() {
                    Some(c) if c.is_alphanumeric() || c == '\u{200c}' || c == '\u{200d}' => {
                        if !plain {
                            out.push(c);
                        }
                        self.at += c.len_utf8();
                        continue;
                    }
                    _ => {}
                }
            }
            if !plain && b < 0x80 && is_name_byte(b) {
                continue;
            }
            break;
        }
        if self.at == start {
            return self.error(
                format!("a character not in the language: {:?}", self.char_here()),
                start,
            );
        }
        Ok(Kind::Name(if plain {
            self.src[start..self.at].into()
        } else {
            out.into()
        }))
    }

    fn number(&mut self, start: usize) -> Lexed<Kind> {
        let radix = match (self.peek(0), self.peek(1) | 0x20) {
            (b'0', b'x') => 16,
            (b'0', b'o') => 8,
            (b'0', b'b') => 2,
            _ => 10,
        };
        if radix != 10 {
            self.at += 2;
            let digits_at = self.at;
            while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                if self.peek(0) == b'n' {
                    break;
                }
                self.at += 1;
            }
            let digits: String = self.src[digits_at..self.at]
                .chars()
                .filter(|c| *c != '_')
                .collect();
            if self.peek(0) == b'n' {
                self.at += 1;
                return Ok(Kind::BigInt(big_in_base(&digits, radix).into()));
            }
            let mut value = 0f64;
            for c in digits.chars() {
                let d = c
                    .to_digit(radix)
                    .ok_or(())
                    .or_else(|_| self.error("a digit not of its base", start))?;
                value = value * radix as f64 + d as f64;
            }
            return Ok(Kind::Number(value));
        }
        // A legacy octal number (`017`), outside strict code.
        if self.peek(0) == b'0' && self.peek(1).is_ascii_digit() {
            let mut end = self.at + 1;
            while self.bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            let digits = &self.src[self.at + 1..end];
            if digits.bytes().all(|d| d < b'8') {
                self.at = end;
                let value = digits
                    .bytes()
                    .fold(0f64, |v, d| v * 8.0 + (d - b'0') as f64);
                return Ok(Kind::Number(value));
            }
        }
        while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
            self.at += 1;
        }
        if self.peek(0) == b'n' {
            let digits: String = self.src[start..self.at]
                .chars()
                .filter(|c| *c != '_')
                .collect();
            self.at += 1;
            return Ok(Kind::BigInt(digits.into()));
        }
        if self.peek(0) == b'.' {
            self.at += 1;
            while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
                self.at += 1;
            }
        }
        if self.peek(0) | 0x20 == b'e'
            && (self.peek(1).is_ascii_digit()
                || (matches!(self.peek(1), b'+' | b'-') && self.peek(2).is_ascii_digit()))
        {
            self.at += 2;
            while self.peek(0).is_ascii_digit() {
                self.at += 1;
            }
        }
        let written: String = self.src[start..self.at]
            .chars()
            .filter(|c| *c != '_')
            .collect();
        written
            .parse::<f64>()
            .map(Kind::Number)
            .or_else(|_| self.error(format!("not a number: {written}"), start))
    }

    /// After `\u`: four hex digits or `{…}`.
    fn unicode_escape(&mut self) -> Lexed<u32> {
        if self.peek(0) == b'{' {
            let from = self.at + 1;
            let close = self.src[from..]
                .find('}')
                .ok_or(())
                .or_else(|_| self.error("a \\u{ is not closed", self.at))?;
            let value = u32::from_str_radix(&self.src[from..from + close], 16)
                .or_else(|_| self.error("not hex in \\u{}", self.at))?;
            self.at = from + close + 1;
            return Ok(value);
        }
        let digits = self
            .src
            .get(self.at..self.at + 4)
            .ok_or(())
            .or_else(|_| self.error("a \\u escape is cut short", self.at))?;
        let value =
            u32::from_str_radix(digits, 16).or_else(|_| self.error("not hex in \\u", self.at))?;
        self.at += 4;
        Ok(value)
    }

    /// An escape after its backslash, into `out` (UTF-16); `Err(())` where it is not a valid
    /// escape (a template's cooked value is then none).
    fn escape(&mut self, out: &mut Vec<u16>) -> Result<(), ()> {
        let b = self.peek(0);
        self.at += 1;
        let unit = |c: u16| Ok::<u16, ()>(c);
        let one = match b {
            b'n' => unit(b'\n' as u16),
            b't' => unit(b'\t' as u16),
            b'r' => unit(b'\r' as u16),
            b'b' => unit(8),
            b'f' => unit(12),
            b'v' => unit(11),
            b'0' if !self.peek(0).is_ascii_digit() => unit(0),
            b'0'..=b'7' => {
                // Legacy octal: up to three digits, at most 0o377.
                let mut value = (b - b'0') as u16;
                for _ in 0..2 {
                    let d = self.peek(0);
                    if (b'0'..=b'7').contains(&d) && value * 8 + ((d - b'0') as u16) < 256 {
                        value = value * 8 + (d - b'0') as u16;
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                unit(value)
            }
            b'x' => {
                let hex = self.src.get(self.at..self.at + 2).ok_or(())?;
                let value = u16::from_str_radix(hex, 16).map_err(|_| ())?;
                self.at += 2;
                unit(value)
            }
            b'u' => {
                let value = self.unicode_escape().map_err(|_| ())?;
                let c = char::from_u32(value);
                match c {
                    Some(c) => {
                        let mut buf = [0u16; 2];
                        out.extend_from_slice(c.encode_utf16(&mut buf));
                    }
                    // A lone surrogate, as written.
                    None if value <= 0xffff => out.push(value as u16),
                    None => return Err(()),
                }
                return Ok(());
            }
            // A line continuation.
            b'\r' => {
                if self.peek(0) == b'\n' {
                    self.at += 1;
                }
                return Ok(());
            }
            b'\n' => return Ok(()),
            _ => {
                // Any other character stands for itself.
                self.at -= 1;
                let c = self.char_here().ok_or(())?;
                self.at += c.len_utf8();
                if c == '\u{2028}' || c == '\u{2029}' {
                    return Ok(());
                }
                let mut buf = [0u16; 2];
                out.extend_from_slice(c.encode_utf16(&mut buf));
                return Ok(());
            }
        }?;
        out.push(one);
        Ok(())
    }

    fn string(&mut self, quote: u8) -> Lexed<Kind> {
        let from = self.at;
        self.at += 1;
        let mut out: Vec<u16> = Vec::new();
        loop {
            // The run of plain ASCII up to the next quote, backslash or line end, at once.
            let run_from = self.at;
            while let Some(&b) = self.bytes.get(self.at) {
                if b == quote || b == b'\\' || b == b'\n' || b == b'\r' || b >= 0x80 {
                    break;
                }
                self.at += 1;
            }
            out.extend(self.bytes[run_from..self.at].iter().map(|&b| b as u16));
            match self.bytes.get(self.at) {
                None | Some(b'\n' | b'\r') => return self.error("a string is not closed", from),
                Some(&b) if b == quote => {
                    self.at += 1;
                    return Ok(Kind::String(out.into()));
                }
                Some(b'\\') => {
                    self.at += 1;
                    if self.escape(&mut out).is_err() {
                        return self.error("an escape not in the language", self.at);
                    }
                }
                Some(_) => {
                    let c = self.char_here().unwrap_or('\u{fffd}');
                    self.at += c.len_utf8();
                    let mut buf = [0u16; 2];
                    out.extend_from_slice(c.encode_utf16(&mut buf));
                }
            }
        }
    }

    /// A template's piece, from just after its backtick or `}` to its next backtick or `${`.
    fn template_piece(&mut self) -> Lexed<Kind> {
        let from = self.at;
        let mut cooked: Option<Vec<u16>> = Some(Vec::new());
        loop {
            match self.bytes.get(self.at) {
                None => return self.error("a template is not closed", from),
                Some(b'`') => {
                    let raw = text(&self.src[from..self.at].replace("\r\n", "\n"));
                    self.at += 1;
                    return Ok(Kind::Template {
                        cooked: cooked.map(Into::into),
                        raw,
                        tail: true,
                    });
                }
                Some(b'$') if self.peek(1) == b'{' => {
                    let raw = text(&self.src[from..self.at].replace("\r\n", "\n"));
                    self.at += 2;
                    return Ok(Kind::Template {
                        cooked: cooked.map(Into::into),
                        raw,
                        tail: false,
                    });
                }
                Some(b'\\') => {
                    self.at += 1;
                    let mut piece = Vec::new();
                    if self.escape(&mut piece).is_err() {
                        cooked = None;
                    }
                    if let Some(c) = cooked.as_mut() {
                        c.extend(piece);
                    }
                }
                Some(b'\r') => {
                    self.at += 1;
                    if self.peek(0) == b'\n' {
                        self.at += 1;
                    }
                    if let Some(c) = cooked.as_mut() {
                        c.push(b'\n' as u16);
                    }
                }
                Some(_) => {
                    let ch = self.char_here().unwrap_or('\u{fffd}');
                    self.at += ch.len_utf8();
                    if let Some(c) = cooked.as_mut() {
                        let mut buf = [0u16; 2];
                        c.extend_from_slice(ch.encode_utf16(&mut buf));
                    }
                }
            }
        }
    }

    /// The token at `token` (a `/` or `/=` where a value is expected) read again as a pattern.
    pub fn pattern_at(&mut self, token: &Token) -> Lexed<Token> {
        self.at = token.start + 1;
        let mut in_class = false;
        loop {
            match self.bytes.get(self.at) {
                None | Some(b'\n' | b'\r') => {
                    return self.error("a pattern is not closed", token.start)
                }
                Some(b'\\') => self.at += 2,
                Some(b'[') => {
                    in_class = true;
                    self.at += 1;
                }
                Some(b']') => {
                    in_class = false;
                    self.at += 1;
                }
                Some(b'/') if !in_class => break,
                Some(_) => self.at += 1,
            }
        }
        let source: Rc<str> = self.src[token.start + 1..self.at].into();
        self.at += 1;
        let flags_from = self.at;
        while is_name_byte(self.peek(0)) {
            self.at += 1;
        }
        Ok(Token {
            kind: Kind::Pattern {
                source,
                flags: self.src[flags_from..self.at].into(),
            },
            start: token.start,
            end: self.at,
            newline_before: token.newline_before,
        })
    }

    /// The template's next piece, read from the `}` at `token` that closed a `${…}`.
    pub fn template_after(&mut self, token: &Token) -> Lexed<Token> {
        self.at = token.start + 1;
        let kind = self.template_piece()?;
        Ok(Token {
            kind,
            start: token.start,
            end: self.at,
            newline_before: token.newline_before,
        })
    }

    /// Where the cursor is, to come back to ([`Lexer::rewind`]).
    pub fn mark(&self) -> usize {
        self.at
    }

    /// Back to a place [`Lexer::mark`] gave.
    pub fn rewind(&mut self, at: usize) {
        self.at = at;
    }
}

fn is_name_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// `digits` in base `radix`, written in base ten (a BigInt literal's value).
fn big_in_base(digits: &str, radix: u32) -> String {
    // Base-ten digits, least significant first.
    let mut out: Vec<u32> = vec![0];
    for c in digits.chars() {
        let d = c.to_digit(radix).unwrap_or(0);
        let mut carry = d;
        for digit in out.iter_mut() {
            let v = *digit * radix + carry;
            *digit = v % 10;
            carry = v / 10;
        }
        while carry > 0 {
            out.push(carry % 10);
            carry /= 10;
        }
    }
    while out.len() > 1 && out.last() == Some(&0) {
        out.pop();
    }
    out.iter()
        .rev()
        .map(|d| char::from_digit(*d, 10).unwrap_or('0'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Kind> {
        let mut lexer = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let token = lexer.next_token().unwrap();
            if token.kind == Kind::End {
                return out;
            }
            out.push(token.kind);
        }
    }

    #[test]
    fn numbers_in_every_way_they_are_written() {
        assert_eq!(
            kinds("1 1.5 .5 1e3 0x1F 0o17 0b101 017 1_000 2n 0x10n"),
            vec![
                Kind::Number(1.0),
                Kind::Number(1.5),
                Kind::Number(0.5),
                Kind::Number(1000.0),
                Kind::Number(31.0),
                Kind::Number(15.0),
                Kind::Number(5.0),
                Kind::Number(15.0),
                Kind::Number(1000.0),
                Kind::BigInt("2".into()),
                Kind::BigInt("16".into()),
            ]
        );
    }

    #[test]
    fn strings_read_their_escapes_as_utf16() {
        assert_eq!(
            kinds(r#""a\nb" 'it\'s' "\x41B\u{1F600}" "\uD800""#),
            vec![
                Kind::String(text("a\nb")),
                Kind::String(text("it's")),
                Kind::String(text("AB\u{1F600}")),
                Kind::String(vec![0xD800].into()),
            ]
        );
    }

    #[test]
    fn the_longest_punctuation_is_taken_and_comments_are_skipped() {
        assert_eq!(
            kinds("a>>>=b /* x */ c?.d // e\n f??g"),
            vec![
                Kind::Name("a".into()),
                Kind::Punct(">>>="),
                Kind::Name("b".into()),
                Kind::Name("c".into()),
                Kind::Punct("?."),
                Kind::Name("d".into()),
                Kind::Name("f".into()),
                Kind::Punct("??"),
                Kind::Name("g".into()),
            ]
        );
    }

    #[test]
    fn a_slash_is_read_again_as_a_pattern_when_asked() {
        let mut lexer = Lexer::new("/a[/]b/gi.x");
        let slash = lexer.next_token().unwrap();
        assert_eq!(slash.kind, Kind::Punct("/"));
        let pattern = lexer.pattern_at(&slash).unwrap();
        assert_eq!(
            pattern.kind,
            Kind::Pattern {
                source: "a[/]b".into(),
                flags: "gi".into()
            }
        );
        assert_eq!(lexer.next_token().unwrap().kind, Kind::Punct("."));
    }

    #[test]
    fn a_template_comes_in_pieces() {
        let mut lexer = Lexer::new("`a${x}b`");
        let first = lexer.next_token().unwrap();
        assert!(matches!(first.kind, Kind::Template { tail: false, .. }));
        assert_eq!(lexer.next_token().unwrap().kind, Kind::Name("x".into()));
        let close = lexer.next_token().unwrap();
        let last = lexer.template_after(&close).unwrap();
        assert!(matches!(last.kind, Kind::Template { tail: true, .. }));
    }

    #[test]
    fn a_line_end_before_a_token_is_told() {
        let mut lexer = Lexer::new("a\nb c");
        assert!(!lexer.next_token().unwrap().newline_before);
        assert!(lexer.next_token().unwrap().newline_before);
        assert!(!lexer.next_token().unwrap().newline_before);
    }
}
