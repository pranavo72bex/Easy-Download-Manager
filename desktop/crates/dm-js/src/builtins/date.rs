// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `Date`. A program here has no clock: `Date.now()` and `new Date()` give one fixed moment
//! ([`NOW`]), and every time is UTC (the computer's time zone is not in reach either).

use super::{arg, constructor, method};
use crate::interp::{Interp, Res};
use crate::object::{Class, Obj};
use crate::value::Value;

/// The moment a program here sees as now: 2026-01-01T00:00:00Z.
pub const NOW: f64 = 1_767_225_600_000.0;

const DAY: f64 = 86_400_000.0;

/// Installs `Date`.
pub fn install(interp: &mut Interp) {
    let dp = interp.intrinsics.date_proto.clone();
    let date = constructor(
        interp,
        "Date",
        7,
        |i, _, a, made| {
            let Some(proto) = made else {
                return Ok(Value::str(&format_date(NOW)));
            };
            let t = match a {
                [] => NOW,
                [Value::Number(n)] => clip(*n),
                [Value::String(s)] => parse(&String::from_utf16_lossy(s)),
                [Value::Object(o)] if matches!(o.borrow().class, Class::Date(_)) => {
                    match &o.borrow().class {
                        Class::Date(t) => *t,
                        _ => f64::NAN,
                    }
                }
                [v] => {
                    let p = i.to_primitive(v, crate::interp::Hint::Default)?;
                    match p {
                        Value::String(s) => parse(&String::from_utf16_lossy(&s)),
                        other => clip(i.to_number(&other)?),
                    }
                }
                parts => from_parts(i, parts)?,
            };
            Ok(Value::Object(Obj::new(Some(proto.clone()), Class::Date(t))))
        },
        &dp,
    );
    method(interp, &date, "now", 0, |_, _, _, _| Ok(Value::Number(NOW)));
    method(interp, &date, "parse", 1, |i, _, a, _| {
        let s = i.to_rust_string(&arg(a, 0))?;
        Ok(Value::Number(parse(&s)))
    });
    method(interp, &date, "UTC", 7, |i, _, a, _| {
        Ok(Value::Number(from_parts(i, a)?))
    });
    method(interp, &dp, "getTime", 0, |i, this, _, _| {
        Ok(Value::Number(time(i, this)?))
    });
    method(interp, &dp, "valueOf", 0, |i, this, _, _| {
        Ok(Value::Number(time(i, this)?))
    });
    method(interp, &dp, "toISOString", 0, |i, this, _, _| {
        let t = time(i, this)?;
        if t.is_nan() {
            return Err(i.range_error("Invalid time value"));
        }
        let (y, mo, d, h, mi, s, ms) = parts(t);
        Ok(Value::str(&format!(
            "{y:04}-{:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z",
            mo + 1
        )))
    });
    method(interp, &dp, "toJSON", 1, |i, this, _, _| {
        let t = time(i, this)?;
        if t.is_nan() {
            return Ok(Value::Null);
        }
        let (y, mo, d, h, mi, s, ms) = parts(t);
        Ok(Value::str(&format!(
            "{y:04}-{:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z",
            mo + 1
        )))
    });
    for name in [
        "toString",
        "toUTCString",
        "toDateString",
        "toLocaleString",
        "toLocaleDateString",
        "toLocaleTimeString",
        "toTimeString",
        "toGMTString",
    ] {
        method(interp, &dp, name, 0, |i, this, _, _| {
            Ok(Value::str(&format_date(time(i, this)?)))
        });
    }
    macro_rules! get {
        ($($name:expr => $k:expr),*) => {
            $(
                method(interp, &dp, $name, 0, |i, this, _, _| {
                    let t = time(i, this)?;
                    if t.is_nan() { return Ok(Value::Number(f64::NAN)); }
                    let p = parts(t);
                    let f: fn((i64, i64, i64, i64, i64, i64, i64), f64) -> f64 = $k;
                    Ok(Value::Number(f(p, t)))
                });
            )*
        };
    }
    get!(
        "getFullYear" => |p, _| p.0 as f64, "getUTCFullYear" => |p, _| p.0 as f64,
        "getMonth" => |p, _| p.1 as f64, "getUTCMonth" => |p, _| p.1 as f64,
        "getDate" => |p, _| p.2 as f64, "getUTCDate" => |p, _| p.2 as f64,
        "getHours" => |p, _| p.3 as f64, "getUTCHours" => |p, _| p.3 as f64,
        "getMinutes" => |p, _| p.4 as f64, "getUTCMinutes" => |p, _| p.4 as f64,
        "getSeconds" => |p, _| p.5 as f64, "getUTCSeconds" => |p, _| p.5 as f64,
        "getMilliseconds" => |p, _| p.6 as f64, "getUTCMilliseconds" => |p, _| p.6 as f64,
        "getDay" => |_, t| ((t / DAY).floor() + 4.0).rem_euclid(7.0),
        "getUTCDay" => |_, t| ((t / DAY).floor() + 4.0).rem_euclid(7.0),
        "getTimezoneOffset" => |_, _| 0.0,
        "getYear" => |p, _| (p.0 - 1900) as f64
    );
    method(interp, &dp, "setTime", 1, |i, this, a, _| {
        let n = clip(i.to_number(&arg(a, 0))?);
        set_time(i, this, n)
    });
    for (name, field) in [
        ("setFullYear", 0),
        ("setMonth", 1),
        ("setDate", 2),
        ("setHours", 3),
        ("setMinutes", 4),
        ("setSeconds", 5),
        ("setMilliseconds", 6),
        ("setUTCFullYear", 0),
        ("setUTCMonth", 1),
        ("setUTCDate", 2),
        ("setUTCHours", 3),
        ("setUTCMinutes", 4),
        ("setUTCSeconds", 5),
        ("setUTCMilliseconds", 6),
    ] {
        let run: crate::object::NativeFn = match field {
            0 => |i, t, a, _| set_field(i, t, a, 0),
            1 => |i, t, a, _| set_field(i, t, a, 1),
            2 => |i, t, a, _| set_field(i, t, a, 2),
            3 => |i, t, a, _| set_field(i, t, a, 3),
            4 => |i, t, a, _| set_field(i, t, a, 4),
            5 => |i, t, a, _| set_field(i, t, a, 5),
            _ => |i, t, a, _| set_field(i, t, a, 6),
        };
        method(interp, &dp, name, 1, run);
    }
    let to_prim = crate::value::Key::Symbol(interp.intrinsics.sym_to_primitive.clone());
    super::symbol_method(
        interp,
        &dp,
        &to_prim,
        "[Symbol.toPrimitive]",
        |i, this, a, _| {
            let t = time(i, this)?;
            let hint = i.to_rust_string(&arg(a, 0))?;
            Ok(if hint == "number" {
                Value::Number(t)
            } else {
                Value::str(&format_date(t))
            })
        },
    );
}

fn time(i: &mut Interp, this: &Value) -> Res<f64> {
    if let Value::Object(o) = this {
        if let Class::Date(t) = &o.borrow().class {
            return Ok(*t);
        }
    }
    Err(i.type_error("this is not a Date object."))
}

fn set_time(i: &mut Interp, this: &Value, t: f64) -> Res<Value> {
    if let Value::Object(o) = this {
        if let Class::Date(old) = &mut o.borrow_mut().class {
            *old = t;
            return Ok(Value::Number(t));
        }
    }
    Err(i.type_error("this is not a Date object."))
}

fn set_field(i: &mut Interp, this: &Value, a: &[Value], field: usize) -> Res<Value> {
    let t = time(i, this)?;
    let (y, mo, d, h, mi, s, ms) = parts(if t.is_nan() { 0.0 } else { t });
    let mut f = [
        y as f64, mo as f64, d as f64, h as f64, mi as f64, s as f64, ms as f64,
    ];
    for (k, v) in a.iter().enumerate() {
        if field + k < 7 {
            f[field + k] = i.to_number(v)?;
        }
    }
    let n = clip(make_time(f[0], f[1], f[2], f[3], f[4], f[5], f[6]));
    set_time(i, this, n)
}

fn from_parts(i: &mut Interp, a: &[Value]) -> Res<f64> {
    let mut f = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (k, v) in a.iter().take(7).enumerate() {
        f[k] = i.to_number(v)?;
    }
    if (0.0..=99.0).contains(&f[0]) && f[0].fract() == 0.0 {
        f[0] += 1900.0;
    }
    Ok(clip(make_time(f[0], f[1], f[2], f[3], f[4], f[5], f[6])))
}

fn clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 {
        f64::NAN
    } else {
        t.trunc()
    }
}

/// Days from 1970-01-01 to `y`-`m`-01 (`m` from 0), by the civil calendar.
fn days_from_civil(y: f64, m: f64) -> f64 {
    let y = y + (m / 12.0).floor();
    let m = m.rem_euclid(12.0) + 1.0;
    let y = if m <= 2.0 { y - 1.0 } else { y };
    let era = (y / 400.0).floor();
    let yoe = y - era * 400.0;
    let mp = (m + 9.0) % 12.0;
    let doy = (153.0 * mp + 2.0) / 5.0;
    let doy = doy.floor();
    let doe = yoe * 365.0 + (yoe / 4.0).floor() - (yoe / 100.0).floor() + doy;
    era * 146097.0 + doe - 719468.0
}

fn make_time(y: f64, mo: f64, d: f64, h: f64, mi: f64, s: f64, ms: f64) -> f64 {
    if [y, mo, d, h, mi, s, ms].iter().any(|x| !x.is_finite()) {
        return f64::NAN;
    }
    (days_from_civil(y.trunc(), mo.trunc()) + d.trunc() - 1.0) * DAY
        + h.trunc() * 3_600_000.0
        + mi.trunc() * 60_000.0
        + s.trunc() * 1000.0
        + ms.trunc()
}

/// A time's year, month (from 0), day, hours, minutes, seconds, milliseconds.
fn parts(t: f64) -> (i64, i64, i64, i64, i64, i64, i64) {
    let days = (t / DAY).floor();
    let ms_in_day = t - days * DAY;
    let z = days + 719468.0;
    let era = (z / 146097.0).floor();
    let doe = z - era * 146097.0;
    let yoe = ((doe - (doe / 1460.0).floor() + (doe / 36524.0).floor() - (doe / 146096.0).floor())
        / 365.0)
        .floor();
    let y = yoe + era * 400.0;
    let doy = doe - (365.0 * yoe + (yoe / 4.0).floor() - (yoe / 100.0).floor());
    let mp = ((5.0 * doy + 2.0) / 153.0).floor();
    let d = doy - ((153.0 * mp + 2.0) / 5.0).floor() + 1.0;
    let m = if mp < 10.0 { mp + 3.0 } else { mp - 9.0 };
    let y = if m <= 2.0 { y + 1.0 } else { y };
    let ms = ms_in_day as i64;
    (
        y as i64,
        m as i64 - 1,
        d as i64,
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000,
    )
}

fn format_date(t: f64) -> String {
    if t.is_nan() {
        return "Invalid Date".into();
    }
    let (y, mo, d, h, mi, s, _) = parts(t);
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let wd = (((t / DAY).floor() + 4.0).rem_euclid(7.0)) as usize;
    format!(
        "{} {} {d:02} {y} {h:02}:{mi:02}:{s:02} GMT+0000 (Coordinated Universal Time)",
        DAYS[wd], MONTHS[mo as usize]
    )
}

/// An ISO date (`2026-01-02`, `2026-01-02T03:04:05.006Z`), as `Date.parse` reads one.
fn parse(s: &str) -> f64 {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<f64>().ok());
    let (Some(y), Some(mo), Some(d)) = (num(0, 4), num(5, 7), num(8, 10)) else {
        return f64::NAN;
    };
    if s.len() == 10 {
        return make_time(y, mo - 1.0, d, 0.0, 0.0, 0.0, 0.0);
    }
    let h = num(11, 13).unwrap_or(0.0);
    let mi = num(14, 16).unwrap_or(0.0);
    let sec = num(17, 19).unwrap_or(0.0);
    let ms = if s.get(19..20) == Some(".") {
        num(20, 23).unwrap_or(0.0)
    } else {
        0.0
    };
    let local = make_time(y, mo - 1.0, d, h, mi, sec, ms);
    // Its offset from UTC, after the time: `Z`, `+07:00`, `-0530`.
    let zone_at = s[10..].find(['Z', '+', '-']).map(|p| p + 10);
    match zone_at.map(|p| (p, &s[p..])) {
        Some((_, "Z")) | None => local,
        Some((_, zone)) => {
            let sign = if zone.starts_with('-') { -1.0 } else { 1.0 };
            let digits: String = zone[1..].chars().filter(|c| c.is_ascii_digit()).collect();
            let (zh, zm) = (
                digits.get(0..2).and_then(|x| x.parse::<f64>().ok()),
                digits
                    .get(2..4)
                    .and_then(|x| x.parse::<f64>().ok())
                    .unwrap_or(0.0),
            );
            match zh {
                Some(zh) => local - sign * (zh * 3_600_000.0 + zm * 60_000.0),
                None => f64::NAN,
            }
        }
    }
}
