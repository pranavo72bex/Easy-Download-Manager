// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Tokens read as a program ([`parse`]): statements by recursive descent, expressions by
//! precedence. Where a line ends instead of a semicolon, the semicolon is taken as the language
//! takes it. An arrow's parameters are read as an expression first and turned into patterns at
//! the `=>`, as are an assignment's left side and a `for … in`'s.

mod expression;
mod statement;

use std::rc::Rc;

use crate::ast::*;
use crate::lexer::{text, Kind, LexError, Lexer, Token};

/// A program that could not be read: what and where (a byte of the text, and its line).
#[derive(Debug, Clone)]
pub struct SyntaxError {
    /// What was wrong.
    pub message: String,
    /// The line, from 1.
    pub line: usize,
    /// The byte in the text.
    pub at: usize,
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SyntaxError: {} (line {})", self.message, self.line)
    }
}

type Parsed<T> = Result<T, SyntaxError>;

/// `src` read as a program.
pub fn parse(src: &str) -> Parsed<Vec<Stmt>> {
    let mut parser = Parser::new(src)?;
    let body = parser.statements_until_end()?;
    Ok(body)
}

/// Names that are never a variable's.
const RESERVED: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "null",
    "true",
    "false",
    "enum",
];

struct Parser<'a> {
    lexer: Lexer<'a>,
    tok: Token,
    /// Where the token before ended.
    prev_end: usize,
    /// `in` is not an operator here (a `for`'s first part).
    no_in: bool,
    /// Inside a generator: `yield` is an operator.
    in_generator: bool,
    /// Inside an async function: `await` is an operator.
    in_async: bool,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Parsed<Self> {
        let mut lexer = Lexer::new(src);
        let tok = lexer.next_token().map_err(|e| lex_error(src, e))?;
        Ok(Parser {
            lexer,
            tok,
            prev_end: 0,
            no_in: false,
            in_generator: false,
            in_async: false,
        })
    }

    // ---- tokens

    fn error<T>(&self, message: impl Into<String>) -> Parsed<T> {
        let at = self.tok.start;
        Err(SyntaxError {
            message: message.into(),
            line: line_of(self.lexer.source(), at),
            at,
        })
    }

    fn unexpected<T>(&self) -> Parsed<T> {
        let what = match &self.tok.kind {
            Kind::End => "the end of the program".to_string(),
            Kind::Punct(p) => format!("'{p}'"),
            Kind::Name(n) => format!("'{n}'"),
            other => format!("{other:?}"),
        };
        self.error(format!("unexpected {what}"))
    }

    fn advance(&mut self) -> Parsed<()> {
        self.prev_end = self.tok.end;
        self.tok = self
            .lexer
            .next_token()
            .map_err(|e| lex_error(self.lexer.source(), e))?;
        Ok(())
    }

    /// The token after this one, without moving.
    fn peek(&mut self) -> Parsed<Token> {
        let mark = self.lexer.mark();
        let next = self
            .lexer
            .next_token()
            .map_err(|e| lex_error(self.lexer.source(), e))?;
        self.lexer.rewind(mark);
        Ok(next)
    }

    fn is(&self, punct: &str) -> bool {
        matches!(self.tok.kind, Kind::Punct(p) if p == punct)
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(&self.tok.kind, Kind::Name(n) if &**n == word)
    }

    fn eat(&mut self, punct: &str) -> Parsed<bool> {
        if self.is(punct) {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn eat_word(&mut self, word: &str) -> Parsed<bool> {
        if self.is_word(word) {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn expect(&mut self, punct: &str) -> Parsed<()> {
        if self.eat(punct)? {
            return Ok(());
        }
        let found = format!("{:?}", self.tok.kind);
        self.error(format!("'{punct}' expected, found {found}"))
    }

    fn expect_word(&mut self, word: &str) -> Parsed<()> {
        if self.eat_word(word)? {
            return Ok(());
        }
        self.error(format!("'{word}' expected"))
    }

    /// A statement's end: a `;`, or where the language puts one (a line end, a `}`, the end).
    fn semicolon(&mut self) -> Parsed<()> {
        if self.eat(";")? {
            return Ok(());
        }
        if self.is("}") || self.tok.kind == Kind::End || self.tok.newline_before {
            return Ok(());
        }
        self.unexpected()
    }

    /// A name that can be a variable's (not a reserved word), taken.
    fn identifier(&mut self) -> Parsed<Name> {
        match &self.tok.kind {
            Kind::Name(n) if !RESERVED.contains(&&**n) => {
                let n = n.clone();
                self.advance()?;
                Ok(n)
            }
            _ => self.unexpected(),
        }
    }

    fn at_identifier(&self) -> bool {
        matches!(&self.tok.kind, Kind::Name(n) if !RESERVED.contains(&&**n))
    }
}

/// A binary or logical operator, as the precedence loop holds it.
#[derive(Clone, Copy)]
enum Op {
    Binary(Binary),
    Logical(Logical),
}

/// A member of an object literal or class, before it is placed.
enum Member {
    Method(Rc<Function>),
    Getter(Rc<Function>),
    Setter(Rc<Function>),
    Field(Option<Expr>),
}

fn member(object: Expr, key: MemberKey, optional: bool) -> Expr {
    Expr::Member {
        object: Box::new(object),
        key: Box::new(key),
        optional,
    }
}

/// A function named by its key (`{f() {}}` is named `f`).
fn key_name(key: &PropKey) -> Option<Name> {
    match key {
        PropKey::Fixed(t) => Some(String::from_utf16_lossy(t).into()),
        PropKey::Private(n) => Some(format!("#{n}").into()),
        PropKey::Computed(_) => None,
    }
}

/// Whether a body starts with the directive `"use strict"`.
fn begins_strict(body: &[Stmt]) -> bool {
    for st in body {
        match st {
            Stmt::Expr(Expr::String(s)) => {
                if **s == *text("use strict") {
                    return true;
                }
            }
            _ => return false,
        }
    }
    false
}

fn line_of(src: &str, at: usize) -> usize {
    src.as_bytes()[..at.min(src.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

fn lex_error(src: &str, e: LexError) -> SyntaxError {
    SyntaxError {
        line: line_of(src, e.at),
        message: e.message,
        at: e.at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads(src: &str) {
        if let Err(e) = parse(src) {
            panic!("{src:?}: {e}");
        }
    }

    fn refuses(src: &str) {
        assert!(parse(src).is_err(), "{src:?} was read");
    }

    #[test]
    fn the_language_dm_meets_is_read() {
        for src in [
            "var a = 1, b; let c = [1, , 2, ...d]; const {x, y: z = 2, ...w} = o;",
            "function f(a, b = 1, ...c) { 'use strict'; return a + b * c ** 2 ** 1; }",
            "a = b ? c : d ? e : f; a ||= b; a ??= c; x = a?.b?.[c]?.(d);",
            "for (var i = 0, n = a.length; i < n; i++) if (i in o) continue; else break;",
            "for (const k in o) {} for (let v of a) {} for (x.y in o); for ([a, b] of c);",
            "out: for (;;) { switch (x) { case 1: break out; default: } }",
            "try { throw new Error('x'); } catch (e) {} finally {} try {} catch {}",
            "do x++; while (x < 3) y = function* g() { yield 1; yield* h(); };",
            "async function f() { await g(); } const h = async (a) => await a; k = async x => x;",
            "class A extends B { static s = 1; #p = 2; constructor() { super(); this.#p++; } get g() { return 1; } set g(v) {} static { A.t = 2; } *it() {} }",
            "o = { a, b: 2, [c]: 3, get d() { return 1; }, set d(v) {}, e() {}, async f() {}, *g() {}, ...h, 1.5: 2, 'q': 3 };",
            "x = (a, b) => a + b; y = () => ({}); z = (...r) => r; w = ([a], {b}) => a;",
            "s = `a${b}c${`d${e}`}`; t = tag`x${y}`; r = /[/]+\\/x/gi.test(s); q = a / b / c;",
            "if (a) b(); else c()\nd()\n++e\nreturn_ = 1",
            "new A; new A.B(); new (f())(); new.target; void 0; typeof x; delete o.p;",
            "a = b\n(c)",
            "x = 1n + 0x10n; y = .5 + 5. + 0b11 + 0o7 + 017 + 1e-3;",
            "var async = 1, of = 2, get = 3, set = 4, let_ = 5; async(1);",
        ] {
            reads(src);
        }
    }

    #[test]
    fn what_is_not_the_language_is_refused_with_its_line() {
        refuses("var = 1;");
        refuses("a b");
        refuses("function () {}");
        refuses("(a, b");
        refuses("x = 'open");
        let e = parse("a;\nb;\nc d").unwrap_err();
        assert_eq!(e.line, 3);
    }

    #[test]
    fn a_line_end_is_a_semicolon_where_the_language_says() {
        let body = parse("a\n++b").unwrap();
        assert_eq!(body.len(), 2);
        let body = parse("return_\n(x)").unwrap();
        assert_eq!(body.len(), 1, "a call across the line");
    }

    #[test]
    fn a_pattern_and_a_division_are_told_apart() {
        let body = parse("x = a / b / c; y = /b/g;").unwrap();
        let Stmt::Expr(Expr::Assign(_, _, ref value)) = body[0] else {
            panic!()
        };
        assert!(matches!(**value, Expr::Binary(Binary::Div, _, _)));
        let Stmt::Expr(Expr::Assign(_, _, ref value)) = body[1] else {
            panic!()
        };
        assert!(matches!(**value, Expr::Pattern(_, _)));
    }
}
