// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `ArrayBuffer` and the typed arrays (`Uint8Array` and its kin). Their reading methods are
//! `Array.prototype`'s, which work on anything with a `length` and indexes; those that make a
//! new array make one of the same kind.

use std::cell::RefCell;
use std::rc::Rc;

use super::{arg, constructor, getter, int_arg, method};
use crate::interp::{Interp, Res};
use crate::object::{Class, Obj, Slot};
use crate::typed::{Bytes, KINDS};
use crate::value::{relative_index, Key, Value};

/// Installs them.
pub fn install(interp: &mut Interp) {
    let abp = interp.intrinsics.array_buffer_proto.clone();
    constructor(
        interp,
        "ArrayBuffer",
        1,
        |i, _, a, made| {
            let Some(proto) = made else {
                return Err(i.type_error("Constructor ArrayBuffer requires 'new'"));
            };
            let n = int_arg(i, a, 0)?;
            if !(0.0..=1e9).contains(&n) {
                return Err(i.range_error("Array buffer allocation failed"));
            }
            let bytes = Bytes {
                buffer: Rc::new(RefCell::new(vec![0; n as usize])),
                offset: 0,
                length: n as usize,
                kind: 0,
            };
            Ok(Value::Object(Obj::new(
                Some(proto.clone()),
                Class::Bytes(bytes),
            )))
        },
        &abp,
    );
    getter(
        interp,
        &abp,
        Key::str("byteLength"),
        "byteLength",
        |i, this, _, _| {
            let (buffer, _, _, _) = view(i, this)?;
            let n = buffer.borrow().len();
            Ok(Value::Number(n as f64))
        },
    );
    method(interp, &abp, "slice", 2, |i, this, a, _| {
        let (buffer, _, _, _) = view(i, this)?;
        let len = buffer.borrow().len();
        let start = relative_index(i.to_number(&arg(a, 0))?, len);
        let end = match arg(a, 1) {
            Value::Undefined => len,
            v => relative_index(i.to_number(&v)?, len),
        };
        let piece = buffer.borrow()[start..end.max(start)].to_vec();
        let n = piece.len();
        let proto = i.intrinsics.array_buffer_proto.clone();
        Ok(Value::Object(Obj::new(
            Some(proto),
            Class::Bytes(Bytes {
                buffer: Rc::new(RefCell::new(piece)),
                offset: 0,
                length: n,
                kind: 0,
            }),
        )))
    });

    let base = interp.intrinsics.typed_protos[10].clone();
    // The array methods that only read.
    let ap = interp.intrinsics.array_proto.clone();
    for name in [
        "join",
        "indexOf",
        "lastIndexOf",
        "includes",
        "forEach",
        "every",
        "some",
        "find",
        "findIndex",
        "findLast",
        "findLastIndex",
        "reduce",
        "reduceRight",
        "at",
        "keys",
        "values",
        "entries",
        "toString",
        "toLocaleString",
    ] {
        if let Some(p) = ap.own(&Key::str(name)) {
            if let Slot::Data(v) = p.slot {
                base.set_hidden(Key::str(name), v);
            }
        }
    }
    let iter_key = Key::Symbol(interp.intrinsics.sym_iterator.clone());
    base.set_hidden(iter_key, interp.intrinsics.array_values.clone());
    getter(
        interp,
        &base,
        Key::str("length"),
        "length",
        |i, this, _, _| Ok(Value::Number(view(i, this)?.2 as f64)),
    );
    getter(
        interp,
        &base,
        Key::str("byteLength"),
        "byteLength",
        |i, this, _, _| {
            let (_, _, len, kind) = view(i, this)?;
            Ok(Value::Number((len * KINDS[kind as usize].1) as f64))
        },
    );
    getter(
        interp,
        &base,
        Key::str("byteOffset"),
        "byteOffset",
        |i, this, _, _| Ok(Value::Number(view(i, this)?.1 as f64)),
    );
    getter(
        interp,
        &base,
        Key::str("buffer"),
        "buffer",
        |i, this, _, _| {
            let (buffer, _, _, _) = view(i, this)?;
            let len = buffer.borrow().len();
            let proto = i.intrinsics.array_buffer_proto.clone();
            Ok(Value::Object(Obj::new(
                Some(proto),
                Class::Bytes(Bytes {
                    buffer,
                    offset: 0,
                    length: len,
                    kind: 0,
                }),
            )))
        },
    );
    method(interp, &base, "set", 1, |i, this, a, _| {
        let offset = int_arg(i, a, 1)?.max(0.0) as usize;
        let items = super::array::items_of(i, &arg(a, 0))?;
        let (_, _, len, _) = view(i, this)?;
        if offset + items.len() > len {
            return Err(i.range_error("offset is out of bounds"));
        }
        for (k, v) in items.into_iter().enumerate() {
            i.set(this, Key::index(offset + k), v)?;
        }
        Ok(Value::Undefined)
    });
    method(interp, &base, "subarray", 2, |i, this, a, _| {
        let (buffer, offset, len, kind) = view(i, this)?;
        let start = relative_index(i.to_number(&arg(a, 0))?, len);
        let end = match arg(a, 1) {
            Value::Undefined => len,
            v => relative_index(i.to_number(&v)?, len),
        };
        let size = KINDS[kind as usize].1;
        Ok(typed(
            i,
            Bytes {
                buffer,
                offset: offset + start * size,
                length: end.max(start) - start,
                kind,
            },
        ))
    });
    method(interp, &base, "slice", 2, |i, this, a, _| {
        let (_, _, len, kind) = view(i, this)?;
        let start = relative_index(i.to_number(&arg(a, 0))?, len);
        let end = match arg(a, 1) {
            Value::Undefined => len,
            v => relative_index(i.to_number(&v)?, len),
        };
        let items = super::array::items_of(i, this)?;
        let piece = items.get(start..end.max(start)).unwrap_or(&[]).to_vec();
        of_values(i, kind, &piece)
    });
    method(interp, &base, "map", 1, |i, this, a, _| {
        let (_, _, _, kind) = view(i, this)?;
        let items = super::array::items_of(i, this)?;
        let mut out = Vec::with_capacity(items.len());
        for (n, v) in items.into_iter().enumerate() {
            out.push(i.call(
                &arg(a, 0),
                arg(a, 1),
                &[v, Value::Number(n as f64), this.clone()],
            )?);
        }
        of_values(i, kind, &out)
    });
    method(interp, &base, "filter", 1, |i, this, a, _| {
        let (_, _, _, kind) = view(i, this)?;
        let items = super::array::items_of(i, this)?;
        let mut out = Vec::new();
        for (n, v) in items.into_iter().enumerate() {
            if i.call(
                &arg(a, 0),
                arg(a, 1),
                &[v.clone(), Value::Number(n as f64), this.clone()],
            )?
            .truthy()
            {
                out.push(v);
            }
        }
        of_values(i, kind, &out)
    });
    method(interp, &base, "fill", 1, |i, this, a, _| {
        let (_, _, len, _) = view(i, this)?;
        let n = i.to_number(&arg(a, 0))?;
        let start = relative_index(i.to_number(&arg(a, 1))?, len);
        let end = match arg(a, 2) {
            Value::Undefined => len,
            v => relative_index(i.to_number(&v)?, len),
        };
        for k in start..end {
            i.set(this, Key::index(k), Value::Number(n))?;
        }
        Ok(this.clone())
    });
    method(interp, &base, "reverse", 0, |i, this, _, _| {
        let mut items = super::array::items_of(i, this)?;
        items.reverse();
        for (k, v) in items.into_iter().enumerate() {
            i.set(this, Key::index(k), v)?;
        }
        Ok(this.clone())
    });
    method(interp, &base, "sort", 1, |i, this, a, _| {
        let mut items = super::array::items_of(i, this)?;
        let compare = arg(a, 0);
        if compare.as_object().is_some_and(|f| f.is_callable()) {
            super::array::sort(i, &mut items, &compare)?;
        } else {
            items.sort_by(|x, y| match (x, y) {
                (Value::Number(p), Value::Number(q)) => {
                    p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal)
                }
                _ => std::cmp::Ordering::Equal,
            });
        }
        for (k, v) in items.into_iter().enumerate() {
            i.set(this, Key::index(k), v)?;
        }
        Ok(this.clone())
    });

    install_data_view(interp);
    for kind in 1u8..=9 {
        let (name, size) = KINDS[kind as usize];
        let proto = interp.intrinsics.typed_protos[kind as usize].clone();
        let run: crate::object::NativeFn = match kind {
            1 => |i, _, a, m| make(i, a, m, 1),
            2 => |i, _, a, m| make(i, a, m, 2),
            3 => |i, _, a, m| make(i, a, m, 3),
            4 => |i, _, a, m| make(i, a, m, 4),
            5 => |i, _, a, m| make(i, a, m, 5),
            6 => |i, _, a, m| make(i, a, m, 6),
            7 => |i, _, a, m| make(i, a, m, 7),
            8 => |i, _, a, m| make(i, a, m, 8),
            _ => |i, _, a, m| make(i, a, m, 9),
        };
        let c = constructor(interp, name, 3, run, &proto);
        c.define(
            Key::str("BYTES_PER_ELEMENT"),
            crate::object::Prop::data(Value::Number(size as f64), false, false, false),
        );
        proto.define(
            Key::str("BYTES_PER_ELEMENT"),
            crate::object::Prop::data(Value::Number(size as f64), false, false, false),
        );
        let from: crate::object::NativeFn = |i, this, a, _| {
            let items: Vec<Value> = i.iterate(arg(a, 0))?.collect::<Res<_>>()?;
            let items = if arg(a, 1).as_object().is_some_and(|f| f.is_callable()) {
                let mut out = Vec::new();
                for (n, v) in items.into_iter().enumerate() {
                    out.push(i.call(&arg(a, 1), arg(a, 2), &[v, Value::Number(n as f64)])?);
                }
                out
            } else {
                items
            };
            let list = i.array(items);
            i.construct(this, &[list], None)
        };
        method(interp, &c, "from", 1, from);
        method(interp, &c, "of", 0, |i, this, a, _| {
            let list = i.array(a.to_vec());
            i.construct(this, &[list], None)
        });
    }
}

/// A view's buffer, offset in bytes, length in elements and kind.
/// A view as [`view`] gives it: buffer, offset in bytes, length in elements, kind.
type View = (Rc<RefCell<Vec<u8>>>, usize, usize, u8);

fn view(i: &mut Interp, this: &Value) -> Res<View> {
    if let Value::Object(o) = this {
        if let Class::Bytes(b) = &o.borrow().class {
            return Ok((b.buffer.clone(), b.offset, b.length, b.kind));
        }
    }
    Err(i.type_error("this is not a typed array"))
}

fn typed(i: &Interp, bytes: Bytes) -> Value {
    let proto = i.intrinsics.typed_protos[bytes.kind as usize].clone();
    Value::Object(Obj::new(Some(proto), Class::Bytes(bytes)))
}

/// A new typed array of `kind` holding `values`.
fn of_values(i: &mut Interp, kind: u8, values: &[Value]) -> Res<Value> {
    let size = KINDS[kind as usize].1;
    let bytes = Bytes {
        buffer: Rc::new(RefCell::new(vec![0; values.len() * size])),
        offset: 0,
        length: values.len(),
        kind,
    };
    for (k, v) in values.iter().enumerate() {
        let n = i.to_number(v)?;
        bytes.set(k, n);
    }
    Ok(typed(i, bytes))
}

/// `new Uint8Array(length | array | iterable | buffer, offset, length)`.
fn make(i: &mut Interp, a: &[Value], made: Option<&Obj>, kind: u8) -> Res<Value> {
    let Some(proto) = made else {
        return Err(i.type_error("Constructor requires 'new'"));
    };
    let size = KINDS[kind as usize].1;
    let bytes = match arg(a, 0) {
        Value::Object(o) if matches!(&o.borrow().class, Class::Bytes(b) if b.kind == 0) => {
            let buffer = match &o.borrow().class {
                Class::Bytes(b) => b.buffer.clone(),
                _ => unreachable!(),
            };
            let total = buffer.borrow().len();
            let offset = int_arg(i, a, 1)?.max(0.0) as usize;
            if !offset.is_multiple_of(size) || offset > total {
                return Err(i.range_error(
                    "start offset of a typed array should be a multiple of its element size",
                ));
            }
            let length = match arg(a, 2) {
                Value::Undefined => (total - offset) / size,
                v => i.to_number(&v)?.max(0.0) as usize,
            };
            if offset + length * size > total {
                return Err(i.range_error("Invalid typed array length"));
            }
            Bytes {
                buffer,
                offset,
                length,
                kind,
            }
        }
        Value::Object(_) => {
            let source = arg(a, 0);
            let iter_key = Key::Symbol(i.intrinsics.sym_iterator.clone());
            let items: Vec<Value> = if !i.get(&source, &iter_key)?.is_nullish() {
                i.iterate(source)?.collect::<Res<_>>()?
            } else {
                super::object::list_of(i, &source)?
            };
            let bytes = Bytes {
                buffer: Rc::new(RefCell::new(vec![0; items.len() * size])),
                offset: 0,
                length: items.len(),
                kind,
            };
            for (k, v) in items.iter().enumerate() {
                let n = i.to_number(v)?;
                bytes.set(k, n);
            }
            bytes
        }
        v => {
            let n = match v {
                Value::Undefined => 0.0,
                v => crate::value::to_integer(i.to_number(&v)?),
            };
            if !(0.0..=1e9).contains(&n) {
                return Err(i.range_error("Invalid typed array length"));
            }
            Bytes {
                buffer: Rc::new(RefCell::new(vec![0; n as usize * size])),
                offset: 0,
                length: n as usize,
                kind,
            }
        }
    };
    Ok(Value::Object(Obj::new(
        Some(proto.clone()),
        Class::Bytes(bytes),
    )))
}

/// `DataView`: numbers of each width read from and written to a buffer at a byte offset, in
/// either order (big-endian unless asked otherwise).
fn install_data_view(interp: &mut Interp) {
    let proto = interp.plain_object();
    constructor(
        interp,
        "DataView",
        1,
        |i, _, a, made| {
            let Some(proto) = made else {
                return Err(i.type_error("Constructor DataView requires 'new'"));
            };
            let buffer = match arg(a, 0) {
                Value::Object(o) => match &o.borrow().class {
                    Class::Bytes(b) if b.kind == 0 => Some(b.buffer.clone()),
                    _ => None,
                },
                _ => None,
            };
            let Some(buffer) = buffer else {
                return Err(
                    i.type_error("First argument to DataView constructor must be an ArrayBuffer")
                );
            };
            let total = buffer.borrow().len();
            let offset = int_arg(i, a, 1)?.max(0.0) as usize;
            let length = match arg(a, 2) {
                Value::Undefined => total.saturating_sub(offset),
                v => i.to_number(&v)?.max(0.0) as usize,
            };
            if offset + length > total {
                return Err(i.range_error("Invalid DataView length"));
            }
            Ok(Value::Object(Obj::new(
                Some(proto.clone()),
                Class::Bytes(Bytes {
                    buffer,
                    offset,
                    length,
                    kind: 255,
                }),
            )))
        },
        &proto,
    );
    getter(
        interp,
        &proto,
        Key::str("byteLength"),
        "byteLength",
        |i, this, _, _| Ok(Value::Number(view(i, this)?.2 as f64)),
    );
    getter(
        interp,
        &proto,
        Key::str("byteOffset"),
        "byteOffset",
        |i, this, _, _| Ok(Value::Number(view(i, this)?.1 as f64)),
    );
    for (name, width, how) in [
        ("Int8", 1, 1u8),
        ("Uint8", 1, 2),
        ("Int16", 2, 4),
        ("Uint16", 2, 5),
        ("Int32", 4, 6),
        ("Uint32", 4, 7),
        ("Float32", 4, 8),
        ("Float64", 8, 9),
    ] {
        let _ = width;
        let (get, set): (crate::object::NativeFn, crate::object::NativeFn) = match how {
            1 => (
                |i, t, a, _| dv_get(i, t, a, 1),
                |i, t, a, _| dv_set(i, t, a, 1),
            ),
            2 => (
                |i, t, a, _| dv_get(i, t, a, 2),
                |i, t, a, _| dv_set(i, t, a, 2),
            ),
            4 => (
                |i, t, a, _| dv_get(i, t, a, 4),
                |i, t, a, _| dv_set(i, t, a, 4),
            ),
            5 => (
                |i, t, a, _| dv_get(i, t, a, 5),
                |i, t, a, _| dv_set(i, t, a, 5),
            ),
            6 => (
                |i, t, a, _| dv_get(i, t, a, 6),
                |i, t, a, _| dv_set(i, t, a, 6),
            ),
            7 => (
                |i, t, a, _| dv_get(i, t, a, 7),
                |i, t, a, _| dv_set(i, t, a, 7),
            ),
            8 => (
                |i, t, a, _| dv_get(i, t, a, 8),
                |i, t, a, _| dv_set(i, t, a, 8),
            ),
            _ => (
                |i, t, a, _| dv_get(i, t, a, 9),
                |i, t, a, _| dv_set(i, t, a, 9),
            ),
        };
        method(interp, &proto, &format!("get{name}"), 1, get);
        method(interp, &proto, &format!("set{name}"), 2, set);
    }
}

/// The bytes at `at` of a view, in little-endian order (read big-endian unless `little`).
fn dv_bytes(
    i: &mut Interp,
    this: &Value,
    at: usize,
    n: usize,
) -> Res<(Rc<RefCell<Vec<u8>>>, usize)> {
    let (buffer, offset, length, _) = view(i, this)?;
    if at + n > length {
        return Err(i.range_error("Offset is outside the bounds of the DataView"));
    }
    Ok((buffer, offset + at))
}

fn dv_get(i: &mut Interp, this: &Value, a: &[Value], kind: u8) -> Res<Value> {
    let size = KINDS[kind as usize].1;
    let at = int_arg(i, a, 0)?.max(0.0) as usize;
    let little = arg(a, 1).truthy();
    let (buffer, start) = dv_bytes(i, this, at, size)?;
    let mut raw: Vec<u8> = buffer.borrow()[start..start + size].to_vec();
    if !little {
        raw.reverse();
    }
    let one = Bytes {
        buffer: Rc::new(RefCell::new(raw)),
        offset: 0,
        length: 1,
        kind,
    };
    Ok(Value::Number(one.get(0).unwrap_or(f64::NAN)))
}

fn dv_set(i: &mut Interp, this: &Value, a: &[Value], kind: u8) -> Res<Value> {
    let size = KINDS[kind as usize].1;
    let at = int_arg(i, a, 0)?.max(0.0) as usize;
    let n = i.to_number(&arg(a, 1))?;
    let little = arg(a, 2).truthy();
    let (buffer, start) = dv_bytes(i, this, at, size)?;
    let one = Bytes {
        buffer: Rc::new(RefCell::new(vec![0; size])),
        offset: 0,
        length: 1,
        kind,
    };
    one.set(0, n);
    let mut raw = one.buffer.borrow().clone();
    if !little {
        raw.reverse();
    }
    buffer.borrow_mut()[start..start + size].copy_from_slice(&raw);
    Ok(Value::Undefined)
}
