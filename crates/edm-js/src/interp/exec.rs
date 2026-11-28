// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Statements: hoisting, blocks, labels and `try` (the loops are the interpreter's own).

use super::*;

impl Interp {
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
}
