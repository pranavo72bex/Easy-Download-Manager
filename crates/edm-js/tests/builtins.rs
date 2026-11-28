// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Each built-in object's methods, one by one: each program's last value, written as `String`
//! writes it, against what an engine of a browser gives (local time is UTC in EDM's
//! interpreter, so a date reads the same on every computer). A player check or a proxy
//! script that leans on a method nobody tested would fail only on the day a site starts
//! calling it.

use edm_js::interp::Interp;

fn show(i: &mut Interp, r: Result<edm_js::value::Value, edm_js::value::Value>) -> String {
    match r {
        Ok(v) => i
            .to_rust_string(&v)
            .unwrap_or_else(|_| "<no string>".into()),
        Err(e) => {
            let s = i
                .to_rust_string(&e)
                .unwrap_or_else(|_| "<no string>".into());
            format!("threw {s}")
        }
    }
}

fn run(src: &str) -> String {
    let mut i = Interp::new();
    let r = i.run(src);
    show(&mut i, r)
}

/// `src` runs, its jobs (promises) run after it, then `read` is read.
fn after(src: &str, read: &str) -> String {
    let mut i = Interp::new();
    if let Err(e) = i.run(src) {
        let s = i.to_rust_string(&e).unwrap_or_default();
        return format!("threw {s}");
    }
    let r = i.run(read);
    show(&mut i, r)
}

fn check(cases: &[(&str, &str)]) {
    let mut wrong = Vec::new();
    for (src, want) in cases {
        let got = run(src);
        if got != *want {
            wrong.push(format!("{src}\n  want {want}\n  got  {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// The start of what a program threw: its kind of error, without the engine's own words.
fn throws(src: &str, kind: &str) {
    let got = run(src);
    assert!(
        got.starts_with(&format!("threw {kind}")),
        "{src}\n  want a {kind}\n  got  {got}"
    );
}

#[test]
fn arrays() {
    check(&[
        ("[1, 2, 3].at(-1)", "3"),
        ("[1, 2, 3].at(5)", "undefined"),
        ("[1, 2].concat([3, [4]], 5).length", "5"),
        ("[1, 2, 3, 4, 5].copyWithin(0, 3).join()", "4,5,3,4,5"),
        ("[1, 2, 3, 4, 5].copyWithin(1, 0, 2).join()", "1,1,2,4,5"),
        ("Array.from([...[1, 2].entries()]).join(';')", "0,1;1,2"),
        ("[2, 4].every(x => x % 2 == 0)", "true"),
        ("[2, 3].every(x => x % 2 == 0)", "false"),
        ("new Array(3).fill(7).join()", "7,7,7"),
        ("[1, 2, 3, 4].fill(0, 1, 3).join()", "1,0,0,4"),
        ("[1, 2, 3, 4].filter(x => x > 2).join()", "3,4"),
        ("[5, 12, 8].find(x => x > 6)", "12"),
        ("[5, 12, 8].findIndex(x => x > 6)", "1"),
        ("[5, 12, 8].findLast(x => x > 6)", "8"),
        ("[5, 12, 8].findLastIndex(x => x > 6)", "2"),
        ("[5, 12, 8].findIndex(x => x > 60)", "-1"),
        ("[1, [2, [3, [4]]]].flat().length", "3"),
        ("[1, [2, [3, [4]]]].flat(Infinity).join()", "1,2,3,4"),
        ("[1, 2].flatMap(x => [x, x * 10]).join()", "1,10,2,20"),
        ("var s = 0; [1, 2, 3].forEach(x => s += x); s", "6"),
        ("Array.from('abc').join('-')", "a-b-c"),
        ("Array.from({ length: 3 }, (_, i) => i * 2).join()", "0,2,4"),
        ("Array.from(new Set([1, 1, 2])).join()", "1,2"),
        ("[1, NaN].includes(NaN)", "true"),
        ("[1, 2, 3].includes(2, 2)", "false"),
        ("[1, 2, 1].indexOf(1, 1)", "2"),
        ("[1, 2, 1].lastIndexOf(1)", "2"),
        ("[1, 2, 1].lastIndexOf(1, 1)", "0"),
        ("Array.isArray([])", "true"),
        ("Array.isArray({ length: 0 })", "false"),
        ("[1, null, undefined, 2].join('|')", "1|||2"),
        ("[...[7, 8].keys()].join()", "0,1"),
        ("[1, 2, 3].map((x, i) => x * i).join()", "0,2,6"),
        ("Array.of(7).length", "1"),
        ("Array.of(1, 2, 3).join()", "1,2,3"),
        ("var a = [1, 2]; a.pop() + ':' + a.length", "2:1"),
        ("[].pop()", "undefined"),
        ("var a = [1]; a.push(2, 3) + ':' + a.join()", "3:1,2,3"),
        ("[1, 2, 3].reduce((a, b) => a + b)", "6"),
        ("[1, 2, 3].reduce((a, b) => a + b, 10)", "16"),
        ("['a', 'b', 'c'].reduceRight((a, b) => a + b)", "cba"),
        ("[1, 2, 3].reverse().join()", "3,2,1"),
        ("var a = [1, 2, 3]; a.shift() + ':' + a.join()", "1:2,3"),
        ("[1, 2, 3, 4].slice(1, -1).join()", "2,3"),
        ("[1, 2, 3, 4].slice(-2).join()", "3,4"),
        ("[0, 5].some(x => x > 4)", "true"),
        ("[0, 1].some(x => x > 4)", "false"),
        ("[10, 9, 1, 100].sort().join()", "1,10,100,9"),
        ("[10, 9, 1, 100].sort((a, b) => a - b).join()", "1,9,10,100"),
        ("['b', undefined, 'a'].sort().join()", "a,b,"),
        (
            "var a = [1, 2, 3, 4, 5]; var r = a.splice(1, 2, 'x', 'y', 'z'); r.join() + ':' + a.join()",
            "2,3:1,x,y,z,4,5",
        ),
        ("var a = [1, 2, 3]; a.splice(-1); a.join()", "1,2"),
        ("var a = [3]; a.unshift(1, 2) + ':' + a.join()", "3:1,2,3"),
        ("[...['p', 'q'].values()].join()", "p,q"),
        ("[1, [2, 3]].toString()", "1,2,3"),
        ("[1, 2].toLocaleString()", "1,2"),
        ("var a = [1, 2, 3]; a.length = 1; a.join()", "1"),
        ("Array(3).length", "3"),
        ("Array(1, 2).join()", "1,2"),
        ("[,1].length", "2"),
    ]);
    throws("[].reduce((a, b) => a)", "TypeError");
}

#[test]
fn strings() {
    check(&[
        ("'abc'.at(-1)", "c"),
        ("'abc'.charAt(1)", "b"),
        ("'abc'.charAt(9)", ""),
        ("'abc'.charCodeAt(0)", "97"),
        ("isNaN('abc'.charCodeAt(9))", "true"),
        ("'😀'.codePointAt(0)", "128512"),
        ("'😀'.length", "2"),
        ("'a'.concat('b', 1)", "ab1"),
        ("'hello'.endsWith('lo')", "true"),
        ("'hello'.endsWith('hel', 3)", "true"),
        ("String.fromCharCode(72, 105)", "Hi"),
        ("String.fromCodePoint(128512) === '😀'", "true"),
        ("'hello'.includes('ell')", "true"),
        ("'hello'.includes('ell', 2)", "false"),
        ("'banana'.indexOf('a', 2)", "3"),
        ("'banana'.lastIndexOf('a')", "5"),
        ("'banana'.lastIndexOf('a', 4)", "3"),
        ("'ab'.isWellFormed()", "true"),
        ("'a'.localeCompare('b')", "-1"),
        ("'b'.localeCompare('a')", "1"),
        ("'a'.localeCompare('a')", "0"),
        ("'a1b22'.match(/\\d+/g).join()", "1,22"),
        ("'a1b22'.match(/(\\d)(\\d)/)[2]", "2"),
        ("'abc'.match(/x/)", "null"),
        (
            "[...'a1b2'.matchAll(/[a-z](\\d)/g)].map(m => m[1] + '@' + m.index).join()",
            "1@0,2@2",
        ),
        ("'é'.normalize('NFC').length", "1"),
        ("'5'.padStart(3, '0')", "005"),
        ("'5'.padEnd(4, 'ab')", "5aba"),
        ("'abc'.padStart(2)", "abc"),
        ("String.raw`a\\nb`", "a\\nb"),
        ("'ab'.repeat(3)", "ababab"),
        ("'aaa'.replace('a', 'b')", "baa"),
        ("'aaa'.replace(/a/g, 'b')", "bbb"),
        (
            "'john smith'.replace(/(\\w+) (\\w+)/, '$2, $1')",
            "smith, john",
        ),
        ("'abc'.replace(/b/, m => m.toUpperCase())", "aBc"),
        ("'a-b'.replace('-', '$&$&')", "a--b"),
        ("'aXbXc'.replaceAll('X', '.')", "a.b.c"),
        ("'aXbXc'.replaceAll(/X/g, (m, i) => i)", "a1b3c"),
        ("'hello'.search(/l/)", "2"),
        ("'hello'.search(/z/)", "-1"),
        ("'hello'.slice(1, -1)", "ell"),
        ("'hello'.slice(-3)", "llo"),
        ("'a,b,,c'.split(',').length", "4"),
        ("'a,b,c'.split(',', 2).join('|')", "a|b"),
        ("'abc'.split('').join('|')", "a|b|c"),
        ("'a1b2c'.split(/\\d/).join('|')", "a|b|c"),
        ("'a1b'.split(/(\\d)/).join('|')", "a|1|b"),
        ("'abc'.startsWith('b', 1)", "true"),
        ("'abcdef'.substr(2, 3)", "cde"),
        ("'abcdef'.substr(-2)", "ef"),
        ("'abcdef'.substring(4, 1)", "bcd"),
        ("'ABC'.toLowerCase()", "abc"),
        ("'abc'.toUpperCase()", "ABC"),
        ("'ÀB'.toLocaleLowerCase()", "àb"),
        ("'àb'.toLocaleUpperCase()", "ÀB"),
        ("'  x  '.trim() + '|'", "x|"),
        ("'  x  '.trimStart() + '|'", "x  |"),
        ("'  x  '.trimEnd() + '|'", "  x|"),
        ("'  x'.trimLeft()", "x"),
        ("'x  '.trimRight() + '|'", "x|"),
        ("new String('s').valueOf()", "s"),
        ("new String('s').toString()", "s"),
        ("typeof new String('s')", "object"),
        (
            "String(null) + String(undefined) + String(12)",
            "nullundefined12",
        ),
        ("'abc'[1]", "b"),
        ("'abc'.length", "3"),
    ]);
}

#[test]
fn numbers_and_math() {
    check(&[
        ("(255).toString(16)", "ff"),
        ("(255).toString(2)", "11111111"),
        ("(-255).toString(36)", "-73"),
        ("(3.14159).toFixed(2)", "3.14"),
        ("(0.5).toFixed(0)", "1"),
        ("(2.5).toFixed(0)", "3"),
        ("(-2.5).toFixed(0)", "-3"),
        ("(1.005).toFixed(2)", "1.00"),
        ("(1.25).toFixed(1)", "1.3"),
        ("(9.995).toFixed(2)", "9.99"),
        ("(99.5).toFixed(0)", "100"),
        ("(-0.0001).toFixed(2)", "-0.00"),
        ("(-0).toFixed(1)", "0.0"),
        ("(123).toFixed()", "123"),
        ("(1e21).toFixed(2)", "1e+21"),
        ("(1234.5).toFixed(3)", "1234.500"),
        ("(123.456).toExponential(2)", "1.23e+2"),
        ("(0.00015).toExponential(1)", "1.5e-4"),
        ("(123.456).toPrecision(4)", "123.5"),
        ("(0.000123).toPrecision(2)", "0.00012"),
        ("(123456).toPrecision(2)", "1.2e+5"),
        ("(5).valueOf()", "5"),
        ("Number('  42  ')", "42"),
        ("Number('0x1f')", "31"),
        ("Number('')", "0"),
        ("Number('abc')", "NaN"),
        ("Number(true)", "1"),
        ("Number.isInteger(5.0)", "true"),
        ("Number.isInteger(5.5)", "false"),
        ("Number.isSafeInteger(2 ** 53)", "false"),
        ("Number.isSafeInteger(2 ** 53 - 1)", "true"),
        ("Number.isNaN('abc')", "false"),
        ("isNaN('abc')", "true"),
        ("Number.isFinite('5')", "false"),
        ("isFinite('5')", "true"),
        ("Number.MAX_SAFE_INTEGER", "9007199254740991"),
        ("Number.MIN_SAFE_INTEGER", "-9007199254740991"),
        ("Number.EPSILON > 0 && Number.EPSILON < 1e-15", "true"),
        ("Number.MAX_VALUE > 1e308", "true"),
        ("Number.MIN_VALUE > 0", "true"),
        ("Number.POSITIVE_INFINITY", "Infinity"),
        ("Number.NEGATIVE_INFINITY", "-Infinity"),
        ("parseInt('42px')", "42"),
        ("parseInt('ff', 16)", "255"),
        ("parseInt('0x10')", "16"),
        ("parseInt('  -7')", "-7"),
        ("parseInt('z')", "NaN"),
        ("parseFloat('3.5e2abc')", "350"),
        ("parseFloat('.5')", "0.5"),
        ("Number.parseInt === parseInt", "true"),
        ("Number.parseFloat === parseFloat", "true"),
        ("Number.parseInt('12', 8)", "10"),
        ("Math.abs(-3)", "3"),
        ("Math.ceil(1.1)", "2"),
        ("Math.floor(-1.1)", "-2"),
        ("Math.round(2.5)", "3"),
        ("Math.round(-2.5)", "-2"),
        ("Math.trunc(-4.7)", "-4"),
        ("Math.sign(-3)", "-1"),
        ("Math.max(1, 5, 3)", "5"),
        ("Math.max()", "-Infinity"),
        ("Math.min(1, 5, 3)", "1"),
        ("Math.min(1, NaN)", "NaN"),
        ("Math.pow(2, 10)", "1024"),
        ("Math.sqrt(16)", "4"),
        ("Math.cbrt(27)", "3"),
        ("Math.hypot(3, 4)", "5"),
        ("Math.clz32(1)", "31"),
        ("Math.imul(3, 4)", "12"),
        ("Math.imul(0xffffffff, 5)", "-5"),
        ("Math.fround(5.5)", "5.5"),
        ("Math.fround(5.05) === 5.05", "false"),
        ("Math.log(Math.E)", "1"),
        ("Math.log2(8)", "3"),
        ("Math.log10(1000)", "3"),
        ("Math.log1p(0)", "0"),
        ("Math.exp(0)", "1"),
        ("Math.expm1(0)", "0"),
        ("Math.sin(0) + Math.cos(0) + Math.tan(0)", "1"),
        ("Math.asin(1) * 2 === Math.PI", "true"),
        ("Math.acos(1)", "0"),
        ("Math.atan(1) * 4 === Math.PI", "true"),
        ("Math.atan2(1, 1) * 4 === Math.PI", "true"),
        ("Math.sinh(0) + Math.cosh(0) + Math.tanh(0)", "1"),
        ("Math.asinh(0) + Math.acosh(1) + Math.atanh(0)", "0"),
        ("Math.SQRT2 * Math.SQRT1_2 > 0.99", "true"),
        ("Math.LN2 > 0.69 && Math.LN10 > 2.3", "true"),
        ("Math.LOG2E > 1.44 && Math.LOG10E > 0.43", "true"),
        ("var r = Math.random(); r >= 0 && r < 1", "true"),
        ("(1e21).toString()", "1e+21"),
        ("(1.5e-7).toString()", "1.5e-7"),
        ("(-0).toString()", "0"),
        ("1 / -0", "-Infinity"),
        ("(12345.678).toLocaleString().length > 0", "true"),
        ("typeof new Number(5)", "object"),
    ]);
    throws("(1).toFixed(101)", "RangeError");
    throws("(1).toString(1)", "RangeError");
}

#[test]
fn big_integers() {
    check(&[
        ("(2n ** 64n).toString()", "18446744073709551616"),
        ("BigInt(10) * 3n", "30"),
        (
            "BigInt('123456789012345678901234567890') + 1n",
            "123456789012345678901234567891",
        ),
        ("typeof 1n", "bigint"),
        ("BigInt.asUintN(8, 257n)", "1"),
        ("BigInt.asIntN(8, 255n)", "-1"),
        ("(255n).toString(16)", "ff"),
        ("7n / 2n", "3"),
        ("-7n % 3n", "-1"),
        ("1n < 2", "true"),
        ("1n == 1", "true"),
        ("1n === 1", "false"),
    ]);
    throws("1n + 1", "TypeError");
}

#[test]
fn dates() {
    check(&[
        ("new Date(0).toISOString()", "1970-01-01T00:00:00.000Z"),
        ("new Date(Date.UTC(2020, 1, 29, 12, 30, 15, 250)).toISOString()", "2020-02-29T12:30:15.250Z"),
        ("Date.UTC(2000, 0, 1)", "946684800000"),
        ("Date.parse('2001-02-03T04:05:06Z')", "981173106000"),
        ("Date.parse('2001-02-03T04:05:06+02:00')", "981165906000"),
        ("Date.parse('2001-02-03')", "981158400000"),
        ("isNaN(Date.parse('not a date'))", "true"),
        ("new Date('2001-02-03T04:05:06Z').getTime()", "981173106000"),
        ("var d = new Date(981173106007); [d.getFullYear(), d.getMonth(), d.getDate(), d.getDay(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds()].join()", "2001,1,3,6,4,5,6,7"),
        ("var d = new Date(981173106007); [d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate(), d.getUTCDay(), d.getUTCHours(), d.getUTCMinutes(), d.getUTCSeconds(), d.getUTCMilliseconds()].join()", "2001,1,3,6,4,5,6,7"),
        ("new Date(981173106000).getYear()", "101"),
        ("new Date(0).getTimezoneOffset()", "0"),
        ("new Date(2001, 1, 3, 4, 5, 6).toISOString()", "2001-02-03T04:05:06.000Z"),
        ("new Date(2001, 13, 1).toISOString()", "2002-02-01T00:00:00.000Z"),
        ("var d = new Date(0); d.setFullYear(2010); d.setMonth(5); d.setDate(15); d.setHours(10); d.setMinutes(20); d.setSeconds(30); d.setMilliseconds(40); d.toISOString()", "2010-06-15T10:20:30.040Z"),
        ("var d = new Date(0); d.setUTCFullYear(2011); d.setUTCMonth(6); d.setUTCDate(16); d.setUTCHours(11); d.setUTCMinutes(21); d.setUTCSeconds(31); d.setUTCMilliseconds(41); d.toISOString()", "2011-07-16T11:21:31.041Z"),
        ("var d = new Date(0); d.setTime(1000); d.getTime()", "1000"),
        ("var d = new Date(0); d.setDate(32); d.toISOString()", "1970-02-01T00:00:00.000Z"),
        ("new Date(0).valueOf()", "0"),
        ("new Date(0).toJSON()", "1970-01-01T00:00:00.000Z"),
        ("new Date(0).toUTCString()", "Thu, 01 Jan 1970 00:00:00 GMT"),
        ("new Date(0).toGMTString()", "Thu, 01 Jan 1970 00:00:00 GMT"),
        ("new Date(0).toDateString()", "Thu Jan 01 1970"),
        ("new Date(981173106000).toUTCString()", "Sat, 03 Feb 2001 04:05:06 GMT"),
        ("new Date(0).toString().slice(0, 24)", "Thu Jan 01 1970 00:00:00"),
        ("new Date(0).toTimeString()", "00:00:00 GMT+0000 (Coordinated Universal Time)"),
        ("new Date(NaN).toUTCString() + new Date(NaN).toDateString()", "Invalid DateInvalid Date"),
        ("new Date(0).toLocaleDateString().length > 0", "true"),
        ("new Date(0).toLocaleTimeString().length > 0", "true"),
        ("new Date(0).toLocaleString().length > 0", "true"),
        ("typeof Date.now()", "number"),
        ("Date.now() > 1.7e12", "true"),
        ("typeof Date()", "string"),
        ("new Date(new Date(5)).getTime()", "5"),
        ("isNaN(new Date('x').getTime())", "true"),
        ("new Date(8.64e15 + 1).getTime()", "NaN"),
        ("new Date(1e12) - new Date(0)", "1000000000000"),
        ("JSON.stringify({ d: new Date(0) })", "{\"d\":\"1970-01-01T00:00:00.000Z\"}"),
    ]);
    throws("new Date(NaN).toISOString()", "RangeError");
}

#[test]
fn json() {
    check(&[
        (
            "JSON.stringify({ a: 1, b: [true, null, 'x'] })",
            "{\"a\":1,\"b\":[true,null,\"x\"]}",
        ),
        ("JSON.stringify('a\"b\\n')", "\"a\\\"b\\n\""),
        ("JSON.stringify(undefined)", "undefined"),
        ("JSON.stringify({ u: undefined, f() {} })", "{}"),
        ("JSON.stringify([undefined, function () {}])", "[null,null]"),
        ("JSON.stringify(NaN)", "null"),
        ("JSON.stringify({ a: 1, b: 2 }, ['b'])", "{\"b\":2}"),
        (
            "JSON.stringify({ a: 1, b: 2 }, (k, v) => k == 'a' ? undefined : v)",
            "{\"b\":2}",
        ),
        (
            "JSON.stringify({ a: [1] }, null, 2)",
            "{\n  \"a\": [\n    1\n  ]\n}",
        ),
        ("JSON.stringify({ a: 1 }, null, '--')", "{\n--\"a\": 1\n}"),
        ("JSON.stringify({ toJSON() { return 5; } })", "5"),
        ("JSON.stringify('\\u0001')", "\"\\u0001\""),
        ("JSON.stringify(new String('s'))", "\"s\""),
        ("JSON.parse('{\"a\":[1,2,{\"b\":null}]}').a[2].b", "null"),
        ("JSON.parse(' 12.5e1 ')", "125"),
        ("JSON.parse('\"\\\\u0041\\\\n\"')", "A\n"),
        ("JSON.parse('true') && !JSON.parse('false')", "true"),
        (
            "JSON.parse('[1,2]', (k, v) => typeof v == 'number' ? v * 10 : v).join()",
            "10,20",
        ),
        ("Object.keys(JSON.parse('{\"b\":1,\"a\":2}')).join()", "b,a"),
        ("JSON.parse('-0') === 0", "true"),
    ]);
    throws("JSON.parse('{a:1}')", "SyntaxError");
    throws("JSON.parse('[1,]')", "SyntaxError");
    throws("JSON.parse('')", "SyntaxError");
    throws("var a = []; a.push(a); JSON.stringify(a)", "TypeError");
}

#[test]
fn objects_and_reflection() {
    check(&[
        ("Object.assign({ a: 1 }, { b: 2 }, null, { a: 3 }).a", "3"),
        ("var p = { x: 1 }; var o = Object.create(p); o.x", "1"),
        ("Object.getPrototypeOf(Object.create(null))", "null"),
        ("var o = {}; Object.defineProperty(o, 'k', { value: 5 }); o.k + ':' + Object.keys(o).length", "5:0"),
        ("var o = {}; Object.defineProperty(o, 'k', { get() { return 9; } }); o.k", "9"),
        ("var o = {}; Object.defineProperties(o, { a: { value: 1, enumerable: true }, b: { value: 2 } }); JSON.stringify(o)", "{\"a\":1}"),
        ("Object.entries({ a: 1, b: 2 }).join(';')", "a,1;b,2"),
        ("Object.values({ a: 1, b: 2 }).join()", "1,2"),
        ("Object.fromEntries([['a', 1], ['b', 2]]).b", "2"),
        ("Object.fromEntries(new Map([['k', 'v']])).k", "v"),
        ("var o = Object.freeze({ a: 1 }); o.a = 2; o.a + ':' + Object.isFrozen(o)", "1:true"),
        ("var o = Object.seal({ a: 1 }); o.b = 2; delete o.a; o.a + ':' + o.b + ':' + Object.isSealed(o)", "1:undefined:true"),
        ("var o = Object.preventExtensions({}); o.z = 1; o.z + ':' + Object.isExtensible(o)", "undefined:false"),
        ("Object.isExtensible({})", "true"),
        ("JSON.stringify(Object.getOwnPropertyDescriptor({ a: 1 }, 'a'))", "{\"value\":1,\"writable\":true,\"enumerable\":true,\"configurable\":true}"),
        ("Object.keys(Object.getOwnPropertyDescriptors({ a: 1, b: 2 })).join()", "a,b"),
        ("Object.getOwnPropertyNames([1]).join()", "0,length"),
        ("var s = Symbol('t'); var o = {}; o[s] = 1; Object.getOwnPropertySymbols(o).length", "1"),
        ("Object.hasOwn({ a: 1 }, 'a')", "true"),
        ("({ a: 1 }).hasOwnProperty('toString')", "false"),
        ("Object.is(NaN, NaN) + ':' + Object.is(0, -0)", "true:false"),
        ("var o = {}; Object.setPrototypeOf(o, { y: 2 }); o.y", "2"),
        ("Object.prototype.isPrototypeOf.call(Array.prototype, [])", "true"),
        ("({ a: 1 }).propertyIsEnumerable('a')", "true"),
        ("[].propertyIsEnumerable('length')", "false"),
        ("({}).toString()", "[object Object]"),
        ("Object.prototype.toString.call([])", "[object Array]"),
        ("Object.prototype.toString.call(null)", "[object Null]"),
        ("Object.prototype.toString.call(new Date(0))", "[object Date]"),
        ("({ [Symbol.toStringTag]: 'Mine' }).toString()", "[object Mine]"),
        ("({}).toLocaleString()", "[object Object]"),
        ("var o = { valueOf() { return 4; } }; o * 2", "8"),
        ("var o = { [Symbol.toPrimitive](h) { return h; } }; `${o}` + (o + '')", "stringdefault"),
        ("var o = {}; o.__defineGetter__('g', () => 3); o.g + ':' + typeof o.__lookupGetter__('g')", "3:function"),
        ("({ __proto__: { z: 1 } }).z", "1"),
        ("Reflect.has({ a: 1 }, 'a')", "true"),
        ("Reflect.get({ a: 7 }, 'a')", "7"),
        ("var o = {}; Reflect.set(o, 'a', 3); o.a", "3"),
        ("Reflect.ownKeys({ a: 1, [Symbol('s')]: 2 }).length", "2"),
        ("var o = { a: 1 }; Reflect.deleteProperty(o, 'a'); 'a' in o", "false"),
        ("Reflect.apply(Math.max, null, [1, 3, 2])", "3"),
        ("class P { constructor(x) { this.x = x; } } Reflect.construct(P, [4]).x", "4"),
        ("var o = {}; Reflect.defineProperty(o, 'q', { value: 1 }); o.q", "1"),
        ("Reflect.getPrototypeOf([]) === Array.prototype", "true"),
        ("var o = {}; Reflect.setPrototypeOf(o, Array.prototype); o instanceof Array", "true"),
        ("Reflect.isExtensible(Object.freeze({}))", "false"),
        ("var o = {}; Reflect.preventExtensions(o); Object.isExtensible(o)", "false"),
        ("JSON.stringify(Reflect.getOwnPropertyDescriptor({ a: 1 }, 'a').value)", "1"),
        ("new Boolean(false) ? 'truthy' : 'falsy'", "truthy"),
        ("Boolean('') + ':' + Boolean('x')", "false:true"),
        ("(true).toString()", "true"),
        ("typeof Object(1)", "object"),
    ]);
    throws("Object.defineProperty(1, 'a', {})", "TypeError");
}

#[test]
fn errors() {
    check(&[
        ("new Error('m').message", "m"),
        ("new Error('m').name", "Error"),
        ("String(new TypeError('t'))", "TypeError: t"),
        ("new RangeError('r') instanceof Error", "true"),
        ("new SyntaxError('s').name", "SyntaxError"),
        ("new ReferenceError('x').name", "ReferenceError"),
        ("new EvalError('x').name", "EvalError"),
        ("new URIError('x').name", "URIError"),
        ("new Error('m', { cause: 5 }).cause", "5"),
        ("var e = new AggregateError([1, 2], 'many'); e.errors.length + ':' + e.message", "2:many"),
        ("var e = AggregateError(new Set(['a']), 'm', { cause: 'c' }); e.errors[0] + e.cause + (e instanceof AggregateError)", "actrue"),
        ("AggregateError.length", "2"),
        ("new AggregateError([]).name", "AggregateError"),
        ("typeof new Error('x').stack", "string"),
        ("var r; try { null.x } catch (e) { r = e instanceof TypeError } r", "true"),
        ("var r; try { undefinedThing } catch (e) { r = e.name } r", "ReferenceError"),
        ("Error('no new').message", "no new"),
        ("class MyErr extends Error {} new MyErr('q') instanceof Error", "true"),
    ]);
}

#[test]
fn functions() {
    check(&[
        (
            "function f(a, b) { return this.k + a + b; } f.call({ k: 1 }, 2, 3)",
            "6",
        ),
        (
            "function f(a, b) { return this.k + a + b; } f.apply({ k: 1 }, [2, 3])",
            "6",
        ),
        (
            "function f(a, b) { return this.k + a + b; } f.bind({ k: 1 }, 2)(3)",
            "6",
        ),
        ("function f(a, b) {} f.length", "2"),
        ("function named() {} named.name", "named"),
        ("(function () {}).bind().name", "bound "),
        ("typeof Function.prototype", "function"),
        ("new Function('a', 'b', 'return a * b')(3, 4)", "12"),
        ("function f() { return arguments.length; } f(1, 2, 3)", "3"),
        (
            "function f() { return Object.prototype.toString.call(arguments); } f()",
            "[object Arguments]",
        ),
        (
            "(function (a) { return a; }).toString().includes('return a')",
            "true",
        ),
        (
            "Function.prototype[Symbol.hasInstance].call(Array, [])",
            "true",
        ),
    ]);
}

#[test]
fn symbols() {
    check(&[
        ("Symbol('d').description", "d"),
        ("Symbol('d').toString()", "Symbol(d)"),
        ("Symbol.for('k') === Symbol.for('k')", "true"),
        ("Symbol.keyFor(Symbol.for('k'))", "k"),
        ("Symbol.keyFor(Symbol('k'))", "undefined"),
        ("typeof Symbol.iterator", "symbol"),
        ("typeof Symbol.asyncIterator", "symbol"),
        ("[Symbol.isConcatSpreadable, Symbol.match, Symbol.matchAll, Symbol.replace, Symbol.search, Symbol.split, Symbol.species, Symbol.unscopables].every(s => typeof s == 'symbol')", "true"),
        ("var o = { [Symbol.iterator]() { var n = 0; return { next: () => ({ value: ++n, done: n > 2 }) }; } }; [...o].join()", "1,2"),
        ("var a = [1]; a[Symbol.isConcatSpreadable] = false; [0].concat(a).length", "2"),
    ]);
    throws("new Symbol()", "TypeError");
}

#[test]
fn patterns_of_the_regexp_object() {
    check(&[
        ("var r = /a(b)?/g; var m = r.exec('xab'); m.index + ':' + m[1] + ':' + r.lastIndex", "1:b:3"),
        ("/a/.exec('b')", "null"),
        ("var r = /o/g; r.test('foo') + ':' + r.lastIndex", "true:2"),
        ("/(?<y>\\d{4})-(?<m>\\d\\d)/.exec('on 2020-05').groups.m", "05"),
        ("/a/gimsuy.flags", "gimsuy"),
        ("/a/d.hasIndices", "true"),
        ("var r = /a/gims; [r.global, r.ignoreCase, r.multiline, r.dotAll, r.sticky, r.unicode].join()", "true,true,true,true,false,false"),
        ("/a\\/b/.source", "a\\/b"),
        ("String(/x+/g)", "/x+/g"),
        ("new RegExp('a+', 'g').toString()", "/a+/g"),
        ("new RegExp(/b/g).flags", "g"),
        ("new RegExp('').source", "(?:)"),
        ("var r = /y/y; r.lastIndex = 1; r.test('xy')", "true"),
        ("var r = /y/y; r.test('xy')", "false"),
        ("/A/i.test('a')", "true"),
        ("/^b$/m.test('a\\nb')", "true"),
        ("/a.b/s.test('a\\nb')", "true"),
        ("/\\u{1F600}/u.test('😀')", "true"),
        ("var m = /b/.exec('abc'); m.input + ':' + m.index", "abc:1"),
        ("/(a)|(b)/.exec('b')[1]", "undefined"),
        ("/(\\w)\\1/.test('aa')", "true"),
        ("/(?=a)a/.test('a') + ':' + /(?!a)b/.test('b')", "true:true"),
        ("/(?<=\\$)\\d+/.exec('$42')[0]", "42"),
        ("/[^abc]/.exec('abcd')[0]", "d"),
        ("/a{2,3}/.exec('aaaa')[0]", "aaa"),
        ("/a+?/.exec('aaa')[0]", "a"),
        ("/\\bfoo\\b/.test('a foo b')", "true"),
        ("/[\\s\\S]/.test('\\n')", "true"),
    ]);
    throws("new RegExp('(')", "SyntaxError");
    throws("new RegExp('a', 'q')", "SyntaxError");
}

#[test]
fn typed_arrays_and_buffers() {
    check(&[
        ("new Uint8Array([1, 2, 300]).join()", "1,2,44"),
        ("new Int8Array([127, 128, -129]).join()", "127,-128,127"),
        ("new Uint16Array([65536 + 5]).join()", "5"),
        ("new Int16Array([32768]).join()", "-32768"),
        ("new Uint32Array([-1]).join()", "4294967295"),
        ("new Int32Array([2 ** 31]).join()", "-2147483648"),
        ("new Float32Array([1.5, 0.1])[1] === 0.1", "false"),
        ("new Float64Array([0.1])[0] === 0.1", "true"),
        ("Uint32Array.BYTES_PER_ELEMENT + ':' + new Float64Array(2).BYTES_PER_ELEMENT", "4:8"),
        ("var b = new ArrayBuffer(8); b.byteLength", "8"),
        ("var b = new ArrayBuffer(8); var a = new Uint8Array(b, 2, 4); a.length + ':' + a.byteOffset + ':' + a.byteLength", "4:2:4"),
        ("var a = new Uint8Array(4); new Uint8Array(a.buffer)[1] = 6; a[1] + ':' + a.buffer.byteLength", "6:4"),
        ("var b = new ArrayBuffer(4); new Uint8Array(b)[0] = 1; new Uint32Array(b)[0]", "1"),
        ("new Uint8Array(new ArrayBuffer(8).slice(2, 6)).length", "4"),
        ("var a = new Uint8Array([1, 2, 3, 4]); var s = a.subarray(1, 3); s[0] = 9; a.join()", "1,9,3,4"),
        ("new Uint8Array([1, 2, 3, 4]).slice(1, 3).join()", "2,3"),
        ("var a = new Uint8Array(4); a.set([7, 8], 1); a.join()", "0,7,8,0"),
        ("var a = new Uint8Array(4); a.set(new Uint8Array([5, 6]), 2); a.join()", "0,0,5,6"),
        ("new Uint8Array([3, 1, 2]).sort().join()", "1,2,3"),
        ("new Int8Array([3, -1, 2]).sort((a, b) => b - a).join()", "3,2,-1"),
        ("new Uint8Array([1, 2, 3]).reverse().join()", "3,2,1"),
        ("new Uint8Array(3).fill(4).join()", "4,4,4"),
        ("new Uint8Array([1, 2, 3]).fill(9, 1).join()", "1,9,9"),
        ("new Uint8Array([1, 2, 3, 4]).filter(x => x % 2).join()", "1,3"),
        ("new Uint8Array([1, 2, 3]).map(x => x * 100).join()", "100,200,44"),
        ("new Uint8Array([1, 2, 3]).reduce((a, b) => a + b)", "6"),
        ("new Uint8Array([1, 2, 3]).reduceRight((a, b) => a + '' + b)", "321"),
        ("new Uint8Array([5, 6]).find(x => x > 5)", "6"),
        ("new Uint8Array([5, 6]).findIndex(x => x > 5)", "1"),
        ("new Uint8Array([6, 5, 6]).findLast(x => x > 5)", "6"),
        ("new Uint8Array([6, 5, 6]).findLastIndex(x => x > 5)", "2"),
        ("new Uint8Array([1, 2]).every(x => x > 0)", "true"),
        ("new Uint8Array([1, 2]).some(x => x > 1)", "true"),
        ("var s = 0; new Uint8Array([1, 2, 3]).forEach(x => s += x); s", "6"),
        ("new Uint8Array([1, 2, 1]).indexOf(1, 1)", "2"),
        ("new Uint8Array([1, 2, 1]).lastIndexOf(1)", "2"),
        ("new Uint8Array([1, 2]).includes(2)", "true"),
        ("new Uint8Array([1, 2, 3]).at(-1)", "3"),
        ("new Uint8Array([1, 2]).join('-')", "1-2"),
        ("new Uint8Array([1, 2]).toString()", "1,2"),
        ("new Uint8Array([1, 2]).toLocaleString()", "1,2"),
        ("[...new Uint8Array([4, 5]).keys()].join()", "0,1"),
        ("[...new Uint8Array([4, 5]).values()].join()", "4,5"),
        ("[...new Uint8Array([4, 5]).entries()].join(';')", "0,4;1,5"),
        ("Uint8Array.from([1, 2], x => x * 2).join()", "2,4"),
        ("Uint8Array.of(1, 2, 3).length", "3"),
        ("new Uint16Array(new Uint8Array([1, 2])).join()", "1,2"),
        ("var v = new DataView(new ArrayBuffer(8)); v.setUint16(0, 258); v.getUint8(0) + ':' + v.getUint8(1)", "1:2"),
        ("var v = new DataView(new ArrayBuffer(8)); v.setUint16(0, 258, true); v.getUint8(0)", "2"),
        ("var v = new DataView(new ArrayBuffer(8)); v.setInt32(0, -2); v.getInt32(0) + ':' + v.getUint32(0)", "-2:4294967294"),
        ("var v = new DataView(new ArrayBuffer(8)); v.setFloat64(0, 1.25); v.getFloat64(0)", "1.25"),
        ("var v = new DataView(new ArrayBuffer(8)); v.setFloat32(0, 0.5, true); v.getFloat32(0, true)", "0.5"),
        ("var v = new DataView(new ArrayBuffer(4)); v.setInt8(0, -1); v.setInt16(2, -2, true); v.getInt8(0) + ':' + v.getInt16(2, true) + ':' + v.getUint16(2, true)", "-1:-2:65534"),
        ("var v = new DataView(new ArrayBuffer(8), 2, 4); v.byteOffset + ':' + v.byteLength", "2:4"),
    ]);
    throws(
        "new DataView(new ArrayBuffer(2)).getUint32(0)",
        "RangeError",
    );
}

#[test]
fn maps_and_sets() {
    check(&[
        ("var m = new Map([[1, 'a']]); m.set(2, 'b').get(2) + ':' + m.size", "b:2"),
        ("var m = new Map([[1, 'a']]); m.has(1) + ':' + m.delete(1) + ':' + m.has(1)", "true:true:false"),
        ("var m = new Map([[1, 'a'], [2, 'b']]); [...m.keys()].join() + ';' + [...m.values()].join() + ';' + [...m.entries()].join('|')", "1,2;a,b;1,a|2,b"),
        ("var m = new Map([[1, 'a']]); var s = ''; m.forEach((v, k) => s += k + v); s", "1a"),
        ("var m = new Map([[1, 'a']]); m.clear(); m.size", "0"),
        ("var m = new Map(); m.set(NaN, 1); m.get(NaN)", "1"),
        ("var s = new Set([1, 2, 2]); s.add(3).size", "3"),
        ("var s = new Set([1]); s.has(1) + ':' + s.delete(1) + ':' + s.size", "true:true:0"),
        ("var s = new Set(['x', 'y']); [...s.values()].join() + ';' + [...s.keys()].join() + ';' + [...s.entries()].join('|')", "x,y;x,y;x,x|y,y"),
        ("var s = new Set([1, 2]); var t = 0; s.forEach(v => t += v); t", "3"),
        ("var s = new Set([1]); s.clear(); s.size", "0"),
        ("var k = {}; var w = new WeakMap([[k, 1]]); w.get(k) + ':' + w.has(k) + ':' + w.delete(k) + ':' + w.has(k)", "1:true:true:false"),
        ("var k = {}; var w = new WeakMap(); w.set(k, 2).get(k)", "2"),
        ("var k = {}; var w = new WeakSet([k]); w.has(k) + ':' + w.delete(k) + ':' + w.has(k)", "true:true:false"),
        ("var k = {}; new WeakSet().add(k).has(k)", "true"),
        ("for (var [k, v] of new Map([['a', 1]])) {} k + v", "a1"),
    ]);
}

#[test]
fn proxies() {
    check(&[
        ("var p = new Proxy({}, { get: (t, k) => 'got ' + String(k) }); p.x", "got x"),
        ("var t = {}; var p = new Proxy(t, { set(o, k, v) { o[k] = v * 2; return true; } }); p.a = 2; t.a", "4"),
        ("var p = new Proxy({}, { has: (t, k) => k == 'magic' }); ('magic' in p) + ':' + ('other' in p)", "true:false"),
        ("var p = new Proxy(function () {}, { construct: (f, args) => ({ n: args[0] }) }); new p(7).n", "7"),
        ("var p = new Proxy({ a: 1 }, {}); p.a", "1"),
        ("var t = {}; var p = new Proxy(t, {}); p.b = 3; t.b", "3"),
    ]);
}

#[test]
fn promises() {
    let cases = [
        ("var r; Promise.resolve(1).then(v => r = v)", "r", "1"),
        ("var r; Promise.reject(new Error('no')).catch(e => r = e.message)", "r", "no"),
        ("var r = []; Promise.resolve(1).finally(() => r.push('f')).then(v => r.push(v))", "r.join()", "f,1"),
        ("var r = []; Promise.reject(2).finally(() => r.push('f')).catch(v => r.push(v))", "r.join()", "f,2"),
        ("var r; Promise.resolve(1).then(v => { throw v + 1; }).catch(e => r = e)", "r", "2"),
        ("var r; Promise.resolve(1).then(null, () => 'no').then(v => r = v)", "r", "1"),
        ("var r; Promise.reject(1).then(v => 'no', e => 'handled ' + e).then(v => r = v)", "r", "handled 1"),
        ("var r; Promise.all([1, Promise.resolve(2), 3]).then(v => r = v.join())", "r", "1,2,3"),
        ("var r; Promise.all([1, Promise.reject('x')]).catch(e => r = e)", "r", "x"),
        ("var r; Promise.all([]).then(v => r = v.length)", "r", "0"),
        (
            "var r; Promise.allSettled([Promise.resolve(1), Promise.reject(2)]).then(v => r = v.map(x => x.status + ':' + (x.value ?? x.reason)).join())",
            "r",
            "fulfilled:1,rejected:2",
        ),
        ("var r; Promise.any([Promise.reject(1), Promise.resolve(2)]).then(v => r = v)", "r", "2"),
        ("var r; Promise.any([]).catch(e => r = (e instanceof AggregateError) + ':' + e.errors.length)", "r", "true:0"),
        (
            "var r; Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => r = e.constructor.name + ':' + e.errors.join())",
            "r",
            "AggregateError:1,2",
        ),
        ("var r; Promise.race([new Promise(() => {}), Promise.resolve(3)]).then(v => r = v)", "r", "3"),
        ("var r; new Promise((res, rej) => res(5)).then(v => r = v)", "r", "5"),
        ("var r; new Promise((res, rej) => { throw 'boom'; }).catch(e => r = e)", "r", "boom"),
        ("var r; new Promise(res => res(Promise.resolve(8))).then(v => r = v)", "r", "8"),
        ("var r; Promise.resolve({ then(f) { f(9); } }).then(v => r = v)", "r", "9"),
        ("var r; async function f() { var a = await 1; var b = await Promise.resolve(2); return a + b; } f().then(v => r = v)", "r", "3"),
        ("var r; async function f() { try { await Promise.reject('e'); } catch (x) { return 'caught ' + x; } } f().then(v => r = v)", "r", "caught e"),
        ("var r; (async () => { throw 'thrown'; })().catch(e => r = e)", "r", "thrown"),
        ("var p = Promise.resolve(1); Promise.resolve(p) === p", "String(Promise.resolve(p) === p)", "true"),
    ];
    let mut wrong = Vec::new();
    for (src, read, want) in cases {
        let got = after(src, read);
        if got != want {
            wrong.push(format!("{src}\n  read {read}\n  want {want}\n  got  {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn iterators() {
    check(&[
        ("var it = [1, 2][Symbol.iterator](); it.next().value + it.next().value", "3"),
        ("var it = 'ab'[Symbol.iterator](); it.next().value + it.next().value", "ab"),
        ("var it = new Map([[1, 2]])[Symbol.iterator](); it.next().value.join()", "1,2"),
        ("Object.prototype.toString.call([][Symbol.iterator]())", "[object Array Iterator]"),
        ("var o = { [Symbol.iterator]() { return { next: () => ({ done: true }) }; } }; [...o].length", "0"),
    ]);
    // Generator functions are refused, not half run.
    throws("function* g() { yield 1; } g().next()", "TypeError");
}

#[test]
fn global_functions() {
    check(&[
        ("encodeURIComponent('a b&c/é')", "a%20b%26c%2F%C3%A9"),
        (
            "encodeURI('http://x.y/a b?q=1&r=é')",
            "http://x.y/a%20b?q=1&r=%C3%A9",
        ),
        ("decodeURIComponent('a%20b%26c%2F%C3%A9')", "a b&c/é"),
        ("decodeURI('%41%2F')", "A%2F"),
        ("escape('a b')", "a%20b"),
        ("unescape('a%20b')", "a b"),
        ("typeof globalThis", "object"),
        ("globalThis.Math === Math", "true"),
        // No `eval` and no generator functions: what a page sent cannot make more code.
        ("typeof eval", "undefined"),
    ]);
    throws("decodeURIComponent('%')", "URIError");
}
