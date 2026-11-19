// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The interpreter: a program's statements run in order, its expressions worked out as the
//! language says, over scopes of names, objects and their prototypes. A thrown value travels
//! back as `Err`, a `return`, `break` or `continue` as a [`Flow`].
//!
//! The parts:
//! - here: scopes and names, and the loops;
//! - `exec`: the other statements;
//! - `eval`: expressions;
//! - `props`: reading and writing properties;
//! - `ops`: the operators and the conversions that may run a program's own code;
//! - `call`: calling and constructing, functions and classes made;
//! - `bind`: destructuring, and iteration.
//!
//! A program cannot run for ever or fill the memory: each step counts against a limit of time
//! ([`Interp::deadline`]) and calls are held to a depth.

mod bind;
mod call;
mod eval;
mod exec;
mod ops;
mod props;

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
            Stmt::Switch(value, cases) => self.switched(value, cases, labels, scope),
            _ => self.exec(st, scope),
        }
    }

    /// A `switch` statement, with its labels: whether a `break` is its.
    fn switched(
        &mut self,
        value: &Expr,
        cases: &[(Option<Expr>, Vec<Stmt>)],
        labels: &[Name],
        scope: &Scope,
    ) -> Res<Flow> {
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
