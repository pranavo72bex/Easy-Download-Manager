// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The language's built-in objects, each family in a file of its own, and the global object's
//! functions (`parseInt`, `encodeURIComponent`, …) here.

pub mod array;
pub mod collections;
pub mod date;
pub mod generator;
pub mod iterator;
pub mod json;
pub mod number;
pub mod object;
pub mod promise;
pub mod proxy;
pub mod regexp;
pub mod string;
pub mod typed;

use crate::interp::{Interp, Res};
use crate::lexer::{text, Text};
use crate::object::{NativeFn, Obj, Prop, Slot};
use crate::value::{Key, Value};

/// Argument `i`, or undefined.
pub fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Undefined)
}

/// A method on `on`, not listed by `for … in`.
pub fn method(interp: &Interp, on: &Obj, name: &str, length: usize, run: NativeFn) -> Obj {
    let f = interp.native(name, length, run, false);
    on.set_hidden(Key::str(name), Value::Object(f.clone()));
    f
}

/// A method keyed by a symbol.
pub fn symbol_method(interp: &Interp, on: &Obj, key: &Key, name: &str, run: NativeFn) -> Obj {
    let f = interp.native(name, 0, run, false);
    on.set_hidden(key.clone(), Value::Object(f.clone()));
    f
}

/// A getter on `on`.
pub fn getter(interp: &Interp, on: &Obj, key: Key, name: &str, run: NativeFn) {
    let f = interp.native(&format!("get {name}"), 0, run, false);
    on.define(
        key,
        Prop {
            slot: Slot::Accessor(Some(f), None),
            enumerable: false,
            configurable: true,
        },
    );
}

/// A constructor named `name` on the global object, with `proto` as its `prototype`.
pub fn constructor(interp: &Interp, name: &str, length: usize, run: NativeFn, proto: &Obj) -> Obj {
    let f = interp.native(name, length, run, true);
    f.define(
        Key::str("prototype"),
        Prop::data(Value::Object(proto.clone()), false, false, false),
    );
    proto.set_hidden(Key::str("constructor"), Value::Object(f.clone()));
    interp
        .global
        .set_hidden(Key::str(name), Value::Object(f.clone()));
    f
}

/// A value on the global object.
pub fn global(interp: &Interp, name: &str, value: Value) {
    interp.global.set_hidden(Key::str(name), value);
}

/// What a built-in function carries (a promise's resolve function, its promise).
pub fn own_data(interp: &Interp) -> Value {
    interp
        .frames
        .last()
        .map(|f| f.new_target.clone())
        .unwrap_or(Value::Undefined)
}

/// A built-in function that carries `data`.
pub fn with_data(interp: &Interp, name: &str, length: usize, run: NativeFn, data: Value) -> Obj {
    let f = interp.native(name, length, run, false);
    if let crate::object::Class::Function(crate::object::Func::Native(n)) =
        &mut f.borrow_mut().class
    {
        n.data = Some(data);
    }
    f
}

/// A number argument as an integer (`ToIntegerOrInfinity`).
pub fn int_arg(interp: &mut Interp, args: &[Value], i: usize) -> Res<f64> {
    let n = interp.to_number(&arg(args, i))?;
    Ok(crate::value::to_integer(n))
}

/// Every built-in object, installed.
pub fn fill(interp: &mut Interp) {
    object::install(interp);
    array::install(interp);
    string::install(interp);
    number::install(interp);
    iterator::install(interp);
    collections::install(interp);
    promise::install(interp);
    regexp::install(interp);
    json::install(interp);
    proxy::install(interp);
    generator::install(interp);
    typed::install(interp);
    date::install(interp);
    let g = Value::Object(interp.global.clone());
    global(interp, "globalThis", g);
    global(interp, "NaN", Value::Number(f64::NAN));
    global(interp, "Infinity", Value::Number(f64::INFINITY));
    interp.global.define(
        Key::str("undefined"),
        Prop::data(Value::Undefined, false, false, false),
    );
    let on = interp.global.clone();
    method(interp, &on, "parseInt", 2, parse_int);
    method(interp, &on, "parseFloat", 1, parse_float);
    method(interp, &on, "isNaN", 1, |i, _, a, _| {
        Ok(Value::Bool(i.to_number(&arg(a, 0))?.is_nan()))
    });
    method(interp, &on, "isFinite", 1, |i, _, a, _| {
        Ok(Value::Bool(i.to_number(&arg(a, 0))?.is_finite()))
    });
    method(interp, &on, "encodeURIComponent", 1, |i, _, a, _| {
        encode(i, a, "-_.!~*'()")
    });
    method(interp, &on, "encodeURI", 1, |i, _, a, _| {
        encode(i, a, "-_.!~*'();/?:@&=+$,#")
    });
    method(interp, &on, "decodeURIComponent", 1, |i, _, a, _| {
        decode(i, a, "")
    });
    method(interp, &on, "decodeURI", 1, |i, _, a, _| {
        decode(i, a, ";/?:@&=+$,#")
    });
    method(interp, &on, "escape", 1, escape);
    method(interp, &on, "unescape", 1, unescape);
}

fn parse_int(interp: &mut Interp, _: &Value, args: &[Value], _: Option<&Obj>) -> Res<Value> {
    let s = interp.to_rust_string(&arg(args, 0))?;
    let mut radix = crate::value::to_int32(interp.to_number(&arg(args, 1))?) as u32;
    let s = s.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (negative, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let mut digits = s;
    if radix == 0 || radix == 16 {
        if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            digits = rest;
            radix = 16;
        }
    }
    if radix == 0 {
        radix = 10;
    }
    if !(2..=36).contains(&radix) {
        return Ok(Value::Number(f64::NAN));
    }
    let mut n = 0f64;
    let mut any = false;
    for c in digits.chars() {
        match c.to_digit(radix) {
            Some(d) => {
                n = n * radix as f64 + d as f64;
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return Ok(Value::Number(f64::NAN));
    }
    Ok(Value::Number(if negative { -n } else { n }))
}

fn parse_float(interp: &mut Interp, _: &Value, args: &[Value], _: Option<&Obj>) -> Res<Value> {
    let s = interp.to_rust_string(&arg(args, 0))?;
    let s = s.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    for (word, n) in [
        ("Infinity", f64::INFINITY),
        ("+Infinity", f64::INFINITY),
        ("-Infinity", f64::NEG_INFINITY),
    ] {
        if s.starts_with(word) {
            return Ok(Value::Number(n));
        }
    }
    // The longest start that reads as a number.
    let bytes = s.as_bytes();
    let mut end = 0;
    let mut i = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let mut seen_digit = false;
    let mut seen_point = false;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                seen_digit = true;
                end = i + 1;
            }
            b'.' if !seen_point => seen_point = true,
            b'e' | b'E' if seen_digit => {
                let mut j = i + 1;
                if matches!(bytes.get(j), Some(b'+' | b'-')) {
                    j += 1;
                }
                if bytes.get(j).is_some_and(u8::is_ascii_digit) {
                    while bytes.get(j).is_some_and(u8::is_ascii_digit) {
                        j += 1;
                    }
                    end = j;
                }
                break;
            }
            _ => break,
        }
        i += 1;
    }
    if !seen_digit {
        return Ok(Value::Number(f64::NAN));
    }
    Ok(Value::Number(s[..end].parse::<f64>().unwrap_or(f64::NAN)))
}

/// `encodeURIComponent`: UTF-8, each byte not kept as `%XX`.
fn encode(interp: &mut Interp, args: &[Value], kept: &str) -> Res<Value> {
    let s = interp.to_text(&arg(args, 0))?;
    let mut out = String::new();
    for c in char::decode_utf16(s.iter().copied()) {
        let Ok(c) = c else {
            return Err(interp.error_of(&interp.intrinsics.error_proto.clone(), "URI malformed"));
        };
        if c.is_ascii_alphanumeric() || kept.contains(c) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    Ok(Value::str(&out))
}

/// `decodeURIComponent`: `%XX` as UTF-8, but those that stand for a character of `reserved`.
fn decode(interp: &mut Interp, args: &[Value], reserved: &str) -> Res<Value> {
    let s = interp.to_rust_string(&arg(args, 0))?;
    let malformed = |i: &Interp| i.error_of(&i.intrinsics.error_proto, "URI malformed");
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or_else(|| malformed(interp))?;
            let b = u8::from_str_radix(hex, 16).map_err(|_| malformed(interp))?;
            if b < 0x80 && reserved.contains(b as char) {
                out.extend_from_slice(&bytes[i..i + 3]);
            } else {
                out.push(b);
            }
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).map_err(|_| malformed(interp))?;
    Ok(Value::str(&decoded))
}

fn escape(interp: &mut Interp, _: &Value, args: &[Value], _: Option<&Obj>) -> Res<Value> {
    let s = interp.to_text(&arg(args, 0))?;
    let mut out = String::new();
    for &u in s.iter() {
        let c = char::from_u32(u as u32).unwrap_or('\0');
        if u < 128 && (c.is_ascii_alphanumeric() || "@*_+-./".contains(c)) {
            out.push(c);
        } else if u < 256 {
            out.push_str(&format!("%{u:02X}"));
        } else {
            out.push_str(&format!("%u{u:04X}"));
        }
    }
    Ok(Value::str(&out))
}

fn unescape(interp: &mut Interp, _: &Value, args: &[Value], _: Option<&Obj>) -> Res<Value> {
    let s = interp.to_text(&arg(args, 0))?;
    let mut out: Vec<u16> = Vec::with_capacity(s.len());
    let mut i = 0;
    let hex = |units: &[u16]| -> Option<u16> {
        u16::from_str_radix(&String::from_utf16_lossy(units), 16).ok()
    };
    while i < s.len() {
        if s[i] == b'%' as u16 {
            if s.get(i + 1) == Some(&(b'u' as u16)) {
                if let Some(v) = s.get(i + 2..i + 6).and_then(hex) {
                    out.push(v);
                    i += 6;
                    continue;
                }
            } else if let Some(v) = s.get(i + 1..i + 3).and_then(hex) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    Ok(Value::String(out.into()))
}

/// The UTF-16 of `s`.
pub fn t(s: &str) -> Text {
    text(s)
}
