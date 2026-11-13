// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Tokens read as a program ([`parse`]): statements by recursive descent, expressions by
//! precedence. Where a line ends instead of a semicolon, the semicolon is taken as the language
//! takes it. An arrow's parameters are read as an expression first and turned into patterns at
//! the `=>`, as are an assignment's left side and a `for … in`'s.

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

    // ---- statements

    fn statements_until_end(&mut self) -> Parsed<Vec<Stmt>> {
        let mut out = Vec::new();
        while self.tok.kind != Kind::End {
            out.push(self.statement()?);
        }
        Ok(out)
    }

    fn block_body(&mut self) -> Parsed<Vec<Stmt>> {
        self.expect("{")?;
        let mut out = Vec::new();
        while !self.is("}") {
            if self.tok.kind == Kind::End {
                return self.error("a block is not closed");
            }
            out.push(self.statement()?);
        }
        self.advance()?;
        Ok(out)
    }

    fn statement(&mut self) -> Parsed<Stmt> {
        if let Kind::Punct(p) = self.tok.kind {
            match p {
                "{" => return Ok(Stmt::Block(self.block_body()?)),
                ";" => {
                    self.advance()?;
                    return Ok(Stmt::Empty);
                }
                _ => {}
            }
        }
        let word = match &self.tok.kind {
            Kind::Name(n) => Some(n.clone()),
            _ => None,
        };
        if let Some(word) = word {
            match &*word {
                "var" => {
                    self.advance()?;
                    let decls = self.declarations(VarKind::Var)?;
                    self.semicolon()?;
                    return Ok(Stmt::Var(VarKind::Var, decls));
                }
                "const" => {
                    self.advance()?;
                    let decls = self.declarations(VarKind::Const)?;
                    self.semicolon()?;
                    return Ok(Stmt::Var(VarKind::Const, decls));
                }
                "let" if self.let_declares()? => {
                    self.advance()?;
                    let decls = self.declarations(VarKind::Let)?;
                    self.semicolon()?;
                    return Ok(Stmt::Var(VarKind::Let, decls));
                }
                "function" => {
                    let f = self.function(false, true)?;
                    return Ok(Stmt::Function(f));
                }
                "async" => {
                    let next = self.peek()?;
                    if matches!(&next.kind, Kind::Name(n) if &**n == "function")
                        && !next.newline_before
                    {
                        self.advance()?;
                        let f = self.function(true, true)?;
                        return Ok(Stmt::Function(f));
                    }
                }
                "class" => return Ok(Stmt::Class(self.class(true)?)),
                "if" => {
                    self.advance()?;
                    self.expect("(")?;
                    let test = self.expression()?;
                    self.expect(")")?;
                    let then = Box::new(self.statement()?);
                    let otherwise = if self.eat_word("else")? {
                        Some(Box::new(self.statement()?))
                    } else {
                        None
                    };
                    return Ok(Stmt::If(test, then, otherwise));
                }
                "for" => return self.for_statement(),
                "while" => {
                    self.advance()?;
                    self.expect("(")?;
                    let test = self.expression()?;
                    self.expect(")")?;
                    return Ok(Stmt::While(test, Box::new(self.statement()?)));
                }
                "do" => {
                    self.advance()?;
                    let body = Box::new(self.statement()?);
                    self.expect_word("while")?;
                    self.expect("(")?;
                    let test = self.expression()?;
                    self.expect(")")?;
                    // A `;` after `do … while (…)` is optional, even on the same line.
                    self.eat(";")?;
                    return Ok(Stmt::DoWhile(body, test));
                }
                "return" => {
                    self.advance()?;
                    let value = if self.is(";")
                        || self.is("}")
                        || self.tok.newline_before
                        || self.tok.kind == Kind::End
                    {
                        None
                    } else {
                        Some(self.expression()?)
                    };
                    self.semicolon()?;
                    return Ok(Stmt::Return(value));
                }
                "break" | "continue" => {
                    self.advance()?;
                    let label = if self.at_identifier() && !self.tok.newline_before {
                        Some(self.identifier()?)
                    } else {
                        None
                    };
                    self.semicolon()?;
                    return Ok(if &*word == "break" {
                        Stmt::Break(label)
                    } else {
                        Stmt::Continue(label)
                    });
                }
                "throw" => {
                    self.advance()?;
                    if self.tok.newline_before {
                        return self.error("a line ends after throw");
                    }
                    let value = self.expression()?;
                    self.semicolon()?;
                    return Ok(Stmt::Throw(value));
                }
                "try" => return self.try_statement(),
                "switch" => return self.switch_statement(),
                "debugger" => {
                    self.advance()?;
                    self.semicolon()?;
                    return Ok(Stmt::Debugger);
                }
                _ => {}
            }
            // `label: statement`
            if !RESERVED.contains(&&*word) {
                let next = self.peek()?;
                if next.kind == Kind::Punct(":") {
                    self.advance()?;
                    self.advance()?;
                    let body = self.statement()?;
                    return Ok(Stmt::Labeled(word, Box::new(body)));
                }
            }
        }
        let expr = self.expression()?;
        self.semicolon()?;
        Ok(Stmt::Expr(expr))
    }

    /// `let` begins a declaration (not `let` used as a name): a name, `[` or `{` follows.
    fn let_declares(&mut self) -> Parsed<bool> {
        let next = self.peek()?;
        Ok(match next.kind {
            Kind::Name(n) => !matches!(&*n, "in" | "instanceof" | "of"),
            Kind::Punct("[" | "{") => true,
            _ => false,
        })
    }

    fn declarations(&mut self, kind: VarKind) -> Parsed<Vec<(Pattern, Option<Expr>)>> {
        let mut out = Vec::new();
        loop {
            let target = self.binding_target()?;
            let value = if self.eat("=")? {
                Some(self.assignment()?)
            } else {
                None
            };
            let _ = kind;
            out.push((target, value));
            if !self.eat(",")? {
                return Ok(out);
            }
        }
    }

    /// A name, `[…]` or `{…}` that a value is put in.
    fn binding_target(&mut self) -> Parsed<Pattern> {
        if self.is("[") {
            self.advance()?;
            let mut items = Vec::new();
            let mut rest = None;
            while !self.is("]") {
                if self.is(",") {
                    self.advance()?;
                    items.push(None);
                    continue;
                }
                if self.eat("...")? {
                    rest = Some(Box::new(self.binding_target()?));
                    self.eat(",")?;
                    continue;
                }
                let item = self.binding_element()?;
                items.push(Some(item));
                if !self.is("]") {
                    self.expect(",")?;
                }
            }
            self.advance()?;
            return Ok(Pattern::Array(items, rest));
        }
        if self.is("{") {
            self.advance()?;
            let mut props = Vec::new();
            let mut rest = None;
            while !self.is("}") {
                if self.eat("...")? {
                    rest = Some(Box::new(Pattern::Name(self.identifier()?)));
                    self.eat(",")?;
                    continue;
                }
                let shorthand = match &self.tok.kind {
                    Kind::Name(n) => Some(n.clone()),
                    _ => None,
                };
                let key = self.property_key()?;
                let target = if self.eat(":")? {
                    self.binding_element()?
                } else {
                    let name = shorthand.ok_or(()).or_else(|_| self.unexpected())?;
                    let plain = Pattern::Name(name);
                    if self.eat("=")? {
                        Pattern::Default(Box::new(plain), Box::new(self.assignment()?))
                    } else {
                        plain
                    }
                };
                props.push((key, target));
                if !self.is("}") {
                    self.expect(",")?;
                }
            }
            self.advance()?;
            return Ok(Pattern::Object(props, rest));
        }
        Ok(Pattern::Name(self.identifier()?))
    }

    /// A target with its default (`a = 1`).
    fn binding_element(&mut self) -> Parsed<Pattern> {
        let target = self.binding_target()?;
        if self.eat("=")? {
            return Ok(Pattern::Default(
                Box::new(target),
                Box::new(self.assignment()?),
            ));
        }
        Ok(target)
    }

    fn for_statement(&mut self) -> Parsed<Stmt> {
        self.advance()?;
        if self.is_word("await") {
            return self.error("for await is not supported");
        }
        self.expect("(")?;
        let mut init: Option<Box<Stmt>> = None;
        if self.is(";") {
            // nothing before the first turn
        } else {
            let kind = if self.is_word("var") {
                Some(VarKind::Var)
            } else if self.is_word("const") {
                Some(VarKind::Const)
            } else if self.is_word("let") && self.let_declares()? {
                Some(VarKind::Let)
            } else {
                None
            };
            if let Some(kind) = kind {
                self.advance()?;
                let outer = std::mem::replace(&mut self.no_in, true);
                let mut decls = self.declarations(kind)?;
                self.no_in = outer;
                let of = self.is_word("of");
                if (self.is_word("in") || of) && decls.len() == 1 && decls[0].1.is_none() {
                    self.advance()?;
                    let right = if of {
                        self.assignment()?
                    } else {
                        self.expression()?
                    };
                    self.expect(")")?;
                    let body = Box::new(self.statement()?);
                    let (target, _) = decls.pop().ok_or(()).or_else(|_| self.unexpected())?;
                    return Ok(Stmt::ForIn {
                        left: ForTarget::Declare(kind, target),
                        right,
                        body,
                        of,
                    });
                }
                init = Some(Box::new(Stmt::Var(kind, decls)));
            } else {
                let outer = std::mem::replace(&mut self.no_in, true);
                let expr = self.expression()?;
                self.no_in = outer;
                let of = self.is_word("of");
                if self.is_word("in") || of {
                    self.advance()?;
                    let target = self.to_pattern(expr)?;
                    let right = if of {
                        self.assignment()?
                    } else {
                        self.expression()?
                    };
                    self.expect(")")?;
                    let body = Box::new(self.statement()?);
                    return Ok(Stmt::ForIn {
                        left: ForTarget::Assign(target),
                        right,
                        body,
                        of,
                    });
                }
                init = Some(Box::new(Stmt::Expr(expr)));
            }
        }
        self.expect(";")?;
        let test = if self.is(";") {
            None
        } else {
            Some(self.expression()?)
        };
        self.expect(";")?;
        let update = if self.is(")") {
            None
        } else {
            Some(self.expression()?)
        };
        self.expect(")")?;
        let body = Box::new(self.statement()?);
        Ok(Stmt::For {
            init,
            test,
            update,
            body,
        })
    }

    fn try_statement(&mut self) -> Parsed<Stmt> {
        self.advance()?;
        let block = self.block_body()?;
        let handler = if self.eat_word("catch")? {
            let param = if self.eat("(")? {
                let p = self.binding_target()?;
                self.expect(")")?;
                Some(p)
            } else {
                None
            };
            Some((param, self.block_body()?))
        } else {
            None
        };
        let finalizer = if self.eat_word("finally")? {
            Some(self.block_body()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return self.error("try without catch or finally");
        }
        Ok(Stmt::Try {
            block,
            handler,
            finalizer,
        })
    }

    fn switch_statement(&mut self) -> Parsed<Stmt> {
        self.advance()?;
        self.expect("(")?;
        let value = self.expression()?;
        self.expect(")")?;
        self.expect("{")?;
        let mut cases = Vec::new();
        while !self.eat("}")? {
            let test = if self.eat_word("case")? {
                Some(self.expression()?)
            } else {
                self.expect_word("default")?;
                None
            };
            self.expect(":")?;
            let mut body = Vec::new();
            while !self.is_word("case") && !self.is_word("default") && !self.is("}") {
                if self.tok.kind == Kind::End {
                    return self.error("a switch is not closed");
                }
                body.push(self.statement()?);
            }
            cases.push((test, body));
        }
        Ok(Stmt::Switch(value, cases))
    }

    // ---- functions and classes

    /// `function name(params) { body }`, at `function`. `declared`: a statement, whose name is
    /// required.
    fn function(&mut self, is_async: bool, declared: bool) -> Parsed<Rc<Function>> {
        let start = self.tok.start;
        self.expect_word("function")?;
        let generator = self.eat("*")?;
        let name = if self.at_identifier() || self.is_word("yield") || self.is_word("await") {
            Some(self.identifier()?)
        } else if declared {
            return self.error("a function statement has no name");
        } else {
            None
        };
        self.function_rest(name, start, generator, is_async, false)
    }

    /// From a function's `(` to its `}`.
    fn function_rest(
        &mut self,
        name: Option<Name>,
        start: usize,
        generator: bool,
        is_async: bool,
        method: bool,
    ) -> Parsed<Rc<Function>> {
        let outer = (self.in_generator, self.in_async, self.no_in);
        self.in_generator = generator;
        self.in_async = is_async;
        self.no_in = false;
        self.expect("(")?;
        let mut params = Vec::new();
        let mut rest = None;
        while !self.is(")") {
            if self.eat("...")? {
                rest = Some(self.binding_target()?);
                self.eat(",")?;
                continue;
            }
            params.push(self.binding_element()?);
            if !self.is(")") {
                self.expect(",")?;
            }
        }
        self.advance()?;
        let body = self.block_body()?;
        (self.in_generator, self.in_async, self.no_in) = outer;
        Ok(Rc::new(Function {
            name,
            params,
            rest,
            strict: begins_strict(&body),
            body,
            arrow: false,
            generator,
            is_async,
            span: (start, self.prev_end),
            method,
        }))
    }

    fn class(&mut self, declared: bool) -> Parsed<Rc<Class>> {
        self.expect_word("class")?;
        let name = if self.at_identifier() && !self.is_word("extends") {
            Some(self.identifier()?)
        } else if declared {
            return self.error("a class statement has no name");
        } else {
            None
        };
        let extends = if self.eat_word("extends")? {
            Some(self.left_hand()?)
        } else {
            None
        };
        self.expect("{")?;
        let mut constructor = None;
        let mut members = Vec::new();
        while !self.eat("}")? {
            if self.eat(";")? {
                continue;
            }
            let start = self.tok.start;
            let mut is_static = false;
            if self.is_word("static") {
                let next = self.peek()?;
                if !matches!(next.kind, Kind::Punct("(" | "=" | ";" | "}")) {
                    self.advance()?;
                    is_static = true;
                    if self.is("{") {
                        let body = self.block_body()?;
                        let f = Rc::new(Function {
                            name: None,
                            params: Vec::new(),
                            rest: None,
                            body,
                            arrow: false,
                            generator: false,
                            is_async: false,
                            strict: true,
                            span: (start, self.prev_end),
                            method: true,
                        });
                        members.push(ClassMember {
                            is_static: true,
                            key: PropKey::Fixed(text("")),
                            kind: MemberKind::StaticBlock(f),
                        });
                        continue;
                    }
                }
            }
            let (key, kind) = self.member(start, true)?;
            let member = match kind {
                Member::Method(f) => {
                    let is_constructor = !is_static
                        && matches!(&key, PropKey::Fixed(k) if **k == *text("constructor"));
                    if is_constructor {
                        constructor = Some(f);
                        continue;
                    }
                    MemberKind::Method(f)
                }
                Member::Getter(f) => MemberKind::Getter(f),
                Member::Setter(f) => MemberKind::Setter(f),
                Member::Field(value) => {
                    self.semicolon()?;
                    MemberKind::Field(value)
                }
            };
            members.push(ClassMember {
                is_static,
                key,
                kind: member,
            });
        }
        Ok(Rc::new(Class {
            name,
            extends,
            constructor,
            members,
        }))
    }

    /// An object literal's or class's member after its `static`: a method, getter, setter, or
    /// (in a class) a field.
    fn member(&mut self, start: usize, in_class: bool) -> Parsed<(PropKey, Member)> {
        let mut is_async = false;
        let mut generator = false;
        let mut accessor: Option<&'static str> = None;
        if self.is_word("async") || self.is_word("get") || self.is_word("set") {
            let next = self.peek()?;
            let names_key = !matches!(next.kind, Kind::Punct("(" | ":" | "," | "}" | "=" | ";"))
                && !(self.is_word("async") && next.newline_before);
            if names_key {
                if self.is_word("async") {
                    is_async = true;
                } else if self.is_word("get") {
                    accessor = Some("get");
                } else {
                    accessor = Some("set");
                }
                self.advance()?;
            }
        }
        if self.eat("*")? {
            generator = true;
        }
        let key = if in_class && self.is("#") {
            self.advance()?;
            match &self.tok.kind {
                Kind::Name(n) => {
                    let n = n.clone();
                    self.advance()?;
                    PropKey::Private(n)
                }
                _ => return self.unexpected(),
            }
        } else {
            self.property_key()?
        };
        if self.is("(") {
            let name = key_name(&key);
            let f = self.function_rest(name, start, generator, is_async, true)?;
            return Ok((
                key,
                match accessor {
                    Some("get") => Member::Getter(f),
                    Some(_) => Member::Setter(f),
                    None => Member::Method(f),
                },
            ));
        }
        if in_class {
            let value = if self.eat("=")? {
                Some(self.assignment()?)
            } else {
                None
            };
            return Ok((key, Member::Field(value)));
        }
        self.unexpected()
    }

    /// A key: a name (any, keywords too), a string, a number, or `[expr]`.
    fn property_key(&mut self) -> Parsed<PropKey> {
        let key = match &self.tok.kind {
            Kind::Name(n) => PropKey::Fixed(text(n)),
            Kind::String(s) => PropKey::Fixed(s.clone()),
            Kind::Number(n) => PropKey::Fixed(text(&crate::number::to_string(*n))),
            Kind::BigInt(digits) => PropKey::Fixed(text(digits)),
            Kind::Punct("[") => {
                self.advance()?;
                let outer = std::mem::replace(&mut self.no_in, false);
                let e = self.assignment()?;
                self.no_in = outer;
                self.expect("]")?;
                return Ok(PropKey::Computed(Box::new(e)));
            }
            _ => return self.unexpected(),
        };
        self.advance()?;
        Ok(key)
    }

    // ---- expressions

    fn expression(&mut self) -> Parsed<Expr> {
        let first = self.assignment()?;
        if !self.is(",") {
            return Ok(first);
        }
        let mut all = vec![first];
        while self.eat(",")? {
            all.push(self.assignment()?);
        }
        Ok(Expr::Sequence(all))
    }

    fn assignment(&mut self) -> Parsed<Expr> {
        // `yield`, in a generator.
        if self.in_generator && self.is_word("yield") {
            self.advance()?;
            let delegate = !self.tok.newline_before && self.eat("*")?;
            let ends = self.tok.newline_before
                || matches!(
                    self.tok.kind,
                    Kind::Punct(")" | "]" | "}" | "," | ";" | ":")
                )
                || self.tok.kind == Kind::End;
            let value = if ends && !delegate {
                None
            } else {
                Some(Box::new(self.assignment()?))
            };
            return Ok(Expr::Yield(value, delegate));
        }
        // `x => …` and `async x => …`
        if self.at_identifier() {
            let next = self.peek()?;
            if next.kind == Kind::Punct("=>") && !next.newline_before {
                let start = self.tok.start;
                let name = self.identifier()?;
                return self.arrow(vec![Pattern::Name(name)], None, start, false);
            }
            if self.is_word("async") && !next.newline_before {
                if let Kind::Name(n) = &next.kind {
                    if !RESERVED.contains(&&**n) && &**n != "function" {
                        let start = self.tok.start;
                        let mark = (self.lexer.mark(), self.tok.clone(), self.prev_end);
                        self.advance()?;
                        let name = self.identifier()?;
                        if self.is("=>") && !self.tok.newline_before {
                            return self.arrow(vec![Pattern::Name(name)], None, start, true);
                        }
                        self.restore(mark);
                    }
                }
            }
        }
        let start = self.tok.start;
        let is_async_call = self.is_word("async");
        let left = self.conditional()?;
        // `(a, b) => …`: the parenthesised list read as an expression, turned into parameters.
        if self.is("=>") && !self.tok.newline_before {
            let (params, rest, is_async) = self.arrow_params(left, is_async_call)?;
            return self.arrow(params, rest, start, is_async);
        }
        let op = match self.tok.kind {
            Kind::Punct("=") => Assign::Set,
            Kind::Punct("+=") => Assign::With(Binary::Add),
            Kind::Punct("-=") => Assign::With(Binary::Sub),
            Kind::Punct("*=") => Assign::With(Binary::Mul),
            Kind::Punct("/=") => Assign::With(Binary::Div),
            Kind::Punct("%=") => Assign::With(Binary::Rem),
            Kind::Punct("**=") => Assign::With(Binary::Pow),
            Kind::Punct("<<=") => Assign::With(Binary::Shl),
            Kind::Punct(">>=") => Assign::With(Binary::Shr),
            Kind::Punct(">>>=") => Assign::With(Binary::UShr),
            Kind::Punct("&=") => Assign::With(Binary::BitAnd),
            Kind::Punct("|=") => Assign::With(Binary::BitOr),
            Kind::Punct("^=") => Assign::With(Binary::BitXor),
            Kind::Punct("&&=") => Assign::Logical(Logical::And),
            Kind::Punct("||=") => Assign::Logical(Logical::Or),
            Kind::Punct("??=") => Assign::Logical(Logical::Nullish),
            _ => return Ok(left),
        };
        self.advance()?;
        let target = if op == Assign::Set {
            self.to_pattern(left)?
        } else {
            match left {
                Expr::Name(n) => Pattern::Name(n),
                place @ Expr::Member { .. } => Pattern::Place(Box::new(place)),
                _ => return self.error("the left side cannot be assigned to"),
            }
        };
        let value = self.assignment()?;
        Ok(Expr::Assign(op, Box::new(target), Box::new(value)))
    }

    fn restore(&mut self, mark: (usize, Token, usize)) {
        self.lexer.rewind(mark.0);
        self.tok = mark.1;
        self.prev_end = mark.2;
    }

    /// The parameters an arrow's `(…)` held, read before as `left`.
    fn arrow_params(
        &mut self,
        left: Expr,
        maybe_async: bool,
    ) -> Parsed<(Vec<Pattern>, Option<Pattern>, bool)> {
        let (items, is_async) = match left {
            // `async (a, b) => …` was read as a call of `async`.
            Expr::Call { callee, args, .. }
                if maybe_async && matches!(&*callee, Expr::Name(n) if &**n == "async") =>
            {
                (args, true)
            }
            Expr::ArrowParams(items) => (items, false),
            _ => return self.error("not an arrow function's parameters"),
        };
        let mut params = Vec::new();
        let mut rest = None;
        let count = items.len();
        for (i, arg) in items.into_iter().enumerate() {
            let p = self.to_pattern(arg.value)?;
            if arg.spread {
                if i + 1 != count {
                    return self.error("a rest parameter is not last");
                }
                rest = Some(p);
            } else {
                params.push(p);
            }
        }
        Ok((params, rest, is_async))
    }

    fn arrow(
        &mut self,
        params: Vec<Pattern>,
        rest: Option<Pattern>,
        start: usize,
        is_async: bool,
    ) -> Parsed<Expr> {
        self.expect("=>")?;
        let outer = (self.in_generator, self.in_async, self.no_in);
        self.in_generator = false;
        self.in_async = is_async;
        let body = if self.is("{") {
            self.no_in = false;
            self.block_body()?
        } else {
            vec![Stmt::Return(Some(self.assignment()?))]
        };
        (self.in_generator, self.in_async, self.no_in) = outer;
        Ok(Expr::Function(Rc::new(Function {
            name: None,
            params,
            rest,
            strict: begins_strict(&body),
            body,
            arrow: true,
            generator: false,
            is_async,
            span: (start, self.prev_end),
            method: false,
        })))
    }

    /// An expression read as a target (`[a, b] = …`, `({x} = …)`, `(a, b) =>`).
    fn to_pattern(&self, expr: Expr) -> Parsed<Pattern> {
        Ok(match expr {
            Expr::Name(n) => Pattern::Name(n),
            place @ Expr::Member { .. } => Pattern::Place(Box::new(place)),
            Expr::Assign(Assign::Set, target, value) => Pattern::Default(target, value),
            Expr::Array(items) => {
                let mut out = Vec::new();
                let mut rest = None;
                for item in items {
                    match item {
                        None => out.push(None),
                        Some(Arg {
                            spread: true,
                            value,
                        }) => rest = Some(Box::new(self.to_pattern(value)?)),
                        Some(Arg { value, .. }) => out.push(Some(self.to_pattern(value)?)),
                    }
                }
                Pattern::Array(out, rest)
            }
            Expr::Object(props) => {
                let mut out = Vec::new();
                let mut rest = None;
                for prop in props {
                    match prop {
                        Prop::Value(key, value) => out.push((key, self.to_pattern(value)?)),
                        Prop::Spread(value) => rest = Some(Box::new(self.to_pattern(value)?)),
                        _ => return self.error("a getter or setter in a pattern"),
                    }
                }
                Pattern::Object(out, rest)
            }
            _ => return self.error("not something a value can be put in"),
        })
    }

    fn conditional(&mut self) -> Parsed<Expr> {
        let test = self.binary(1)?;
        if !self.eat("?")? {
            return Ok(test);
        }
        let outer = std::mem::replace(&mut self.no_in, false);
        let then = self.assignment()?;
        self.no_in = outer;
        self.expect(":")?;
        let otherwise = self.assignment()?;
        Ok(Expr::Conditional(
            Box::new(test),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    /// The operator here, its precedence and what it is, where it is a binary or logical one.
    fn binary_op(&self) -> Option<(u8, Op)> {
        let op = match &self.tok.kind {
            Kind::Punct(p) => match *p {
                "??" => (1, Op::Logical(Logical::Nullish)),
                "||" => (1, Op::Logical(Logical::Or)),
                "&&" => (2, Op::Logical(Logical::And)),
                "|" => (3, Op::Binary(Binary::BitOr)),
                "^" => (4, Op::Binary(Binary::BitXor)),
                "&" => (5, Op::Binary(Binary::BitAnd)),
                "==" => (6, Op::Binary(Binary::Eq)),
                "!=" => (6, Op::Binary(Binary::Ne)),
                "===" => (6, Op::Binary(Binary::StrictEq)),
                "!==" => (6, Op::Binary(Binary::StrictNe)),
                "<" => (7, Op::Binary(Binary::Lt)),
                ">" => (7, Op::Binary(Binary::Gt)),
                "<=" => (7, Op::Binary(Binary::Le)),
                ">=" => (7, Op::Binary(Binary::Ge)),
                "<<" => (8, Op::Binary(Binary::Shl)),
                ">>" => (8, Op::Binary(Binary::Shr)),
                ">>>" => (8, Op::Binary(Binary::UShr)),
                "+" => (9, Op::Binary(Binary::Add)),
                "-" => (9, Op::Binary(Binary::Sub)),
                "*" => (10, Op::Binary(Binary::Mul)),
                "/" => (10, Op::Binary(Binary::Div)),
                "%" => (10, Op::Binary(Binary::Rem)),
                "**" => (11, Op::Binary(Binary::Pow)),
                _ => return None,
            },
            Kind::Name(n) => match &**n {
                "instanceof" => (7, Op::Binary(Binary::Instanceof)),
                "in" if !self.no_in => (7, Op::Binary(Binary::In)),
                _ => return None,
            },
            _ => return None,
        };
        Some(op)
    }

    /// Operators of precedence `min` and above, by precedence climbing: the left ones first,
    /// `**` its right one first.
    fn binary(&mut self, min: u8) -> Parsed<Expr> {
        let mut left = self.unary()?;
        while let Some((prec, op)) = self.binary_op() {
            if prec < min {
                break;
            }
            self.advance()?;
            let next = if prec == 11 { prec } else { prec + 1 };
            let right = self.binary(next)?;
            left = match op {
                Op::Binary(b) => Expr::Binary(b, Box::new(left), Box::new(right)),
                Op::Logical(l) => Expr::Logical(l, Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Parsed<Expr> {
        let op = match &self.tok.kind {
            Kind::Punct("!") => Some(Unary::Not),
            Kind::Punct("-") => Some(Unary::Neg),
            Kind::Punct("+") => Some(Unary::Plus),
            Kind::Punct("~") => Some(Unary::BitNot),
            Kind::Name(n) => match &**n {
                "typeof" => Some(Unary::Typeof),
                "void" => Some(Unary::Void),
                "delete" => Some(Unary::Delete),
                _ => None,
            },
            _ => None,
        };
        if let Some(op) = op {
            self.advance()?;
            let value = self.unary()?;
            return Ok(Expr::Unary(op, Box::new(value)));
        }
        if self.in_async && self.is_word("await") {
            self.advance()?;
            let value = self.unary()?;
            return Ok(Expr::Await(Box::new(value)));
        }
        if self.is("++") || self.is("--") {
            let up = self.is("++");
            self.advance()?;
            let target = self.unary()?;
            return Ok(Expr::Update {
                prefix: true,
                up,
                target: Box::new(target),
            });
        }
        let value = self.left_hand_calls()?;
        if (self.is("++") || self.is("--")) && !self.tok.newline_before {
            let up = self.is("++");
            self.advance()?;
            return Ok(Expr::Update {
                prefix: false,
                up,
                target: Box::new(value),
            });
        }
        Ok(value)
    }

    /// A member expression without calls (`new`'s callee, `extends`'s class).
    fn left_hand(&mut self) -> Parsed<Expr> {
        let mut expr = self.primary_or_new()?;
        loop {
            if self.eat(".")? {
                expr = self.dot(expr, false)?;
            } else if self.is("[") {
                self.advance()?;
                let key = self.inner_expression()?;
                self.expect("]")?;
                expr = member(expr, MemberKey::Computed(key), false);
            } else if matches!(self.tok.kind, Kind::Template { .. }) {
                let (template, values) = self.template()?;
                expr = Expr::Tagged(Box::new(expr), template, values);
            } else {
                return Ok(expr);
            }
        }
    }

    /// An expression inside brackets, where `in` is an operator again.
    fn inner_expression(&mut self) -> Parsed<Expr> {
        let outer = std::mem::replace(&mut self.no_in, false);
        let e = self.expression();
        self.no_in = outer;
        e
    }

    fn dot(&mut self, object: Expr, optional: bool) -> Parsed<Expr> {
        if self.eat("#")? {
            if let Kind::Name(n) = &self.tok.kind {
                let n = n.clone();
                self.advance()?;
                return Ok(member(object, MemberKey::Private(n), optional));
            }
            return self.unexpected();
        }
        match &self.tok.kind {
            Kind::Name(n) => {
                let n = n.clone();
                self.advance()?;
                Ok(member(object, MemberKey::Name(n), optional))
            }
            _ => self.unexpected(),
        }
    }

    /// Members, calls and tagged templates after a primary expression.
    fn left_hand_calls(&mut self) -> Parsed<Expr> {
        let mut expr = self.primary_or_new()?;
        let mut chained = false;
        loop {
            if self.eat(".")? {
                expr = self.dot(expr, false)?;
            } else if self.eat("?.")? {
                chained = true;
                if self.is("(") {
                    let args = self.arguments()?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                        optional: true,
                    };
                } else if self.is("[") {
                    self.advance()?;
                    let key = self.inner_expression()?;
                    self.expect("]")?;
                    expr = member(expr, MemberKey::Computed(key), true);
                } else {
                    expr = self.dot(expr, true)?;
                }
            } else if self.is("[") {
                self.advance()?;
                let key = self.inner_expression()?;
                self.expect("]")?;
                expr = member(expr, MemberKey::Computed(key), false);
            } else if self.is("(") {
                let args = self.arguments()?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    optional: false,
                };
            } else if matches!(self.tok.kind, Kind::Template { .. }) {
                let (template, values) = self.template()?;
                expr = Expr::Tagged(Box::new(expr), template, values);
            } else {
                break;
            }
        }
        if chained {
            expr = Expr::Chain(Box::new(expr));
        }
        Ok(expr)
    }

    fn arguments(&mut self) -> Parsed<Vec<Arg>> {
        self.expect("(")?;
        let outer = std::mem::replace(&mut self.no_in, false);
        let mut args = Vec::new();
        while !self.is(")") {
            let spread = self.eat("...")?;
            args.push(Arg {
                spread,
                value: self.assignment()?,
            });
            if !self.is(")") {
                self.expect(",")?;
            }
        }
        self.no_in = outer;
        self.advance()?;
        Ok(args)
    }

    fn primary_or_new(&mut self) -> Parsed<Expr> {
        if !self.is_word("new") {
            return self.primary();
        }
        self.advance()?;
        if self.eat(".")? {
            self.expect_word("target")?;
            return Ok(Expr::NewTarget);
        }
        let callee = self.left_hand()?;
        let args = if self.is("(") {
            self.arguments()?
        } else {
            Vec::new()
        };
        Ok(Expr::New(Box::new(callee), args))
    }

    fn template(&mut self) -> Parsed<(Rc<Template>, Vec<Expr>)> {
        let mut cooked = Vec::new();
        let mut raw = Vec::new();
        let mut values = Vec::new();
        loop {
            let Kind::Template {
                cooked: c,
                raw: r,
                tail,
            } = self.tok.kind.clone()
            else {
                return self.unexpected();
            };
            cooked.push(c);
            raw.push(r);
            if tail {
                self.advance()?;
                break;
            }
            self.advance()?;
            values.push(self.inner_expression()?);
            if !self.is("}") {
                return self.error("a template's ${ is not closed");
            }
            self.tok = self
                .lexer
                .template_after(&self.tok)
                .map_err(|e| lex_error(self.lexer.source(), e))?;
        }
        Ok((Rc::new(Template { cooked, raw }), values))
    }

    fn primary(&mut self) -> Parsed<Expr> {
        let tok = self.tok.clone();
        match tok.kind {
            Kind::Number(n) => {
                self.advance()?;
                Ok(Expr::Number(n))
            }
            Kind::String(s) => {
                self.advance()?;
                Ok(Expr::String(s))
            }
            Kind::BigInt(digits) => {
                self.advance()?;
                Ok(Expr::BigInt(digits))
            }
            Kind::Template { .. } => {
                let (template, values) = self.template()?;
                Ok(Expr::Template(template, values))
            }
            Kind::Punct("/" | "/=") => {
                self.tok = self
                    .lexer
                    .pattern_at(&tok)
                    .map_err(|e| lex_error(self.lexer.source(), e))?;
                let Kind::Pattern { source, flags } = self.tok.kind.clone() else {
                    return self.unexpected();
                };
                self.advance()?;
                Ok(Expr::Pattern(source, flags))
            }
            Kind::Punct("(") => {
                self.advance()?;
                // `()` or `(a, ...b)`: only an arrow's parameters look so.
                let outer = std::mem::replace(&mut self.no_in, false);
                let mut items = Vec::new();
                let mut arrow_only = false;
                while !self.is(")") {
                    let spread = self.eat("...")?;
                    arrow_only |= spread;
                    items.push(Arg {
                        spread,
                        value: self.assignment()?,
                    });
                    if !self.is(")") {
                        self.expect(",")?;
                        if self.is(")") {
                            arrow_only = true;
                        }
                    }
                }
                self.no_in = outer;
                self.advance()?;
                if items.is_empty() || arrow_only || self.is("=>") {
                    if !self.is("=>") {
                        return self.error("'=>' expected");
                    }
                    return Ok(Expr::ArrowParams(items));
                }
                let mut values: Vec<Expr> = items.into_iter().map(|a| a.value).collect();
                Ok(if values.len() == 1 {
                    values.remove(0)
                } else {
                    Expr::Sequence(values)
                })
            }
            Kind::Punct("[") => {
                self.advance()?;
                let outer = std::mem::replace(&mut self.no_in, false);
                let mut items = Vec::new();
                while !self.is("]") {
                    if self.is(",") {
                        self.advance()?;
                        items.push(None);
                        continue;
                    }
                    let spread = self.eat("...")?;
                    items.push(Some(Arg {
                        spread,
                        value: self.assignment()?,
                    }));
                    if !self.is("]") {
                        self.expect(",")?;
                    }
                }
                self.no_in = outer;
                self.advance()?;
                Ok(Expr::Array(items))
            }
            Kind::Punct("{") => self.object(),
            Kind::Punct("#") => {
                // `#x in obj`
                self.advance()?;
                let name = self.identifier()?;
                Ok(Expr::String(text(&format!("#{name}"))))
            }
            Kind::Name(n) => match &*n {
                "this" => {
                    self.advance()?;
                    Ok(Expr::This)
                }
                "null" => {
                    self.advance()?;
                    Ok(Expr::Null)
                }
                "true" | "false" => {
                    self.advance()?;
                    Ok(Expr::Bool(&*n == "true"))
                }
                "function" => Ok(Expr::Function(self.function(false, false)?)),
                "class" => Ok(Expr::Class(self.class(false)?)),
                "super" => {
                    self.advance()?;
                    Ok(Expr::Super)
                }
                "async" => {
                    let next = self.peek()?;
                    if matches!(&next.kind, Kind::Name(f) if &**f == "function")
                        && !next.newline_before
                    {
                        self.advance()?;
                        return Ok(Expr::Function(self.function(true, false)?));
                    }
                    self.advance()?;
                    Ok(Expr::Name(n))
                }
                _ if RESERVED.contains(&&*n) => self.unexpected(),
                _ => {
                    self.advance()?;
                    Ok(Expr::Name(n))
                }
            },
            _ => self.unexpected(),
        }
    }

    fn object(&mut self) -> Parsed<Expr> {
        self.expect("{")?;
        let outer = std::mem::replace(&mut self.no_in, false);
        let mut props = Vec::new();
        while !self.is("}") {
            if self.eat("...")? {
                props.push(Prop::Spread(self.assignment()?));
            } else {
                let start = self.tok.start;
                // `a` alone, or `a = 1` (a pattern's default, read as an assignment).
                let plain = match &self.tok.kind {
                    Kind::Name(n) if !RESERVED.contains(&&**n) => Some(n.clone()),
                    _ => None,
                };
                if let Some(name) = plain {
                    let next = self.peek()?;
                    if matches!(next.kind, Kind::Punct("," | "}")) {
                        self.advance()?;
                        props.push(Prop::Value(PropKey::Fixed(text(&name)), Expr::Name(name)));
                        if !self.is("}") {
                            self.expect(",")?;
                        }
                        continue;
                    }
                    if next.kind == Kind::Punct("=") {
                        self.advance()?;
                        self.advance()?;
                        let value = self.assignment()?;
                        props.push(Prop::Value(
                            PropKey::Fixed(text(&name)),
                            Expr::Assign(
                                Assign::Set,
                                Box::new(Pattern::Name(name)),
                                Box::new(value),
                            ),
                        ));
                        if !self.is("}") {
                            self.expect(",")?;
                        }
                        continue;
                    }
                }
                let is_plain_key = !self.is_word("get")
                    && !self.is_word("set")
                    && !self.is_word("async")
                    && !self.is("*");
                if is_plain_key {
                    let key = self.property_key()?;
                    if self.eat(":")? {
                        props.push(Prop::Value(key, self.assignment()?));
                    } else {
                        let name = key_name(&key);
                        let f = self.function_rest(name, start, false, false, true)?;
                        props.push(Prop::Value(key, Expr::Function(f)));
                    }
                } else {
                    // `get: 1`, `get() {}`, `get x() {}`, `async x() {}`, `*x() {}`.
                    let next = self.peek()?;
                    if !self.is("*") && next.kind == Kind::Punct(":") {
                        let key = self.property_key()?;
                        self.advance()?;
                        props.push(Prop::Value(key, self.assignment()?));
                    } else {
                        let (key, kind) = self.member(start, false)?;
                        props.push(match kind {
                            Member::Method(f) => Prop::Value(key, Expr::Function(f)),
                            Member::Getter(f) => Prop::Getter(key, f),
                            Member::Setter(f) => Prop::Setter(key, f),
                            Member::Field(_) => return self.unexpected(),
                        });
                    }
                }
            }
            if !self.is("}") {
                self.expect(",")?;
            }
        }
        self.no_in = outer;
        self.advance()?;
        Ok(Expr::Object(props))
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
