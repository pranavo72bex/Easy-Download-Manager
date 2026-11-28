// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Expressions, by precedence: assignment and arrows, the operators, calls and members, and
//! the primary ones (literals, templates, objects).

use super::*;

impl Parser<'_> {
    // ---- expressions

    pub(super) fn expression(&mut self) -> Parsed<Expr> {
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

    pub(super) fn assignment(&mut self) -> Parsed<Expr> {
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
    pub(super) fn to_pattern(&self, expr: Expr) -> Parsed<Pattern> {
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
    pub(super) fn left_hand(&mut self) -> Parsed<Expr> {
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
