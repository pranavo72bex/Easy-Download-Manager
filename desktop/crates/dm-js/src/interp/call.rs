// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Functions and classes: made from their code, called, and constructed with `new`; `super`;
//! and `await`, which here waits for a promise already settled (dm's programs settle theirs at
//! once: nothing they wait on comes from outside).

use std::rc::Rc;

use crate::ast::{Class as ClassCode, Function, MemberKind, VarKind};
use crate::lexer::text;
use crate::object::{Class, ClassInfo, Func, Obj, Prop, ScriptFunc, Slot};
use crate::value::{Key, Value};

use super::{Flow, Frame, Interp, Res, Scope, MOST_DEPTH};

impl Interp {
    /// A function object for `code`, closing over `scope`.
    pub fn make_function(&mut self, code: &Rc<Function>, scope: &Scope) -> Value {
        let this = if code.arrow {
            // An arrow's `this` is the function's around it, from when it is made.
            self.frames.last().and_then(|f| f.this.clone())
        } else {
            None
        };
        let home = if code.arrow {
            self.frames.last().and_then(|f| f.home.clone())
        } else {
            None
        };
        let f = Obj::new(
            Some(self.intrinsics.function_proto.clone()),
            Class::Function(Func::Script(Rc::new(ScriptFunc {
                code: code.clone(),
                scope: scope.clone(),
                this,
                home,
                class: None,
            }))),
        );
        let length = code
            .params
            .iter()
            .take_while(|p| !matches!(p, crate::ast::Pattern::Default(..)))
            .count();
        f.define(
            Key::str("length"),
            Prop::data(Value::Number(length as f64), false, false, true),
        );
        f.define(
            Key::str("name"),
            Prop::data(
                Value::str(code.name.as_deref().unwrap_or("")),
                false,
                false,
                true,
            ),
        );
        // A function that `new` can make objects with has its own `prototype`.
        if !code.arrow && !code.method && !code.is_async {
            let proto = if code.generator {
                Obj::new(
                    Some(self.intrinsics.generator_proto.clone()),
                    Class::Ordinary,
                )
            } else {
                let p = self.plain_object();
                p.set_hidden(Key::str("constructor"), Value::Object(f.clone()));
                p
            };
            f.define(
                Key::str("prototype"),
                Prop::data(Value::Object(proto), true, false, false),
            );
        }
        Value::Object(f)
    }

    /// Makes `f` a method of `home` (where its `super` looks).
    pub fn set_home(&mut self, f: &Obj, home: &Obj) {
        let replaced = match &f.borrow().class {
            Class::Function(Func::Script(sf)) => Some(Rc::new(ScriptFunc {
                code: sf.code.clone(),
                scope: sf.scope.clone(),
                this: sf.this.clone(),
                home: Some(home.clone()),
                class: None,
            })),
            _ => None,
        };
        if let Some(sf) = replaced {
            f.borrow_mut().class = Class::Function(Func::Script(sf));
        }
    }

    /// `f(…args)` with `this`.
    pub fn call(&mut self, f: &Value, this: Value, args: &[Value]) -> Res<Value> {
        let Value::Object(fo) = f else {
            return Err(self.type_error("not a function"));
        };
        enum Kind {
            Script(Rc<ScriptFunc>),
            Native(crate::object::Native),
            Bound(Obj, Value, Vec<Value>),
            Proxy(Obj, Obj),
        }
        let kind = match &fo.borrow().class {
            Class::Function(Func::Script(s)) => Kind::Script(s.clone()),
            Class::Function(Func::Native(n)) => Kind::Native(n.clone()),
            Class::Function(Func::Bound { target, this, args }) => {
                Kind::Bound(target.clone(), this.clone(), args.clone())
            }
            Class::Proxy(target, handler) => Kind::Proxy(target.clone(), handler.clone()),
            _ => return Err(self.type_error("not a function")),
        };
        if self.frames.len() > MOST_DEPTH {
            return Err(self.range_error("Maximum call stack size exceeded"));
        }
        match kind {
            Kind::Native(n) => {
                self.frames.push(Frame {
                    this: Some(this.clone()),
                    function: Some(fo.clone()),
                    new_target: n.data.clone().unwrap_or(Value::Undefined),
                    home: None,
                });
                let r = (n.run)(self, &this, args, None);
                self.frames.pop();
                r
            }
            Kind::Bound(target, bound_this, mut bound_args) => {
                bound_args.extend_from_slice(args);
                self.call(&Value::Object(target), bound_this, &bound_args)
            }
            Kind::Proxy(target, handler) => {
                crate::builtins::proxy::apply(self, &target, &handler, this, args)
            }
            Kind::Script(sf) => {
                if sf.class.is_some() {
                    return Err(
                        self.type_error("Class constructor cannot be invoked without 'new'")
                    );
                }
                if sf.code.generator {
                    return crate::builtins::generator::start(self, fo, &sf, this, args);
                }
                let r = self.call_script(fo, &sf, Some(this), args, Value::Undefined);
                if sf.code.is_async {
                    // An async function's value is a promise of what it returned or threw.
                    return Ok(match r {
                        Ok(v) => crate::builtins::promise::resolved(self, v),
                        Err(e) => crate::builtins::promise::rejected(self, e),
                    });
                }
                r
            }
        }
    }

    /// Runs a script function's body.
    pub(crate) fn call_script(
        &mut self,
        f: &Obj,
        sf: &Rc<ScriptFunc>,
        this: Option<Value>,
        args: &[Value],
        new_target: Value,
    ) -> Res<Value> {
        let code = &sf.code;
        let this = if code.arrow {
            sf.this.clone()
        } else {
            // Outside strict code, a call without `this` gets the global object.
            match this {
                Some(Value::Undefined | Value::Null) if !code.strict => {
                    Some(Value::Object(self.global.clone()))
                }
                other => other,
            }
        };
        let (new_target, home) = if code.arrow {
            let outer = self.frames.last();
            (
                outer
                    .map(|f| f.new_target.clone())
                    .unwrap_or(Value::Undefined),
                sf.home.clone(),
            )
        } else {
            (new_target, sf.home.clone())
        };
        self.frames.push(Frame {
            this,
            function: Some(f.clone()),
            new_target,
            home,
        });
        let r = self.run_body(f, sf, args);
        self.frames.pop();
        r
    }

    fn run_body(&mut self, f: &Obj, sf: &Rc<ScriptFunc>, args: &[Value]) -> Res<Value> {
        let code = sf.code.clone();
        let scope = Interp::scope_in(&sf.scope);
        // A named function expression sees its own name.
        if let Some(name) = &code.name {
            if !code.arrow {
                scope.borrow_mut().vars.insert(
                    name.clone(),
                    super::Binding {
                        value: Value::Object(f.clone()),
                        mutable: true,
                    },
                );
            }
        }
        if !code.arrow {
            let key = Rc::as_ptr(&code);
            let uses = match self.uses_arguments.get(&key) {
                Some(u) => *u,
                None => {
                    let u = super::names_arguments(&code);
                    self.uses_arguments.insert(key, u);
                    u
                }
            };
            if uses {
                let a = Obj::new(Some(self.intrinsics.object_proto.clone()), Class::Arguments);
                for (i, v) in args.iter().enumerate() {
                    a.set_own(Key::index(i), v.clone());
                }
                a.set_hidden(Key::str("length"), Value::Number(args.len() as f64));
                a.set_hidden(
                    Key::Symbol(self.intrinsics.sym_iterator.clone()),
                    self.intrinsics.array_values.clone(),
                );
                scope.borrow_mut().vars.insert(
                    "arguments".into(),
                    super::Binding {
                        value: Value::Object(a),
                        mutable: true,
                    },
                );
            }
        }
        for (i, p) in code.params.iter().enumerate() {
            let v = args.get(i).cloned().unwrap_or(Value::Undefined);
            self.bind(p, v, &scope, Some(VarKind::Let))?;
        }
        if let Some(rest) = &code.rest {
            let rest_values = args.get(code.params.len()..).unwrap_or(&[]).to_vec();
            let arr = self.array(rest_values);
            self.bind(rest, arr, &scope, Some(VarKind::Let))?;
        }
        self.hoist(&code.body, &scope, true)?;
        match self.exec_all(&code.body, &scope)? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Undefined),
        }
    }

    /// `new f(…args)`; `new_target` where a subclass's `super()` constructs for it.
    pub fn construct(
        &mut self,
        f: &Value,
        args: &[Value],
        new_target: Option<&Value>,
    ) -> Res<Value> {
        let Value::Object(fo) = f else {
            return Err(self.type_error("not a constructor"));
        };
        let new_target = new_target.cloned().unwrap_or_else(|| f.clone());
        enum Kind {
            Script(Rc<ScriptFunc>),
            Native(crate::object::Native),
            Bound(Obj, Vec<Value>),
            Proxy(Obj, Obj),
        }
        let kind = match &fo.borrow().class {
            Class::Function(Func::Script(s)) => Kind::Script(s.clone()),
            Class::Function(Func::Native(n)) => Kind::Native(n.clone()),
            Class::Function(Func::Bound { target, args, .. }) => {
                Kind::Bound(target.clone(), args.clone())
            }
            Class::Proxy(target, handler) => Kind::Proxy(target.clone(), handler.clone()),
            _ => return Err(self.type_error("not a constructor")),
        };
        match kind {
            Kind::Native(n) => {
                if !n.constructor {
                    return Err(self.type_error("not a constructor"));
                }
                let proto = self.prototype_for(&new_target)?;
                self.frames.push(Frame {
                    this: None,
                    function: Some(fo.clone()),
                    new_target: new_target.clone(),
                    home: None,
                });
                let r = (n.run)(self, &Value::Undefined, args, Some(&proto));
                self.frames.pop();
                r
            }
            Kind::Bound(target, mut bound_args) => {
                bound_args.extend_from_slice(args);
                let target = Value::Object(target);
                let nt = if new_target.strict_equals(f) {
                    target.clone()
                } else {
                    new_target
                };
                self.construct(&target, &bound_args, Some(&nt))
            }
            Kind::Proxy(target, handler) => {
                crate::builtins::proxy::construct(self, &target, &handler, args, &new_target)
            }
            Kind::Script(sf) => {
                // A class's constructor is written as a method, and is the one `new` takes.
                if sf.class.is_none()
                    && (sf.code.arrow || sf.code.method || sf.code.generator || sf.code.is_async)
                {
                    return Err(self.type_error("not a constructor"));
                }
                let derived = sf.class.as_ref().is_some_and(|c| c.derived);
                let this = if derived {
                    None
                } else {
                    let proto = self.prototype_for(&new_target)?;
                    let o = Obj::new(Some(proto), Class::Ordinary);
                    if let Some(info) = &sf.class {
                        self.init_fields(&o, info)?;
                    }
                    Some(Value::Object(o))
                };
                self.frames.push(Frame {
                    this,
                    function: Some(fo.clone()),
                    new_target: new_target.clone(),
                    home: sf.class.as_ref().map(|c| c.prototype.clone()),
                });
                let r = self.run_body(fo, &sf, args);
                let frame = self.frames.pop();
                let returned = r?;
                if let Value::Object(_) = returned {
                    return Ok(returned);
                }
                match frame.and_then(|f| f.this) {
                    Some(this) => Ok(this),
                    None => Err(self.reference_error(
                        "Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
                    )),
                }
            }
        }
    }

    /// The prototype an object made by `new` with `new_target` gets.
    fn prototype_for(&mut self, new_target: &Value) -> Res<Obj> {
        let p = self.get(new_target, &Key::str("prototype"))?;
        Ok(match p {
            Value::Object(p) => p,
            _ => self.intrinsics.object_proto.clone(),
        })
    }

    /// `super(…args)` in a derived class's constructor.
    pub fn super_call(&mut self, args: &[Value]) -> Res<Value> {
        let (function, new_target) = match self.frames.last() {
            Some(f) => (f.function.clone(), f.new_target.clone()),
            None => (None, Value::Undefined),
        };
        // In an arrow, the constructor around it: its frame is the one with a class.
        let Some(function) = function else {
            return Err(self.syntax_error("'super' keyword unexpected here"));
        };
        let parent = function.proto().map(Value::Object).unwrap_or(Value::Null);
        let made = self.construct(&parent, args, Some(&new_target))?;
        if let Some(frame) = self.frames.last_mut() {
            frame.this = Some(made.clone());
        }
        // Its own class's fields, on the object made.
        let info = match &function.borrow().class {
            Class::Function(Func::Script(sf)) => sf
                .class
                .as_ref()
                .map(|c| (c.code.clone(), c.scope.clone(), c.prototype.clone())),
            _ => None,
        };
        if let (Some((code, scope, prototype)), Value::Object(o)) = (info, &made) {
            let info = ClassInfo {
                code,
                derived: true,
                scope,
                prototype,
            };
            self.init_fields(o, &info)?;
        }
        Ok(made)
    }

    /// `super[key]` in a method: looked up from the method's object's prototype, on `this`.
    pub fn super_get(&mut self, key: &Key) -> Res<Value> {
        let home = self.frames.last().and_then(|f| f.home.clone());
        let Some(home) = home else {
            return Err(self.syntax_error("'super' keyword unexpected here"));
        };
        let this = self.this_value()?;
        match home.proto() {
            Some(p) => self.get_from(&p, key, &this),
            None => Ok(Value::Undefined),
        }
    }

    /// The value `await v` gives: a settled promise's value or reason (thrown); any other value
    /// as it is.
    pub fn await_value(&mut self, v: Value) -> Res<Value> {
        crate::builtins::promise::settled_value(self, v)
    }

    /// A class: its constructor, with its prototype's methods and its static members.
    pub fn make_class(&mut self, code: &Rc<ClassCode>, scope: &Scope) -> Res<Value> {
        let (proto_parent, ctor_parent) = match &code.extends {
            None => (
                Some(self.intrinsics.object_proto.clone()),
                self.intrinsics.function_proto.clone(),
            ),
            Some(e) => {
                let parent = self.eval(e, scope)?;
                match &parent {
                    Value::Null => (None, self.intrinsics.function_proto.clone()),
                    Value::Object(p) if p.is_callable() => {
                        let pp = self.get(&parent, &Key::str("prototype"))?;
                        let pp =
                            match pp {
                                Value::Object(o) => Some(o),
                                Value::Null => None,
                                _ => return Err(self.type_error(
                                    "Class extends value does not have valid prototype property",
                                )),
                            };
                        (pp, p.clone())
                    }
                    _ => {
                        return Err(
                            self.type_error("Class extends value is not a constructor or null")
                        )
                    }
                }
            }
        };
        let class_scope = Interp::scope_in(scope);
        let prototype = Obj::new(proto_parent, Class::Ordinary);
        let derived = code.extends.is_some();
        let ctor_code = code.constructor.clone().unwrap_or_else(|| {
            Rc::new(Function {
                name: code.name.clone(),
                params: Vec::new(),
                rest: if derived {
                    Some(crate::ast::Pattern::Name("args".into()))
                } else {
                    None
                },
                body: if derived {
                    // `constructor(...args) { super(...args); }`
                    vec![crate::ast::Stmt::Expr(crate::ast::Expr::Call {
                        callee: Box::new(crate::ast::Expr::Super),
                        args: vec![crate::ast::Arg {
                            spread: true,
                            value: crate::ast::Expr::Name("args".into()),
                        }],
                        optional: false,
                    })]
                } else {
                    Vec::new()
                },
                arrow: false,
                generator: false,
                is_async: false,
                strict: true,
                span: (0, 0),
                method: false,
            })
        });
        let ctor = Obj::new(
            Some(ctor_parent),
            Class::Function(Func::Script(Rc::new(ScriptFunc {
                code: ctor_code,
                scope: class_scope.clone(),
                this: None,
                home: Some(prototype.clone()),
                class: Some(ClassInfo {
                    code: code.clone(),
                    derived,
                    scope: class_scope.clone(),
                    prototype: prototype.clone(),
                }),
            }))),
        );
        ctor.define(
            Key::str("prototype"),
            Prop::data(Value::Object(prototype.clone()), false, false, false),
        );
        ctor.define(
            Key::str("name"),
            Prop::data(
                Value::str(code.name.as_deref().unwrap_or("")),
                false,
                false,
                true,
            ),
        );
        prototype.set_hidden(Key::str("constructor"), Value::Object(ctor.clone()));
        if let Some(name) = &code.name {
            self.declare(&class_scope, name, Value::Object(ctor.clone()), false);
        }
        for member in &code.members {
            let target = if member.is_static {
                ctor.clone()
            } else {
                prototype.clone()
            };
            match &member.kind {
                MemberKind::Method(f) | MemberKind::Getter(f) | MemberKind::Setter(f) => {
                    let key = self.prop_key(&member.key, &class_scope)?;
                    let fv = self.make_function(f, &class_scope);
                    let Value::Object(fo) = fv else { continue };
                    self.set_home(&fo, &target);
                    let prop = match &member.kind {
                        MemberKind::Method(_) => Prop::data(Value::Object(fo), true, false, true),
                        kind => {
                            let (mut get, mut set) = match target.own(&key).map(|p| p.slot) {
                                Some(Slot::Accessor(g, s)) => (g, s),
                                _ => (None, None),
                            };
                            if matches!(kind, MemberKind::Getter(_)) {
                                get = Some(fo);
                            } else {
                                set = Some(fo);
                            }
                            Prop {
                                slot: Slot::Accessor(get, set),
                                enumerable: false,
                                configurable: true,
                            }
                        }
                    };
                    target.define(key, prop);
                }
                MemberKind::Field(_) | MemberKind::StaticBlock(_) => {}
            }
        }
        // Static fields and blocks, in order, with `this` the class.
        self.frames.push(Frame {
            this: Some(Value::Object(ctor.clone())),
            function: None,
            new_target: Value::Undefined,
            home: Some(ctor.clone()),
        });
        let r = (|| {
            for member in &code.members {
                if !member.is_static {
                    continue;
                }
                match &member.kind {
                    MemberKind::Field(value) => {
                        let key = self.prop_key(&member.key, &class_scope)?;
                        let v = match value {
                            Some(e) => self.eval(e, &class_scope)?,
                            None => Value::Undefined,
                        };
                        ctor.set_own(key, v);
                    }
                    MemberKind::StaticBlock(f) => {
                        let body_scope = Interp::scope_in(&class_scope);
                        self.hoist(&f.body, &body_scope, true)?;
                        self.exec_all(&f.body, &body_scope)?;
                    }
                    _ => {}
                }
            }
            Ok(())
        })();
        self.frames.pop();
        r?;
        Ok(Value::Object(ctor))
    }

    /// A class's instance fields, set on `o` (`this` is `o` while their values are worked out).
    pub(crate) fn init_fields(&mut self, o: &Obj, info: &ClassInfo) -> Res<()> {
        let fields: Vec<_> = info
            .code
            .members
            .iter()
            .filter(|m| !m.is_static && matches!(m.kind, MemberKind::Field(_)))
            .collect();
        if fields.is_empty() {
            return Ok(());
        }
        self.frames.push(Frame {
            this: Some(Value::Object(o.clone())),
            function: None,
            new_target: Value::Undefined,
            home: Some(info.prototype.clone()),
        });
        let r = (|| {
            for m in fields {
                let MemberKind::Field(value) = &m.kind else {
                    continue;
                };
                let key = self.prop_key(&m.key, &info.scope)?;
                let v = match value {
                    Some(e) => self.eval(e, &info.scope)?,
                    None => Value::Undefined,
                };
                o.set_own(key, v);
            }
            Ok(())
        })();
        self.frames.pop();
        r
    }

    /// A function's source text (`f.toString()`).
    pub fn function_source(&self, f: &Obj) -> String {
        match &f.borrow().class {
            Class::Function(Func::Script(sf)) => {
                let (from, to) = sf.code.span;
                self.source
                    .get(from..to)
                    .map(str::to_string)
                    .unwrap_or_else(|| "function () { [native code] }".into())
            }
            _ => {
                let name = f
                    .own(&Key::str("name"))
                    .and_then(|p| match p.slot {
                        Slot::Data(Value::String(s)) | Slot::ReadOnly(Value::String(s)) => {
                            Some(String::from_utf16_lossy(&s))
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                format!("function {name}() {{ [native code] }}")
            }
        }
    }

    /// A built-in function object.
    pub fn native(
        &self,
        name: &str,
        length: usize,
        run: crate::object::NativeFn,
        constructor: bool,
    ) -> Obj {
        let f = Obj::new(
            Some(self.intrinsics.function_proto.clone()),
            Class::Function(Func::Native(crate::object::Native {
                run,
                constructor,
                data: None,
            })),
        );
        f.define(
            Key::str("name"),
            Prop::data(Value::String(text(name)), false, false, true),
        );
        f.define(
            Key::str("length"),
            Prop::data(Value::Number(length as f64), false, false, true),
        );
        f
    }
}
