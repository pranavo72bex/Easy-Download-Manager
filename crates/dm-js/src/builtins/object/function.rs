// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Function` and `Symbol`, each with its prototype's methods.

use std::rc::Rc;

use super::{list_of, this_symbol};
use crate::builtins::{arg, constructor, getter, method, symbol_method};
use crate::interp::Interp;
use crate::object::{Class, Func, Obj, Prop};
use crate::value::{Key, Symbol, Value};

/// `Function`, and `Function.prototype`'s methods.
pub(super) fn install_function(interp: &mut Interp, fp: &Obj) {
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
        fp,
    );
    let _ = function;
    method(interp, fp, "call", 1, |i, this, a, _| {
        let rest = a.get(1..).unwrap_or(&[]).to_vec();
        i.call(this, arg(a, 0), &rest)
    });
    method(interp, fp, "apply", 2, |i, this, a, _| {
        let list = list_of(i, &arg(a, 1))?;
        i.call(this, arg(a, 0), &list)
    });
    method(interp, fp, "bind", 1, |i, this, a, _| {
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
    method(interp, fp, "toString", 0, |i, this, _, _| match this {
        Value::Object(f) if f.is_callable() => Ok(Value::str(&i.function_source(f))),
        _ => Err(i.type_error("Function.prototype.toString requires that 'this' be a Function")),
    });
    let has_instance = Key::Symbol(interp.intrinsics.sym_has_instance.clone());
    symbol_method(
        interp,
        fp,
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
}

/// `Symbol`, with the well-known symbols, and `Symbol.prototype`'s methods.
pub(super) fn install_symbol(interp: &mut Interp, sp: &Obj) {
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
        sp,
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
    method(interp, sp, "toString", 0, |i, this, _, _| {
        let s = this_symbol(i, this)?;
        let d = s
            .description
            .clone()
            .map(|d| String::from_utf16_lossy(&d))
            .unwrap_or_default();
        Ok(Value::str(&format!("Symbol({d})")))
    });
    method(interp, sp, "valueOf", 0, |i, this, _, _| {
        Ok(Value::Symbol(this_symbol(i, this)?))
    });
    getter(
        interp,
        sp,
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
        sp,
        &to_prim,
        "[Symbol.toPrimitive]",
        |i, this, _, _| Ok(Value::Symbol(this_symbol(i, this)?)),
    );
}
