// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The interpreter: a program's statements run in order, its expressions worked out as the
//! language says, over scopes of names, objects and their prototypes. A thrown value travels
//! back as `Err`, a `return`, `break` or `continue` as a [`Flow`].
//!
//! The parts:
//! - here: scopes, statements and expressions;
//! - `ops`: the operators and the conversions that may run a program's own code;
//! - `call`: calling and constructing, functions and classes made;
//! - `bind`: destructuring, and iteration.
//!
//! A program cannot run for ever or fill the memory: each step counts against a limit of time
//! ([`Interp::deadline`]) and calls are held to a depth.

mod bind;
mod call;
mod ops;

pub use ops::{js_pow, js_rem, Hint};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crate::ast::*;
use crate::lexer::{text, Text};
use crate::object::{Class, Obj, Prop, Slot};
use crate::realm::Intrinsics;
use crate::value::{Key, Value};

/// A result: the value, or what was thrown.
pub type Res<T> = Result<T, Value>;

/// How a statement ended, when not by running to its end or throwing.
#[derive(Debug)]
pub enum Flow {
    /// It ran to its end.
    Normal,
    /// `return value`
    Return(Value),
    /// `break label`
    Break(Option<Name>),
    /// `continue label`
    Continue(Option<Name>),
}

/// A scope: its names and the scope around it.
pub type Scope = Rc<RefCell<ScopeData>>;

/// A scope's names.
pub struct ScopeData {
    vars: HashMap<Name, Binding>,
    parent: Option<Scope>,
    /// The program's own scope: its `var`s and functions are the global object's properties.
    global: bool,
}

/// A name's value, and whether it can be changed.
pub struct Binding {
    value: Value,
    mutable: bool,
}

/// A function's call, as its body sees it.
pub struct Frame {
    /// `this`, where it has been set (a derived class's constructor has none until `super()`).
    pub this: Option<Value>,
    /// The function called.
    pub function: Option<Obj>,
    /// `new.target`
    pub new_target: Value,
    /// Where `super` looks: the method's object.
    pub home: Option<Obj>,
}

/// The interpreter: the program's world and where it is in it.
pub struct Interp {
    /// The global object (`window`, `globalThis`, `this` at the top).
    pub global: Obj,
    /// The program's own scope.
    pub global_scope: Scope,
    /// The built-in prototypes and constructors.
    pub intrinsics: Intrinsics,
    /// The calls under way.
    pub frames: Vec<Frame>,
    /// When the program must stop.
    pub deadline: Option<Instant>,
    steps: u32,
    /// Whether each function's code names `arguments` (worked out once per function).
    uses_arguments: HashMap<*const Function, bool>,
    /// The program's text (for a function's `toString`).
    pub source: Rc<str>,
    /// What a promise's settling runs, in turn, once the program's own code has run.
    pub jobs: std::collections::VecDeque<(Obj, Value)>,
}

/// The deepest calls go.
const MOST_DEPTH: usize = 400;

impl Interp {
    /// An interpreter with the language's built-in objects and nothing else.
    pub fn new() -> Interp {
        let (global, intrinsics) = crate::realm::make();
        let global_scope = Rc::new(RefCell::new(ScopeData {
            vars: HashMap::new(),
            parent: None,
            global: true,
        }));
        let mut interp = Interp {
            global,
            global_scope,
            intrinsics,
            frames: Vec::new(),
            deadline: None,
            steps: 0,
            uses_arguments: HashMap::new(),
            source: "".into(),
            jobs: Default::default(),
        };
        crate::realm::fill(&mut interp);
        interp
    }

    /// Runs `src` as a program; the value of its last expression statement.
    pub fn run(&mut self, src: &str) -> Res<Value> {
        let body = crate::parser::parse(src).map_err(|e| self.syntax_error(&e.to_string()))?;
        self.source = src.into();
        // The program's tree lives as long as the interpreter: its functions point into it.
        let body: &'static [Stmt] = Box::leak(body.into_boxed_slice());
        let strict = body.first().is_some_and(
            |s| matches!(s, Stmt::Expr(Expr::String(t)) if **t == *text("use strict")),
        );
        let _ = strict;
        self.frames.push(Frame {
            this: Some(Value::Object(self.global.clone())),
            function: None,
            new_target: Value::Undefined,
            home: None,
        });
        let scope = self.global_scope.clone();
        self.hoist(body, &scope, true)?;
        let mut last = Value::Undefined;
        let mut result = Ok(());
        for st in body {
            if let Stmt::Expr(e) = st {
                match self.eval(e, &scope) {
                    Ok(v) => last = v,
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
                continue;
            }
            match self.exec(st, &scope) {
                Ok(_) => {}
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        self.frames.pop();
        result?;
        self.run_jobs()?;
        Ok(last)
    }

    /// Runs what settled promises wait for, in turn, until none waits.
    pub fn run_jobs(&mut self) -> Res<()> {
        while let Some((job, arg)) = self.jobs.pop_front() {
            self.call(&Value::Object(job), Value::Undefined, &[arg])?;
        }
        Ok(())
    }

    // ---- errors

    /// A new error of the built-in kind `proto` with `message`.
    pub fn error_of(&self, proto: &Obj, message: &str) -> Value {
        let e = Obj::new(Some(proto.clone()), Class::Error);
        e.set_hidden(Key::str("message"), Value::str(message));
        Value::Object(e)
    }

    /// A `TypeError` to throw.
    pub fn type_error(&self, message: &str) -> Value {
        self.error_of(&self.intrinsics.type_error_proto, message)
    }

    /// A `RangeError` to throw.
    pub fn range_error(&self, message: &str) -> Value {
        self.error_of(&self.intrinsics.range_error_proto, message)
    }

    /// A `ReferenceError` to throw.
    pub fn reference_error(&self, message: &str) -> Value {
        self.error_of(&self.intrinsics.reference_error_proto, message)
    }

    /// A `SyntaxError` to throw.
    pub fn syntax_error(&self, message: &str) -> Value {
        self.error_of(&self.intrinsics.syntax_error_proto, message)
    }

    /// Counts a step, and stops the program where its time is up.
    fn step(&mut self) -> Res<()> {
        self.steps = self.steps.wrapping_add(1);
        if self.steps.is_multiple_of(4096) {
            if let Some(deadline) = self.deadline {
                if Instant::now() >= deadline {
                    return Err(self.range_error("the program ran out of time"));
                }
            }
        }
        Ok(())
    }

    // ---- scopes

    /// A scope inside `parent`.
    pub fn scope_in(parent: &Scope) -> Scope {
        Rc::new(RefCell::new(ScopeData {
            vars: HashMap::new(),
            parent: Some(parent.clone()),
            global: false,
        }))
    }

    /// Declares `name` in `scope` with `value`.
    pub fn declare(&mut self, scope: &Scope, name: &Name, value: Value, mutable: bool) {
        if scope.borrow().global && mutable {
            // A `var` or function of the program: a property of the global object.
            self.global.set_own(Key::String(text(name)), value);
            return;
        }
        scope
            .borrow_mut()
            .vars
            .insert(name.clone(), Binding { value, mutable });
    }

    /// Declares `name` as `var` declares it: where it is not there already, undefined.
    fn declare_var(&mut self, scope: &Scope, name: &Name) {
        if scope.borrow().global {
            let key = Key::String(text(name));
            if self.global.own(&key).is_none() {
                self.global.set_own(key, Value::Undefined);
            }
            return;
        }
        scope
            .borrow_mut()
            .vars
            .entry(name.clone())
            .or_insert(Binding {
                value: Value::Undefined,
                mutable: true,
            });
    }

    /// The value of `name`, or a `ReferenceError`.
    pub fn lookup(&mut self, scope: &Scope, name: &Name) -> Res<Value> {
        match self.find(scope, name)? {
            Some(v) => Ok(v),
            None => Err(self.reference_error(&format!("{name} is not defined"))),
        }
    }

    /// The value of `name`, where it is defined.
    fn find(&mut self, scope: &Scope, name: &Name) -> Res<Option<Value>> {
        let mut at = scope.clone();
        loop {
            let next = {
                let s = at.borrow();
                if let Some(b) = s.vars.get(name) {
                    return Ok(Some(b.value.clone()));
                }
                if s.global {
                    break;
                }
                s.parent.clone()
            };
            match next {
                Some(p) => at = p,
                None => break,
            }
        }
        let key = Key::String(text(name));
        if self.has_property(&self.global.clone(), &key) {
            let global = Value::Object(self.global.clone());
            return Ok(Some(self.get(&global, &key)?));
        }
        Ok(None)
    }

    /// Sets `name` to `value` where it is defined (else on the global object, as sloppy code
    /// does).
    pub fn assign_name(&mut self, scope: &Scope, name: &Name, value: Value) -> Res<()> {
        let mut at = scope.clone();
        loop {
            let next = {
                let mut s = at.borrow_mut();
                if let Some(b) = s.vars.get_mut(name) {
                    if !b.mutable {
                        drop(s);
                        return Err(self.type_error("Assignment to constant variable."));
                    }
                    b.value = value;
                    return Ok(());
                }
                if s.global {
                    break;
                }
                s.parent.clone()
            };
            match next {
                Some(p) => at = p,
                None => break,
            }
        }
        let global = Value::Object(self.global.clone());
        self.set(&global, Key::String(text(name)), value)
    }

    /// Puts each `var` and function of `body` in `scope` before it runs: functions with their
    /// values, `var`s as undefined; `let`, `const` and classes of this block too.
    pub fn hoist(&mut self, body: &[Stmt], scope: &Scope, function_level: bool) -> Res<()> {
        if function_level {
            let mut names = Vec::new();
            for st in body {
                var_names(st, &mut names);
            }
            for name in &names {
                self.declare_var(scope, name);
            }
        }
        for st in body {
            match st {
                Stmt::Function(f) => {
                    let value = self.make_function(f, scope);
                    if let Some(name) = &f.name {
                        self.declare(scope, name, value, true);
                    }
                }
                Stmt::Var(VarKind::Let | VarKind::Const, decls) => {
                    for (pattern, _) in decls {
                        let mut names = Vec::new();
                        pattern_names(pattern, &mut names);
                        for name in names {
                            scope.borrow_mut().vars.insert(
                                name,
                                Binding {
                                    value: Value::Undefined,
                                    mutable: true,
                                },
                            );
                        }
                    }
                }
                Stmt::Class(c) => {
                    if let Some(name) = &c.name {
                        scope.borrow_mut().vars.insert(
                            name.clone(),
                            Binding {
                                value: Value::Undefined,
                                mutable: true,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    // ---- statements

    /// Runs statements in order.
    pub fn exec_all(&mut self, body: &[Stmt], scope: &Scope) -> Res<Flow> {
        for st in body {
            match self.exec(st, scope)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    /// Runs a block in a scope of its own (where it declares anything block-wide).
    fn block(&mut self, body: &[Stmt], scope: &Scope) -> Res<Flow> {
        let needs_scope = body.iter().any(|st| {
            matches!(
                st,
                Stmt::Var(VarKind::Let | VarKind::Const, _) | Stmt::Class(_) | Stmt::Function(_)
            )
        });
        if !needs_scope {
            return self.exec_all(body, scope);
        }
        let inner = Interp::scope_in(scope);
        self.hoist(body, &inner, false)?;
        // A function declared in a block is also the enclosing function's `var` (as browsers
        // have it outside strict code).
        for st in body {
            if let Stmt::Function(f) = st {
                if let Some(name) = &f.name {
                    let value = self.lookup(&inner, name)?;
                    let _ = self.assign_outer_var(scope, name, value);
                }
            }
        }
        self.exec_all(body, &inner)
    }

    fn assign_outer_var(&mut self, scope: &Scope, name: &Name, value: Value) -> Res<()> {
        if self.find(scope, name)?.is_some() {
            return self.assign_name(scope, name, value);
        }
        Ok(())
    }

    /// Runs one statement.
    pub fn exec(&mut self, st: &Stmt, scope: &Scope) -> Res<Flow> {
        self.step()?;
        match st {
            Stmt::Expr(e) => {
                self.eval(e, scope)?;
                Ok(Flow::Normal)
            }
            Stmt::Var(kind, decls) => {
                for (pattern, value) in decls {
                    match value {
                        Some(e) => {
                            let v = self.eval_named(e, scope, pattern_name(pattern))?;
                            self.bind(pattern, v, scope, Some(*kind))?;
                        }
                        None if *kind != VarKind::Var => {
                            self.bind(pattern, Value::Undefined, scope, Some(*kind))?;
                        }
                        None => {}
                    }
                }
                Ok(Flow::Normal)
            }
            // Made when its scope was entered.
            Stmt::Function(_) | Stmt::Empty | Stmt::Debugger => Ok(Flow::Normal),
            Stmt::Class(c) => {
                let class = self.make_class(c, scope)?;
                if let Some(name) = &c.name {
                    // Block-wide, as `let` is (at the top too: not the global object's).
                    scope.borrow_mut().vars.insert(
                        name.clone(),
                        Binding {
                            value: class,
                            mutable: true,
                        },
                    );
                }
                Ok(Flow::Normal)
            }
            Stmt::Block(body) => self.block(body, scope),
            Stmt::If(test, then, otherwise) => {
                if self.eval(test, scope)?.truthy() {
                    self.exec(then, scope)
                } else if let Some(o) = otherwise {
                    self.exec(o, scope)
                } else {
                    Ok(Flow::Normal)
                }
            }
            Stmt::Return(value) => {
                let v = match value {
                    Some(e) => self.eval(e, scope)?,
                    None => Value::Undefined,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Break(label) => Ok(Flow::Break(label.clone())),
            Stmt::Continue(label) => Ok(Flow::Continue(label.clone())),
            Stmt::Throw(e) => Err(self.eval(e, scope)?),
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => self.try_statement(block, handler, finalizer, scope),
            Stmt::Labeled(label, body) => self.labeled(std::slice::from_ref(label), body, scope),
            Stmt::While(..)
            | Stmt::DoWhile(..)
            | Stmt::For { .. }
            | Stmt::ForIn { .. }
            | Stmt::Switch(..) => self.looped(st, &[], scope),
        }
    }

    fn labeled(&mut self, labels: &[Name], body: &Stmt, scope: &Scope) -> Res<Flow> {
        match body {
            Stmt::Labeled(more, inner) => {
                let mut all = labels.to_vec();
                all.push(more.clone());
                self.labeled(&all, inner, scope)
            }
            Stmt::While(..)
            | Stmt::DoWhile(..)
            | Stmt::For { .. }
            | Stmt::ForIn { .. }
            | Stmt::Switch(..) => self.looped(body, labels, scope),
            other => match self.exec(other, scope)? {
                Flow::Break(Some(l)) if labels.contains(&l) => Ok(Flow::Normal),
                flow => Ok(flow),
            },
        }
    }

    /// A loop's or switch's statement, with its labels: whether a `break` or `continue` is its.
    fn looped(&mut self, st: &Stmt, labels: &[Name], scope: &Scope) -> Res<Flow> {
        // What a turn's ending means: go on (true), or leave with this flow.
        let after = |flow: Flow| -> Result<bool, Flow> {
            match flow {
                Flow::Normal => Ok(true),
                Flow::Continue(None) => Ok(true),
                Flow::Continue(Some(l)) if labels.contains(&l) => Ok(true),
                Flow::Break(None) => Err(Flow::Normal),
                Flow::Break(Some(l)) if labels.contains(&l) => Err(Flow::Normal),
                other => Err(other),
            }
        };
        match st {
            Stmt::While(test, body) => {
                while self.eval(test, scope)?.truthy() {
                    self.step()?;
                    if let Err(flow) = after(self.exec(body, scope)?) {
                        return Ok(flow);
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::DoWhile(body, test) => loop {
                self.step()?;
                if let Err(flow) = after(self.exec(body, scope)?) {
                    return Ok(flow);
                }
                if !self.eval(test, scope)?.truthy() {
                    return Ok(Flow::Normal);
                }
            },
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                // `let` in the head: each turn's own copy of the names, as closures see them.
                let per_turn: Vec<Name> = match init.as_deref() {
                    Some(Stmt::Var(VarKind::Let | VarKind::Const, decls)) => {
                        let mut names = Vec::new();
                        for (p, _) in decls {
                            pattern_names(p, &mut names);
                        }
                        names
                    }
                    _ => Vec::new(),
                };
                let mut turn = if per_turn.is_empty() {
                    scope.clone()
                } else {
                    Interp::scope_in(scope)
                };
                if let Some(init) = init {
                    if !per_turn.is_empty() {
                        self.hoist(std::slice::from_ref(init), &turn, false)?;
                    }
                    self.exec(init, &turn)?;
                }
                loop {
                    self.step()?;
                    if let Some(test) = test {
                        if !self.eval(test, &turn)?.truthy() {
                            return Ok(Flow::Normal);
                        }
                    }
                    if let Err(flow) = after(self.exec(body, &turn)?) {
                        return Ok(flow);
                    }
                    if !per_turn.is_empty() {
                        let next = Interp::scope_in(scope);
                        for name in &per_turn {
                            let v = self.lookup(&turn, name)?;
                            next.borrow_mut().vars.insert(
                                name.clone(),
                                Binding {
                                    value: v,
                                    mutable: true,
                                },
                            );
                        }
                        turn = next;
                    }
                    if let Some(update) = update {
                        self.eval(update, &turn)?;
                    }
                }
            }
            Stmt::ForIn {
                left,
                right,
                body,
                of,
            } => {
                let subject = self.eval(right, scope)?;
                let mut items: Box<dyn Iterator<Item = Res<Value>>> = if *of {
                    self.iterate(subject)?
                } else {
                    let keys = self.for_in_keys(&subject)?;
                    Box::new(keys.into_iter().map(Ok))
                };
                loop {
                    let item = match items.next() {
                        None => return Ok(Flow::Normal),
                        Some(item) => item?,
                    };
                    self.step()?;
                    let turn = Interp::scope_in(scope);
                    match left {
                        ForTarget::Declare(kind, pattern) => {
                            if *kind == VarKind::Var {
                                self.bind(pattern, item, scope, None)?;
                            } else {
                                self.bind(pattern, item, &turn, Some(*kind))?;
                            }
                        }
                        ForTarget::Assign(pattern) => self.bind(pattern, item, scope, None)?,
                    }
                    if let Err(flow) = after(self.exec(body, &turn)?) {
                        return Ok(flow);
                    }
                }
            }
            Stmt::Switch(value, cases) => {
                let v = self.eval(value, scope)?;
                let inner = Interp::scope_in(scope);
                for (_, body) in cases {
                    self.hoist(body, &inner, false)?;
                }
                let mut matched = None;
                for (i, (test, _)) in cases.iter().enumerate() {
                    if let Some(test) = test {
                        if self.eval(test, &inner)?.strict_equals(&v) {
                            matched = Some(i);
                            break;
                        }
                    }
                }
                let start = matched.or_else(|| cases.iter().position(|(t, _)| t.is_none()));
                let Some(start) = start else {
                    return Ok(Flow::Normal);
                };
                for (_, body) in &cases[start..] {
                    match self.exec_all(body, &inner)? {
                        Flow::Normal => {}
                        Flow::Break(None) => return Ok(Flow::Normal),
                        Flow::Break(Some(l)) if labels.contains(&l) => return Ok(Flow::Normal),
                        other => return Ok(other),
                    }
                }
                Ok(Flow::Normal)
            }
            _ => self.exec(st, scope),
        }
    }

    fn try_statement(
        &mut self,
        block: &[Stmt],
        handler: &Option<(Option<Pattern>, Vec<Stmt>)>,
        finalizer: &Option<Vec<Stmt>>,
        scope: &Scope,
    ) -> Res<Flow> {
        let depth = self.frames.len();
        let mut outcome = self.block(block, scope);
        if let (Err(thrown), Some((param, body))) = (&outcome, handler) {
            // A throw from deeper calls left their frames: back to this one.
            self.frames.truncate(depth);
            let thrown = thrown.clone();
            let inner = Interp::scope_in(scope);
            if let Some(param) = param {
                self.bind(param, thrown, &inner, Some(VarKind::Let))?;
            }
            outcome = self.block(body, &inner);
        }
        if let Some(finalizer) = finalizer {
            self.frames.truncate(depth);
            match self.block(finalizer, scope)? {
                Flow::Normal => {}
                // A `return` or `break` in `finally` wins over what came before.
                other => return Ok(other),
            }
        }
        outcome
    }

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
                .map_err(|_| self.range_error("a BigInt too large for dm")),
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
            Expr::Yield(..) => Err(self.syntax_error("generators are not supported by dm-js yet")),
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

    /// A plain object.
    pub fn plain_object(&self) -> Obj {
        Obj::new(Some(self.intrinsics.object_proto.clone()), Class::Ordinary)
    }

    /// Whether `o` or its prototypes have `key`.
    pub fn has_property(&self, o: &Obj, key: &Key) -> bool {
        let mut at = Some(o.clone());
        while let Some(o) = at {
            if o.own(key).is_some() {
                return true;
            }
            at = o.proto();
        }
        false
    }

    /// `value[key]`, a primitive's from its prototype.
    pub fn get_of(&mut self, value: &Value, key: &Key) -> Res<Value> {
        match value {
            Value::Object(_) => self.get(value, key),
            Value::Undefined | Value::Null => {
                let what = String::from_utf16_lossy(&self.key_text(key));
                Err(self.type_error(&format!(
                    "Cannot read properties of {} (reading '{what}')",
                    if matches!(value, Value::Null) {
                        "null"
                    } else {
                        "undefined"
                    }
                )))
            }
            Value::String(s) => {
                if let Some(i) = key.as_index() {
                    return Ok(s
                        .get(i)
                        .map(|c| Value::String(vec![*c].into()))
                        .unwrap_or(Value::Undefined));
                }
                if *key == Key::str("length") {
                    return Ok(Value::Number(s.len() as f64));
                }
                let proto = self.intrinsics.string_proto.clone();
                self.get_from(&proto, key, value)
            }
            other => {
                let proto = self.proto_of_primitive(other);
                self.get_from(&proto, key, value)
            }
        }
    }

    /// The prototype a primitive's methods are on.
    pub fn proto_of_primitive(&self, v: &Value) -> Obj {
        match v {
            Value::Number(_) => self.intrinsics.number_proto.clone(),
            Value::Bool(_) => self.intrinsics.boolean_proto.clone(),
            Value::String(_) => self.intrinsics.string_proto.clone(),
            Value::Symbol(_) => self.intrinsics.symbol_proto.clone(),
            Value::BigInt(_) => self.intrinsics.bigint_proto.clone(),
            _ => self.intrinsics.object_proto.clone(),
        }
    }

    /// `o[key]` (an object's).
    pub fn get(&mut self, o: &Value, key: &Key) -> Res<Value> {
        match o {
            Value::Object(obj) => {
                if let Class::Proxy(target, handler) = &obj.borrow().class {
                    let (target, handler) = (target.clone(), handler.clone());
                    return crate::builtins::proxy::get(self, &target, &handler, key, o);
                }
                let obj = obj.clone();
                self.get_from(&obj, key, o)
            }
            other => self.get_of(other, key),
        }
    }

    /// `key` looked up from `start` along its prototypes, a getter called on `receiver`.
    pub fn get_from(&mut self, start: &Obj, key: &Key, receiver: &Value) -> Res<Value> {
        let mut at = Some(start.clone());
        while let Some(o) = at {
            if let Some(p) = o.own(key) {
                return match p.slot {
                    Slot::Data(v) | Slot::ReadOnly(v) => Ok(v),
                    Slot::Accessor(Some(getter), _) => {
                        self.call(&Value::Object(getter), receiver.clone(), &[])
                    }
                    Slot::Accessor(None, _) => Ok(Value::Undefined),
                };
            }
            at = o.proto();
        }
        Ok(Value::Undefined)
    }

    /// `o[key] = value`: a setter along the prototypes is called; a value is set on `o`.
    pub fn set(&mut self, o: &Value, key: Key, value: Value) -> Res<()> {
        let obj = match o {
            Value::Object(obj) => obj.clone(),
            // Set on a primitive: lost, as sloppy code loses it.
            _ => return Ok(()),
        };
        if let Class::Proxy(target, handler) = &obj.borrow().class {
            let (target, handler) = (target.clone(), handler.clone());
            return crate::builtins::proxy::set(self, &target, &handler, key, value, o);
        }
        // A typed array's element: the value as a number.
        let typed = matches!(&obj.borrow().class, Class::Bytes(b) if (1..10).contains(&b.kind));
        if typed && key.as_index().is_some() {
            let n = self.to_number(&value)?;
            obj.define(key, Prop::data(Value::Number(n), true, true, false));
            return Ok(());
        }
        // Own data property: changed in place, the usual case.
        if let Some(p) = obj.own(&key) {
            match p.slot {
                Slot::Data(_) => {
                    obj.define(
                        key,
                        Prop {
                            slot: Slot::Data(value),
                            enumerable: p.enumerable,
                            configurable: p.configurable,
                        },
                    );
                    return Ok(());
                }
                Slot::ReadOnly(_) => return Ok(()),
                Slot::Accessor(Some(setter), _) => {
                    self.call(&Value::Object(setter), o.clone(), &[value])?;
                    return Ok(());
                }
                Slot::Accessor(None, _) => return Ok(()),
            }
        }
        let mut at = obj.proto();
        while let Some(p) = at {
            if let Some(prop) = p.own(&key) {
                match prop.slot {
                    Slot::Accessor(Some(setter), _) => {
                        self.call(&Value::Object(setter), o.clone(), &[value])?;
                        return Ok(());
                    }
                    Slot::Accessor(None, _) | Slot::ReadOnly(_) => return Ok(()),
                    Slot::Data(_) => break,
                }
            }
            at = p.proto();
        }
        if !obj.borrow().extensible {
            return Ok(());
        }
        obj.set_own(key, value);
        Ok(())
    }

    /// The keys `for … in` walks: enumerable string keys, own first, then the prototypes',
    /// each once.
    fn for_in_keys(&mut self, v: &Value) -> Res<Vec<Value>> {
        let start = match v {
            Value::Undefined | Value::Null => return Ok(Vec::new()),
            Value::Object(o) => o.clone(),
            other => self.to_object(other)?,
        };
        let mut seen: Vec<Key> = Vec::new();
        let mut out = Vec::new();
        let mut at = Some(start);
        while let Some(o) = at {
            for key in o.own_keys(false) {
                if matches!(key, Key::Symbol(_)) || seen.contains(&key) {
                    continue;
                }
                let enumerable = o.own(&key).is_some_and(|p| p.enumerable);
                seen.push(key.clone());
                if enumerable {
                    out.push(key.to_value());
                }
            }
            at = o.proto();
        }
        Ok(out)
    }
}

impl Default for Interp {
    fn default() -> Self {
        Interp::new()
    }
}

/// An object literal's property (named apart from the object model's [`Prop`]).
pub use crate::ast::Prop as Prop_;

/// The names `var` declares in a statement (and in its blocks, not in nested functions).
fn var_names(st: &Stmt, out: &mut Vec<Name>) {
    match st {
        Stmt::Var(VarKind::Var, decls) => {
            for (p, _) in decls {
                pattern_names(p, out);
            }
        }
        Stmt::Block(body) => {
            for s in body {
                var_names(s, out);
            }
        }
        Stmt::If(_, a, b) => {
            var_names(a, out);
            if let Some(b) = b {
                var_names(b, out);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                var_names(init, out);
            }
            var_names(body, out);
        }
        Stmt::ForIn { left, body, .. } => {
            if let ForTarget::Declare(VarKind::Var, p) = left {
                pattern_names(p, out);
            }
            var_names(body, out);
        }
        Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::Labeled(_, body) => {
            var_names(body, out)
        }
        Stmt::Try {
            block,
            handler,
            finalizer,
        } => {
            for s in block {
                var_names(s, out);
            }
            if let Some((_, h)) = handler {
                for s in h {
                    var_names(s, out);
                }
            }
            if let Some(f) = finalizer {
                for s in f {
                    var_names(s, out);
                }
            }
        }
        Stmt::Switch(_, cases) => {
            for (_, body) in cases {
                for s in body {
                    var_names(s, out);
                }
            }
        }
        // A function declared in a block is a `var` of the function around it too.
        Stmt::Function(f) => {
            if let Some(n) = &f.name {
                out.push(n.clone());
            }
        }
        _ => {}
    }
}

/// The names a pattern binds.
pub fn pattern_names(p: &Pattern, out: &mut Vec<Name>) {
    match p {
        Pattern::Name(n) => out.push(n.clone()),
        Pattern::Place(_) => {}
        Pattern::Array(items, rest) => {
            for i in items.iter().flatten() {
                pattern_names(i, out);
            }
            if let Some(r) = rest {
                pattern_names(r, out);
            }
        }
        Pattern::Object(props, rest) => {
            for (_, p) in props {
                pattern_names(p, out);
            }
            if let Some(r) = rest {
                pattern_names(r, out);
            }
        }
        Pattern::Default(p, _) => pattern_names(p, out),
    }
}

/// The name a plain pattern is (to name an anonymous function after it).
fn pattern_name(p: &Pattern) -> Option<&Name> {
    match p {
        Pattern::Name(n) => Some(n),
        _ => None,
    }
}

/// The expression a pattern is, where it is a place (`a.b += 1`).
fn pattern_expr(p: &Pattern) -> Option<&Expr> {
    match p {
        Pattern::Place(e) => Some(e),
        _ => None,
    }
}

/// An expression as an error names it (`a.b is not a function`).
fn describe(e: &Expr) -> String {
    match e {
        Expr::Name(n) => n.to_string(),
        Expr::Member { object, key, .. } => {
            let o = describe(object);
            match &**key {
                MemberKey::Name(n) => format!("{o}.{n}"),
                MemberKey::Private(n) => format!("{o}.#{n}"),
                MemberKey::Computed(_) => format!("{o}[…]"),
            }
        }
        Expr::This => "this".into(),
        Expr::Call { callee, .. } => format!("{}(…)", describe(callee)),
        _ => "expression".into(),
    }
}

/// Whether the function's code names `arguments` (outside functions of its own, arrows aside).
fn names_arguments(f: &Function) -> bool {
    fn in_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Var(_, decls) => decls
                .iter()
                .any(|(p, v)| in_pattern(p) || v.as_ref().is_some_and(in_expr)),
            Stmt::Function(_) | Stmt::Class(_) | Stmt::Empty | Stmt::Debugger => false,
            Stmt::Expr(e) | Stmt::Throw(e) => in_expr(e),
            Stmt::Block(b) => b.iter().any(in_stmt),
            Stmt::If(t, a, b) => in_expr(t) || in_stmt(a) || b.as_deref().is_some_and(in_stmt),
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                init.as_deref().is_some_and(in_stmt)
                    || test.as_ref().is_some_and(in_expr)
                    || update.as_ref().is_some_and(in_expr)
                    || in_stmt(body)
            }
            Stmt::ForIn { right, body, .. } => in_expr(right) || in_stmt(body),
            Stmt::While(t, b) | Stmt::DoWhile(b, t) => in_expr(t) || in_stmt(b),
            Stmt::Return(v) => v.as_ref().is_some_and(in_expr),
            Stmt::Break(_) | Stmt::Continue(_) => false,
            Stmt::Try {
                block,
                handler,
                finalizer,
            } => {
                block.iter().any(in_stmt)
                    || handler.as_ref().is_some_and(|(_, h)| h.iter().any(in_stmt))
                    || finalizer.as_ref().is_some_and(|f| f.iter().any(in_stmt))
            }
            Stmt::Switch(v, cases) => {
                in_expr(v)
                    || cases
                        .iter()
                        .any(|(t, b)| t.as_ref().is_some_and(in_expr) || b.iter().any(in_stmt))
            }
            Stmt::Labeled(_, b) => in_stmt(b),
        }
    }
    fn in_pattern(p: &Pattern) -> bool {
        match p {
            Pattern::Name(_) => false,
            Pattern::Place(e) => in_expr(e),
            Pattern::Default(p, d) => in_pattern(p) || in_expr(d),
            Pattern::Array(items, rest) => {
                items.iter().flatten().any(in_pattern) || rest.as_deref().is_some_and(in_pattern)
            }
            Pattern::Object(props, rest) => {
                props.iter().any(|(_, p)| in_pattern(p)) || rest.as_deref().is_some_and(in_pattern)
            }
        }
    }
    fn in_args(args: &[Arg]) -> bool {
        args.iter().any(|a| in_expr(&a.value))
    }
    fn in_expr(e: &Expr) -> bool {
        match e {
            Expr::Name(n) => &**n == "arguments",
            // An arrow's `arguments` is the function's around it.
            Expr::Function(f) => {
                f.arrow && (f.body.iter().any(in_stmt) || f.params.iter().any(in_pattern))
            }
            Expr::Class(_) => false,
            Expr::Template(_, v) | Expr::Sequence(v) => v.iter().any(in_expr),
            Expr::Tagged(t, _, v) => in_expr(t) || v.iter().any(in_expr),
            Expr::Array(items) => items.iter().flatten().any(|a| in_expr(&a.value)),
            Expr::Object(props) => props.iter().any(|p| match p {
                Prop_::Value(k, v) => in_key(k) || in_expr(v),
                Prop_::Spread(v) => in_expr(v),
                Prop_::Getter(k, _) | Prop_::Setter(k, _) => in_key(k),
            }),
            Expr::Unary(_, v) | Expr::Await(v) | Expr::Chain(v) => in_expr(v),
            Expr::Update { target, .. } => in_expr(target),
            Expr::Binary(_, a, b) | Expr::Logical(_, a, b) => in_expr(a) || in_expr(b),
            Expr::Assign(_, p, v) => in_pattern(p) || in_expr(v),
            Expr::Conditional(a, b, c) => in_expr(a) || in_expr(b) || in_expr(c),
            Expr::Call { callee, args, .. } => in_expr(callee) || in_args(args),
            Expr::New(c, args) => in_expr(c) || in_args(args),
            Expr::Member { object, key, .. } => {
                in_expr(object) || matches!(&**key, MemberKey::Computed(k) if in_expr(k))
            }
            Expr::Yield(v, _) => v.as_deref().is_some_and(in_expr),
            Expr::ArrowParams(args) => in_args(args),
            _ => false,
        }
    }
    fn in_key(k: &PropKey) -> bool {
        matches!(k, PropKey::Computed(e) if in_expr(e))
    }
    f.body.iter().any(in_stmt) || f.params.iter().any(in_pattern)
}
