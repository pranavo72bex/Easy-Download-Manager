// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Map`, `Set`, `WeakMap` and `WeakSet`: entries in the order added. A weak one holds its
//! keys like any other (nothing is collected while a program here runs).

use super::{arg, constructor, getter, method};
use crate::interp::{Interp, Res};
use crate::object::{Class, Entries, IterState, Obj};
use crate::value::{Key, Value};

/// Which collection.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Map,
    Set,
    WeakMap,
    WeakSet,
}

/// Installs them.
pub fn install(interp: &mut Interp) {
    let i = &interp.intrinsics;
    let (mp, sp, wmp, wsp) = (
        i.map_proto.clone(),
        i.set_proto.clone(),
        i.weakmap_proto.clone(),
        i.weakset_proto.clone(),
    );
    constructor(
        interp,
        "Map",
        0,
        |i, _, a, made| make(i, a, made, Kind::Map),
        &mp,
    );
    constructor(
        interp,
        "Set",
        0,
        |i, _, a, made| make(i, a, made, Kind::Set),
        &sp,
    );
    constructor(
        interp,
        "WeakMap",
        0,
        |i, _, a, made| make(i, a, made, Kind::WeakMap),
        &wmp,
    );
    constructor(
        interp,
        "WeakSet",
        0,
        |i, _, a, made| make(i, a, made, Kind::WeakSet),
        &wsp,
    );
    let tag = Key::Symbol(interp.intrinsics.sym_to_string_tag.clone());
    for (p, name) in [
        (&mp, "Map"),
        (&sp, "Set"),
        (&wmp, "WeakMap"),
        (&wsp, "WeakSet"),
    ] {
        p.set_hidden(tag.clone(), Value::str(name));
        method(interp, p, "has", 1, |i, this, a, _| {
            let found = with(i, this, |e| e.find(&arg(a, 0)).is_some())?;
            Ok(Value::Bool(found))
        });
        method(interp, p, "delete", 1, |i, this, a, _| {
            let gone = with(i, this, |e| match e.find(&arg(a, 0)) {
                Some(n) => {
                    e.list[n] = None;
                    true
                }
                None => false,
            })?;
            Ok(Value::Bool(gone))
        });
    }
    for p in [&mp, &wmp] {
        method(interp, p, "get", 1, |i, this, a, _| {
            let v = with(i, this, |e| {
                e.find(&arg(a, 0))
                    .and_then(|n| e.list[n].as_ref().map(|(_, v)| v.clone()))
            })?;
            Ok(v.unwrap_or(Value::Undefined))
        });
        method(interp, p, "set", 2, |i, this, a, _| {
            let key = normal_key(arg(a, 0));
            with(i, this, |e| match e.find(&key) {
                Some(n) => e.list[n] = Some((key, arg(a, 1))),
                None => e.list.push(Some((key, arg(a, 1)))),
            })?;
            Ok(this.clone())
        });
    }
    for p in [&sp, &wsp] {
        method(interp, p, "add", 1, |i, this, a, _| {
            let key = normal_key(arg(a, 0));
            with(i, this, |e| {
                if e.find(&key).is_none() {
                    e.list.push(Some((key.clone(), key)));
                }
            })?;
            Ok(this.clone())
        });
    }
    for p in [&mp, &sp] {
        method(interp, p, "clear", 0, |i, this, _, _| {
            with(i, this, |e| e.list.clear())?;
            Ok(Value::Undefined)
        });
        getter(interp, p, Key::str("size"), "size", |i, this, _, _| {
            let n = with(i, this, |e| e.size())?;
            Ok(Value::Number(n as f64))
        });
        method(interp, p, "forEach", 1, |i, this, a, _| {
            let Value::Object(o) = this else {
                return Err(i.type_error("not a collection"));
            };
            let mut n = 0;
            loop {
                let entry = match &o.borrow().class {
                    Class::Map(e) | Class::Set(e) => match e.list.get(n) {
                        None => break,
                        Some(x) => x.clone(),
                    },
                    _ => break,
                };
                n += 1;
                if let Some((k, v)) = entry {
                    i.call(&arg(a, 0), arg(a, 1), &[v, k, this.clone()])?;
                }
            }
            Ok(Value::Undefined)
        });
        method(interp, p, "keys", 0, |i, this, _, _| iterator(i, this, 0));
        method(interp, p, "values", 0, |i, this, _, _| iterator(i, this, 1));
        method(interp, p, "entries", 0, |i, this, _, _| {
            iterator(i, this, 2)
        });
    }
    let iter_key = Key::Symbol(interp.intrinsics.sym_iterator.clone());
    let entries = mp.own(&Key::str("entries")).map(|p| match p.slot {
        crate::object::Slot::Data(v) => v,
        _ => Value::Undefined,
    });
    if let Some(e) = entries {
        mp.set_hidden(iter_key.clone(), e);
    }
    let values = sp.own(&Key::str("values")).map(|p| match p.slot {
        crate::object::Slot::Data(v) => v,
        _ => Value::Undefined,
    });
    if let Some(v) = values {
        sp.set_hidden(iter_key, v);
    }
}

/// A key as a map holds it: `-0` as `0`.
fn normal_key(v: Value) -> Value {
    match v {
        Value::Number(n) if n == 0.0 && n.is_sign_negative() => Value::Number(0.0),
        other => other,
    }
}

fn make(i: &mut Interp, a: &[Value], made: Option<&Obj>, kind: Kind) -> Res<Value> {
    let Some(proto) = made else {
        return Err(i.type_error("Constructor requires 'new'"));
    };
    let class = match kind {
        Kind::Map => Class::Map(Entries::default()),
        Kind::Set => Class::Set(Entries::default()),
        Kind::WeakMap => Class::WeakMap(Entries::default()),
        Kind::WeakSet => Class::WeakSet(Entries::default()),
    };
    let o = Value::Object(Obj::new(Some(proto.clone()), class));
    let source = arg(a, 0);
    if source.is_nullish() {
        return Ok(o);
    }
    let adder = i.get(
        &o,
        &Key::str(if matches!(kind, Kind::Map | Kind::WeakMap) {
            "set"
        } else {
            "add"
        }),
    )?;
    for item in i.iterate(source)? {
        let item = item?;
        if matches!(kind, Kind::Map | Kind::WeakMap) {
            let k = i.get_of(&item, &Key::index(0))?;
            let v = i.get_of(&item, &Key::index(1))?;
            i.call(&adder, o.clone(), &[k, v])?;
        } else {
            i.call(&adder, o.clone(), &[item])?;
        }
    }
    Ok(o)
}

/// Runs `f` on `this`'s entries.
fn with<R>(i: &mut Interp, this: &Value, f: impl FnOnce(&mut Entries) -> R) -> Res<R> {
    let Value::Object(o) = this else {
        return Err(i.type_error("Method called on incompatible receiver"));
    };
    let mut me = o.borrow_mut();
    match &mut me.class {
        Class::Map(e) | Class::Set(e) | Class::WeakMap(e) | Class::WeakSet(e) => Ok(f(e)),
        _ => {
            drop(me);
            Err(i.type_error("Method called on incompatible receiver"))
        }
    }
}

fn iterator(i: &mut Interp, this: &Value, kind: u8) -> Res<Value> {
    let Value::Object(o) = this else {
        return Err(i.type_error("not a collection"));
    };
    Ok(Value::Object(Obj::new(
        Some(i.intrinsics.array_iterator_proto.clone()),
        Class::Iterator(IterState::Entries {
            of: o.clone(),
            at: 0,
            kind,
        }),
    )))
}
