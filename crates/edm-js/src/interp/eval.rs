// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Expressions: names and `this`, optional chains and callees, object literals, the unary
//! operators, assignment to a place, and arrays and objects made.

use super::*;

impl Interp {
    // ---- expressions

    /// An expression's value, an anonymous function or class taking `name` (as `var f =
    /// function () {}` names it `f`).
    pub fn eval_named(&mut self, e: &Expr, scope: &Scope, name: Option<&Name>) -> Res<Value> {
        let v = self.eval(e, scope)?;
        if let (Some(name), Expr::Function(f)) = (name, e) {
            if f.name.is_none() {
                if let Value::Object(o) = &v {
                    o.define(
                        Key::str("name"),
                        Prop::data(Value::str(name), false, false, true),
                    );
                }
            }
        }
        Ok(v)
    }

    /// An expression's value.
    pub fn eval(&mut self, e: &Expr, scope: &Scope) -> Res<Value> {
        match e {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::BigInt(digits) => digits
                .parse::<i128>()
                .map(Value::BigInt)
                .map_err(|_| self.range_error("a BigInt too large for EDM")),
            Expr::Name(n) => {
                if &**n == "undefined" {
                    // Not changeable by a program, so read without a look.
                    if let Some(v) = self.find_local(scope, n) {
                        return Ok(v);
                    }
                    return Ok(Value::Undefined);
                }
                self.lookup(scope, n)
            }
            Expr::This => self.this_value(),
            Expr::NewTarget => Ok(self
                .frames
                .last()
                .map(|f| f.new_target.clone())
                .unwrap_or(Value::Undefined)),
            Expr::Super => Err(self.syntax_error("'super' keyword unexpected here")),
            Expr::Pattern(source, flags) => crate::builtins::regexp::make(self, source, flags),
            Expr::Template(template, values) => {
                let mut out: Vec<u16> = Vec::new();
                for (i, piece) in template.cooked.iter().enumerate() {
                    if let Some(p) = piece {
                        out.extend_from_slice(p);
                    }
                    if let Some(v) = values.get(i) {
                        let v = self.eval(v, scope)?;
                        out.extend_from_slice(&self.to_text(&v)?);
                    }
                }
                Ok(Value::String(out.into()))
            }
            Expr::Tagged(tag, template, values) => {
                let (f, this) = self.callee(tag, scope)?;
                let cooked: Vec<Value> = template
                    .cooked
                    .iter()
                    .map(|c| c.clone().map(Value::String).unwrap_or(Value::Undefined))
                    .collect();
                let raw: Vec<Value> = template.raw.iter().cloned().map(Value::String).collect();
                let strings = self.array(cooked);
                let raw = self.array(raw);
                if let Value::Object(o) = &strings {
                    o.set_hidden(Key::str("raw"), raw);
                }
                let mut args = vec![strings];
                for v in values {
                    args.push(self.eval(v, scope)?);
                }
                self.call(&f, this, &args)
            }
            Expr::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        None => out.push(Value::Undefined),
                        Some(Arg {
                            spread: true,
                            value,
                        }) => {
                            let v = self.eval(value, scope)?;
                            for x in self.iterate(v)? {
                                out.push(x?);
                            }
                        }
                        Some(Arg { value, .. }) => out.push(self.eval(value, scope)?),
                    }
                }
                Ok(self.array(out))
            }
            Expr::Object(props) => self.object_literal(props, scope),
            Expr::Function(f) => Ok(self.make_function(f, scope)),
            Expr::Class(c) => self.make_class(c, scope),
            Expr::ArrowParams(_) => Err(self.syntax_error("'=>' expected")),
            Expr::Unary(op, value) => self.unary(*op, value, scope),
            Expr::Update { prefix, up, target } => {
                let old = self.eval(target, scope)?;
                let old = self.to_numeric(&old)?;
                let new = match &old {
                    Value::BigInt(n) => Value::BigInt(if *up { n + 1 } else { n - 1 }),
                    Value::Number(n) => Value::Number(if *up { n + 1.0 } else { n - 1.0 }),
                    _ => Value::Number(f64::NAN),
                };
                self.assign_to(target, new.clone(), scope)?;
                Ok(if *prefix { new } else { old })
            }
            Expr::Binary(op, l, r) => {
                let l = self.eval(l, scope)?;
                let r = self.eval(r, scope)?;
                self.binary(*op, &l, &r)
            }
            Expr::Logical(op, l, r) => {
                let l = self.eval(l, scope)?;
                let short = match op {
                    Logical::And => !l.truthy(),
                    Logical::Or => l.truthy(),
                    Logical::Nullish => !l.is_nullish(),
                };
                if short {
                    Ok(l)
                } else {
                    self.eval(r, scope)
                }
            }
            Expr::Conditional(test, then, otherwise) => {
                if self.eval(test, scope)?.truthy() {
                    self.eval(then, scope)
                } else {
                    self.eval(otherwise, scope)
                }
            }
            Expr::Assign(op, target, value) => self.assignment(*op, target, value, scope),
            Expr::Sequence(all) => {
                let mut last = Value::Undefined;
                for e in all {
                    last = self.eval(e, scope)?;
                }
                Ok(last)
            }
            Expr::Member { .. } | Expr::Call { .. } | Expr::Chain(_) => {
                Ok(self.chain(e, scope)?.unwrap_or(Value::Undefined))
            }
            Expr::New(callee, args) => {
                let f = self.eval(callee, scope)?;
                let args = self.arguments(args, scope)?;
                self.construct(&f, &args, None)
            }
            Expr::Yield(..) => Err(self.syntax_error("generators are not supported by edm-js yet")),
            Expr::Await(value) => {
                let v = self.eval(value, scope)?;
                self.await_value(v)
            }
        }
    }

    /// `name` in a scope inside the function, without the global object.
    fn find_local(&self, scope: &Scope, name: &Name) -> Option<Value> {
        let mut at = scope.clone();
        loop {
            let next = {
                let s = at.borrow();
                if let Some(b) = s.vars.get(name) {
                    return Some(b.value.clone());
                }
                s.parent.clone()
            };
            at = next?;
        }
    }

    /// `this` in the code running now.
    pub fn this_value(&self) -> Res<Value> {
        match self.frames.last() {
            Some(Frame { this: Some(v), .. }) => Ok(v.clone()),
            Some(_) => Err(self.reference_error(
                "Must call super constructor in derived class before accessing 'this'",
            )),
            None => Ok(Value::Object(self.global.clone())),
        }
    }

    /// A member, call or optional chain: `None` where a `?.` met null or undefined.
    fn chain(&mut self, e: &Expr, scope: &Scope) -> Res<Option<Value>> {
        match e {
            Expr::Chain(inner) => self.chain(inner, scope),
            Expr::Member {
                object,
                key,
                optional,
            } => {
                if matches!(**object, Expr::Super) {
                    let key = self.member_key(key, scope)?;
                    return self.super_get(&key).map(Some);
                }
                let Some(o) = self.chain_part(object, scope)? else {
                    return Ok(None);
                };
                if *optional && o.is_nullish() {
                    return Ok(None);
                }
                let key = self.member_key(key, scope)?;
                self.get_of(&o, &key).map(Some)
            }
            Expr::Call {
                callee,
                args,
                optional,
            } => {
                let Some((f, this)) = self.chain_callee(callee, scope)? else {
                    return Ok(None);
                };
                if *optional && f.is_nullish() {
                    return Ok(None);
                }
                if matches!(**callee, Expr::Super) {
                    let args = self.arguments(args, scope)?;
                    return self.super_call(&args).map(Some);
                }
                let args = self.arguments(args, scope)?;
                if !matches!(&f, Value::Object(o) if o.is_callable()) {
                    let what = describe(callee);
                    return Err(self.type_error(&format!("{what} is not a function")));
                }
                self.call(&f, this, &args).map(Some)
            }
            other => self.eval(other, scope).map(Some),
        }
    }

    /// A chain's inner part (its object, its callee): `None` where it ended early.
    fn chain_part(&mut self, e: &Expr, scope: &Scope) -> Res<Option<Value>> {
        match e {
            Expr::Member { .. } | Expr::Call { .. } => self.chain(e, scope),
            other => self.eval(other, scope).map(Some),
        }
    }

    /// What a call calls, and its `this`.
    fn chain_callee(&mut self, callee: &Expr, scope: &Scope) -> Res<Option<(Value, Value)>> {
        match callee {
            Expr::Super => Ok(Some((Value::Undefined, Value::Undefined))),
            Expr::Member {
                object,
                key,
                optional,
            } => {
                if matches!(**object, Expr::Super) {
                    let key = self.member_key(key, scope)?;
                    let f = self.super_get(&key)?;
                    let this = self.this_value()?;
                    return Ok(Some((f, this)));
                }
                let Some(o) = self.chain_part(object, scope)? else {
                    return Ok(None);
                };
                if *optional && o.is_nullish() {
                    return Ok(None);
                }
                let key = self.member_key(key, scope)?;
                let f = self.get_of(&o, &key)?;
                Ok(Some((f, o)))
            }
            Expr::Chain(inner) => self.chain_callee(inner, scope),
            other => {
                let f = self.chain_part(other, scope)?;
                Ok(f.map(|f| (f, Value::Undefined)))
            }
        }
    }

    /// What `tag` (a tagged template's) calls, and its `this`.
    fn callee(&mut self, tag: &Expr, scope: &Scope) -> Res<(Value, Value)> {
        Ok(self
            .chain_callee(tag, scope)?
            .unwrap_or((Value::Undefined, Value::Undefined)))
    }

    /// A member's key.
    fn member_key(&mut self, key: &MemberKey, scope: &Scope) -> Res<Key> {
        match key {
            MemberKey::Name(n) => Ok(Key::String(text(n))),
            MemberKey::Private(n) => Ok(Key::String(text(&format!("#{n}")))),
            MemberKey::Computed(e) => {
                let v = self.eval(e, scope)?;
                self.to_key(&v)
            }
        }
    }

    /// Calls' arguments, spread ones spread.
    pub fn arguments(&mut self, args: &[Arg], scope: &Scope) -> Res<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            let v = self.eval(&a.value, scope)?;
            if a.spread {
                for x in self.iterate(v)? {
                    out.push(x?);
                }
            } else {
                out.push(v);
            }
        }
        Ok(out)
    }

    fn object_literal(&mut self, props: &[Prop_], scope: &Scope) -> Res<Value> {
        let o = Obj::new(Some(self.intrinsics.object_proto.clone()), Class::Ordinary);
        for prop in props {
            match prop {
                Prop_::Value(key, value) => {
                    let key = self.prop_key(key, scope)?;
                    let name = match &key {
                        Key::String(s) => Some(Name::from(String::from_utf16_lossy(s))),
                        Key::Symbol(_) => None,
                    };
                    let v = self.eval_named(value, scope, name.as_ref())?;
                    if let (Expr::Function(f), Value::Object(fo)) = (value, &v) {
                        if f.method {
                            self.set_home(fo, &o);
                        }
                    }
                    // `__proto__: value` sets the prototype.
                    if key == Key::str("__proto__") && !matches!(value, Expr::Function(_)) {
                        match &v {
                            Value::Object(p) => o.borrow_mut().proto = Some(p.clone()),
                            Value::Null => o.borrow_mut().proto = None,
                            _ => {}
                        }
                        continue;
                    }
                    o.set_own(key, v);
                }
                Prop_::Getter(key, f) | Prop_::Setter(key, f) => {
                    let key = self.prop_key(key, scope)?;
                    let fv = self.make_function(f, scope);
                    let Value::Object(fo) = fv else { continue };
                    self.set_home(&fo, &o);
                    let existing = o.own(&key);
                    let (mut get, mut set) = match existing.map(|p| p.slot) {
                        Some(Slot::Accessor(g, s)) => (g, s),
                        _ => (None, None),
                    };
                    if matches!(prop, Prop_::Getter(..)) {
                        get = Some(fo);
                    } else {
                        set = Some(fo);
                    }
                    o.define(
                        key,
                        Prop {
                            slot: Slot::Accessor(get, set),
                            enumerable: true,
                            configurable: true,
                        },
                    );
                }
                Prop_::Spread(value) => {
                    let v = self.eval(value, scope)?;
                    self.copy_own(&v, &o, &[])?;
                }
            }
        }
        Ok(Value::Object(o))
    }

    /// Copies `from`'s own enumerable properties onto `to`, but those in `except`.
    pub fn copy_own(&mut self, from: &Value, to: &Obj, except: &[Key]) -> Res<()> {
        let source = match from {
            Value::Undefined | Value::Null => return Ok(()),
            Value::Object(o) => o.clone(),
            other => self.to_object(other)?,
        };
        for key in source.own_keys(true) {
            if except.contains(&key) {
                continue;
            }
            let v = self.get(&Value::Object(source.clone()), &key)?;
            to.set_own(key, v);
        }
        Ok(())
    }

    /// A key as an object literal or class writes it.
    pub fn prop_key(&mut self, key: &PropKey, scope: &Scope) -> Res<Key> {
        match key {
            PropKey::Fixed(t) => Ok(Key::String(t.clone())),
            PropKey::Private(n) => Ok(Key::String(text(&format!("#{n}")))),
            PropKey::Computed(e) => {
                let v = self.eval(e, scope)?;
                self.to_key(&v)
            }
        }
    }

    fn unary(&mut self, op: Unary, value: &Expr, scope: &Scope) -> Res<Value> {
        match op {
            Unary::Typeof => {
                // An undeclared name is "undefined", not an error.
                if let Expr::Name(n) = value {
                    return Ok(match self.find(scope, n)? {
                        Some(v) => Value::str(v.type_of()),
                        None => Value::str("undefined"),
                    });
                }
                let v = self.eval(value, scope)?;
                Ok(Value::str(v.type_of()))
            }
            Unary::Delete => match value {
                Expr::Member { object, key, .. } => {
                    let o = self.eval(object, scope)?;
                    let key = self.member_key(key, scope)?;
                    match o {
                        Value::Object(o) => Ok(Value::Bool(o.delete(&key))),
                        _ => Ok(Value::Bool(true)),
                    }
                }
                Expr::Chain(inner) => self.unary(op, inner, scope),
                _ => {
                    self.eval(value, scope)?;
                    Ok(Value::Bool(true))
                }
            },
            Unary::Void => {
                self.eval(value, scope)?;
                Ok(Value::Undefined)
            }
            Unary::Not => Ok(Value::Bool(!self.eval(value, scope)?.truthy())),
            Unary::Neg => {
                let v = self.eval(value, scope)?;
                match self.to_numeric(&v)? {
                    Value::BigInt(n) => Ok(Value::BigInt(-n)),
                    Value::Number(n) => Ok(Value::Number(-n)),
                    _ => Ok(Value::Number(f64::NAN)),
                }
            }
            Unary::Plus => {
                let v = self.eval(value, scope)?;
                Ok(Value::Number(self.to_number(&v)?))
            }
            Unary::BitNot => {
                let v = self.eval(value, scope)?;
                match self.to_numeric(&v)? {
                    Value::BigInt(n) => Ok(Value::BigInt(!n)),
                    Value::Number(n) => Ok(Value::Number(!crate::value::to_int32(n) as f64)),
                    _ => Ok(Value::Number(-1.0)),
                }
            }
        }
    }

    fn assignment(
        &mut self,
        op: Assign,
        target: &Pattern,
        value: &Expr,
        scope: &Scope,
    ) -> Res<Value> {
        match op {
            Assign::Set => {
                let v = self.eval_named(value, scope, pattern_name(target))?;
                self.bind(target, v.clone(), scope, None)?;
                Ok(v)
            }
            Assign::With(bin) => {
                let place = pattern_expr(target);
                let old = self.read_place(target, scope)?;
                let r = self.eval(value, scope)?;
                let v = self.binary(bin, &old, &r)?;
                match place {
                    Some(e) => self.assign_to(e, v.clone(), scope)?,
                    None => self.bind(target, v.clone(), scope, None)?,
                }
                Ok(v)
            }
            Assign::Logical(logical) => {
                let old = self.read_place(target, scope)?;
                let keep = match logical {
                    Logical::And => !old.truthy(),
                    Logical::Or => old.truthy(),
                    Logical::Nullish => !old.is_nullish(),
                };
                if keep {
                    return Ok(old);
                }
                let v = self.eval(value, scope)?;
                self.bind(target, v.clone(), scope, None)?;
                Ok(v)
            }
        }
    }

    /// The value at an assignment's target (`a += 1` reads `a` first).
    fn read_place(&mut self, target: &Pattern, scope: &Scope) -> Res<Value> {
        match target {
            Pattern::Name(n) => self.lookup(scope, n),
            Pattern::Place(e) => self.eval(e, scope),
            _ => Err(self.syntax_error("Invalid left-hand side in assignment")),
        }
    }

    /// Puts `value` where the expression `target` (a name or a member) is.
    pub fn assign_to(&mut self, target: &Expr, value: Value, scope: &Scope) -> Res<()> {
        match target {
            Expr::Name(n) => self.assign_name(scope, n, value),
            Expr::Member { object, key, .. } => {
                let o = self.eval(object, scope)?;
                let key = self.member_key(key, scope)?;
                if o.is_nullish() {
                    let what = String::from_utf16_lossy(&self.key_text(&key));
                    return Err(self.type_error(&format!(
                        "Cannot set properties of {} (setting '{what}')",
                        if matches!(o, Value::Null) {
                            "null"
                        } else {
                            "undefined"
                        }
                    )));
                }
                self.set(&o, key, value)
            }
            _ => Err(self.syntax_error("Invalid left-hand side in assignment")),
        }
    }

    /// A key's text (a symbol's description).
    pub fn key_text(&self, key: &Key) -> Text {
        match key {
            Key::String(s) => s.clone(),
            Key::Symbol(s) => s.description.clone().unwrap_or_else(|| text("")),
        }
    }

    /// An array of `items`.
    pub fn array(&self, items: Vec<Value>) -> Value {
        Value::Object(Obj::new(
            Some(self.intrinsics.array_proto.clone()),
            Class::Array(items),
        ))
    }
}
