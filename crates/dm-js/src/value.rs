// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A JavaScript value, a property's key, and the conversions that need no program to run
//! (those that may call a program's `valueOf` or `toString` are the interpreter's).

use std::rc::Rc;

use crate::lexer::{text, Text};
use crate::object::Obj;

/// A symbol: a key no string can be.
#[derive(Debug)]
pub struct Symbol {
    /// What `Symbol(description)` was given.
    pub description: Option<Text>,
}

/// A JavaScript value.
#[derive(Clone, Debug)]
pub enum Value {
    /// `undefined`
    Undefined,
    /// `null`
    Null,
    /// `true`, `false`
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(Text),
    /// A BigInt, as a signed whole number of up to 128 bits (what dm meets fits).
    BigInt(i128),
    /// A symbol.
    Symbol(Rc<Symbol>),
    /// An object.
    Object(Obj),
}

impl Value {
    /// The value of `s`.
    pub fn str(s: &str) -> Value {
        Value::String(text(s))
    }

    /// `typeof value`.
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::BigInt(_) => "bigint",
            Value::Symbol(_) => "symbol",
            Value::Object(o) => {
                if o.is_callable() {
                    "function"
                } else {
                    "object"
                }
            }
        }
    }

    /// `!!value`.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::BigInt(n) => *n != 0,
            Value::Symbol(_) | Value::Object(_) => true,
        }
    }

    /// `null` or `undefined`.
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// The object, where this is one.
    pub fn as_object(&self) -> Option<&Obj> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// `a === b`.
    pub fn strict_equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::Symbol(a), Value::Symbol(b)) => Rc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => a.same(b),
            _ => false,
        }
    }

    /// `Object.is(a, b)`'s sameness, which `Map` keys and `includes` use with zeros alike.
    pub fn same_value_zero(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) if a.is_nan() && b.is_nan() => true,
            _ => self.strict_equals(other),
        }
    }
}

/// A property's key: a string or a symbol.
#[derive(Clone, Debug)]
pub enum Key {
    /// A string key (an array's index too, written in base ten).
    String(Text),
    /// A symbol key.
    Symbol(Rc<Symbol>),
}

impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        match (self, other) {
            (Key::String(a), Key::String(b)) => a == b,
            (Key::Symbol(a), Key::Symbol(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Key {}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Key::String(s) => s.hash(state),
            Key::Symbol(s) => (Rc::as_ptr(s) as usize).hash(state),
        }
    }
}

impl Key {
    /// The key `s`.
    pub fn str(s: &str) -> Key {
        Key::String(text(s))
    }

    /// The key of an array's index.
    pub fn index(i: usize) -> Key {
        Key::String(text(&i.to_string()))
    }

    /// The index this key is (`"0"`, `"12"`, below 2³²−1), where it is one.
    pub fn as_index(&self) -> Option<usize> {
        match self {
            Key::String(s) => index_of(s),
            Key::Symbol(_) => None,
        }
    }

    /// The key as a value (`Object.keys` gives strings; a symbol stays a symbol).
    pub fn to_value(&self) -> Value {
        match self {
            Key::String(s) => Value::String(s.clone()),
            Key::Symbol(s) => Value::Symbol(s.clone()),
        }
    }
}

/// The array index `s` writes, where it writes one in its one way (`"01"` does not).
pub fn index_of(s: &[u16]) -> Option<usize> {
    if s.is_empty() || s.len() > 10 || (s.len() > 1 && s[0] == b'0' as u16) {
        return None;
    }
    let mut n: u64 = 0;
    for &c in s {
        if !(b'0' as u16..=b'9' as u16).contains(&c) {
            return None;
        }
        n = n * 10 + (c - b'0' as u16) as u64;
    }
    (n < u32::MAX as u64).then_some(n as usize)
}

/// A string's number, as `Number(string)` reads it.
pub fn string_to_number(s: &[u16]) -> f64 {
    let s = String::from_utf16_lossy(s);
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    let (radix, digits) = match t.get(..2) {
        Some("0x" | "0X") => (16, &t[2..]),
        Some("0o" | "0O") => (8, &t[2..]),
        Some("0b" | "0B") => (2, &t[2..]),
        _ => (10, t),
    };
    if radix != 10 {
        if digits.is_empty() {
            return f64::NAN;
        }
        let mut n = 0f64;
        for c in digits.chars() {
            match c.to_digit(radix) {
                Some(d) => n = n * radix as f64 + d as f64,
                None => return f64::NAN,
            }
        }
        return n;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Rust reads "inf", "nan" and "1_0" in ways JavaScript does not: digits, a point, an
    // exponent and a sign only.
    if !t
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
    {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `n` as `ToInt32` gives it (`n | 0`).
pub fn to_int32(n: f64) -> i32 {
    to_uint32(n) as i32
}

/// `n` as `ToUint32` gives it (`n >>> 0`).
pub fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let n = n.trunc() % 4294967296.0;
    let n = if n < 0.0 { n + 4294967296.0 } else { n };
    n as u32
}

/// `n` as `ToIntegerOrInfinity` gives it.
pub fn to_integer(n: f64) -> f64 {
    if n.is_nan() {
        0.0
    } else {
        n.trunc()
    }
}

/// A relative index (`slice`'s, `at`'s): a negative one counts from `len`, and the whole is
/// held between 0 and `len`.
pub fn relative_index(n: f64, len: usize) -> usize {
    let n = to_integer(n);
    let len_f = len as f64;
    if n < 0.0 {
        (len_f + n).max(0.0) as usize
    } else {
        n.min(len_f) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_read_as_numbers_as_number_reads_them() {
        for (s, n) in [
            ("12", 12.0),
            ("  1.5  ", 1.5),
            ("", 0.0),
            ("0x1f", 31.0),
            ("1e3", 1000.0),
            ("-Infinity", f64::NEG_INFINITY),
        ] {
            assert_eq!(string_to_number(&text(s)), n, "{s}");
        }
        for s in ["abc", "1_0", "inf", "nan", "0x", "1 2"] {
            assert!(string_to_number(&text(s)).is_nan(), "{s}");
        }
    }

    #[test]
    fn whole_numbers_wrap_as_the_bit_operators_wrap_them() {
        assert_eq!(to_int32(4294967296.0 + 5.0), 5);
        assert_eq!(to_int32(2147483648.0), -2147483648);
        assert_eq!(to_uint32(-1.0), 4294967295);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(-3.7), -3);
    }

    #[test]
    fn only_a_canonical_index_is_an_index() {
        assert_eq!(index_of(&text("0")), Some(0));
        assert_eq!(index_of(&text("42")), Some(42));
        assert_eq!(index_of(&text("01")), None);
        assert_eq!(index_of(&text("-1")), None);
        assert_eq!(index_of(&text("4294967295")), None);
    }
}
