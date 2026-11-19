// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The errors (`Error` and its kinds) and `Boolean`.

use super::{make_error, this_bool};
use crate::builtins::{arg, constructor, method};
use crate::interp::Interp;
use crate::object::{Class, Obj, Slot};
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
