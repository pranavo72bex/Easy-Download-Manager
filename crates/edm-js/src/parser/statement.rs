// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Statements: blocks, declarations, the loops, `try` and `switch`, functions and classes.

use super::*;

impl Parser<'_> {
    // ---- statements

    pub(super) fn statements_until_end(&mut self) -> Parsed<Vec<Stmt>> {
        let mut out = Vec::new();
        while self.tok.kind != Kind::End {
            out.push(self.statement()?);
        }
        Ok(out)
    }

    pub(super) fn block_body(&mut self) -> Parsed<Vec<Stmt>> {
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
    pub(super) fn function(&mut self, is_async: bool, declared: bool) -> Parsed<Rc<Function>> {
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
    pub(super) fn function_rest(
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

    pub(super) fn class(&mut self, declared: bool) -> Parsed<Rc<Class>> {
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
    pub(super) fn member(&mut self, start: usize, in_class: bool) -> Parsed<(PropKey, Member)> {
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
    pub(super) fn property_key(&mut self) -> Parsed<PropKey> {
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
}
