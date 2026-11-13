// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Array`. A real array's methods work on its list of elements directly; on another object
//! (an array-like: `arguments`, `{length: 2, 0: a, 1: b}`) through its `length` and indexes.

use std::cmp::Ordering;

use super::{arg, constructor, int_arg, method, symbol_method};
use crate::interp::{Interp, Res};
use crate::object::{Class, Obj};
use crate::value::{relative_index, Key, Value};

/// Installs `Array`.
pub fn install(interp: &mut Interp) {
    let ap = interp.intrinsics.array_proto.clone();
    let array = constructor(
        interp,
        "Array",
        1,
        |i, _, a, made| {
            let items = match a {
                [Value::Number(n)] => {
                    if n.fract() != 0.0 || *n < 0.0 || *n >= 4294967296.0 {
                        return Err(i.range_error("Invalid array length"));
                    }
                    vec![Value::Undefined; *n as usize]
                }
                _ => a.to_vec(),
            };
            let proto = made
                .cloned()
                .unwrap_or_else(|| i.intrinsics.array_proto.clone());
            Ok(Value::Object(Obj::new(Some(proto), Class::Array(items))))
        },
        &ap,
    );
    method(interp, &array, "isArray", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Object(o) if matches!(o.borrow().class, Class::Array(_))),
        ))
    });
    method(
        interp,
        &array,
        "of",
        0,
        |i, _, a, _| Ok(i.array(a.to_vec())),
    );
    method(interp, &array, "from", 1, |i, _, a, _| {
        let source = arg(a, 0);
        let map = arg(a, 1);
        let iter_key = Key::Symbol(i.intrinsics.sym_iterator.clone());
        let items: Vec<Value> = if source.is_nullish() {
            return Err(i.type_error("Array.from requires an array-like object"));
        } else if !i.get_of(&source, &iter_key)?.is_nullish() {
            i.iterate(source)?.collect::<Res<_>>()?
        } else {
            super::object::list_of(i, &source)?
        };
        let items = if map.as_object().is_some_and(|f| f.is_callable()) {
            let mut out = Vec::with_capacity(items.len());
            for (n, v) in items.into_iter().enumerate() {
                out.push(i.call(&map, arg(a, 2), &[v, Value::Number(n as f64)])?);
            }
            out
        } else {
            items
        };
        Ok(i.array(items))
    });

    method(interp, &ap, "push", 1, |i, this, a, _| {
        if let Some(n) = with_items(this, |items| {
            items.extend_from_slice(a);
            items.len()
        }) {
            return Ok(Value::Number(n as f64));
        }
        let mut len = length(i, this)?;
        for v in a {
            i.set(this, Key::index(len), v.clone())?;
            len += 1;
        }
        i.set(this, Key::str("length"), Value::Number(len as f64))?;
        Ok(Value::Number(len as f64))
    });
    method(interp, &ap, "pop", 0, |i, this, _, _| {
        if let Some(v) = with_items(this, |items| items.pop()) {
            return Ok(v.unwrap_or(Value::Undefined));
        }
        let mut list = items_of(i, this)?;
        let v = list.pop().unwrap_or(Value::Undefined);
        write_back(i, this, list)?;
        Ok(v)
    });
    method(interp, &ap, "shift", 0, |i, this, _, _| {
        if let Some(v) = with_items(this, |items| (!items.is_empty()).then(|| items.remove(0))) {
            return Ok(v.unwrap_or(Value::Undefined));
        }
        let mut list = items_of(i, this)?;
        let v = if list.is_empty() {
            Value::Undefined
        } else {
            list.remove(0)
        };
        write_back(i, this, list)?;
        Ok(v)
    });
    method(interp, &ap, "unshift", 1, |i, this, a, _| {
        if let Some(n) = with_items(this, |items| {
            items.splice(0..0, a.iter().cloned());
            items.len()
        }) {
            return Ok(Value::Number(n as f64));
        }
        let mut list = items_of(i, this)?;
        list.splice(0..0, a.iter().cloned());
        let n = list.len();
        write_back(i, this, list)?;
        Ok(Value::Number(n as f64))
    });
    method(interp, &ap, "splice", 2, |i, this, a, _| {
        let len = length(i, this)?;
        let start = relative_index(i.to_number(&arg(a, 0))?, len);
        let count = match a.len() {
            0 => 0,
            1 => len - start,
            _ => (int_arg(i, a, 1)?.max(0.0) as usize).min(len - start),
        };
        let inserted: Vec<Value> = a.get(2..).unwrap_or(&[]).to_vec();
        if let Some(removed) = with_items(this, |items| {
            items
                .splice(start..start + count, inserted.iter().cloned())
                .collect::<Vec<_>>()
        }) {
            return Ok(i.array(removed));
        }
        let mut list = items_of(i, this)?;
        let removed: Vec<Value> = list.splice(start..start + count, inserted).collect();
        write_back(i, this, list)?;
        Ok(i.array(removed))
    });
    method(interp, &ap, "slice", 2, |i, this, a, _| {
        let list = items_of(i, this)?;
        let len = list.len();
        let start = relative_index(i.to_number(&arg(a, 0))?, len);
        let end = match arg(a, 1) {
            Value::Undefined => len,
            v => relative_index(i.to_number(&v)?, len),
        };
        Ok(i.array(list.get(start..end.max(start)).unwrap_or(&[]).to_vec()))
    });
    method(interp, &ap, "concat", 1, |i, this, a, _| {
        let mut out = items_of(i, this)?;
        let spreadable = Key::Symbol(i.intrinsics.sym_concat_spreadable.clone());
        for v in a {
            let spread = match v {
                Value::Object(o) => {
                    let s = i.get(v, &spreadable)?;
                    if s.is_nullish() {
                        matches!(o.borrow().class, Class::Array(_))
                    } else {
                        s.truthy()
                    }
                }
                _ => false,
            };
            if spread {
                out.extend(items_of(i, v)?);
            } else {
                out.push(v.clone());
            }
        }
        Ok(i.array(out))
    });
    method(interp, &ap, "join", 1, |i, this, a, _| {
        let sep = match arg(a, 0) {
            Value::Undefined => crate::lexer::text(","),
            v => i.to_text(&v)?,
        };
        let list = items_of(i, this)?;
        let mut out: Vec<u16> = Vec::new();
        for (n, v) in list.iter().enumerate() {
            if n > 0 {
                out.extend_from_slice(&sep);
            }
            if !v.is_nullish() {
                out.extend_from_slice(&i.to_text(v)?);
            }
        }
        Ok(Value::String(out.into()))
    });
    method(interp, &ap, "toString", 0, |i, this, _, _| {
        let join = i.get_of(this, &Key::str("join"))?;
        if join.as_object().is_some_and(|f| f.is_callable()) {
            return i.call(&join, this.clone(), &[]);
        }
        super::object::object_to_string(i, this, &[], None)
    });
    method(interp, &ap, "toLocaleString", 0, |i, this, _, _| {
        let join = i.get_of(this, &Key::str("join"))?;
        i.call(&join, this.clone(), &[])
    });
    method(interp, &ap, "reverse", 0, |i, this, _, _| {
        if with_items(this, |items| items.reverse()).is_some() {
            return Ok(this.clone());
        }
        let mut list = items_of(i, this)?;
        list.reverse();
        write_back(i, this, list)?;
        Ok(this.clone())
    });
    method(interp, &ap, "indexOf", 1, |i, this, a, _| {
        let list = items_of(i, this)?;
        let from = relative_index(i.to_number(&arg(a, 1))?, list.len());
        let wanted = arg(a, 0);
        Ok(Value::Number(
            list.iter()
                .skip(from)
                .position(|v| v.strict_equals(&wanted))
                .map(|p| (p + from) as f64)
                .unwrap_or(-1.0),
        ))
    });
    method(interp, &ap, "lastIndexOf", 1, |i, this, a, _| {
        let list = items_of(i, this)?;
        let wanted = arg(a, 0);
        let from = match a.get(1) {
            Some(v) => {
                let n = i.to_number(v)?;
                if n < 0.0 {
                    list.len() as f64 + n
                } else {
                    n.min(list.len() as f64 - 1.0)
                }
            }
            None => list.len() as f64 - 1.0,
        };
        let mut k = from;
        while k >= 0.0 {
            if list[k as usize].strict_equals(&wanted) {
                return Ok(Value::Number(k));
            }
            k -= 1.0;
        }
        Ok(Value::Number(-1.0))
    });
    method(interp, &ap, "includes", 1, |i, this, a, _| {
        let list = items_of(i, this)?;
        let from = relative_index(i.to_number(&arg(a, 1))?, list.len());
        let wanted = arg(a, 0);
        Ok(Value::Bool(
            list.iter().skip(from).any(|v| v.same_value_zero(&wanted)),
        ))
    });
    method(interp, &ap, "at", 1, |i, this, a, _| {
        let list = items_of(i, this)?;
        let n = int_arg(i, a, 0)?;
        let at = if n < 0.0 { list.len() as f64 + n } else { n };
        Ok(if at >= 0.0 {
            list.get(at as usize).cloned().unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        })
    });
    method(interp, &ap, "fill", 1, |i, this, a, _| {
        let len = length(i, this)?;
        let start = relative_index(i.to_number(&arg(a, 1))?, len);
        let end = match arg(a, 2) {
            Value::Undefined => len,
            v => relative_index(i.to_number(&v)?, len),
        };
        for n in start..end {
            i.set(this, Key::index(n), arg(a, 0))?;
        }
        Ok(this.clone())
    });
    method(interp, &ap, "copyWithin", 2, |i, this, a, _| {
        let mut list = items_of(i, this)?;
        let len = list.len();
        let target = relative_index(i.to_number(&arg(a, 0))?, len);
        let start = relative_index(i.to_number(&arg(a, 1))?, len);
        let end = match arg(a, 2) {
            Value::Undefined => len,
            v => relative_index(i.to_number(&v)?, len),
        };
        let piece: Vec<Value> = list.get(start..end.max(start)).unwrap_or(&[]).to_vec();
        for (k, v) in piece.into_iter().enumerate() {
            if target + k < len {
                list[target + k] = v;
            }
        }
        write_back(i, this, list)?;
        Ok(this.clone())
    });
    method(interp, &ap, "sort", 1, |i, this, a, _| {
        let compare = arg(a, 0);
        let mut list = items_of(i, this)?;
        sort(i, &mut list, &compare)?;
        write_back(i, this, list)?;
        Ok(this.clone())
    });
    // The callback methods: each element with its index and the array.
    method(interp, &ap, "forEach", 1, |i, this, a, _| {
        each(i, this, a, |_, _, _, _| Ok(None))?;
        Ok(Value::Undefined)
    });
    method(interp, &ap, "map", 1, |i, this, a, _| {
        let mut out = Vec::new();
        each(i, this, a, |_, _, _, r| {
            out.push(r);
            Ok(None)
        })?;
        Ok(i.array(out))
    });
    method(interp, &ap, "filter", 1, |i, this, a, _| {
        let mut out = Vec::new();
        each(i, this, a, |_, v, _, r| {
            if r.truthy() {
                out.push(v);
            }
            Ok(None)
        })?;
        Ok(i.array(out))
    });
    method(interp, &ap, "some", 1, |i, this, a, _| {
        let found = each(i, this, a, |_, _, _, r| {
            Ok(r.truthy().then_some(Value::Bool(true)))
        })?;
        Ok(found.unwrap_or(Value::Bool(false)))
    });
    method(interp, &ap, "every", 1, |i, this, a, _| {
        let found = each(i, this, a, |_, _, _, r| {
            Ok((!r.truthy()).then_some(Value::Bool(false)))
        })?;
        Ok(found.unwrap_or(Value::Bool(true)))
    });
    method(interp, &ap, "find", 1, |i, this, a, _| {
        let found = each(i, this, a, |_, v, _, r| Ok(r.truthy().then_some(v)))?;
        Ok(found.unwrap_or(Value::Undefined))
    });
    method(interp, &ap, "findIndex", 1, |i, this, a, _| {
        let found = each(i, this, a, |_, _, n, r| {
            Ok(r.truthy().then_some(Value::Number(n as f64)))
        })?;
        Ok(found.unwrap_or(Value::Number(-1.0)))
    });
    method(interp, &ap, "findLast", 1, |i, this, a, _| {
        let list = items_of(i, this)?;
        for n in (0..list.len()).rev() {
            let r = i.call(
                &arg(a, 0),
                arg(a, 1),
                &[list[n].clone(), Value::Number(n as f64), this.clone()],
            )?;
            if r.truthy() {
                return Ok(list[n].clone());
            }
        }
        Ok(Value::Undefined)
    });
    method(interp, &ap, "findLastIndex", 1, |i, this, a, _| {
        let list = items_of(i, this)?;
        for n in (0..list.len()).rev() {
            let r = i.call(
                &arg(a, 0),
                arg(a, 1),
                &[list[n].clone(), Value::Number(n as f64), this.clone()],
            )?;
            if r.truthy() {
                return Ok(Value::Number(n as f64));
            }
        }
        Ok(Value::Number(-1.0))
    });
    method(interp, &ap, "reduce", 1, |i, this, a, _| {
        reduce(i, this, a, false)
    });
    method(interp, &ap, "reduceRight", 1, |i, this, a, _| {
        reduce(i, this, a, true)
    });
    method(interp, &ap, "flat", 0, |i, this, a, _| {
        let depth = match arg(a, 0) {
            Value::Undefined => 1.0,
            v => i.to_number(&v)?,
        };
        let list = items_of(i, this)?;
        let mut out = Vec::new();
        flatten(i, list, depth, &mut out)?;
        Ok(i.array(out))
    });
    method(interp, &ap, "flatMap", 1, |i, this, a, _| {
        let mut mapped = Vec::new();
        each(i, this, a, |_, _, _, r| {
            mapped.push(r);
            Ok(None)
        })?;
        let mut out = Vec::new();
        flatten(i, mapped, 1.0, &mut out)?;
        Ok(i.array(out))
    });
    let values = method(interp, &ap, "values", 0, |i, this, _, _| {
        let o = i.to_object(this)?;
        Ok(i.array_iterator(Value::Object(o), 1))
    });
    interp.intrinsics.array_values = Value::Object(values.clone());
    let iter_key = Key::Symbol(interp.intrinsics.sym_iterator.clone());
    ap.set_hidden(iter_key, Value::Object(values));
    method(interp, &ap, "keys", 0, |i, this, _, _| {
        let o = i.to_object(this)?;
        Ok(i.array_iterator(Value::Object(o), 0))
    });
    method(interp, &ap, "entries", 0, |i, this, _, _| {
        let o = i.to_object(this)?;
        Ok(i.array_iterator(Value::Object(o), 2))
    });
    let species = Key::Symbol(interp.intrinsics.sym_species.clone());
    super::getter(
        interp,
        &array,
        species,
        "[Symbol.species]",
        |_, this, _, _| Ok(this.clone()),
    );
    let _ = symbol_method;
}

/// Runs `f` on a real array's elements; none where `this` is not one.
fn with_items<R>(this: &Value, f: impl FnOnce(&mut Vec<Value>) -> R) -> Option<R> {
    let Value::Object(o) = this else { return None };
    let mut me = o.borrow_mut();
    match &mut me.class {
        Class::Array(items) => Some(f(items)),
        _ => None,
    }
}

/// `this.length`.
fn length(i: &mut Interp, this: &Value) -> Res<usize> {
    if let Some(n) = with_items(this, |items| items.len()) {
        return Ok(n);
    }
    let l = i.get_of(this, &Key::str("length"))?;
    Ok(i.to_number(&l)?.clamp(0.0, 4294967295.0) as usize)
}

/// `this`'s elements, in order.
pub fn items_of(i: &mut Interp, this: &Value) -> Res<Vec<Value>> {
    if let Some(list) = with_items(this, |items| items.clone()) {
        return Ok(list);
    }
    if let Value::String(s) = this {
        return Ok(s.iter().map(|c| Value::String(vec![*c].into())).collect());
    }
    let o = i.to_object(this)?;
    super::object::list_of(i, &Value::Object(o))
}

/// An array-like's elements set back from `list`.
fn write_back(i: &mut Interp, this: &Value, list: Vec<Value>) -> Res<()> {
    let is_array = with_items(this, |_| ()).is_some();
    if is_array {
        with_items(this, |items| *items = list);
        return Ok(());
    }
    let old = length(i, this)?;
    for (n, v) in list.iter().enumerate() {
        i.set(this, Key::index(n), v.clone())?;
    }
    if let Value::Object(o) = this {
        for n in list.len()..old {
            o.delete(&Key::index(n));
        }
    }
    i.set(this, Key::str("length"), Value::Number(list.len() as f64))
}

/// Calls the callback (`a[0]`, with `a[1]` its `this`) on each element; `step` is given
/// the element, its index and what the callback gave, and may end the walk with a value.
fn each(
    i: &mut Interp,
    this: &Value,
    a: &[Value],
    mut step: impl FnMut(&mut Interp, Value, usize, Value) -> Res<Option<Value>>,
) -> Res<Option<Value>> {
    let f = arg(a, 0);
    if !f.as_object().is_some_and(|o| o.is_callable()) {
        return Err(i.type_error("callback is not a function"));
    }
    let len = length(i, this)?;
    for n in 0..len {
        let v = i.get_of(this, &Key::index(n))?;
        let r = i.call(
            &f,
            arg(a, 1),
            &[v.clone(), Value::Number(n as f64), this.clone()],
        )?;
        if let Some(done) = step(i, v, n, r)? {
            return Ok(Some(done));
        }
    }
    Ok(None)
}

fn reduce(i: &mut Interp, this: &Value, a: &[Value], right: bool) -> Res<Value> {
    let f = arg(a, 0);
    let list = items_of(i, this)?;
    let mut order: Vec<usize> = (0..list.len()).collect();
    if right {
        order.reverse();
    }
    let mut it = order.into_iter();
    let mut acc = match a.get(1) {
        Some(v) => v.clone(),
        None => match it.next() {
            Some(n) => list[n].clone(),
            None => return Err(i.type_error("Reduce of empty array with no initial value")),
        },
    };
    for n in it {
        acc = i.call(
            &f,
            Value::Undefined,
            &[acc, list[n].clone(), Value::Number(n as f64), this.clone()],
        )?;
    }
    Ok(acc)
}

fn flatten(i: &mut Interp, list: Vec<Value>, depth: f64, out: &mut Vec<Value>) -> Res<()> {
    for v in list {
        let is_array =
            matches!(&v, Value::Object(o) if matches!(o.borrow().class, Class::Array(_)));
        if is_array && depth >= 1.0 {
            let inner = items_of(i, &v)?;
            flatten(i, inner, depth - 1.0, out)?;
        } else {
            out.push(v);
        }
    }
    Ok(())
}

/// Sorts `list` as `Array.prototype.sort` does: stably, undefined last, by `compare` or by
/// the elements' strings.
pub fn sort(i: &mut Interp, list: &mut [Value], compare: &Value) -> Res<()> {
    let mut error = None;
    let custom = compare.as_object().is_some_and(|f| f.is_callable());
    // Strings worked out once, where elements are compared as strings.
    let mut keyed: Vec<(Value, Option<crate::lexer::Text>)> = Vec::with_capacity(list.len());
    for v in list.iter() {
        let k = if custom || matches!(v, Value::Undefined) {
            None
        } else {
            Some(i.to_text(v)?)
        };
        keyed.push((v.clone(), k));
    }
    // A merge sort, as the comparison may run the program's own code.
    let mut order: Vec<usize> = (0..keyed.len()).collect();
    let cmp = |i: &mut Interp, x: usize, y: usize, error: &mut Option<Value>| -> Ordering {
        if error.is_some() {
            return Ordering::Equal;
        }
        let (a, ak) = &keyed[x];
        let (b, bk) = &keyed[y];
        match (matches!(a, Value::Undefined), matches!(b, Value::Undefined)) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        if custom {
            match i
                .call(compare, Value::Undefined, &[a.clone(), b.clone()])
                .and_then(|r| i.to_number(&r))
            {
                Ok(n) if n < 0.0 => Ordering::Less,
                Ok(n) if n > 0.0 => Ordering::Greater,
                Ok(_) => Ordering::Equal,
                Err(e) => {
                    *error = Some(e);
                    Ordering::Equal
                }
            }
        } else {
            ak.as_deref().cmp(&bk.as_deref())
        }
    };
    merge_sort(&mut order, &mut |x, y| cmp(i, x, y, &mut error));
    if let Some(e) = error {
        return Err(e);
    }
    let sorted: Vec<Value> = order.into_iter().map(|n| keyed[n].0.clone()).collect();
    list.clone_from_slice(&sorted);
    Ok(())
}

fn merge_sort(v: &mut [usize], cmp: &mut dyn FnMut(usize, usize) -> Ordering) {
    if v.len() <= 1 {
        return;
    }
    let mid = v.len() / 2;
    merge_sort(&mut v[..mid], cmp);
    merge_sort(&mut v[mid..], cmp);
    let mut merged = Vec::with_capacity(v.len());
    let (mut a, mut b) = (0, mid);
    while a < mid && b < v.len() {
        if cmp(v[b], v[a]) == Ordering::Less {
            merged.push(v[b]);
            b += 1;
        } else {
            merged.push(v[a]);
            a += 1;
        }
    }
    merged.extend_from_slice(&v[a..mid]);
    merged.extend_from_slice(&v[b..]);
    v.copy_from_slice(&merged);
}
