// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Proxy`: an object whose reading, writing and calling go through a handler's traps (`get`,
//! `set`, `has`, `apply`, `construct`), where it has them, else to its target.

use super::{arg, global};
use crate::interp::{Interp, Res};
use crate::object::{Class, Obj};
use crate::value::{Key, Value};

/// Installs `Proxy`.
pub fn install(interp: &mut Interp) {
    let proxy = interp.native(
        "Proxy",
        2,
        |i, _, a, made| {
            if made.is_none() {
                return Err(i.type_error("Constructor Proxy requires 'new'"));
            }
            let (Value::Object(target), Value::Object(handler)) = (arg(a, 0), arg(a, 1)) else {
                return Err(
                    i.type_error("Cannot create proxy with a non-object as target or handler")
                );
            };
            let proto = target.proto();
            Ok(Value::Object(Obj::new(
                proto,
                Class::Proxy(target, handler),
            )))
        },
        true,
    );
    global(interp, "Proxy", Value::Object(proxy));
}

fn trap(i: &mut Interp, handler: &Obj, name: &str) -> Res<Option<Value>> {
    let f = i.get(&Value::Object(handler.clone()), &Key::str(name))?;
    Ok((!f.is_nullish()).then_some(f))
}

/// `proxy[key]`
pub fn get(i: &mut Interp, target: &Obj, handler: &Obj, key: &Key, receiver: &Value) -> Res<Value> {
    match trap(i, handler, "get")? {
        Some(f) => i.call(
            &f,
            Value::Object(handler.clone()),
            &[
                Value::Object(target.clone()),
                key.to_value(),
                receiver.clone(),
            ],
        ),
        None => i.get_from(target, key, receiver),
    }
}

/// `proxy[key] = value`
pub fn set(
    i: &mut Interp,
    target: &Obj,
    handler: &Obj,
    key: Key,
    value: Value,
    receiver: &Value,
) -> Res<()> {
    match trap(i, handler, "set")? {
        Some(f) => {
            i.call(
                &f,
                Value::Object(handler.clone()),
                &[
                    Value::Object(target.clone()),
                    key.to_value(),
                    value,
                    receiver.clone(),
                ],
            )?;
            Ok(())
        }
        None => i.set(&Value::Object(target.clone()), key, value),
    }
}

/// `key in proxy`
pub fn has(i: &mut Interp, target: &Obj, handler: &Obj, key: &Key) -> Res<Value> {
    match trap(i, handler, "has")? {
        Some(f) => {
            let r = i.call(
                &f,
                Value::Object(handler.clone()),
                &[Value::Object(target.clone()), key.to_value()],
            )?;
            Ok(Value::Bool(r.truthy()))
        }
        None => Ok(Value::Bool(i.has_property(target, key))),
    }
}

/// `proxy(…args)`
pub fn apply(
    i: &mut Interp,
    target: &Obj,
    handler: &Obj,
    this: Value,
    args: &[Value],
) -> Res<Value> {
    match trap(i, handler, "apply")? {
        Some(f) => {
            let list = i.array(args.to_vec());
            i.call(
                &f,
                Value::Object(handler.clone()),
                &[Value::Object(target.clone()), this, list],
            )
        }
        None => i.call(&Value::Object(target.clone()), this, args),
    }
}

/// `new proxy(…args)`
pub fn construct(
    i: &mut Interp,
    target: &Obj,
    handler: &Obj,
    args: &[Value],
    new_target: &Value,
) -> Res<Value> {
    match trap(i, handler, "construct")? {
        Some(f) => {
            let list = i.array(args.to_vec());
            i.call(
                &f,
                Value::Object(handler.clone()),
                &[Value::Object(target.clone()), list, new_target.clone()],
            )
        }
        None => i.construct(&Value::Object(target.clone()), args, None),
    }
}
