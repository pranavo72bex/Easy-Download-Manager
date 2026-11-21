// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The language as dm's interpreter runs it: each program's last value, written as `String`
//! writes it, against what an engine of a browser gives.

use dm_js::interp::Interp;

fn run(src: &str) -> String {
    let mut i = Interp::new();
    match i.run(src) {
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

#[test]
fn values_operators_and_conversions() {
    check(&[
        ("1 + 2 * 3", "7"),
        ("'a' + 1 + 2", "a12"),
        ("1 + 2 + 'a'", "3a"),
        ("7 % -3", "1"),
        ("-7 % 3", "-1"),
        ("2 ** 10", "1024"),
        ("(-1) >>> 0", "4294967295"),
        ("1 << 31", "-2147483648"),
        ("~5", "-6"),
        ("0.1 + 0.2", "0.30000000000000004"),
        ("'5' * '2'", "10"),
        ("[] + []", ""),
        ("[1, 2] + ''", "1,2"),
        ("({}) + ''", "[object Object]"),
        ("null == undefined", "true"),
        ("null === undefined", "false"),
        ("'1' == 1", "true"),
        ("NaN == NaN", "false"),
        ("typeof null", "object"),
        ("typeof function () {}", "function"),
        ("typeof undeclared", "undefined"),
        ("'b' > 'a'", "true"),
        ("void 0", "undefined"),
        ("!!''", "false"),
        ("1 / 0", "Infinity"),
        ("0 || 'x'", "x"),
        ("0 ?? 'x'", "0"),
        ("null ?? 'x'", "x"),
        ("10n ** 3n", "1000"),
        ("typeof 1n", "bigint"),
    ]);
}

#[test]
fn scopes_closures_and_hoisting() {
    check(&[
        ("var a = 1; function f() { return a; } f()", "1"),
        ("f(); function f() { return 2; } f()", "2"),
        ("function f() { var x = 1; { var x = 2; } return x; } f()", "2"),
        ("let x = 1; { let x = 2; } x", "1"),
        ("var fs = []; for (let i = 0; i < 3; i++) fs.push(() => i); fs.map(f => f()).join()", "0,1,2"),
        ("var fs = []; for (var i = 0; i < 3; i++) fs.push(() => i); fs.map(f => f()).join()", "3,3,3"),
        ("function counter() { var n = 0; return function () { return ++n; }; } var c = counter(); c(); c()", "2"),
        ("var f = function g(n) { return n ? n * g(n - 1) : 1; }; f(5)", "120"),
        ("(function () { return typeof arguments; })()", "object"),
        ("(function (a, b) { return arguments.length; })(1, 2, 3)", "3"),
        ("const k = 1; var r; try { k = 2; } catch (e) { r = e.name } r", "TypeError"),
        ("x = 5; x", "5"),
        ("var g = this; g.y = 3; y", "3"),
    ]);
}

#[test]
fn objects_prototypes_and_this() {
    check(&[
        ("var o = {a: 1, get b() { return this.a + 1; }}; o.b", "2"),
        ("function P(x) { this.x = x; } P.prototype.get = function () { return this.x; }; new P(4).get()", "4"),
        ("function P() {} var p = new P(); p instanceof P", "true"),
        ("Object.keys({b: 1, a: 2, 1: 3, 0: 4}).join()", "0,1,b,a"),
        ("var o = Object.create({inherited: 1}); o.own = 2; var k = []; for (var p in o) k.push(p); k.join()", "own,inherited"),
        ("var o = {}; Object.defineProperty(o, 'x', {value: 1, enumerable: false}); Object.keys(o).length + ',' + o.x", "0,1"),
        ("var o = {a: 1}; delete o.a; 'a' in o", "false"),
        ("var f = function () { return this; }; f.call(5) == 5", "true"),
        ("function f(a, b) { return a + b; } f.apply(null, [1, 2])", "3"),
        ("var o = {v: 1, f() { return this.v; }}; var g = o.f.bind({v: 9}); g()", "9"),
        ("Object.getPrototypeOf([]) === Array.prototype", "true"),
        ("var {a, b: {c}, ...rest} = {a: 1, b: {c: 2}, d: 3, e: 4}; a + c + Object.keys(rest).join()", "3d,e"),
        ("var [x, , y = 5, ...z] = [1, 2, undefined, 4, 5]; x + ',' + y + ',' + z", "1,5,4,5"),
        ("({...{a: 1}, b: 2}).a", "1"),
        ("var o = {['k' + 1]: 2}; o.k1", "2"),
        ("({}).toString.call([])", "[object Array]"),
    ]);
}

#[test]
fn control_flow_and_errors() {
    check(&[
        ("var s = 0; for (var i = 0; i < 10; i++) { if (i == 5) break; if (i % 2) continue; s += i; } s", "6"),
        ("var n = 0; outer: for (var i = 0; i < 3; i++) for (var j = 0; j < 3; j++) { if (j == 1) continue outer; n++; } n", "3"),
        ("var r = ''; switch (2) { case 1: r += 'a'; case 2: r += 'b'; case 3: r += 'c'; break; default: r += 'd'; } r", "bc"),
        ("var i = 0; do { i++; } while (i < 5); i", "5"),
        ("function f() { try { return 1; } finally { r = 2; } } var r = 0; f() + r", "3"),
        ("var r; try { null.x; } catch (e) { r = e instanceof TypeError } r", "true"),
        ("var r; try { throw new RangeError('bad'); } catch (e) { r = e.name + ': ' + e.message } r", "RangeError: bad"),
        ("var r; try { undefinedThing; } catch (e) { r = e.name } r", "ReferenceError"),
        ("String(new Error('x'))", "Error: x"),
        ("var t = 0; for (const v of [1, 2, 3]) t += v; t", "6"),
        ("var t = ''; for (const c of 'ab') t += c + '.'; t", "a.b."),
        ("var o = {a: {b: null}}; o?.a?.b?.c === undefined", "true"),
        ("var f = null; f?.() === undefined", "true"),
    ]);
}

#[test]
fn classes_and_functions_of_today() {
    check(&[
        ("class A { constructor(x) { this.x = x; } get double() { return this.x * 2; } static make() { return new A(3); } } A.make().double", "6"),
        ("class A { hi() { return 'a'; } } class B extends A { hi() { return super.hi() + 'b'; } } new B().hi()", "ab"),
        ("class A { #p = 1; inc() { return ++this.#p; } } new A().inc()", "2"),
        ("class A { static n = 5; } A.n", "5"),
        ("class A { constructor() { this.k = 'a'; } } class B extends A { constructor() { super(); this.k += 'b'; } } new B().k", "ab"),
        ("var add = (a, b = 2) => a + b; add(1)", "3"),
        ("((...xs) => xs.length)(1, 2, 3)", "3"),
        ("var o = {v: 7, f() { return (() => this.v)(); }}; o.f()", "7"),
        ("`a${1 + 1}b${'c'}`", "a2bc"),
        ("String.raw`a\\nb`", "a\\nb"),
        ("Math.max(...[1, 5, 3])", "5"),
    ]);
}

#[test]
fn the_built_in_objects() {
    check(&[
        ("[3, 1, 2].sort().join()", "1,2,3"),
        ("[10, 9, 1].sort().join()", "1,10,9"),
        ("[10, 9, 1].sort((a, b) => a - b).join()", "1,9,10"),
        (
            "var a = [1, 2, 3, 4]; a.splice(1, 2, 'x').join() + '|' + a.join()",
            "2,3|1,x,4",
        ),
        ("[1, 2, 3].reverse().join('-')", "3-2-1"),
        ("[1, [2, [3]]].flat(Infinity).join()", "1,2,3"),
        ("[1, 2, 3].reduce((a, b) => a + b)", "6"),
        ("Array.from({length: 3}, (_, i) => i * 2).join()", "0,2,4"),
        (
            "[1, 2, 3].indexOf(2) + ',' + [1, 2, 3].includes(4)",
            "1,false",
        ),
        ("var a = []; a[3] = 1; a.length", "4"),
        ("var a = [1, 2, 3]; a.length = 1; a.join()", "1"),
        ("'abc'.split('').reverse().join('')", "cba"),
        ("'a,b,,c'.split(',').length", "4"),
        ("'hello'.charCodeAt(1)", "101"),
        ("String.fromCharCode(104, 105)", "hi"),
        ("'Hello'.slice(-3)", "llo"),
        ("'  x '.trim() + '|'", "x|"),
        ("'abc'.padStart(5, '-')", "--abc"),
        ("'a-b-c'.replace('-', '+')", "a+b-c"),
        ("'a-b-c'.replaceAll('-', '+')", "a+b+c"),
        ("(255).toString(16)", "ff"),
        ("(0.5).toString(2)", "0.1"),
        ("(3.14159).toFixed(2)", "3.14"),
        (
            "parseInt('0x1f') + parseInt('12px') + parseFloat('1.5e1x')",
            "58",
        ),
        ("Number('') + Number(' 12 ') + Number('1e3')", "1012"),
        (
            "JSON.stringify({a: [1, 'x', null, true], b: undefined})",
            "{\"a\":[1,\"x\",null,true]}",
        ),
        ("JSON.parse('{\"a\":[1,2,{\"b\":\"c\"}]}').a[2].b", "c"),
        ("JSON.stringify({a: 1}, null, 2)", "{\n  \"a\": 1\n}"),
        (
            "var m = new Map([[1, 'a']]); m.set('k', 2); m.size + m.get(1) + [...m.keys()].join()",
            "2a1,k",
        ),
        (
            "var s = new Set([1, 2, 2, 3]); s.size + ':' + [...s].join()",
            "3:1,2,3",
        ),
        ("Math.floor(-1.5) + Math.round(2.5) + Math.abs(-3)", "4"),
        ("encodeURIComponent('a b&c/é')", "a%20b%26c%2F%C3%A9"),
        ("decodeURIComponent('a%20b%26c%2F%C3%A9')", "a b&c/é"),
        ("typeof Symbol() + String(Symbol('x'))", "symbolSymbol(x)"),
        (
            "var o = {[Symbol.iterator]: function* () {}}; typeof o[Symbol.iterator]",
            "function",
        ),
        (
            "Object.entries({a: 1, b: 2}).map(([k, v]) => k + v).join()",
            "a1,b2",
        ),
        ("Object.assign({}, {a: 1}, {b: 2}).b", "2"),
        ("var f = Object.freeze({a: 1}); f.a = 2; f.a", "1"),
    ]);
}

#[test]
fn patterns() {
    check(&[
        ("/a(b+)c/.exec('xabbbc')[1]", "bbb"),
        ("/(\\d+)/g.test('a1')", "true"),
        ("'a1b22c333'.match(/\\d+/g).join()", "1,22,333"),
        (
            "'2026-11-13'.replace(/(\\d+)-(\\d+)-(\\d+)/, '$3.$2.$1')",
            "13.11.2026",
        ),
        ("'aaa'.replace(/a/g, (m, i) => i)", "012"),
        ("'a, b,c'.split(/\\s*,\\s*/).join('|')", "a|b|c"),
        ("'abc'.replace(/(?<first>a)/, '$<first>$<first>')", "aabc"),
        ("'x'.search(/y/)", "-1"),
        (
            "var r = /o/g; var n = 0; while (r.exec('foo boo')) n++; n",
            "4",
        ),
        ("String(/a\\/b/gi)", "/a\\/b/gi"),
        (
            "new RegExp('a+', 'g').source + new RegExp('a+', 'g').flags",
            "a+g",
        ),
        (
            "[...'a1b2'.matchAll(/\\d/g)].map(m => m[0] + m.index).join()",
            "11,23",
        ),
        ("'Hello World'.replace(/o/g, '0')", "Hell0 W0rld"),
        ("/^[\\w-]{11}$/.test('jNQXAC9IVRw')", "true"),
    ]);
}

#[test]
fn promises_and_await() {
    check(&[
        (
            "var r; Promise.resolve(1).then(v => { r = v + 1; }); r",
            "undefined",
        ),
        (
            "var log = []; Promise.resolve(1).then(v => log.push(v)); log.push(0); log.join()",
            "0",
        ),
        (
            "async function f() { return 5; } var x; f().then(v => x = v); typeof f()",
            "object",
        ),
    ]);
    // The jobs run once the program's own code has run.
    let mut i = Interp::new();
    i.run("var out = []; Promise.resolve(1).then(v => out.push(v)); Promise.all([1, Promise.resolve(2)]).then(v => out.push(v.join('+')));")
        .unwrap();
    let v = i.run("out.join()").unwrap();
    assert_eq!(i.to_rust_string(&v).unwrap(), "1,1+2");
}

#[test]
fn a_program_that_runs_too_long_is_stopped() {
    let mut i = Interp::new();
    i.deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(200));
    let e = i.run("while (true) {}").unwrap_err();
    let s = i.to_rust_string(&e).unwrap();
    assert!(s.contains("ran out of time"), "{s}");
    let e = dm_js::evaluate(
        "function f() { return f(); }",
        "f()",
        std::time::Duration::from_secs(10),
    )
    .unwrap_err();
    assert!(e.contains("Maximum call stack"), "{e}");
    assert_eq!(
        dm_js::evaluate(
            "function f(n) { return n ? f(n - 1) + 1 : 0; }",
            "f(300)",
            std::time::Duration::from_secs(10)
        ),
        Ok("300".into())
    );
}
