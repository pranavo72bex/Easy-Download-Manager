// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A number written as JavaScript writes it (`String(n)`): the fewest digits that read back as
//! the same number, in plain form from 1e-7 to 1e21 and with an exponent outside it.

/// `n` as `String(n)` gives it.
pub fn to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n < 0.0 {
        return format!("-{}", to_string(-n));
    }
    // Rust's `{:e}` writes the shortest digits that read back as `n`: `d.ddde±x`.
    let sci = format!("{n:e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    // n = 0.digits × 10^point
    let point = exponent.parse::<i32>().unwrap_or(0) + 1;
    if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let e = point - 1;
        let sign = if e < 0 { '-' } else { '+' };
        if k == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_string;

    #[test]
    fn numbers_read_as_javascript_writes_them() {
        for (n, written) in [
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456.789, "123456.789"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-10, "1.5e-10"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (-0.0, "0"),
            (4294967295.0, "4294967295"),
        ] {
            assert_eq!(to_string(n), written, "{n}");
        }
    }
}
