// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The errors (`Error` and its kinds) and `Boolean`.

use super::{make_error, this_bool};
use crate::builtins::{arg, constructor, method};
use crate::interp::Interp;
use crate::interp::Res;
use crate::object::{Class, NativeFn, Obj, Slot};
use crate::value::{Key, Value};

/// `Error`, with its prototype's `toString`, and its kinds (`TypeError`, …).
pub(super) fn install_errors(interp: &mut Interp, ep: &Obj) {
    let error = constructor(interp, "Error", 1, make_error, ep);
    let _ = error;
    ep.set_hidden(Key::str("name"), Value::str("Error"));
    ep.set_hidden(Key::str("message"), Value::str(""));
    method(interp, ep, "toString", 0, |i, this, _, _| {
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
        let (make, length): (NativeFn, usize) = if name == "AggregateError" {
            (make_aggregate_error, 2)
        } else {
            (make_error, 1)
        };
        let c = constructor(interp, name, length, make, &proto);
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
}

/// `AggregateError(errors, message, options)`: an error whose `errors` are the values
/// `errors` gives when iterated.
fn make_aggregate_error(
    i: &mut Interp,
    this: &Value,
    a: &[Value],
    made: Option<&Obj>,
) -> Res<Value> {
    let rest: Vec<Value> = a.iter().skip(1).cloned().collect();
    let e = make_error(i, this, &rest, made)?;
    let mut errors = Vec::new();
    for v in i.iterate(arg(a, 0))? {
        errors.push(v?);
    }
    if let Value::Object(o) = &e {
        o.set_hidden(Key::str("errors"), i.array(errors));
    }
    Ok(e)
}

/// `Boolean`, and its prototype's methods.
pub(super) fn install_boolean(interp: &mut Interp, bp: &Obj) {
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
        bp,
    );
    method(interp, bp, "toString", 0, |i, this, _, _| {
        Ok(Value::str(if this_bool(i, this)? {
            "true"
        } else {
            "false"
        }))
    });
    method(interp, bp, "valueOf", 0, |i, this, _, _| {
        Ok(Value::Bool(this_bool(i, this)?))
    });
}
