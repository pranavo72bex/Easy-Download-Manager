// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The built-in iterators: an array's, a string's, a map's and a set's, all walked by one
//! `next` ([`step`]), and `%IteratorPrototype%`, whose `[Symbol.iterator]` is the iterator itself.

use super::{method, symbol_method};
use crate::interp::{Interp, Res};
use crate::object::{Class, IterState, Obj};
use crate::value::{Key, Value};

/// Installs them.
pub fn install(interp: &mut Interp) {
    let ip = interp.intrinsics.iterator_proto.clone();
    let key = Key::Symbol(interp.intrinsics.sym_iterator.clone());
    symbol_method(interp, &ip, &key, "[Symbol.iterator]", |_, this, _, _| {
        Ok(this.clone())
    });
    let aip = interp.intrinsics.array_iterator_proto.clone();
    method(interp, &aip, "next", 0, |i, this, _, _| {
        let Value::Object(o) = this else {
            return Err(i.type_error("next called on a non-iterator"));
        };
        let o = o.clone();
        let item = step(i, &o)?;
        Ok(result(i, item))
    });
    let tag = Key::Symbol(interp.intrinsics.sym_to_string_tag.clone());
    aip.set_hidden(tag, Value::str("Array Iterator"));
}

/// `{value, done}`.
pub fn result(i: &Interp, item: Option<Value>) -> Value {
    let r = i.plain_object();
    let done = item.is_none();
    r.set_own(Key::str("value"), item.unwrap_or(Value::Undefined));
    r.set_own(Key::str("done"), Value::Bool(done));
    Value::Object(r)
}

/// The built-in iterator `o`'s next value, or none at its end.
pub fn step(i: &mut Interp, o: &Obj) -> Res<Option<Value>> {
    // What to read, worked out without holding the iterator (reading may run a getter).
    enum Next {
        Index(Value, usize, u8),
        Done,
        Value(Value),
        Entry(Obj, usize, u8),
        Match(Obj, crate::lexer::Text),
    }
    let next = {
        let mut me = o.borrow_mut();
        let Class::Iterator(state) = &mut me.class else {
            return Err(i.type_error("not an iterator"));
        };
        match state {
            IterState::Array { of, at, kind, done } => {
                if *done {
                    Next::Done
                } else {
                    let n = *at;
                    *at += 1;
                    Next::Index(of.clone(), n, *kind)
                }
            }
            IterState::String { of, at } => {
                if *at >= of.len() {
                    Next::Done
                } else {
                    let c = of[*at];
                    let pair = (0xD800..0xDC00).contains(&c)
                        && of
                            .get(*at + 1)
                            .is_some_and(|d| (0xDC00..0xE000).contains(d));
                    let n = if pair { 2 } else { 1 };
                    let piece: Vec<u16> = of[*at..*at + n].to_vec();
                    *at += n;
                    Next::Value(Value::String(piece.into()))
                }
            }
            IterState::Entries { of, at, kind } => {
                let n = *at;
                *at += 1;
                Next::Entry(of.clone(), n, *kind)
            }
            IterState::Matches { pattern, of, done } => {
                if *done {
                    Next::Done
                } else {
                    Next::Match(pattern.clone(), of.clone())
                }
            }
        }
    };
    match next {
        Next::Done => Ok(None),
        Next::Value(v) => Ok(Some(v)),
        Next::Index(of, n, kind) => {
            let len = match &of {
                Value::Object(a) => match &a.borrow().class {
                    Class::Array(items) => Some(items.len()),
                    _ => None,
                },
                _ => None,
            };
            let len = match len {
                Some(l) => l,
                None => {
                    let l = i.get_of(&of, &Key::str("length"))?;
                    i.to_number(&l)?.max(0.0) as usize
                }
            };
            if n >= len {
                if let Class::Iterator(IterState::Array { done, .. }) = &mut o.borrow_mut().class {
                    *done = true;
                }
                return Ok(None);
            }
            Ok(Some(match kind {
                0 => Value::Number(n as f64),
                1 => i.get_of(&of, &Key::index(n))?,
                _ => {
                    let v = i.get_of(&of, &Key::index(n))?;
                    i.array(vec![Value::Number(n as f64), v])
                }
            }))
        }
        Next::Entry(of, mut n, kind) => {
            // Past deleted entries, to the next one there.
            loop {
                let entry = match &of.borrow().class {
                    Class::Map(e) | Class::Set(e) => match e.list.get(n) {
                        None => return Ok(None),
                        Some(entry) => entry.clone(),
                    },
                    _ => return Ok(None),
                };
                if let Some((k, v)) = entry {
                    if let Class::Iterator(IterState::Entries { at, .. }) =
                        &mut o.borrow_mut().class
                    {
                        *at = n + 1;
                    }
                    return Ok(Some(match kind {
                        0 => k,
                        1 => v,
                        _ => i.array(vec![k, v]),
                    }));
                }
                n += 1;
            }
        }
        Next::Match(pattern, of) => {
            let found = crate::builtins::regexp::exec(i, &pattern, &of)?;
            match found {
                Value::Null => {
                    if let Class::Iterator(IterState::Matches { done, .. }) =
                        &mut o.borrow_mut().class
                    {
                        *done = true;
                    }
                    Ok(None)
                }
                m => {
                    // An empty match moves on by one, or it would be found for ever.
                    let whole = i.get_of(&m, &Key::index(0))?;
                    if matches!(&whole, Value::String(s) if s.is_empty()) {
                        let pv = Value::Object(pattern.clone());
                        let last = i.get(&pv, &Key::str("lastIndex"))?;
                        let last = i.to_number(&last)?;
                        i.set(&pv, Key::str("lastIndex"), Value::Number(last + 1.0))?;
                    }
                    Ok(Some(m))
                }
            }
        }
    }
}
