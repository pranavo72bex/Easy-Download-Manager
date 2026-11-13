// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `String`: its methods work on UTF-16, as the language indexes strings.

use super::{arg, constructor, int_arg, method, symbol_method};
use crate::interp::{Interp, Res};
use crate::lexer::{text, Text};
use crate::object::{Class, IterState, Obj};
use crate::value::{relative_index, Key, Value};

/// Installs `String`.
pub fn install(interp: &mut Interp) {
    let sp = interp.intrinsics.string_proto.clone();
    let string = constructor(
        interp,
        "String",
        1,
        |i, _, a, made| {
            let s = match a.first() {
                None => text(""),
                Some(Value::Symbol(s)) if made.is_none() => {
                    let d = s
                        .description
                        .clone()
                        .map(|d| String::from_utf16_lossy(&d))
                        .unwrap_or_default();
                    text(&format!("Symbol({d})"))
                }
                Some(v) => i.to_text(v)?,
            };
            Ok(match made {
                Some(p) => Value::Object(Obj::new(Some(p.clone()), Class::String(s))),
                None => Value::String(s),
            })
        },
        &sp,
    );
    method(interp, &string, "fromCharCode", 1, |i, _, a, _| {
        let mut out = Vec::with_capacity(a.len());
        for v in a {
            out.push(crate::value::to_uint32(i.to_number(v)?) as u16);
        }
        Ok(Value::String(out.into()))
    });
    method(interp, &string, "fromCodePoint", 1, |i, _, a, _| {
        let mut out = Vec::new();
        for v in a {
            let n = i.to_number(v)?;
            let c = (n.fract() == 0.0 && (0.0..=1114111.0).contains(&n))
                .then(|| char::from_u32(n as u32))
                .flatten();
            match c {
                Some(c) => {
                    let mut buf = [0u16; 2];
                    out.extend_from_slice(c.encode_utf16(&mut buf));
                }
                None if (0xD800..0xE000).contains(&(n as u32)) => out.push(n as u16),
                None => return Err(i.range_error(&format!("Invalid code point {n}"))),
            }
        }
        Ok(Value::String(out.into()))
    });
    method(interp, &string, "raw", 1, |i, _, a, _| {
        let strings = arg(a, 0);
        let raw = i.get_of(&strings, &Key::str("raw"))?;
        let pieces = super::object::list_of(i, &raw)?;
        let mut out: Vec<u16> = Vec::new();
        for (n, p) in pieces.iter().enumerate() {
            out.extend_from_slice(&i.to_text(p)?);
            if n + 1 < pieces.len() {
                if let Some(v) = a.get(n + 1) {
                    out.extend_from_slice(&i.to_text(v)?);
                }
            }
        }
        Ok(Value::String(out.into()))
    });

    method(interp, &sp, "toString", 0, |i, this, _, _| {
        Ok(Value::String(this_text(i, this)?))
    });
    method(interp, &sp, "valueOf", 0, |i, this, _, _| {
        Ok(Value::String(this_text(i, this)?))
    });
    method(interp, &sp, "charAt", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let n = int_arg(i, a, 0)?;
        Ok(Value::String(if n >= 0.0 && (n as usize) < s.len() {
            vec![s[n as usize]].into()
        } else {
            text("")
        }))
    });
    method(interp, &sp, "charCodeAt", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let n = int_arg(i, a, 0)?;
        Ok(Value::Number(if n >= 0.0 && (n as usize) < s.len() {
            s[n as usize] as f64
        } else {
            f64::NAN
        }))
    });
    method(interp, &sp, "codePointAt", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let n = int_arg(i, a, 0)?;
        if n < 0.0 || n as usize >= s.len() {
            return Ok(Value::Undefined);
        }
        let n = n as usize;
        let c = s[n];
        if (0xD800..0xDC00).contains(&c) {
            if let Some(&d) = s.get(n + 1) {
                if (0xDC00..0xE000).contains(&d) {
                    let cp = 0x10000 + ((c as u32 - 0xD800) << 10) + (d as u32 - 0xDC00);
                    return Ok(Value::Number(cp as f64));
                }
            }
        }
        Ok(Value::Number(c as f64))
    });
    method(interp, &sp, "at", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let n = int_arg(i, a, 0)?;
        let at = if n < 0.0 { s.len() as f64 + n } else { n };
        Ok(if at >= 0.0 && (at as usize) < s.len() {
            Value::String(vec![s[at as usize]].into())
        } else {
            Value::Undefined
        })
    });
    method(interp, &sp, "indexOf", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let wanted = i.to_text(&arg(a, 0))?;
        let from = (int_arg(i, a, 1)?.max(0.0) as usize).min(s.len());
        Ok(Value::Number(
            find(&s, &wanted, from).map(|p| p as f64).unwrap_or(-1.0),
        ))
    });
    method(interp, &sp, "lastIndexOf", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let wanted = i.to_text(&arg(a, 0))?;
        let from = match arg(a, 1) {
            Value::Undefined => s.len(),
            v => {
                let n = i.to_number(&v)?;
                if n.is_nan() {
                    s.len()
                } else {
                    n.max(0.0) as usize
                }
            }
        };
        let last = s.len().saturating_sub(wanted.len()).min(from);
        for p in (0..=last).rev() {
            if s[p..].starts_with(&wanted) {
                return Ok(Value::Number(p as f64));
            }
        }
        Ok(Value::Number(-1.0))
    });
    method(interp, &sp, "includes", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        not_pattern(i, &arg(a, 0), "includes")?;
        let wanted = i.to_text(&arg(a, 0))?;
        let from = (int_arg(i, a, 1)?.max(0.0) as usize).min(s.len());
        Ok(Value::Bool(find(&s, &wanted, from).is_some()))
    });
    method(interp, &sp, "startsWith", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        not_pattern(i, &arg(a, 0), "startsWith")?;
        let wanted = i.to_text(&arg(a, 0))?;
        let from = (int_arg(i, a, 1)?.max(0.0) as usize).min(s.len());
        Ok(Value::Bool(s[from..].starts_with(&wanted)))
    });
    method(interp, &sp, "endsWith", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        not_pattern(i, &arg(a, 0), "endsWith")?;
        let wanted = i.to_text(&arg(a, 0))?;
        let end = match arg(a, 1) {
            Value::Undefined => s.len(),
            v => (crate::value::to_integer(i.to_number(&v)?).max(0.0) as usize).min(s.len()),
        };
        Ok(Value::Bool(s[..end].ends_with(&wanted)))
    });
    method(interp, &sp, "slice", 2, |i, this, a, _| {
        let s = this_str(i, this)?;
        let start = relative_index(i.to_number(&arg(a, 0))?, s.len());
        let end = match arg(a, 1) {
            Value::Undefined => s.len(),
            v => relative_index(i.to_number(&v)?, s.len()),
        };
        Ok(Value::String(
            s.get(start..end.max(start)).unwrap_or(&[]).to_vec().into(),
        ))
    });
    method(interp, &sp, "substring", 2, |i, this, a, _| {
        let s = this_str(i, this)?;
        let clamp = |n: f64| (crate::value::to_integer(n).max(0.0) as usize).min(s.len());
        let start = clamp(i.to_number(&arg(a, 0))?);
        let end = match arg(a, 1) {
            Value::Undefined => s.len(),
            v => clamp(i.to_number(&v)?),
        };
        let (lo, hi) = (start.min(end), start.max(end));
        Ok(Value::String(s[lo..hi].to_vec().into()))
    });
    method(interp, &sp, "substr", 2, |i, this, a, _| {
        let s = this_str(i, this)?;
        let start = relative_index(i.to_number(&arg(a, 0))?, s.len());
        let count = match arg(a, 1) {
            Value::Undefined => s.len() - start,
            v => {
                (crate::value::to_integer(i.to_number(&v)?).max(0.0) as usize).min(s.len() - start)
            }
        };
        Ok(Value::String(s[start..start + count].to_vec().into()))
    });
    method(interp, &sp, "toUpperCase", 0, |i, this, _, _| {
        map_case(i, this, true)
    });
    method(interp, &sp, "toLowerCase", 0, |i, this, _, _| {
        map_case(i, this, false)
    });
    method(interp, &sp, "toLocaleUpperCase", 0, |i, this, _, _| {
        map_case(i, this, true)
    });
    method(interp, &sp, "toLocaleLowerCase", 0, |i, this, _, _| {
        map_case(i, this, false)
    });
    method(interp, &sp, "trim", 0, |i, this, _, _| {
        trim(i, this, true, true)
    });
    method(interp, &sp, "trimStart", 0, |i, this, _, _| {
        trim(i, this, true, false)
    });
    method(interp, &sp, "trimEnd", 0, |i, this, _, _| {
        trim(i, this, false, true)
    });
    method(interp, &sp, "trimLeft", 0, |i, this, _, _| {
        trim(i, this, true, false)
    });
    method(interp, &sp, "trimRight", 0, |i, this, _, _| {
        trim(i, this, false, true)
    });
    method(interp, &sp, "padStart", 2, |i, this, a, _| {
        pad(i, this, a, true)
    });
    method(interp, &sp, "padEnd", 2, |i, this, a, _| {
        pad(i, this, a, false)
    });
    method(interp, &sp, "repeat", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let n = int_arg(i, a, 0)?;
        if n < 0.0 || n.is_infinite() || n * s.len() as f64 > 1e9 {
            return Err(i.range_error("Invalid count value"));
        }
        Ok(Value::String(s.repeat(n as usize).into()))
    });
    method(interp, &sp, "concat", 1, |i, this, a, _| {
        let mut out = this_str(i, this)?.to_vec();
        for v in a {
            out.extend_from_slice(&i.to_text(v)?);
        }
        Ok(Value::String(out.into()))
    });
    method(interp, &sp, "localeCompare", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let other = i.to_text(&arg(a, 0))?;
        Ok(Value::Number(match s[..].cmp(&other[..]) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    });
    method(interp, &sp, "normalize", 0, |i, this, _, _| {
        Ok(Value::String(this_str(i, this)?))
    });
    method(interp, &sp, "isWellFormed", 0, |i, this, _, _| {
        let s = this_str(i, this)?;
        Ok(Value::Bool(
            char::decode_utf16(s.iter().copied()).all(|c| c.is_ok()),
        ))
    });
    method(interp, &sp, "split", 2, |i, this, a, _| {
        let sep = arg(a, 0);
        if super::regexp::is_pattern(&sep) {
            let s = this_str(i, this)?;
            return super::regexp::split(i, &sep, &s, arg(a, 1));
        }
        let s = this_str(i, this)?;
        let limit = match arg(a, 1) {
            Value::Undefined => u32::MAX as usize,
            v => crate::value::to_uint32(i.to_number(&v)?) as usize,
        };
        if matches!(sep, Value::Undefined) {
            return Ok(i.array(vec![Value::String(s)].into_iter().take(limit).collect()));
        }
        let sep = i.to_text(&sep)?;
        let mut out = Vec::new();
        if sep.is_empty() {
            for c in s.iter().take(limit) {
                out.push(Value::String(vec![*c].into()));
            }
            return Ok(i.array(out));
        }
        let mut start = 0;
        while let Some(p) = find(&s, &sep, start) {
            if out.len() >= limit {
                break;
            }
            out.push(Value::String(s[start..p].to_vec().into()));
            start = p + sep.len();
        }
        if out.len() < limit {
            out.push(Value::String(s[start..].to_vec().into()));
        }
        Ok(i.array(out))
    });
    method(interp, &sp, "replace", 2, |i, this, a, _| {
        let s = this_str(i, this)?;
        if super::regexp::is_pattern(&arg(a, 0)) {
            return super::regexp::replace(i, &arg(a, 0), &s, arg(a, 1));
        }
        let wanted = i.to_text(&arg(a, 0))?;
        replace_plain(i, &s, &wanted, &arg(a, 1), false)
    });
    method(interp, &sp, "replaceAll", 2, |i, this, a, _| {
        let s = this_str(i, this)?;
        if super::regexp::is_pattern(&arg(a, 0)) {
            return super::regexp::replace(i, &arg(a, 0), &s, arg(a, 1));
        }
        let wanted = i.to_text(&arg(a, 0))?;
        replace_plain(i, &s, &wanted, &arg(a, 1), true)
    });
    method(interp, &sp, "match", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let pattern = super::regexp::coerce(i, &arg(a, 0))?;
        super::regexp::match_all_or_first(i, &pattern, &s)
    });
    method(interp, &sp, "matchAll", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let pattern = super::regexp::coerce(i, &arg(a, 0))?;
        let Value::Object(p) = pattern else {
            return Err(i.type_error("not a pattern"));
        };
        // A copy of the pattern walks the string, so the one given keeps its place.
        let copy = super::regexp::copy(i, &p)?;
        Ok(Value::Object(Obj::new(
            Some(i.intrinsics.array_iterator_proto.clone()),
            Class::Iterator(IterState::Matches {
                pattern: copy,
                of: s,
                done: false,
            }),
        )))
    });
    method(interp, &sp, "search", 1, |i, this, a, _| {
        let s = this_str(i, this)?;
        let pattern = super::regexp::coerce(i, &arg(a, 0))?;
        let Value::Object(p) = pattern else {
            return Ok(Value::Number(-1.0));
        };
        let copy = super::regexp::copy(i, &p)?;
        i.set(
            &Value::Object(copy.clone()),
            Key::str("lastIndex"),
            Value::Number(0.0),
        )?;
        let found = super::regexp::exec(i, &copy, &s)?;
        if matches!(found, Value::Null) {
            return Ok(Value::Number(-1.0));
        }
        i.get(&found, &Key::str("index"))
    });
    let iter_key = Key::Symbol(interp.intrinsics.sym_iterator.clone());
    let f = symbol_method(
        interp,
        &sp,
        &iter_key,
        "[Symbol.iterator]",
        |i, this, _, _| {
            let s = this_str(i, this)?;
            Ok(Value::Object(Obj::new(
                Some(i.intrinsics.array_iterator_proto.clone()),
                Class::Iterator(IterState::String { of: s, at: 0 }),
            )))
        },
    );
    interp.intrinsics.string_iterator = Value::Object(f);
}

/// `this` as a string, for a string method (any value but null or undefined is one).
fn this_str(i: &mut Interp, this: &Value) -> Res<Text> {
    match this {
        Value::String(s) => Ok(s.clone()),
        Value::Undefined | Value::Null => {
            Err(i.type_error("String.prototype method called on null or undefined"))
        }
        other => i.to_text(other),
    }
}

/// `this` as `String.prototype.toString` takes it: a string or a `String` object only.
fn this_text(i: &mut Interp, this: &Value) -> Res<Text> {
    match this {
        Value::String(s) => Ok(s.clone()),
        Value::Object(o) => match &o.borrow().class {
            Class::String(s) => Ok(s.clone()),
            _ => Err(i.type_error("String.prototype.toString requires that 'this' be a String")),
        },
        _ => Err(i.type_error("String.prototype.toString requires that 'this' be a String")),
    }
}

fn not_pattern(i: &mut Interp, v: &Value, name: &str) -> Res<()> {
    if super::regexp::is_pattern(v) {
        return Err(i.type_error(&format!(
            "First argument to String.prototype.{name} must not be a regular expression"
        )));
    }
    Ok(())
}

/// Where `wanted` is in `s`, from `from`.
pub fn find(s: &[u16], wanted: &[u16], from: usize) -> Option<usize> {
    if wanted.is_empty() {
        return (from <= s.len()).then_some(from);
    }
    if wanted.len() > s.len() {
        return None;
    }
    (from..=s.len() - wanted.len()).find(|&p| s[p..p + wanted.len()] == *wanted)
}

fn map_case(i: &mut Interp, this: &Value, upper: bool) -> Res<Value> {
    let s = this_str(i, this)?;
    let rust = String::from_utf16_lossy(&s);
    let mapped = if upper {
        rust.to_uppercase()
    } else {
        rust.to_lowercase()
    };
    Ok(Value::str(&mapped))
}

fn is_space(c: u16) -> bool {
    matches!(c, 9..=13 | 32 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
}

fn trim(i: &mut Interp, this: &Value, start: bool, end: bool) -> Res<Value> {
    let s = this_str(i, this)?;
    let mut lo = 0;
    let mut hi = s.len();
    if start {
        while lo < hi && is_space(s[lo]) {
            lo += 1;
        }
    }
    if end {
        while hi > lo && is_space(s[hi - 1]) {
            hi -= 1;
        }
    }
    Ok(Value::String(s[lo..hi].to_vec().into()))
}

fn pad(i: &mut Interp, this: &Value, a: &[Value], at_start: bool) -> Res<Value> {
    let s = this_str(i, this)?;
    let target = int_arg(i, a, 0)?.max(0.0) as usize;
    let filler = match arg(a, 1) {
        Value::Undefined => text(" "),
        v => i.to_text(&v)?,
    };
    if target <= s.len() || filler.is_empty() {
        return Ok(Value::String(s));
    }
    let need = target - s.len();
    let fill: Vec<u16> = filler.iter().copied().cycle().take(need).collect();
    let mut out = Vec::with_capacity(target);
    if at_start {
        out.extend(fill);
        out.extend_from_slice(&s);
    } else {
        out.extend_from_slice(&s);
        out.extend(fill);
    }
    Ok(Value::String(out.into()))
}

/// `s.replace(wanted, by)` (each one where `all`) with a string `wanted`.
fn replace_plain(i: &mut Interp, s: &Text, wanted: &Text, by: &Value, all: bool) -> Res<Value> {
    let mut out: Vec<u16> = Vec::new();
    let mut start = 0;
    let mut at = find(s, wanted, 0);
    while let Some(p) = at {
        out.extend_from_slice(&s[start..p]);
        let piece = if by.as_object().is_some_and(|f| f.is_callable()) {
            let r = i.call(
                by,
                Value::Undefined,
                &[
                    Value::String(wanted.clone()),
                    Value::Number(p as f64),
                    Value::String(s.clone()),
                ],
            )?;
            i.to_text(&r)?.to_vec()
        } else {
            let template = i.to_text(by)?;
            super::regexp::expand(&template, s, p, &[Some((p, p + wanted.len()))], &[])
        };
        out.extend_from_slice(&piece);
        start = p + wanted.len();
        if !all {
            break;
        }
        at = if wanted.is_empty() {
            if start >= s.len() {
                None
            } else {
                out.push(s[start]);
                start += 1;
                find(s, wanted, start)
            }
        } else {
            find(s, wanted, start)
        };
    }
    out.extend_from_slice(&s[start.min(s.len())..]);
    Ok(Value::String(out.into()))
}
