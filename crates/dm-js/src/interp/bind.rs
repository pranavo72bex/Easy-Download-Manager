// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A value put into a pattern (`[a, b] = …`, `{x, y: z = 1} = …`, a parameter, a `for`'s
//! variable), and iteration (`for … of`, spreading, `Array.from`).

use crate::ast::{Pattern, VarKind};
use crate::object::{Class, IterState};
use crate::value::{Key, Value};

use super::{Binding, Interp, Res, Scope};

/// The most items an iteration takes (an endless iterator spread would fill the memory).
const MOST_ITEMS: usize = 10_000_000;

impl Interp {
    /// Puts `value` into `pattern`: declared in `scope` as `kind` (`var` is declared already, by
    /// hoisting), or assigned where `kind` is none.
    pub fn bind(
        &mut self,
        pattern: &Pattern,
        value: Value,
        scope: &Scope,
        kind: Option<VarKind>,
    ) -> Res<()> {
        match pattern {
            Pattern::Name(name) => match kind {
                Some(VarKind::Let) | Some(VarKind::Const) => {
                    scope.borrow_mut().vars.insert(
                        name.clone(),
                        Binding {
                            value,
                            mutable: kind != Some(VarKind::Const),
                        },
                    );
                    Ok(())
                }
                _ => self.assign_name(scope, name, value),
            },
            Pattern::Place(e) => self.assign_to(e, value, scope),
            Pattern::Default(target, default) => {
                let value = if matches!(value, Value::Undefined) {
                    let name = match &**target {
                        Pattern::Name(n) => Some(n.clone()),
                        _ => None,
                    };
                    self.eval_named(default, scope, name.as_ref())?
                } else {
                    value
                };
                self.bind(target, value, scope, kind)
            }
            Pattern::Array(items, rest) => {
                if value.is_nullish() {
                    return Err(self.type_error(&format!(
                        "{} is not iterable",
                        if matches!(value, Value::Null) {
                            "null"
                        } else {
                            "undefined"
                        }
                    )));
                }
                let all: Vec<Value> = self.iterate(value)?.collect::<Res<_>>()?;
                let mut it = all.into_iter();
                for item in items {
                    let v = it.next().unwrap_or(Value::Undefined);
                    if let Some(target) = item {
                        self.bind(target, v, scope, kind)?;
                    }
                }
                if let Some(rest) = rest {
                    let left = self.array(it.collect());
                    self.bind(rest, left, scope, kind)?;
                }
                Ok(())
            }
            Pattern::Object(props, rest) => {
                if value.is_nullish() {
                    return Err(self.type_error(&format!(
                        "Cannot destructure '{}' as it is {}.",
                        if matches!(value, Value::Null) {
                            "null"
                        } else {
                            "undefined"
                        },
                        if matches!(value, Value::Null) {
                            "null"
                        } else {
                            "undefined"
                        }
                    )));
                }
                let mut taken = Vec::new();
                for (key, target) in props {
                    let key = self.prop_key(key, scope)?;
                    let v = self.get_of(&value, &key)?;
                    taken.push(key);
                    self.bind(target, v, scope, kind)?;
                }
                if let Some(rest) = rest {
                    let o = self.plain_object();
                    self.copy_own(&value, &o, &taken)?;
                    self.bind(rest, Value::Object(o), scope, kind)?;
                }
                Ok(())
            }
        }
    }

    /// The values `v` gives when iterated, by its `Symbol.iterator` (arrays, strings, maps and
    /// sets read directly while their iterator is the built-in one).
    pub fn iterate(&mut self, v: Value) -> Res<Box<dyn Iterator<Item = Res<Value>>>> {
        let iter_key = Key::Symbol(self.intrinsics.sym_iterator.clone());
        let f = self.get_of(&v, &iter_key)?;
        let builtin = f.strict_equals(&self.intrinsics.array_values);
        if builtin {
            if let Value::Object(o) = &v {
                if let Class::Array(items) = &o.borrow().class {
                    return Ok(Box::new(items.clone().into_iter().map(Ok)));
                }
            }
        }
        if let Value::String(s) = &v {
            if f.strict_equals(&self.intrinsics.string_iterator) {
                let chars: Vec<Value> = code_points(s)
                    .into_iter()
                    .map(|c| Value::String(c.into()))
                    .collect();
                return Ok(Box::new(chars.into_iter().map(Ok)));
            }
        }
        if !matches!(&f, Value::Object(fo) if fo.is_callable()) {
            return Err(self.type_error("object is not iterable"));
        }
        let iterator = self.call(&f, v, &[])?;
        let next_fn = self.get_of(&iterator, &Key::str("next"))?;
        let mut out = Vec::new();
        loop {
            // A built-in iterator walked without calling through the program.
            let step = match &iterator {
                Value::Object(o) if matches!(o.borrow().class, Class::Iterator(_)) => {
                    crate::builtins::iterator::step(self, o)?
                }
                _ => {
                    let r = self.call(&next_fn, iterator.clone(), &[])?;
                    if !matches!(r, Value::Object(_)) {
                        return Err(self.type_error("Iterator result is not an object"));
                    }
                    let done = self.get(&r, &Key::str("done"))?.truthy();
                    if done {
                        None
                    } else {
                        Some(self.get(&r, &Key::str("value"))?)
                    }
                }
            };
            match step {
                Some(v) => out.push(v),
                None => break,
            }
            if out.len() > MOST_ITEMS {
                return Err(self.range_error("an iteration that does not end"));
            }
        }
        Ok(Box::new(out.into_iter().map(Ok)))
    }

    /// A built-in iterator over `of`.
    pub fn array_iterator(&self, of: Value, kind: u8) -> Value {
        Value::Object(crate::object::Obj::new(
            Some(self.intrinsics.array_iterator_proto.clone()),
            Class::Iterator(IterState::Array {
                of,
                at: 0,
                kind,
                done: false,
            }),
        ))
    }
}

/// A string's code points, each as UTF-16 (a pair where it is one).
pub fn code_points(s: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if (0xD800..0xDC00).contains(&c)
            && s.get(i + 1).is_some_and(|d| (0xDC00..0xE000).contains(d))
        {
            out.push(vec![c, s[i + 1]]);
            i += 2;
        } else {
            out.push(vec![c]);
            i += 1;
        }
    }
    out
}
