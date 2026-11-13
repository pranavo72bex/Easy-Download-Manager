// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Number`, `Math` and `BigInt`. `Math.random` gives numbers from a fixed seed: a program
//! here has no clock and no randomness of the computer's.

use super::{arg, constructor, global, int_arg, method};
use crate::interp::{Interp, Res};
use crate::object::{Class, Obj, Prop};
use crate::value::{Key, Value};

/// Installs them.
pub fn install(interp: &mut Interp) {
    let np = interp.intrinsics.number_proto.clone();
    let number = constructor(
        interp,
        "Number",
        1,
        |i, _, a, made| {
            let n = match a.first() {
                None => 0.0,
                Some(v) => match i.to_numeric(v)? {
                    Value::BigInt(b) => b as f64,
                    Value::Number(n) => n,
                    _ => f64::NAN,
                },
            };
            Ok(match made {
                Some(p) => Value::Object(Obj::new(Some(p.clone()), Class::Number(n))),
                None => Value::Number(n),
            })
        },
        &np,
    );
    for (name, v) in [
        ("MAX_SAFE_INTEGER", 9007199254740991.0),
        ("MIN_SAFE_INTEGER", -9007199254740991.0),
        ("MAX_VALUE", f64::MAX),
        ("MIN_VALUE", 5e-324),
        ("EPSILON", f64::EPSILON),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ("NaN", f64::NAN),
    ] {
        number.define(
            Key::str(name),
            Prop::data(Value::Number(v), false, false, false),
        );
    }
    method(interp, &number, "isInteger", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Number(n) if n.is_finite() && n.fract() == 0.0),
        ))
    });
    method(interp, &number, "isSafeInteger", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0),
        ))
    });
    method(interp, &number, "isFinite", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Number(n) if n.is_finite()),
        ))
    });
    method(interp, &number, "isNaN", 1, |_, _, a, _| {
        Ok(Value::Bool(
            matches!(arg(a, 0), Value::Number(n) if n.is_nan()),
        ))
    });
    for name in ["parseInt", "parseFloat"] {
        if let Some(p) = interp.global.own(&Key::str(name)) {
            if let crate::object::Slot::Data(v) = p.slot {
                number.set_hidden(Key::str(name), v);
            }
        }
    }
    method(interp, &np, "toString", 1, |i, this, a, _| {
        let n = this_number(i, this)?;
        let radix = match arg(a, 0) {
            Value::Undefined => 10,
            v => i.to_number(&v)? as u32,
        };
        if !(2..=36).contains(&radix) {
            return Err(i.range_error("toString() radix must be between 2 and 36"));
        }
        Ok(Value::str(&if radix == 10 {
            crate::number::to_string(n)
        } else {
            in_radix(n, radix)
        }))
    });
    method(interp, &np, "toLocaleString", 0, |i, this, _, _| {
        let n = this_number(i, this)?;
        Ok(Value::str(&crate::number::to_string(n)))
    });
    method(interp, &np, "valueOf", 0, |i, this, _, _| {
        Ok(Value::Number(this_number(i, this)?))
    });
    method(interp, &np, "toFixed", 1, |i, this, a, _| {
        let n = this_number(i, this)?;
        let digits = int_arg(i, a, 0)?;
        if !(0.0..=100.0).contains(&digits) {
            return Err(i.range_error("toFixed() digits argument must be between 0 and 100"));
        }
        if !n.is_finite() || n.abs() >= 1e21 {
            return Ok(Value::str(&crate::number::to_string(n)));
        }
        Ok(Value::str(&format!("{:.*}", digits as usize, n)))
    });
    method(interp, &np, "toPrecision", 1, |i, this, a, _| {
        let n = this_number(i, this)?;
        if matches!(arg(a, 0), Value::Undefined) || !n.is_finite() {
            return Ok(Value::str(&crate::number::to_string(n)));
        }
        let p = int_arg(i, a, 0)?;
        if !(1.0..=100.0).contains(&p) {
            return Err(i.range_error("toPrecision() argument must be between 1 and 100"));
        }
        let p = p as usize;
        if n == 0.0 {
            return Ok(Value::str(&format!("{:.*}", p - 1, 0.0)));
        }
        let e = n.abs().log10().floor() as i32;
        if e < -6 || e >= p as i32 {
            let s = format!("{:.*e}", p - 1, n);
            let (m, x) = s.split_once('e').unwrap_or((&s, "0"));
            let x: i32 = x.parse().unwrap_or(0);
            return Ok(Value::str(&format!(
                "{m}e{}{}",
                if x < 0 { '-' } else { '+' },
                x.abs()
            )));
        }
        Ok(Value::str(&format!(
            "{:.*}",
            (p as i32 - 1 - e).max(0) as usize,
            n
        )))
    });
    method(interp, &np, "toExponential", 1, |i, this, a, _| {
        let n = this_number(i, this)?;
        let s = match arg(a, 0) {
            Value::Undefined => format!("{n:e}"),
            v => format!("{:.*e}", i.to_number(&v)? as usize, n),
        };
        let (m, x) = s.split_once('e').unwrap_or((&s, "0"));
        let x: i32 = x.parse().unwrap_or(0);
        Ok(Value::str(&format!(
            "{m}e{}{}",
            if x < 0 { '-' } else { '+' },
            x.abs()
        )))
    });

    // ---- Math
    let math = interp.plain_object();
    global(interp, "Math", Value::Object(math.clone()));
    let tag = Key::Symbol(interp.intrinsics.sym_to_string_tag.clone());
    math.set_hidden(tag, Value::str("Math"));
    for (name, v) in [
        ("PI", std::f64::consts::PI),
        ("E", std::f64::consts::E),
        ("LN2", std::f64::consts::LN_2),
        ("LN10", std::f64::consts::LN_10),
        ("LOG2E", std::f64::consts::LOG2_E),
        ("LOG10E", std::f64::consts::LOG10_E),
        ("SQRT2", std::f64::consts::SQRT_2),
        ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
    ] {
        math.define(
            Key::str(name),
            Prop::data(Value::Number(v), false, false, false),
        );
    }
    macro_rules! unary {
        ($name:expr, $f:expr) => {
            method(interp, &math, $name, 1, |i, _, a, _| {
                let x = i.to_number(&arg(a, 0))?;
                let f: fn(f64) -> f64 = $f;
                Ok(Value::Number(f(x)))
            });
        };
    }
    unary!("abs", f64::abs);
    unary!("floor", f64::floor);
    unary!("ceil", f64::ceil);
    unary!("trunc", f64::trunc);
    unary!("sign", |x| if x.is_nan() || x == 0.0 {
        x
    } else {
        x.signum()
    });
    unary!("sqrt", f64::sqrt);
    unary!("cbrt", f64::cbrt);
    unary!("exp", f64::exp);
    unary!("expm1", f64::exp_m1);
    unary!("log", f64::ln);
    unary!("log2", f64::log2);
    unary!("log10", f64::log10);
    unary!("log1p", f64::ln_1p);
    unary!("sin", f64::sin);
    unary!("cos", f64::cos);
    unary!("tan", f64::tan);
    unary!("asin", f64::asin);
    unary!("acos", f64::acos);
    unary!("atan", f64::atan);
    unary!("sinh", f64::sinh);
    unary!("cosh", f64::cosh);
    unary!("tanh", f64::tanh);
    unary!("asinh", f64::asinh);
    unary!("acosh", f64::acosh);
    unary!("atanh", f64::atanh);
    unary!("fround", |x| x as f32 as f64);
    unary!("clz32", |x| crate::value::to_uint32(x).leading_zeros()
        as f64);
    unary!("round", |x| if x.is_nan() || x.is_infinite() {
        x
    } else if x.fract() == -0.5 {
        x.ceil()
    } else {
        (x + 0.5).floor()
    });
    method(interp, &math, "atan2", 2, |i, _, a, _| {
        let (y, x) = (i.to_number(&arg(a, 0))?, i.to_number(&arg(a, 1))?);
        Ok(Value::Number(y.atan2(x)))
    });
    method(interp, &math, "pow", 2, |i, _, a, _| {
        let (x, y) = (i.to_number(&arg(a, 0))?, i.to_number(&arg(a, 1))?);
        Ok(Value::Number(crate::interp::js_pow(x, y)))
    });
    method(interp, &math, "imul", 2, |i, _, a, _| {
        let x = crate::value::to_int32(i.to_number(&arg(a, 0))?);
        let y = crate::value::to_int32(i.to_number(&arg(a, 1))?);
        Ok(Value::Number(x.wrapping_mul(y) as f64))
    });
    method(interp, &math, "max", 2, |i, _, a, _| {
        let mut m = f64::NEG_INFINITY;
        for v in a {
            let x = i.to_number(v)?;
            if x.is_nan() || m.is_nan() {
                m = f64::NAN
            } else if x > m || (x == 0.0 && m == 0.0 && m.is_sign_negative()) {
                m = x
            }
        }
        Ok(Value::Number(m))
    });
    method(interp, &math, "min", 2, |i, _, a, _| {
        let mut m = f64::INFINITY;
        for v in a {
            let x = i.to_number(v)?;
            if x.is_nan() || m.is_nan() {
                m = f64::NAN
            } else if x < m || (x == 0.0 && m == 0.0 && x.is_sign_negative()) {
                m = x
            }
        }
        Ok(Value::Number(m))
    });
    method(interp, &math, "hypot", 2, |i, _, a, _| {
        let mut s = 0.0;
        for v in a {
            let x = i.to_number(v)?;
            s += x * x;
        }
        Ok(Value::Number(s.sqrt()))
    });
    method(interp, &math, "random", 0, |_, _, _, _| {
        // A fixed sequence (xorshift): what dm runs needs no real chance.
        thread_local! { static SEED: std::cell::Cell<u64> = const { std::cell::Cell::new(0x2545F4914F6CDD1D) }; }
        let n = SEED.with(|s| {
            let mut x = s.get();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.set(x);
            x
        });
        Ok(Value::Number((n >> 11) as f64 / (1u64 << 53) as f64))
    });

    // ---- BigInt
    let bp = interp.intrinsics.bigint_proto.clone();
    let bigint = constructor(
        interp,
        "BigInt",
        1,
        |i, _, a, made| {
            if made.is_some() {
                return Err(i.type_error("BigInt is not a constructor"));
            }
            let v = i.to_primitive(&arg(a, 0), crate::interp::Hint::Number)?;
            Ok(Value::BigInt(match v {
                Value::Number(n) if n.fract() == 0.0 && n.is_finite() => n as i128,
                Value::Number(_) => {
                    return Err(i.range_error(
                        "The number cannot be converted to a BigInt because it is not an integer",
                    ))
                }
                Value::BigInt(b) => b,
                Value::Bool(b) => b as i128,
                Value::String(s) => {
                    let t = String::from_utf16_lossy(&s);
                    let t = t.trim();
                    let parsed =
                        if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                            i128::from_str_radix(h, 16).ok()
                        } else if t.is_empty() {
                            Some(0)
                        } else {
                            t.parse::<i128>().ok()
                        };
                    parsed
                        .ok_or_else(|| i.syntax_error(&format!("Cannot convert {t} to a BigInt")))?
                }
                _ => return Err(i.type_error("Cannot convert to a BigInt")),
            }))
        },
        &bp,
    );
    method(interp, &bigint, "asUintN", 2, |i, _, a, _| {
        let bits = int_arg(i, a, 0)? as u32;
        let Value::BigInt(n) = arg(a, 1) else {
            return Err(i.type_error("not a BigInt"));
        };
        Ok(Value::BigInt(if bits >= 127 {
            n
        } else {
            n & ((1i128 << bits) - 1)
        }))
    });
    method(interp, &bigint, "asIntN", 2, |i, _, a, _| {
        let bits = int_arg(i, a, 0)? as u32;
        let Value::BigInt(n) = arg(a, 1) else {
            return Err(i.type_error("not a BigInt"));
        };
        if bits == 0 || bits >= 127 {
            return Ok(Value::BigInt(if bits == 0 { 0 } else { n }));
        }
        let m = n & ((1i128 << bits) - 1);
        Ok(Value::BigInt(if m >= 1i128 << (bits - 1) {
            m - (1i128 << bits)
        } else {
            m
        }))
    });
    method(interp, &bp, "toString", 0, |i, this, a, _| {
        let n = this_bigint(i, this)?;
        let radix = match arg(a, 0) {
            Value::Undefined => 10,
            v => i.to_number(&v)? as u32,
        };
        if radix == 10 {
            return Ok(Value::str(&n.to_string()));
        }
        let mut digits = Vec::new();
        let mut m = n.unsigned_abs();
        if m == 0 {
            digits.push('0');
        }
        while m > 0 {
            digits.push(std::char::from_digit((m % radix as u128) as u32, radix).unwrap_or('0'));
            m /= radix as u128;
        }
        if n < 0 {
            digits.push('-');
        }
        Ok(Value::str(&digits.into_iter().rev().collect::<String>()))
    });
    method(interp, &bp, "valueOf", 0, |i, this, _, _| {
        Ok(Value::BigInt(this_bigint(i, this)?))
    });
    method(interp, &bp, "toLocaleString", 0, |i, this, _, _| {
        Ok(Value::str(&this_bigint(i, this)?.to_string()))
    });
}

fn this_number(i: &mut Interp, this: &Value) -> Res<f64> {
    match this {
        Value::Number(n) => Ok(*n),
        Value::Object(o) => match &o.borrow().class {
            Class::Number(n) => Ok(*n),
            _ => Err(i.type_error("not a number")),
        },
        _ => Err(i.type_error("not a number")),
    }
}

fn this_bigint(i: &mut Interp, this: &Value) -> Res<i128> {
    match this {
        Value::BigInt(n) => Ok(*n),
        Value::Object(o) => match &o.borrow().class {
            Class::BigInt(n) => Ok(*n),
            _ => Err(i.type_error("not a BigInt")),
        },
        _ => Err(i.type_error("not a BigInt")),
    }
}

/// `n.toString(radix)`, with a fraction to as many digits as hold it (at most 52).
fn in_radix(n: f64, radix: u32) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let negative = n < 0.0;
    let n = n.abs();
    let mut whole = n.trunc();
    let mut frac = n - whole;
    let mut digits = Vec::new();
    if whole == 0.0 {
        digits.push('0');
    }
    while whole >= 1.0 {
        let d = (whole % radix as f64) as u32;
        digits.push(std::char::from_digit(d, radix).unwrap_or('0'));
        whole = (whole / radix as f64).trunc();
    }
    let mut out: String = digits.into_iter().rev().collect();
    if frac > 0.0 {
        out.push('.');
        let mut count = 0;
        while frac > 0.0 && count < 52 {
            frac *= radix as f64;
            let d = frac.trunc() as u32;
            out.push(std::char::from_digit(d, radix).unwrap_or('0'));
            frac -= d as f64;
            count += 1;
        }
    }
    if negative {
        out.insert(0, '-');
    }
    out
}
