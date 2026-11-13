// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Object`, `Function`, `Symbol`, the errors, `Boolean` and `Reflect`.

use std::rc::Rc;

use super::{arg, constructor, getter, global, method, symbol_method};
use crate::interp::{Interp, Res};
use crate::lexer::text;
use crate::object::{Class, Func, Obj, Prop, Slot};
use crate::value::{Key, Symbol, Value};

/// Installs them.
pub fn install(interp: &mut Interp) {
    let i = &interp.intrinsics;
    let (op, fp, ep, sp, bp) = (
        i.object_proto.clone(),
        i.function_proto.clone(),
        i.error_proto.clone(),
        i.symbol_proto.clone(),
        i.boolean_proto.clone(),
    );

    // ---- Object
    let object = constructor(
        interp,
        "Object",
        1,
        |i, _, a, made| {
            let v = arg(a, 0);
            match v {
                Value::Undefined | Value::Null => Ok(Value::Object(match made {
                    Some(p) => Obj::new(Some(p.clone()), Class::Ordinary),
                    None => i.plain_object(),
                })),
                other => Ok(Value::Object(i.to_object(&other)?)),
            }
        },
        &op,
    );
    method(interp, &object, "keys", 1, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        let keys: Vec<Value> = string_keys(&o, true);
        Ok(i.array(keys))
    });
    method(interp, &object, "values", 1, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        let ov = Value::Object(o.clone());
        let mut out = Vec::new();
        for k in o.own_keys(true) {
            if matches!(k, Key::String(_)) {
                out.push(i.get(&ov, &k)?);
            }
        }
        Ok(i.array(out))
    });
    method(interp, &object, "entries", 1, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        let ov = Value::Object(o.clone());
        let mut out = Vec::new();
        for k in o.own_keys(true) {
            if matches!(k, Key::String(_)) {
                let v = i.get(&ov, &k)?;
                out.push(i.array(vec![k.to_value(), v]));
            }
        }
        Ok(i.array(out))
    });
    method(interp, &object, "fromEntries", 1, |i, _, a, _| {
        let o = i.plain_object();
        for entry in i.iterate(arg(a, 0))? {
            let entry = entry?;
            let k = i.get_of(&entry, &Key::index(0))?;
            let v = i.get_of(&entry, &Key::index(1))?;
            let k = i.to_key(&k)?;
            o.set_own(k, v);
        }
        Ok(Value::Object(o))
    });
    method(interp, &object, "assign", 2, |i, _, a, _| {
        let target = i.to_object(&arg(a, 0))?;
        for src in a.iter().skip(1) {
            if let Value::Object(s) = src {
                let sv = src.clone();
                for k in s.own_keys(true) {
                    let v = i.get(&sv, &k)?;
                    i.set(&Value::Object(target.clone()), k, v)?;
                }
            } else if let Value::String(_) = src {
                i.copy_own(src, &target, &[])?;
            }
        }
        Ok(Value::Object(target))
    });
    method(interp, &object, "create", 2, |i, _, a, _| {
        let proto = match arg(a, 0) {
            Value::Object(p) => Some(p),
            Value::Null => None,
            _ => return Err(i.type_error("Object prototype may only be an Object or null")),
        };
        let o = Obj::new(proto, Class::Ordinary);
        if let Value::Object(props) = arg(a, 1) {
            define_properties(i, &o, &props)?;
        }
        Ok(Value::Object(o))
    });
    method(interp, &object, "getPrototypeOf", 1, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        Ok(o.proto().map(Value::Object).unwrap_or(Value::Null))
    });
    method(interp, &object, "setPrototypeOf", 2, |_, _, a, _| {
        let o = arg(a, 0);
        if let Value::Object(obj) = &o {
            obj.borrow_mut().proto = match arg(a, 1) {
                Value::Object(p) => Some(p),
                _ => None,
            };
        }
        Ok(o)
    });
    method(interp, &object, "defineProperty", 3, |i, _, a, _| {
        let Value::Object(o) = arg(a, 0) else {
            return Err(i.type_error("Object.defineProperty called on non-object"));
        };
        let key = i.to_key(&arg(a, 1))?;
        let Value::Object(desc) = arg(a, 2) else {
            return Err(i.type_error("Property description must be an object"));
        };
        define_property(i, &o, key, &desc)?;
        Ok(Value::Object(o))
    });
    method(interp, &object, "defineProperties", 2, |i, _, a, _| {
        let Value::Object(o) = arg(a, 0) else {
            return Err(i.type_error("Object.defineProperties called on non-object"));
        };
        if let Value::Object(props) = arg(a, 1) {
            define_properties(i, &o, &props)?;
        }
        Ok(Value::Object(o))
    });
    method(interp, &object, "getOwnPropertyNames", 1, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        Ok(i.array(string_keys(&o, false)))
    });
    method(interp, &object, "getOwnPropertySymbols", 1, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        let keys = o
            .own_keys(false)
            .into_iter()
            .filter(|k| matches!(k, Key::Symbol(_)))
            .map(|k| k.to_value())
            .collect();
        Ok(i.array(keys))
    });
    method(
        interp,
        &object,
        "getOwnPropertyDescriptor",
        2,
        |i, _, a, _| {
            let o = i.to_object(&arg(a, 0))?;
            let key = i.to_key(&arg(a, 1))?;
            Ok(match o.own(&key) {
                Some(p) => descriptor(i, &p),
                None => Value::Undefined,
            })
        },
    );
    method(
        interp,
        &object,
        "getOwnPropertyDescriptors",
        1,
        |i, _, a, _| {
            let o = i.to_object(&arg(a, 0))?;
            let out = i.plain_object();
            for k in o.own_keys(false) {
                if let Some(p) = o.own(&k) {
                    out.set_own(k, descriptor(i, &p));
                }
            }
            Ok(Value::Object(out))
        },
    );
    for (name, frozen) in [
        ("freeze", true),
        ("seal", false),
        ("preventExtensions", false),
    ] {
        let run: crate::object::NativeFn = match (name, frozen) {
            ("freeze", _) => |_, _, a, _| {
                if let Value::Object(o) = arg(a, 0) {
                    lock(&o, true);
                }
                Ok(arg(a, 0))
            },
            ("seal", _) => |_, _, a, _| {
                if let Value::Object(o) = arg(a, 0) {
                    lock(&o, false);
                }
                Ok(arg(a, 0))
            },
            _ => |_, _, a, _| {
                if let Value::Object(o) = arg(a, 0) {
                    o.borrow_mut().extensible = false;
                }
                Ok(arg(a, 0))
            },
        };
        method(interp, &object, name, 1, run);
    }
    method(interp, &object, "isFrozen", 1, |_, _, a, _| {
        Ok(Value::Bool(match arg(a, 0) {
            Value::Object(o) => {
                !o.borrow().extensible
                    && o.own_keys(false)
                        .iter()
                        .all(|k| o.own(k).is_some_and(|p| !p.configurable && !p.writable()))
            }
            _ => true,
        }))
    });
    method(interp, &object, "isSealed", 1, |_, _, a, _| {
        Ok(Value::Bool(match arg(a, 0) {
            Value::Object(o) => {
                !o.borrow().extensible
                    && o.own_keys(false)
                        .iter()
                        .all(|k| o.own(k).is_some_and(|p| !p.configurable))
            }
            _ => true,
        }))
    });
    method(interp, &object, "isExtensible", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Object(o) if o.borrow().extensible),
        ))
    });
    method(interp, &object, "is", 2, |_, _, a, _| {
        let (x, y) = (arg(a, 0), arg(a, 1));
        Ok(Value::Bool(match (&x, &y) {
            (Value::Number(p), Value::Number(q)) => {
                (p.is_nan() && q.is_nan())
                    || (p == q && p.is_sign_negative() == q.is_sign_negative())
            }
            _ => x.strict_equals(&y),
        }))
    });
    method(interp, &object, "hasOwn", 2, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        let key = i.to_key(&arg(a, 1))?;
        Ok(Value::Bool(o.own(&key).is_some()))
    });

    // Object.prototype
    method(interp, &op, "hasOwnProperty", 1, |i, this, a, _| {
        let key = i.to_key(&arg(a, 0))?;
        let o = i.to_object(this)?;
        Ok(Value::Bool(o.own(&key).is_some()))
    });
    method(interp, &op, "isPrototypeOf", 1, |i, this, a, _| {
        let Value::Object(v) = arg(a, 0) else {
            return Ok(Value::Bool(false));
        };
        let o = i.to_object(this)?;
        let mut at = v.proto();
        while let Some(p) = at {
            if p.same(&o) {
                return Ok(Value::Bool(true));
            }
            at = p.proto();
        }
        Ok(Value::Bool(false))
    });
    method(interp, &op, "propertyIsEnumerable", 1, |i, this, a, _| {
        let key = i.to_key(&arg(a, 0))?;
        let o = i.to_object(this)?;
        Ok(Value::Bool(o.own(&key).is_some_and(|p| p.enumerable)))
    });
    method(interp, &op, "toString", 0, object_to_string);
    method(interp, &op, "toLocaleString", 0, |i, this, _, _| {
        let f = i.get_of(this, &Key::str("toString"))?;
        i.call(&f, this.clone(), &[])
    });
    method(interp, &op, "valueOf", 0, |i, this, _, _| {
        Ok(Value::Object(i.to_object(this)?))
    });
    op.define(
        Key::str("__proto__"),
        Prop {
            slot: Slot::Accessor(
                Some(interp.native(
                    "get __proto__",
                    0,
                    |i, this, _, _| {
                        let o = i.to_object(this)?;
                        Ok(o.proto().map(Value::Object).unwrap_or(Value::Null))
                    },
                    false,
                )),
                Some(interp.native(
                    "set __proto__",
                    1,
                    |_, this, a, _| {
                        if let Value::Object(o) = this {
                            match arg(a, 0) {
                                Value::Object(p) => o.borrow_mut().proto = Some(p),
                                Value::Null => o.borrow_mut().proto = None,
                                _ => {}
                            }
                        }
                        Ok(Value::Undefined)
                    },
                    false,
                )),
            ),
            enumerable: false,
            configurable: true,
        },
    );
    method(interp, &op, "__defineGetter__", 2, |i, this, a, _| {
        let o = i.to_object(this)?;
        let key = i.to_key(&arg(a, 0))?;
        if let Value::Object(f) = arg(a, 1) {
            let set = match o.own(&key).map(|p| p.slot) {
                Some(Slot::Accessor(_, s)) => s,
                _ => None,
            };
            o.define(
                key,
                Prop {
                    slot: Slot::Accessor(Some(f), set),
                    enumerable: true,
                    configurable: true,
                },
            );
        }
        Ok(Value::Undefined)
    });
    method(interp, &op, "__lookupGetter__", 1, |i, this, a, _| {
        let o = i.to_object(this)?;
        let key = i.to_key(&arg(a, 0))?;
        let mut at = Some(o);
        while let Some(x) = at {
            if let Some(p) = x.own(&key) {
                return Ok(match p.slot {
                    Slot::Accessor(Some(g), _) => Value::Object(g),
                    _ => Value::Undefined,
                });
            }
            at = x.proto();
        }
        Ok(Value::Undefined)
    });

    // ---- Function
    let function = constructor(
        interp,
        "Function",
        1,
        |i, _, a, _| {
            // `new Function(a, b, body)`: the program's own text, made here as any other.
            let mut params = Vec::new();
            for p in a.iter().take(a.len().saturating_sub(1)) {
                params.push(i.to_rust_string(p)?);
            }
            let body = match a.last() {
                Some(b) => i.to_rust_string(b)?,
                None => String::new(),
            };
            let src = format!(
                "(function anonymous({}\n) {{\n{}\n}})",
                params.join(","),
                body
            );
            let parsed = crate::parser::parse(&src).map_err(|e| i.syntax_error(&e.message))?;
            let parsed: &'static [crate::ast::Stmt] = Box::leak(parsed.into_boxed_slice());
            match parsed.first() {
                Some(crate::ast::Stmt::Expr(e)) => {
                    let scope = i.global_scope.clone();
                    i.eval(e, &scope)
                }
                _ => Err(i.syntax_error("not a function body")),
            }
        },
        &fp,
    );
    let _ = function;
    method(interp, &fp, "call", 1, |i, this, a, _| {
        let rest = a.get(1..).unwrap_or(&[]).to_vec();
        i.call(this, arg(a, 0), &rest)
    });
    method(interp, &fp, "apply", 2, |i, this, a, _| {
        let list = list_of(i, &arg(a, 1))?;
        i.call(this, arg(a, 0), &list)
    });
    method(interp, &fp, "bind", 1, |i, this, a, _| {
        let Value::Object(target) = this else {
            return Err(i.type_error("Bind must be called on a function"));
        };
        if !target.is_callable() {
            return Err(i.type_error("Bind must be called on a function"));
        }
        let name = i.get(this, &Key::str("name"))?;
        let name = i.to_rust_string(&name).unwrap_or_default();
        let length = i.get(this, &Key::str("length"))?;
        let length = match length {
            Value::Number(n) => (n - a.len().saturating_sub(1) as f64).max(0.0),
            _ => 0.0,
        };
        let f = Obj::new(
            Some(i.intrinsics.function_proto.clone()),
            Class::Function(Func::Bound {
                target: target.clone(),
                this: arg(a, 0),
                args: a.get(1..).unwrap_or(&[]).to_vec(),
            }),
        );
        f.define(
            Key::str("name"),
            Prop::data(Value::str(&format!("bound {name}")), false, false, true),
        );
        f.define(
            Key::str("length"),
            Prop::data(Value::Number(length), false, false, true),
        );
        Ok(Value::Object(f))
    });
    method(interp, &fp, "toString", 0, |i, this, _, _| match this {
        Value::Object(f) if f.is_callable() => Ok(Value::str(&i.function_source(f))),
        _ => Err(i.type_error("Function.prototype.toString requires that 'this' be a Function")),
    });
    let has_instance = Key::Symbol(interp.intrinsics.sym_has_instance.clone());
    symbol_method(
        interp,
        &fp,
        &has_instance,
        "[Symbol.hasInstance]",
        |i, this, a, _| {
            let Value::Object(f) = this else {
                return Ok(Value::Bool(false));
            };
            let f = f.clone();
            Ok(Value::Bool(i.instance_of(&arg(a, 0), &f)?))
        },
    );
    fp.define(
        Key::str("name"),
        Prop::data(Value::str(""), false, false, true),
    );
    fp.define(
        Key::str("length"),
        Prop::data(Value::Number(0.0), false, false, true),
    );

    // ---- Symbol
    let symbol = constructor(
        interp,
        "Symbol",
        0,
        |i, _, a, made| {
            if made.is_some() {
                return Err(i.type_error("Symbol is not a constructor"));
            }
            let description = match arg(a, 0) {
                Value::Undefined => None,
                v => Some(i.to_text(&v)?),
            };
            Ok(Value::Symbol(Rc::new(Symbol { description })))
        },
        &sp,
    );
    let well_known = {
        let i = &interp.intrinsics;
        [
            ("iterator", i.sym_iterator.clone()),
            ("asyncIterator", i.sym_async_iterator.clone()),
            ("toPrimitive", i.sym_to_primitive.clone()),
            ("hasInstance", i.sym_has_instance.clone()),
            ("toStringTag", i.sym_to_string_tag.clone()),
            ("species", i.sym_species.clone()),
            ("isConcatSpreadable", i.sym_concat_spreadable.clone()),
            ("unscopables", i.sym_unscopables.clone()),
            ("match", i.sym_match.clone()),
            ("matchAll", i.sym_match_all.clone()),
            ("replace", i.sym_replace.clone()),
            ("search", i.sym_search.clone()),
            ("split", i.sym_split.clone()),
        ]
    };
    for (name, s) in well_known {
        symbol.define(
            Key::str(name),
            Prop::data(Value::Symbol(s), false, false, false),
        );
    }
    method(interp, &symbol, "for", 1, |i, _, a, _| {
        let key = i.to_text(&arg(a, 0))?;
        if let Some((_, s)) = i.intrinsics.registry.iter().find(|(k, _)| *k == key) {
            return Ok(Value::Symbol(s.clone()));
        }
        let s = Rc::new(Symbol {
            description: Some(key.clone()),
        });
        i.intrinsics.registry.push((key, s.clone()));
        Ok(Value::Symbol(s))
    });
    method(interp, &symbol, "keyFor", 1, |i, _, a, _| {
        let Value::Symbol(s) = arg(a, 0) else {
            return Err(i.type_error("not a symbol"));
        };
        Ok(i.intrinsics
            .registry
            .iter()
            .find(|(_, x)| Rc::ptr_eq(x, &s))
            .map(|(k, _)| Value::String(k.clone()))
            .unwrap_or(Value::Undefined))
    });
    method(interp, &sp, "toString", 0, |i, this, _, _| {
        let s = this_symbol(i, this)?;
        let d = s
            .description
            .clone()
            .map(|d| String::from_utf16_lossy(&d))
            .unwrap_or_default();
        Ok(Value::str(&format!("Symbol({d})")))
    });
    method(interp, &sp, "valueOf", 0, |i, this, _, _| {
        Ok(Value::Symbol(this_symbol(i, this)?))
    });
    getter(
        interp,
        &sp,
        Key::str("description"),
        "description",
        |i, this, _, _| {
            Ok(this_symbol(i, this)?
                .description
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Undefined))
        },
    );
    let to_tag = Key::Symbol(interp.intrinsics.sym_to_string_tag.clone());
    sp.define(
        to_tag.clone(),
        Prop::data(Value::str("Symbol"), false, false, true),
    );
    let to_prim = Key::Symbol(interp.intrinsics.sym_to_primitive.clone());
    symbol_method(
        interp,
        &sp,
        &to_prim,
        "[Symbol.toPrimitive]",
        |i, this, _, _| Ok(Value::Symbol(this_symbol(i, this)?)),
    );

    // ---- errors
    let error = constructor(interp, "Error", 1, make_error, &ep);
    let _ = error;
    ep.set_hidden(Key::str("name"), Value::str("Error"));
    ep.set_hidden(Key::str("message"), Value::str(""));
    method(interp, &ep, "toString", 0, |i, this, _, _| {
        let name = i.get_of(this, &Key::str("name"))?;
        let name = if name.is_nullish() {
            "Error".to_string()
        } else {
            i.to_rust_string(&name)?
        };
        let msg = i.get_of(this, &Key::str("message"))?;
        let msg = if msg.is_nullish() {
            String::new()
        } else {
            i.to_rust_string(&msg)?
        };
        Ok(Value::str(&match (name.is_empty(), msg.is_empty()) {
            (_, true) => name,
            (true, false) => msg,
            _ => format!("{name}: {msg}"),
        }))
    });
    let kinds = {
        let i = &interp.intrinsics;
        vec![
            ("TypeError", i.type_error_proto.clone()),
            ("RangeError", i.range_error_proto.clone()),
            ("ReferenceError", i.reference_error_proto.clone()),
            ("SyntaxError", i.syntax_error_proto.clone()),
            ("EvalError", Obj::new(Some(ep.clone()), Class::Ordinary)),
            ("URIError", Obj::new(Some(ep.clone()), Class::Ordinary)),
            (
                "AggregateError",
                Obj::new(Some(ep.clone()), Class::Ordinary),
            ),
        ]
    };
    for (name, proto) in kinds {
        let c = constructor(interp, name, 1, make_error, &proto);
        c.borrow_mut().proto = interp
            .global
            .own(&Key::str("Error"))
            .and_then(|p| match p.slot {
                Slot::Data(Value::Object(o)) | Slot::ReadOnly(Value::Object(o)) => Some(o),
                _ => None,
            });
        proto.set_hidden(Key::str("name"), Value::str(name));
        proto.set_hidden(Key::str("message"), Value::str(""));
    }

    // ---- Boolean
    constructor(
        interp,
        "Boolean",
        1,
        |_, _, a, made| {
            let b = arg(a, 0).truthy();
            Ok(match made {
                Some(p) => Value::Object(Obj::new(Some(p.clone()), Class::Boolean(b))),
                None => Value::Bool(b),
            })
        },
        &bp,
    );
    method(interp, &bp, "toString", 0, |i, this, _, _| {
        Ok(Value::str(if this_bool(i, this)? {
            "true"
        } else {
            "false"
        }))
    });
    method(interp, &bp, "valueOf", 0, |i, this, _, _| {
        Ok(Value::Bool(this_bool(i, this)?))
    });

    // ---- Reflect
    let reflect = interp.plain_object();
    global(interp, "Reflect", Value::Object(reflect.clone()));
    method(interp, &reflect, "apply", 3, |i, _, a, _| {
        let list = list_of(i, &arg(a, 2))?;
        i.call(&arg(a, 0), arg(a, 1), &list)
    });
    method(interp, &reflect, "construct", 2, |i, _, a, _| {
        let list = list_of(i, &arg(a, 1))?;
        let nt = if a.len() > 2 { Some(arg(a, 2)) } else { None };
        i.construct(&arg(a, 0), &list, nt.as_ref())
    });
    method(interp, &reflect, "get", 2, |i, _, a, _| {
        let key = i.to_key(&arg(a, 1))?;
        let target = arg(a, 0);
        if let (Value::Object(o), Some(receiver)) = (&target, a.get(2)) {
            let o = o.clone();
            return i.get_from(&o, &key, receiver);
        }
        i.get(&target, &key)
    });
    method(interp, &reflect, "set", 3, |i, _, a, _| {
        let key = i.to_key(&arg(a, 1))?;
        i.set(&arg(a, 0), key, arg(a, 2))?;
        Ok(Value::Bool(true))
    });
    method(interp, &reflect, "has", 2, |i, _, a, _| {
        let Value::Object(o) = arg(a, 0) else {
            return Err(i.type_error("Reflect.has called on non-object"));
        };
        let key = i.to_key(&arg(a, 1))?;
        Ok(Value::Bool(i.has_property(&o, &key)))
    });
    method(interp, &reflect, "ownKeys", 1, |i, _, a, _| {
        let Value::Object(o) = arg(a, 0) else {
            return Err(i.type_error("Reflect.ownKeys called on non-object"));
        };
        let keys = o
            .own_keys(false)
            .into_iter()
            .map(|k| k.to_value())
            .collect();
        Ok(i.array(keys))
    });
    method(interp, &reflect, "getPrototypeOf", 1, |i, _, a, _| {
        let Value::Object(o) = arg(a, 0) else {
            return Err(i.type_error("Reflect.getPrototypeOf called on non-object"));
        };
        Ok(o.proto().map(Value::Object).unwrap_or(Value::Null))
    });
    method(interp, &reflect, "setPrototypeOf", 2, |_, _, a, _| {
        if let Value::Object(o) = arg(a, 0) {
            o.borrow_mut().proto = arg(a, 1).as_object().cloned();
        }
        Ok(Value::Bool(true))
    });
    method(interp, &reflect, "defineProperty", 3, |i, _, a, _| {
        let (Value::Object(o), Value::Object(d)) = (arg(a, 0), arg(a, 2)) else {
            return Err(i.type_error("Reflect.defineProperty called on non-object"));
        };
        let key = i.to_key(&arg(a, 1))?;
        define_property(i, &o, key, &d)?;
        Ok(Value::Bool(true))
    });
    method(interp, &reflect, "deleteProperty", 2, |i, _, a, _| {
        let Value::Object(o) = arg(a, 0) else {
            return Err(i.type_error("Reflect.deleteProperty called on non-object"));
        };
        let key = i.to_key(&arg(a, 1))?;
        Ok(Value::Bool(o.delete(&key)))
    });
    method(
        interp,
        &reflect,
        "getOwnPropertyDescriptor",
        2,
        |i, _, a, _| {
            let Value::Object(o) = arg(a, 0) else {
                return Err(i.type_error("Reflect.getOwnPropertyDescriptor called on non-object"));
            };
            let key = i.to_key(&arg(a, 1))?;
            Ok(o.own(&key)
                .map(|p| descriptor(i, &p))
                .unwrap_or(Value::Undefined))
        },
    );
    method(interp, &reflect, "isExtensible", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Object(o) if o.borrow().extensible),
        ))
    });
    method(interp, &reflect, "preventExtensions", 1, |_, _, a, _| {
        if let Value::Object(o) = arg(a, 0) {
            o.borrow_mut().extensible = false;
        }
        Ok(Value::Bool(true))
    });
}

/// `new Error(message, {cause})`.
fn make_error(i: &mut Interp, _: &Value, a: &[Value], made: Option<&Obj>) -> Res<Value> {
    let proto = match made {
        Some(p) => p.clone(),
        None => {
            // Called without `new`: the prototype of the error kind called.
            let f = i.frames.last().and_then(|f| f.function.clone());
            match f.map(|f| i.get(&Value::Object(f), &Key::str("prototype"))) {
                Some(Ok(Value::Object(p))) => p,
                _ => i.intrinsics.error_proto.clone(),
            }
        }
    };
    let e = Obj::new(Some(proto), Class::Error);
    let message = arg(a, 0);
    if !matches!(message, Value::Undefined) {
        let m = i.to_text(&message)?;
        e.set_hidden(Key::str("message"), Value::String(m));
    }
    if let Value::Object(options) = arg(a, 1) {
        let cause = Key::str("cause");
        if i.has_property(&options, &cause) {
            let c = i.get(&Value::Object(options), &cause)?;
            e.set_hidden(cause, c);
        }
    }
    e.set_hidden(Key::str("stack"), Value::str(""));
    Ok(Value::Object(e))
}

fn this_symbol(i: &mut Interp, this: &Value) -> Res<Rc<Symbol>> {
    match this {
        Value::Symbol(s) => Ok(s.clone()),
        Value::Object(o) => match &o.borrow().class {
            Class::Symbol(s) => Ok(s.clone()),
            _ => Err(i.type_error("not a symbol")),
        },
        _ => Err(i.type_error("not a symbol")),
    }
}

fn this_bool(i: &mut Interp, this: &Value) -> Res<bool> {
    match this {
        Value::Bool(b) => Ok(*b),
        Value::Object(o) => match &o.borrow().class {
            Class::Boolean(b) => Ok(*b),
            _ => Err(i.type_error("not a boolean")),
        },
        _ => Err(i.type_error("not a boolean")),
    }
}

/// `Object.prototype.toString`: `[object Tag]`.
pub fn object_to_string(i: &mut Interp, this: &Value, _: &[Value], _: Option<&Obj>) -> Res<Value> {
    let tag = match this {
        Value::Undefined => return Ok(Value::str("[object Undefined]")),
        Value::Null => return Ok(Value::str("[object Null]")),
        _ => {
            let o = i.to_object(this)?;
            let tag_key = Key::Symbol(i.intrinsics.sym_to_string_tag.clone());
            let custom = i.get(&Value::Object(o.clone()), &tag_key)?;
            if let Value::String(s) = custom {
                String::from_utf16_lossy(&s)
            } else {
                match &o.borrow().class {
                    Class::Array(_) => "Array",
                    Class::Function(_) => "Function",
                    Class::Error => "Error",
                    Class::Boolean(_) => "Boolean",
                    Class::Number(_) => "Number",
                    Class::String(_) => "String",
                    Class::Date(_) => "Date",
                    Class::RegExp(_) => "RegExp",
                    Class::Arguments => "Arguments",
                    _ => "Object",
                }
                .to_string()
            }
        }
    };
    Ok(Value::str(&format!("[object {tag}]")))
}

/// An object's own string keys, as values.
fn string_keys(o: &Obj, enumerable_only: bool) -> Vec<Value> {
    o.own_keys(enumerable_only)
        .into_iter()
        .filter(|k| matches!(k, Key::String(_)))
        .map(|k| k.to_value())
        .collect()
}

/// Freezes (`frozen`) or seals `o`.
fn lock(o: &Obj, frozen: bool) {
    let keys = o.own_keys(false);
    for k in keys {
        if let Some(mut p) = o.own(&k) {
            p.configurable = false;
            if frozen {
                if let Slot::Data(v) = p.slot {
                    p.slot = Slot::ReadOnly(v);
                }
            }
            // An array's elements stay in its list: frozen arrays are read only as wholes.
            if !matches!(o.borrow().class, Class::Array(_)) {
                o.borrow_mut().props.insert(k, p);
            }
        }
    }
    o.borrow_mut().extensible = false;
}

/// A property as `getOwnPropertyDescriptor` gives it.
fn descriptor(i: &mut Interp, p: &Prop) -> Value {
    let d = i.plain_object();
    match &p.slot {
        Slot::Data(v) | Slot::ReadOnly(v) => {
            d.set_own(Key::str("value"), v.clone());
            d.set_own(Key::str("writable"), Value::Bool(p.writable()));
        }
        Slot::Accessor(g, s) => {
            d.set_own(
                Key::str("get"),
                g.clone().map(Value::Object).unwrap_or(Value::Undefined),
            );
            d.set_own(
                Key::str("set"),
                s.clone().map(Value::Object).unwrap_or(Value::Undefined),
            );
        }
    }
    d.set_own(Key::str("enumerable"), Value::Bool(p.enumerable));
    d.set_own(Key::str("configurable"), Value::Bool(p.configurable));
    Value::Object(d)
}

/// `Object.defineProperty(o, key, desc)`.
pub fn define_property(i: &mut Interp, o: &Obj, key: Key, desc: &Obj) -> Res<()> {
    let dv = Value::Object(desc.clone());
    let field = |i: &mut Interp, name: &str| -> Res<Option<Value>> {
        let k = Key::str(name);
        if i.has_property(desc, &k) {
            Ok(Some(i.get(&dv, &k)?))
        } else {
            Ok(None)
        }
    };
    let existing = o.own(&key);
    let get = field(i, "get")?;
    let set = field(i, "set")?;
    let value = field(i, "value")?;
    let writable = field(i, "writable")?.map(|v| v.truthy());
    let enumerable = field(i, "enumerable")?
        .map(|v| v.truthy())
        .unwrap_or_else(|| existing.as_ref().is_some_and(|p| p.enumerable));
    let configurable = field(i, "configurable")?
        .map(|v| v.truthy())
        .unwrap_or_else(|| existing.as_ref().is_some_and(|p| p.configurable));
    let slot = if get.is_some() || set.is_some() {
        let (old_get, old_set) = match existing.as_ref().map(|p| &p.slot) {
            Some(Slot::Accessor(g, s)) => (g.clone(), s.clone()),
            _ => (None, None),
        };
        Slot::Accessor(
            match get {
                Some(Value::Object(f)) => Some(f),
                Some(_) => None,
                None => old_get,
            },
            match set {
                Some(Value::Object(f)) => Some(f),
                Some(_) => None,
                None => old_set,
            },
        )
    } else {
        let old = match existing.as_ref().map(|p| &p.slot) {
            Some(Slot::Data(v) | Slot::ReadOnly(v)) => Some(v.clone()),
            _ => None,
        };
        let v = value.or(old).unwrap_or(Value::Undefined);
        let writable = writable.unwrap_or_else(|| existing.as_ref().is_some_and(|p| p.writable()));
        if writable {
            Slot::Data(v)
        } else {
            Slot::ReadOnly(v)
        }
    };
    o.define(
        key,
        Prop {
            slot,
            enumerable,
            configurable,
        },
    );
    Ok(())
}

fn define_properties(i: &mut Interp, o: &Obj, props: &Obj) -> Res<()> {
    let pv = Value::Object(props.clone());
    for k in props.own_keys(true) {
        if let Value::Object(d) = i.get(&pv, &k)? {
            define_property(i, o, k, &d)?;
        }
    }
    Ok(())
}

/// An array-like's items (`apply`'s arguments).
pub fn list_of(i: &mut Interp, v: &Value) -> Res<Vec<Value>> {
    match v {
        Value::Undefined | Value::Null => Ok(Vec::new()),
        Value::Object(o) => {
            if let Class::Array(items) = &o.borrow().class {
                return Ok(items.clone());
            }
            let len = i.get(v, &Key::str("length"))?;
            let len = i.to_number(&len)?.max(0.0) as usize;
            let mut out = Vec::with_capacity(len.min(65536));
            for n in 0..len {
                out.push(i.get(v, &Key::index(n))?);
            }
            Ok(out)
        }
        _ => Err(i.type_error("CreateListFromArrayLike called on non-object")),
    }
}

/// A key's text, for messages.
pub fn key_words(key: &Key) -> String {
    match key {
        Key::String(s) => String::from_utf16_lossy(s),
        Key::Symbol(s) => format!(
            "Symbol({})",
            s.description
                .clone()
                .map(|d| String::from_utf16_lossy(&d))
                .unwrap_or_default()
        ),
    }
}

/// The UTF-16 of `s`.
pub fn t(s: &str) -> crate::lexer::Text {
    text(s)
}
