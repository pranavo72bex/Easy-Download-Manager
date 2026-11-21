// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Reflect`.

use super::{define_property, descriptor, list_of};
use crate::builtins::{arg, global, method};
use crate::interp::Interp;
use crate::value::Value;

/// `Reflect` and its functions.
pub(super) fn install_reflect(interp: &mut Interp) {
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
