// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The operators, and the conversions that may run a program's own code: an object becomes a
//! primitive through its `Symbol.toPrimitive`, `valueOf` or `toString`.

use crate::ast::Binary;
use crate::lexer::{text, Text};
use crate::object::{Class, Obj};
use crate::value::{string_to_number, to_int32, to_uint32, Key, Value};

use super::{Interp, Res};

/// Which conversion an object is asked for first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// `valueOf` first (arithmetic, comparison).
    Number,
    /// `toString` first (a key, a string).
    String,
    /// As `+` and `==` ask: `valueOf` first, but a date's string.
    Default,
}

impl Interp {
    /// A primitive for `v`.
    pub fn to_primitive(&mut self, v: &Value, hint: Hint) -> Res<Value> {
        let Value::Object(o) = v else {
            return Ok(v.clone());
        };
        let exotic = Key::Symbol(self.intrinsics.sym_to_primitive.clone());
        let f = self.get(v, &exotic)?;
        if !f.is_nullish() {
            let word = match hint {
                Hint::Number => "number",
                Hint::String => "string",
                Hint::Default => "default",
            };
            let r = self.call(&f, v.clone(), &[Value::str(word)])?;
            if matches!(r, Value::Object(_)) {
                return Err(self.type_error("Cannot convert object to primitive value"));
            }
            return Ok(r);
        }
        let is_date = matches!(o.borrow().class, Class::Date(_));
        let order = if hint == Hint::String || (hint == Hint::Default && is_date) {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for name in order {
            let f = self.get(v, &Key::str(name))?;
            if matches!(&f, Value::Object(fo) if fo.is_callable()) {
                let r = self.call(&f, v.clone(), &[])?;
                if !matches!(r, Value::Object(_)) {
                    return Ok(r);
                }
            }
        }
        Err(self.type_error("Cannot convert object to primitive value"))
    }

    /// `Number(v)`.
    pub fn to_number(&mut self, v: &Value) -> Res<f64> {
        Ok(match v {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => *b as u8 as f64,
            Value::Number(n) => *n,
            Value::String(s) => string_to_number(s),
            Value::BigInt(_) => {
                return Err(self.type_error("Cannot convert a BigInt value to a number"))
            }
            Value::Symbol(_) => {
                return Err(self.type_error("Cannot convert a Symbol value to a number"))
            }
            Value::Object(_) => {
                let p = self.to_primitive(v, Hint::Number)?;
                return self.to_number(&p);
            }
        })
    }

    /// A number or a BigInt, as arithmetic takes it.
    pub fn to_numeric(&mut self, v: &Value) -> Res<Value> {
        let p = self.to_primitive(v, Hint::Number)?;
        if let Value::BigInt(_) = p {
            return Ok(p);
        }
        Ok(Value::Number(self.to_number(&p)?))
    }

    /// `String(v)`'s UTF-16.
    pub fn to_text(&mut self, v: &Value) -> Res<Text> {
        Ok(match v {
            Value::String(s) => s.clone(),
            Value::Undefined => text("undefined"),
            Value::Null => text("null"),
            Value::Bool(b) => text(if *b { "true" } else { "false" }),
            Value::Number(n) => text(&crate::number::to_string(*n)),
            Value::BigInt(n) => text(&n.to_string()),
            Value::Symbol(_) => {
                return Err(self.type_error("Cannot convert a Symbol value to a string"))
            }
            Value::Object(_) => {
                let p = self.to_primitive(v, Hint::String)?;
                return self.to_text(&p);
            }
        })
    }

    /// `String(v)` as Rust text.
    pub fn to_rust_string(&mut self, v: &Value) -> Res<String> {
        Ok(String::from_utf16_lossy(&self.to_text(v)?))
    }

    /// A property key for `v`.
    pub fn to_key(&mut self, v: &Value) -> Res<Key> {
        match v {
            Value::Symbol(s) => Ok(Key::Symbol(s.clone())),
            Value::String(s) => Ok(Key::String(s.clone())),
            Value::Number(n) if *n >= 0.0 && *n < 4294967295.0 && n.fract() == 0.0 => {
                Ok(Key::index(*n as usize))
            }
            Value::Object(_) => {
                let p = self.to_primitive(v, Hint::String)?;
                self.to_key(&p)
            }
            other => Ok(Key::String(self.to_text(other)?)),
        }
    }

    /// `Object(v)`: a primitive in its wrapper object.
    pub fn to_object(&mut self, v: &Value) -> Res<Obj> {
        let class = match v {
            Value::Object(o) => return Ok(o.clone()),
            Value::Undefined | Value::Null => {
                return Err(self.type_error("Cannot convert undefined or null to object"))
            }
            Value::Bool(b) => Class::Boolean(*b),
            Value::Number(n) => Class::Number(*n),
            Value::String(s) => Class::String(s.clone()),
            Value::Symbol(s) => Class::Symbol(s.clone()),
            Value::BigInt(n) => Class::BigInt(*n),
        };
        Ok(Obj::new(Some(self.proto_of_primitive(v)), class))
    }

    /// `a == b`.
    pub fn loose_equals(&mut self, a: &Value, b: &Value) -> Res<bool> {
        Ok(match (a, b) {
            (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => true,
            (Value::Undefined | Value::Null, _) | (_, Value::Undefined | Value::Null) => false,
            (Value::Number(_), Value::String(s)) => {
                a.strict_equals(&Value::Number(string_to_number(s)))
            }
            (Value::String(s), Value::Number(_)) => {
                Value::Number(string_to_number(s)).strict_equals(b)
            }
            (Value::BigInt(x), Value::Number(y)) | (Value::Number(y), Value::BigInt(x)) => {
                (*x as f64) == *y
            }
            (Value::BigInt(x), Value::String(s)) | (Value::String(s), Value::BigInt(x)) => {
                String::from_utf16_lossy(s).trim().parse::<i128>().ok() == Some(*x)
            }
            (Value::Bool(x), _) => {
                return self.loose_equals(&Value::Number(*x as u8 as f64), b);
            }
            (_, Value::Bool(y)) => {
                return self.loose_equals(a, &Value::Number(*y as u8 as f64));
            }
            (Value::Object(_), Value::Object(_)) => a.strict_equals(b),
            (Value::Object(_), _) => {
                let p = self.to_primitive(a, Hint::Default)?;
                return self.loose_equals(&p, b);
            }
            (_, Value::Object(_)) => {
                let p = self.to_primitive(b, Hint::Default)?;
                return self.loose_equals(a, &p);
            }
            _ => a.strict_equals(b),
        })
    }

    /// `a < b` (with `left_first` false, `b > a` as written): `None` where either is NaN.
    fn less_than(&mut self, a: &Value, b: &Value, left_first: bool) -> Res<Option<bool>> {
        let (pa, pb) = if left_first {
            let pa = self.to_primitive(a, Hint::Number)?;
            (pa, self.to_primitive(b, Hint::Number)?)
        } else {
            let pb = self.to_primitive(b, Hint::Number)?;
            (self.to_primitive(a, Hint::Number)?, pb)
        };
        if let (Value::String(x), Value::String(y)) = (&pa, &pb) {
            return Ok(Some(x[..] < y[..]));
        }
        match (&pa, &pb) {
            (Value::BigInt(x), Value::BigInt(y)) => return Ok(Some(x < y)),
            (Value::BigInt(x), other) => {
                let y = self.to_number(other)?;
                return Ok((!y.is_nan()).then_some((*x as f64) < y));
            }
            (other, Value::BigInt(y)) => {
                let x = self.to_number(other)?;
                return Ok((!x.is_nan()).then_some(x < (*y as f64)));
            }
            _ => {}
        }
        let x = self.to_number(&pa)?;
        let y = self.to_number(&pb)?;
        if x.is_nan() || y.is_nan() {
            return Ok(None);
        }
        Ok(Some(x < y))
    }

    /// `a op b`.
    pub fn binary(&mut self, op: Binary, a: &Value, b: &Value) -> Res<Value> {
        // The usual case first: two numbers.
        if let (Value::Number(x), Value::Number(y)) = (a, b) {
            let (x, y) = (*x, *y);
            let n = match op {
                Binary::Add => x + y,
                Binary::Sub => x - y,
                Binary::Mul => x * y,
                Binary::Div => x / y,
                Binary::Rem => js_rem(x, y),
                Binary::Pow => js_pow(x, y),
                Binary::Lt => return Ok(Value::Bool(x < y)),
                Binary::Le => return Ok(Value::Bool(x <= y)),
                Binary::Gt => return Ok(Value::Bool(x > y)),
                Binary::Ge => return Ok(Value::Bool(x >= y)),
                Binary::StrictEq | Binary::Eq => return Ok(Value::Bool(x == y)),
                Binary::StrictNe | Binary::Ne => return Ok(Value::Bool(x != y)),
                Binary::BitAnd => (to_int32(x) & to_int32(y)) as f64,
                Binary::BitOr => (to_int32(x) | to_int32(y)) as f64,
                Binary::BitXor => (to_int32(x) ^ to_int32(y)) as f64,
                Binary::Shl => to_int32(x).wrapping_shl(to_uint32(y) & 31) as f64,
                Binary::Shr => (to_int32(x) >> (to_uint32(y) & 31)) as f64,
                Binary::UShr => (to_uint32(x) >> (to_uint32(y) & 31)) as f64,
                Binary::In | Binary::Instanceof => return self.relation(op, a, b),
            };
            return Ok(Value::Number(n));
        }
        match op {
            Binary::StrictEq => Ok(Value::Bool(a.strict_equals(b))),
            Binary::StrictNe => Ok(Value::Bool(!a.strict_equals(b))),
            Binary::Eq => Ok(Value::Bool(self.loose_equals(a, b)?)),
            Binary::Ne => Ok(Value::Bool(!self.loose_equals(a, b)?)),
            Binary::Lt => Ok(Value::Bool(self.less_than(a, b, true)? == Some(true))),
            Binary::Gt => Ok(Value::Bool(self.less_than(b, a, false)? == Some(true))),
            Binary::Le => Ok(Value::Bool(self.less_than(b, a, false)? == Some(false))),
            Binary::Ge => Ok(Value::Bool(self.less_than(a, b, true)? == Some(false))),
            Binary::In | Binary::Instanceof => self.relation(op, a, b),
            Binary::Add => {
                let pa = self.to_primitive(a, Hint::Default)?;
                let pb = self.to_primitive(b, Hint::Default)?;
                if matches!(pa, Value::String(_)) || matches!(pb, Value::String(_)) {
                    let x = self.to_text(&pa)?;
                    let y = self.to_text(&pb)?;
                    let mut out = Vec::with_capacity(x.len() + y.len());
                    out.extend_from_slice(&x);
                    out.extend_from_slice(&y);
                    return Ok(Value::String(out.into()));
                }
                self.arithmetic(op, &pa, &pb)
            }
            _ => {
                let pa = self.to_numeric(a)?;
                let pb = self.to_numeric(b)?;
                self.arithmetic(op, &pa, &pb)
            }
        }
    }

    fn arithmetic(&mut self, op: Binary, a: &Value, b: &Value) -> Res<Value> {
        match (a, b) {
            (Value::BigInt(x), Value::BigInt(y)) => {
                let (x, y) = (*x, *y);
                let overflow = || self.range_error("a BigInt too large for dm");
                let n = match op {
                    Binary::Add => x.checked_add(y).ok_or_else(overflow)?,
                    Binary::Sub => x.checked_sub(y).ok_or_else(overflow)?,
                    Binary::Mul => x.checked_mul(y).ok_or_else(overflow)?,
                    Binary::Div => {
                        if y == 0 {
                            return Err(self.range_error("Division by zero"));
                        }
                        x / y
                    }
                    Binary::Rem => {
                        if y == 0 {
                            return Err(self.range_error("Division by zero"));
                        }
                        x % y
                    }
                    Binary::Pow => {
                        let e = u32::try_from(y).map_err(|_| overflow())?;
                        x.checked_pow(e).ok_or_else(overflow)?
                    }
                    Binary::BitAnd => x & y,
                    Binary::BitOr => x | y,
                    Binary::BitXor => x ^ y,
                    Binary::Shl => {
                        if y >= 0 {
                            x.checked_shl(y as u32).ok_or_else(overflow)?
                        } else {
                            x >> (-y).min(127)
                        }
                    }
                    Binary::Shr => {
                        if y >= 0 {
                            x >> y.min(127)
                        } else {
                            x.checked_shl((-y) as u32).ok_or_else(overflow)?
                        }
                    }
                    Binary::UShr => {
                        return Err(self.type_error("BigInts have no unsigned right shift"))
                    }
                    _ => return Err(self.type_error("not an arithmetic operator")),
                };
                Ok(Value::BigInt(n))
            }
            (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                Err(self.type_error("Cannot mix BigInt and other types, use explicit conversions"))
            }
            _ => {
                let x = self.to_number(a)?;
                let y = self.to_number(b)?;
                self.binary(op, &Value::Number(x), &Value::Number(y))
            }
        }
    }

    /// `a in b`, `a instanceof b`.
    fn relation(&mut self, op: Binary, a: &Value, b: &Value) -> Res<Value> {
        let Value::Object(o) = b else {
            let word = if op == Binary::In { "in" } else { "instanceof" };
            return Err(self.type_error(&format!(
                "Right-hand side of '{word}' is not {}",
                if op == Binary::In {
                    "an object"
                } else {
                    "callable"
                }
            )));
        };
        if op == Binary::In {
            let key = self.to_key(a)?;
            if let Class::Proxy(target, handler) = &o.borrow().class {
                let (target, handler) = (target.clone(), handler.clone());
                return crate::builtins::proxy::has(self, &target, &handler, &key);
            }
            return Ok(Value::Bool(self.has_property(o, &key)));
        }
        let custom = Key::Symbol(self.intrinsics.sym_has_instance.clone());
        let f = self.get(b, &custom)?;
        if !f.is_nullish() {
            let r = self.call(&f, b.clone(), std::slice::from_ref(a))?;
            return Ok(Value::Bool(r.truthy()));
        }
        if !o.is_callable() {
            return Err(self.type_error("Right-hand side of 'instanceof' is not callable"));
        }
        Ok(Value::Bool(self.instance_of(a, o)?))
    }

    /// Whether `v`'s prototypes hold `f.prototype` (bound functions look through to their
    /// target).
    pub fn instance_of(&mut self, v: &Value, f: &Obj) -> Res<bool> {
        if let Class::Function(crate::object::Func::Bound { target, .. }) = &f.borrow().class {
            let target = target.clone();
            return self.instance_of(v, &target);
        }
        let Value::Object(o) = v else {
            return Ok(false);
        };
        let proto = self.get(&Value::Object(f.clone()), &Key::str("prototype"))?;
        let Value::Object(proto) = proto else {
            return Err(self.type_error("Function has non-object prototype in instanceof check"));
        };
        let mut at = o.proto();
        while let Some(p) = at {
            if p.same(&proto) {
                return Ok(true);
            }
            at = p.proto();
        }
        Ok(false)
    }
}

/// `x % y`, the sign of `x`.
pub fn js_rem(x: f64, y: f64) -> f64 {
    if y.is_infinite() && x.is_finite() {
        return x;
    }
    x % y
}

/// `x ** y`, where JavaScript differs from `powf` (`1 ** NaN` is NaN).
pub fn js_pow(x: f64, y: f64) -> f64 {
    if y.is_nan() || ((x == 1.0 || x == -1.0) && y.is_infinite()) {
        return f64::NAN;
    }
    x.powf(y)
}
