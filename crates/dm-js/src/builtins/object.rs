// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Object`, `Function`, `Symbol`, the errors, `Boolean` and `Reflect`: `Object` here, the
//! others in files of their own beside it.

mod errors;
mod function;
mod reflect;

use std::rc::Rc;

use super::{arg, constructor, method};
use crate::interp::{Interp, Res};
use crate::lexer::text;
use crate::object::{Class, Obj, Prop, Slot};
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

    let object = install_object(interp, &op);
    install_object_checks(interp, &object);
    install_object_prototype(interp, &op);
    function::install_function(interp, &fp);
    function::install_symbol(interp, &sp);
    errors::install_errors(interp, &ep);
    errors::install_boolean(interp, &bp);
    reflect::install_reflect(interp);
}

/// `Object`, with its functions that list, copy, make and define.
fn install_object(interp: &mut Interp, op: &Obj) -> Obj {
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
        op,
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
    object
}

/// `Object`'s functions that read descriptors, lock, check and compare.
fn install_object_checks(interp: &mut Interp, object: &Obj) {
    method(interp, object, "getOwnPropertyNames", 1, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        Ok(i.array(string_keys(&o, false)))
    });
    method(interp, object, "getOwnPropertySymbols", 1, |i, _, a, _| {
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
        object,
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
        object,
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
        method(interp, object, name, 1, run);
    }
    method(interp, object, "isFrozen", 1, |_, _, a, _| {
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
    method(interp, object, "isSealed", 1, |_, _, a, _| {
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
    method(interp, object, "isExtensible", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Object(o) if o.borrow().extensible),
        ))
    });
    method(interp, object, "is", 2, |_, _, a, _| {
        let (x, y) = (arg(a, 0), arg(a, 1));
        Ok(Value::Bool(match (&x, &y) {
            (Value::Number(p), Value::Number(q)) => {
                (p.is_nan() && q.is_nan())
                    || (p == q && p.is_sign_negative() == q.is_sign_negative())
            }
            _ => x.strict_equals(&y),
        }))
    });
    method(interp, object, "hasOwn", 2, |i, _, a, _| {
        let o = i.to_object(&arg(a, 0))?;
        let key = i.to_key(&arg(a, 1))?;
        Ok(Value::Bool(o.own(&key).is_some()))
    });
}

/// `Object.prototype`'s methods and its `__proto__`.
fn install_object_prototype(interp: &mut Interp, op: &Obj) {
    method(interp, op, "hasOwnProperty", 1, |i, this, a, _| {
        let key = i.to_key(&arg(a, 0))?;
        let o = i.to_object(this)?;
        Ok(Value::Bool(o.own(&key).is_some()))
    });
    method(interp, op, "isPrototypeOf", 1, |i, this, a, _| {
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
    method(interp, op, "propertyIsEnumerable", 1, |i, this, a, _| {
        let key = i.to_key(&arg(a, 0))?;
        let o = i.to_object(this)?;
        Ok(Value::Bool(o.own(&key).is_some_and(|p| p.enumerable)))
    });
    method(interp, op, "toString", 0, object_to_string);
    method(interp, op, "toLocaleString", 0, |i, this, _, _| {
        let f = i.get_of(this, &Key::str("toString"))?;
        i.call(&f, this.clone(), &[])
    });
    method(interp, op, "valueOf", 0, |i, this, _, _| {
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
    method(interp, op, "__defineGetter__", 2, |i, this, a, _| {
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
    method(interp, op, "__lookupGetter__", 1, |i, this, a, _| {
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
