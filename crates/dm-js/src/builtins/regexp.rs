// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `RegExp`, and the pattern side of `split`, `replace`, `match` and `search`
//! (`crate::regexp` matches).

use std::rc::Rc;

use super::{arg, constructor, getter, method};
use crate::interp::{Interp, Res};
use crate::lexer::{text, Text};
use crate::object::{Class, Obj, Prop};
use crate::regexp::Regex;
use crate::value::{Key, Value};

/// Installs `RegExp`.
pub fn install(interp: &mut Interp) {
    let rp = interp.intrinsics.regexp_proto.clone();
    constructor(
        interp,
        "RegExp",
        2,
        |i, _, a, made| {
            let (source, flags) = match arg(a, 0) {
                Value::Object(o) if matches!(o.borrow().class, Class::RegExp(_)) => {
                    let Class::RegExp(r) = &o.borrow().class else {
                        unreachable!()
                    };
                    let flags = match arg(a, 1) {
                        Value::Undefined => r.flags.clone(),
                        f => i.to_rust_string(&f)?,
                    };
                    (String::from_utf16_lossy(&r.source), flags)
                }
                Value::Undefined => ("(?:)".to_string(), String::new()),
                v => {
                    let s = i.to_rust_string(&v)?;
                    let flags = match arg(a, 1) {
                        Value::Undefined => String::new(),
                        f => i.to_rust_string(&f)?,
                    };
                    (if s.is_empty() { "(?:)".into() } else { s }, flags)
                }
            };
            let v = make(i, &source, &flags)?;
            // A subclass's `new` gives the object its own prototype.
            if let (Some(proto), Value::Object(o)) = (made, &v) {
                o.borrow_mut().proto = Some(proto.clone());
            }
            Ok(v)
        },
        &rp,
    );
    method(interp, &rp, "exec", 1, |i, this, a, _| {
        let p = this_pattern(i, this)?;
        let s = i.to_text(&arg(a, 0))?;
        exec(i, &p, &s)
    });
    method(interp, &rp, "test", 1, |i, this, a, _| {
        let p = this_pattern(i, this)?;
        let s = i.to_text(&arg(a, 0))?;
        Ok(Value::Bool(!matches!(exec(i, &p, &s)?, Value::Null)))
    });
    method(interp, &rp, "toString", 0, |i, this, _, _| {
        let source = i.get_of(this, &Key::str("source"))?;
        let flags = i.get_of(this, &Key::str("flags"))?;
        let (s, f) = (i.to_rust_string(&source)?, i.to_rust_string(&flags)?);
        Ok(Value::str(&format!("/{s}/{f}")))
    });
    method(interp, &rp, "compile", 2, |_, this, _, _| Ok(this.clone()));
    getter(
        interp,
        &rp,
        Key::str("source"),
        "source",
        |i, this, _, _| {
            if let Value::Object(o) = this {
                if o.same(&i.intrinsics.regexp_proto) {
                    return Ok(Value::str("(?:)"));
                }
            }
            let p = this_pattern(i, this)?;
            let Class::RegExp(r) = &p.borrow().class else {
                return Ok(Value::Undefined);
            };
            Ok(Value::String(r.source.clone()))
        },
    );
    getter(interp, &rp, Key::str("flags"), "flags", |i, this, _, _| {
        if let Value::Object(o) = this {
            if o.same(&i.intrinsics.regexp_proto) {
                return Ok(Value::str(""));
            }
        }
        let p = this_pattern(i, this)?;
        let Class::RegExp(r) = &p.borrow().class else {
            return Ok(Value::Undefined);
        };
        let mut f: Vec<char> = r.flags.chars().collect();
        f.sort_by_key(|c| "dgimsuvy".find(*c));
        Ok(Value::str(&f.into_iter().collect::<String>()))
    });
    for (name, flag) in [
        ("global", 'g'),
        ("ignoreCase", 'i'),
        ("multiline", 'm'),
        ("dotAll", 's'),
        ("unicode", 'u'),
        ("sticky", 'y'),
        ("hasIndices", 'd'),
    ] {
        let run: crate::object::NativeFn = match flag {
            'g' => |i, t, _, _| flag_of(i, t, 'g'),
            'i' => |i, t, _, _| flag_of(i, t, 'i'),
            'm' => |i, t, _, _| flag_of(i, t, 'm'),
            's' => |i, t, _, _| flag_of(i, t, 's'),
            'u' => |i, t, _, _| flag_of(i, t, 'u'),
            'y' => |i, t, _, _| flag_of(i, t, 'y'),
            _ => |i, t, _, _| flag_of(i, t, 'd'),
        };
        getter(interp, &rp, Key::str(name), name, run);
    }
}

fn flag_of(i: &mut Interp, this: &Value, flag: char) -> Res<Value> {
    if let Value::Object(o) = this {
        if o.same(&i.intrinsics.regexp_proto) {
            return Ok(Value::Undefined);
        }
    }
    let p = this_pattern(i, this)?;
    let Class::RegExp(r) = &p.borrow().class else {
        return Ok(Value::Undefined);
    };
    Ok(Value::Bool(r.flags.contains(flag)))
}

fn this_pattern(i: &mut Interp, this: &Value) -> Res<Obj> {
    match this {
        Value::Object(o) if matches!(o.borrow().class, Class::RegExp(_)) => Ok(o.clone()),
        _ => Err(i.type_error("not a RegExp")),
    }
}

/// A pattern object of `source` and `flags`.
pub fn make(i: &mut Interp, source: &str, flags: &str) -> Res<Value> {
    let r = Regex::new(source, flags).map_err(|e| {
        i.syntax_error(&format!(
            "Invalid regular expression: /{source}/{flags}: {e}"
        ))
    })?;
    let o = Obj::new(
        Some(i.intrinsics.regexp_proto.clone()),
        Class::RegExp(Rc::new(r)),
    );
    o.define(
        Key::str("lastIndex"),
        Prop::data(Value::Number(0.0), true, false, false),
    );
    Ok(Value::Object(o))
}

/// Whether `v` is a pattern object.
pub fn is_pattern(v: &Value) -> bool {
    matches!(v, Value::Object(o) if matches!(o.borrow().class, Class::RegExp(_)))
}

/// `v` as a pattern (a string becomes one, as `match` takes it).
pub fn coerce(i: &mut Interp, v: &Value) -> Res<Value> {
    if is_pattern(v) {
        return Ok(v.clone());
    }
    let s = match v {
        Value::Undefined => String::new(),
        other => i.to_rust_string(other)?,
    };
    make(i, &s, "")
}

/// A copy of the pattern `p`, at its own `lastIndex`.
pub fn copy(i: &mut Interp, p: &Obj) -> Res<Obj> {
    let r = match &p.borrow().class {
        Class::RegExp(r) => r.clone(),
        _ => return Err(i.type_error("not a RegExp")),
    };
    let last = i.get(&Value::Object(p.clone()), &Key::str("lastIndex"))?;
    let o = Obj::new(Some(i.intrinsics.regexp_proto.clone()), Class::RegExp(r));
    o.define(Key::str("lastIndex"), Prop::data(last, true, false, false));
    Ok(o)
}

/// The pattern's match in `s`, as `exec` gives it (an array with `index`, `input` and
/// `groups`), or null; `lastIndex` moved as the flags say.
pub fn exec(i: &mut Interp, p: &Obj, s: &Text) -> Res<Value> {
    let r = match &p.borrow().class {
        Class::RegExp(r) => r.clone(),
        _ => return Err(i.type_error("not a RegExp")),
    };
    let pv = Value::Object(p.clone());
    let global = r.flags.contains('g');
    let sticky = r.flags.contains('y');
    let start = if global || sticky {
        let l = i.get(&pv, &Key::str("lastIndex"))?;
        crate::value::to_integer(i.to_number(&l)?).max(0.0) as usize
    } else {
        0
    };
    if start > s.len() {
        i.set(&pv, Key::str("lastIndex"), Value::Number(0.0))?;
        return Ok(Value::Null);
    }
    let found = r.exec_at(s, start, sticky).map_err(|e| i.range_error(&e))?;
    let Some(groups) = found else {
        if global || sticky {
            i.set(&pv, Key::str("lastIndex"), Value::Number(0.0))?;
        }
        return Ok(Value::Null);
    };
    let (m_start, m_end) = groups[0].unwrap_or((start, start));
    if global || sticky {
        i.set(&pv, Key::str("lastIndex"), Value::Number(m_end as f64))?;
    }
    let items: Vec<Value> = groups
        .iter()
        .map(|g| {
            g.map(|(a, b)| Value::String(s[a..b].to_vec().into()))
                .unwrap_or(Value::Undefined)
        })
        .collect();
    let arr = i.array(items.clone());
    let Value::Object(ao) = &arr else {
        return Ok(Value::Null);
    };
    ao.set_own(Key::str("index"), Value::Number(m_start as f64));
    ao.set_own(Key::str("input"), Value::String(s.clone()));
    let named = if r.names.is_empty() {
        Value::Undefined
    } else {
        let g = Obj::new(None, Class::Ordinary);
        for (name, n) in &r.names {
            g.set_own(
                Key::str(name),
                items.get(*n).cloned().unwrap_or(Value::Undefined),
            );
        }
        Value::Object(g)
    };
    ao.set_own(Key::str("groups"), named);
    Ok(arr)
}

/// Every match where the pattern is global (an array of strings, or null), else `exec`'s.
pub fn match_all_or_first(i: &mut Interp, pattern: &Value, s: &Text) -> Res<Value> {
    let Value::Object(p) = pattern else {
        return Ok(Value::Null);
    };
    let global = matches!(&p.borrow().class, Class::RegExp(r) if r.flags.contains('g'));
    if !global {
        return exec(i, p, s);
    }
    i.set(pattern, Key::str("lastIndex"), Value::Number(0.0))?;
    let mut out = Vec::new();
    loop {
        let m = exec(i, p, s)?;
        if matches!(m, Value::Null) {
            break;
        }
        let whole = i.get(&m, &Key::index(0))?;
        if matches!(&whole, Value::String(w) if w.is_empty()) {
            let l = i.get(pattern, &Key::str("lastIndex"))?;
            let l = i.to_number(&l)?;
            i.set(pattern, Key::str("lastIndex"), Value::Number(l + 1.0))?;
        }
        out.push(whole);
    }
    Ok(if out.is_empty() {
        Value::Null
    } else {
        i.array(out)
    })
}

/// `s.split(pattern, limit)`.
pub fn split(i: &mut Interp, pattern: &Value, s: &Text, limit: Value) -> Res<Value> {
    let Value::Object(p) = pattern else {
        return Ok(i.array(vec![]));
    };
    let r = match &p.borrow().class {
        Class::RegExp(r) => r.clone(),
        _ => return Ok(i.array(vec![])),
    };
    let limit = match limit {
        Value::Undefined => u32::MAX as usize,
        v => crate::value::to_uint32(i.to_number(&v)?) as usize,
    };
    let mut out = Vec::new();
    if limit == 0 {
        return Ok(i.array(out));
    }
    if s.is_empty() {
        let m = r.exec_at(s, 0, true).map_err(|e| i.range_error(&e))?;
        if m.is_none() {
            out.push(Value::String(s.clone()));
        }
        return Ok(i.array(out));
    }
    let mut last = 0;
    let mut q = 0;
    while q < s.len() {
        let m = r.exec_at(s, q, true).map_err(|e| i.range_error(&e))?;
        let Some(groups) = m else {
            q += 1;
            continue;
        };
        let (_, end) = groups[0].unwrap_or((q, q));
        if end == last || end > s.len() {
            q += 1;
            continue;
        }
        out.push(Value::String(s[last..q].to_vec().into()));
        if out.len() >= limit {
            return Ok(i.array(out));
        }
        for g in groups.iter().skip(1) {
            out.push(
                g.map(|(a, b)| Value::String(s[a..b].to_vec().into()))
                    .unwrap_or(Value::Undefined),
            );
            if out.len() >= limit {
                return Ok(i.array(out));
            }
        }
        last = end;
        q = if end == q { q + 1 } else { end };
    }
    out.push(Value::String(s[last..].to_vec().into()));
    Ok(i.array(out))
}

/// `s.replace(pattern, by)`: every match where the pattern is global, else the first.
pub fn replace(i: &mut Interp, pattern: &Value, s: &Text, by: Value) -> Res<Value> {
    let Value::Object(p) = pattern else {
        return Ok(Value::String(s.clone()));
    };
    let r = match &p.borrow().class {
        Class::RegExp(r) => r.clone(),
        _ => return Ok(Value::String(s.clone())),
    };
    let global = r.flags.contains('g');
    let sticky = r.flags.contains('y');
    let callable = by.as_object().is_some_and(|f| f.is_callable());
    let template = if callable { text("") } else { i.to_text(&by)? };
    let mut out: Vec<u16> = Vec::new();
    let mut last = 0;
    let mut at = if global {
        0
    } else if sticky {
        let l = i.get(pattern, &Key::str("lastIndex"))?;
        i.to_number(&l)?.max(0.0) as usize
    } else {
        0
    };
    loop {
        if at > s.len() {
            break;
        }
        let m = r.exec_at(s, at, sticky).map_err(|e| i.range_error(&e))?;
        let Some(groups) = m else { break };
        let (a, b) = groups[0].unwrap_or((at, at));
        out.extend_from_slice(&s[last..a]);
        let piece = if callable {
            let mut args: Vec<Value> = groups
                .iter()
                .map(|g| {
                    g.map(|(x, y)| Value::String(s[x..y].to_vec().into()))
                        .unwrap_or(Value::Undefined)
                })
                .collect();
            args.push(Value::Number(a as f64));
            args.push(Value::String(s.clone()));
            if !r.names.is_empty() {
                let g = Obj::new(None, Class::Ordinary);
                for (name, n) in &r.names {
                    g.set_own(
                        Key::str(name),
                        args.get(*n).cloned().unwrap_or(Value::Undefined),
                    );
                }
                args.push(Value::Object(g));
            }
            let v = i.call(&by, Value::Undefined, &args)?;
            i.to_text(&v)?.to_vec()
        } else {
            expand(&template, s, a, &groups, &r.names)
        };
        out.extend_from_slice(&piece);
        last = b;
        if !global {
            if sticky {
                i.set(pattern, Key::str("lastIndex"), Value::Number(b as f64))?;
            }
            break;
        }
        at = if b == a { b + 1 } else { b };
        if b == a && a < s.len() {
            // An empty match: the character after it is kept as it is.
            out.push(s[a]);
            last = a + 1;
        }
    }
    if global {
        i.set(pattern, Key::str("lastIndex"), Value::Number(0.0))?;
    }
    out.extend_from_slice(&s[last.min(s.len())..]);
    Ok(Value::String(out.into()))
}

/// A replacement string's `$&`, `$1`, `$<name>`, `$\``, `$'`, `$$` filled in.
pub fn expand(
    template: &[u16],
    s: &[u16],
    at: usize,
    groups: &[Option<(usize, usize)>],
    names: &[(String, usize)],
) -> Vec<u16> {
    let mut out = Vec::with_capacity(template.len());
    let group = |n: usize| -> &[u16] {
        match groups.get(n).copied().flatten() {
            Some((a, b)) => &s[a..b],
            None => &[],
        }
    };
    let whole = groups.first().copied().flatten().unwrap_or((at, at));
    let mut k = 0;
    while k < template.len() {
        let c = template[k];
        if c != b'$' as u16 || k + 1 >= template.len() {
            out.push(c);
            k += 1;
            continue;
        }
        let n = template[k + 1];
        match n {
            0x24 => {
                out.push(0x24);
                k += 2;
            }
            0x26 => {
                out.extend_from_slice(&s[whole.0..whole.1]);
                k += 2;
            }
            0x60 => {
                out.extend_from_slice(&s[..whole.0]);
                k += 2;
            }
            0x27 => {
                out.extend_from_slice(&s[whole.1..]);
                k += 2;
            }
            0x3c if !names.is_empty() => {
                let close = template[k + 2..].iter().position(|&x| x == b'>' as u16);
                match close {
                    Some(p) => {
                        let name = String::from_utf16_lossy(&template[k + 2..k + 2 + p]);
                        if let Some((_, g)) = names.iter().find(|(x, _)| *x == name) {
                            out.extend_from_slice(group(*g));
                        }
                        k += 3 + p;
                    }
                    None => {
                        out.push(c);
                        k += 1;
                    }
                }
            }
            d if (b'0' as u16..=b'9' as u16).contains(&d) => {
                let first = (d - b'0' as u16) as usize;
                let two = template
                    .get(k + 2)
                    .filter(|x| (b'0' as u16..=b'9' as u16).contains(x))
                    .map(|x| first * 10 + (x - b'0' as u16) as usize);
                if let Some(t) = two.filter(|t| *t >= 1 && *t < groups.len()) {
                    out.extend_from_slice(group(t));
                    k += 3;
                } else if first >= 1 && first < groups.len() {
                    out.extend_from_slice(group(first));
                    k += 2;
                } else {
                    out.push(c);
                    k += 1;
                }
            }
            _ => {
                out.push(c);
                k += 1;
            }
        }
    }
    out
}
