// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `JSON.parse` and `JSON.stringify`.

use super::{arg, global, method};
use crate::interp::{Interp, Res};
use crate::lexer::Text;
use crate::object::{Class, Obj};
use crate::value::{Key, Value};

/// Installs `JSON`.
pub fn install(interp: &mut Interp) {
    let json = interp.plain_object();
    global(interp, "JSON", Value::Object(json.clone()));
    let tag = Key::Symbol(interp.intrinsics.sym_to_string_tag.clone());
    json.set_hidden(tag, Value::str("JSON"));
    method(interp, &json, "parse", 2, |i, _, a, _| {
        let s = i.to_text(&arg(a, 0))?;
        let mut p = Reader { s: &s, at: 0 };
        p.space();
        let v = p.value(i)?;
        p.space();
        if p.at < s.len() {
            return Err(i.syntax_error("Unexpected non-whitespace character after JSON"));
        }
        let reviver = arg(a, 1);
        if reviver.as_object().is_some_and(|f| f.is_callable()) {
            let holder = i.plain_object();
            holder.set_own(Key::str(""), v);
            return revive(i, &Value::Object(holder), Key::str(""), &reviver);
        }
        Ok(v)
    });
    method(interp, &json, "stringify", 3, |i, _, a, _| {
        let indent = match arg(a, 2) {
            Value::Number(n) => " ".repeat(n.clamp(0.0, 10.0) as usize),
            Value::String(s) => String::from_utf16_lossy(&s[..s.len().min(10)]),
            _ => String::new(),
        };
        let mut w = Writer {
            out: String::new(),
            indent,
            seen: Vec::new(),
            replacer: arg(a, 1),
        };
        let holder = i.plain_object();
        holder.set_own(Key::str(""), arg(a, 0));
        let wrote = w.value(i, &Value::Object(holder), Key::str(""), arg(a, 0), "")?;
        Ok(if wrote {
            Value::str(&w.out)
        } else {
            Value::Undefined
        })
    });
}

fn revive(i: &mut Interp, holder: &Value, key: Key, reviver: &Value) -> Res<Value> {
    let v = i.get(holder, &key)?;
    if let Value::Object(o) = &v {
        for k in o.own_keys(true) {
            let r = revive(i, &v, k.clone(), reviver)?;
            if matches!(r, Value::Undefined) {
                o.delete(&k);
            } else {
                o.set_own(k, r);
            }
        }
    }
    i.call(reviver, holder.clone(), &[key.to_value(), v])
}

struct Reader<'a> {
    s: &'a Text,
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self
            .s
            .get(self.at)
            .is_some_and(|&c| matches!(c, 9 | 10 | 13 | 32))
        {
            self.at += 1;
        }
    }

    fn fail<T>(&self, i: &Interp) -> Res<T> {
        Err(i.syntax_error(&format!("Unexpected token in JSON at position {}", self.at)))
    }

    fn word(&mut self, w: &str, v: Value, i: &Interp) -> Res<Value> {
        let units: Vec<u16> = w.encode_utf16().collect();
        if self.s.get(self.at..self.at + units.len()) == Some(&units[..]) {
            self.at += units.len();
            return Ok(v);
        }
        self.fail(i)
    }

    fn value(&mut self, i: &mut Interp) -> Res<Value> {
        let Some(&c) = self.s.get(self.at) else {
            return self.fail(i);
        };
        match c {
            0x7b => {
                self.at += 1;
                let o = i.plain_object();
                self.space();
                if self.s.get(self.at) == Some(&0x7d) {
                    self.at += 1;
                    return Ok(Value::Object(o));
                }
                loop {
                    self.space();
                    if self.s.get(self.at) != Some(&0x22) {
                        return self.fail(i);
                    }
                    let k = self.string(i)?;
                    self.space();
                    if self.s.get(self.at) != Some(&0x3a) {
                        return self.fail(i);
                    }
                    self.at += 1;
                    self.space();
                    let v = self.value(i)?;
                    o.set_own(Key::String(k), v);
                    self.space();
                    match self.s.get(self.at) {
                        Some(0x2c) => self.at += 1,
                        Some(0x7d) => {
                            self.at += 1;
                            return Ok(Value::Object(o));
                        }
                        _ => return self.fail(i),
                    }
                }
            }
            0x5b => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.s.get(self.at) == Some(&0x5d) {
                    self.at += 1;
                    return Ok(i.array(items));
                }
                loop {
                    self.space();
                    items.push(self.value(i)?);
                    self.space();
                    match self.s.get(self.at) {
                        Some(0x2c) => self.at += 1,
                        Some(0x5d) => {
                            self.at += 1;
                            return Ok(i.array(items));
                        }
                        _ => return self.fail(i),
                    }
                }
            }
            0x22 => Ok(Value::String(self.string(i)?)),
            0x74 => self.word("true", Value::Bool(true), i),
            0x66 => self.word("false", Value::Bool(false), i),
            0x6e => self.word("null", Value::Null, i),
            _ => {
                let from = self.at;
                while self
                    .s
                    .get(self.at)
                    .is_some_and(|&c| matches!(c, 0x30..=0x39 | 0x2b | 0x2d | 0x2e | 0x65 | 0x45))
                {
                    self.at += 1;
                }
                let t = String::from_utf16_lossy(&self.s[from..self.at]);
                match t.parse::<f64>() {
                    Ok(n) if !t.is_empty() && !t.starts_with('+') && !t.starts_with('.') => {
                        Ok(Value::Number(n))
                    }
                    _ => {
                        self.at = from;
                        self.fail(i)
                    }
                }
            }
        }
    }

    fn string(&mut self, i: &Interp) -> Res<Text> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.at) else {
                return self.fail(i);
            };
            self.at += 1;
            match c {
                0x22 => return Ok(out.into()),
                0x5c => {
                    let Some(&e) = self.s.get(self.at) else {
                        return self.fail(i);
                    };
                    self.at += 1;
                    out.push(match e {
                        0x6e => 10,
                        0x74 => 9,
                        0x72 => 13,
                        0x62 => 8,
                        0x66 => 12,
                        0x75 => {
                            let hex = self
                                .s
                                .get(self.at..self.at + 4)
                                .map(String::from_utf16_lossy);
                            let v = hex.and_then(|h| u16::from_str_radix(&h, 16).ok());
                            let Some(v) = v else { return self.fail(i) };
                            self.at += 4;
                            v
                        }
                        0x22 | 0x5c | 0x2f => e,
                        _ => return self.fail(i),
                    });
                }
                c if c < 0x20 => return self.fail(i),
                c => out.push(c),
            }
        }
    }
}

struct Writer {
    out: String,
    indent: String,
    seen: Vec<Obj>,
    replacer: Value,
}

impl Writer {
    /// Writes `v` (`holder[key]`); false where it writes nothing (undefined, a function).
    fn value(
        &mut self,
        i: &mut Interp,
        holder: &Value,
        key: Key,
        v: Value,
        gap: &str,
    ) -> Res<bool> {
        let mut v = v;
        if let Value::Object(_) | Value::BigInt(_) = &v {
            let to_json = i.get_of(&v, &Key::str("toJSON"))?;
            if to_json.as_object().is_some_and(|f| f.is_callable()) {
                v = i.call(&to_json, v.clone(), &[key.to_value()])?;
            }
        }
        if self.replacer.as_object().is_some_and(|f| f.is_callable()) {
            let r = self.replacer.clone();
            v = i.call(&r, holder.clone(), &[key.to_value(), v])?;
        }
        if let Value::Object(o) = &v {
            let unwrapped = match &o.borrow().class {
                Class::Number(n) => Some(Value::Number(*n)),
                Class::String(s) => Some(Value::String(s.clone())),
                Class::Boolean(b) => Some(Value::Bool(*b)),
                _ => None,
            };
            if let Some(u) = unwrapped {
                v = u;
            }
        }
        match &v {
            Value::Null => self.out.push_str("null"),
            Value::Bool(b) => self.out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => self.out.push_str(&if n.is_finite() {
                crate::number::to_string(*n)
            } else {
                "null".into()
            }),
            Value::String(s) => quote(&mut self.out, s),
            Value::BigInt(_) => return Err(i.type_error("Do not know how to serialize a BigInt")),
            Value::Undefined | Value::Symbol(_) => return Ok(false),
            Value::Object(o) if o.is_callable() => return Ok(false),
            Value::Object(o) => {
                if self.seen.iter().any(|s| s.same(o)) {
                    return Err(i.type_error("Converting circular structure to JSON"));
                }
                self.seen.push(o.clone());
                let inner = format!("{gap}{}", self.indent);
                let is_array = matches!(o.borrow().class, Class::Array(_));
                if is_array {
                    let items = super::array::items_of(i, &v)?;
                    self.out.push('[');
                    for (n, item) in items.into_iter().enumerate() {
                        if n > 0 {
                            self.out.push(',');
                        }
                        self.newline(&inner);
                        if !self.value(i, &v, Key::index(n), item, &inner)? {
                            self.out.push_str("null");
                        }
                    }
                    if !self.out.ends_with('[') {
                        self.newline(gap);
                    }
                    self.out.push(']');
                } else {
                    self.out.push('{');
                    let mut first = true;
                    let keys: Vec<Key> = match &self.replacer {
                        Value::Object(r) if matches!(r.borrow().class, Class::Array(_)) => {
                            let list = super::array::items_of(i, &self.replacer.clone())?;
                            let mut ks = Vec::new();
                            for k in list {
                                ks.push(i.to_key(&k)?);
                            }
                            ks
                        }
                        _ => o
                            .own_keys(true)
                            .into_iter()
                            .filter(|k| matches!(k, Key::String(_)))
                            .collect(),
                    };
                    for k in keys {
                        let item = i.get(&v, &k)?;
                        let mark = self.out.len();
                        if !first {
                            self.out.push(',');
                        }
                        self.newline(&inner);
                        quote(&mut self.out, &i.key_text(&k));
                        self.out.push(':');
                        if !self.indent.is_empty() {
                            self.out.push(' ');
                        }
                        if self.value(i, &v, k, item, &inner)? {
                            first = false;
                        } else {
                            self.out.truncate(mark);
                        }
                    }
                    if !first {
                        self.newline(gap);
                    }
                    self.out.push('}');
                }
                self.seen.pop();
            }
        }
        Ok(true)
    }

    fn newline(&mut self, gap: &str) {
        if !self.indent.is_empty() {
            self.out.push('\n');
            self.out.push_str(gap);
        }
    }
}

fn quote(out: &mut String, s: &[u16]) {
    out.push('"');
    for c in char::decode_utf16(s.iter().copied()) {
        match c {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\t') => out.push_str("\\t"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok(c) if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            Ok(c) => out.push(c),
            Err(e) => out.push_str(&format!("\\u{:04x}", e.unpaired_surrogate())),
        }
    }
    out.push('"');
}
