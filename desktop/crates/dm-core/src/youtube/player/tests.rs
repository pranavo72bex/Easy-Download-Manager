// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

fn words(src: &str) -> Vec<String> {
    let wrapped = format!("{src}}}");
    let (toks, end) = tokens(&wrapped, 0).unwrap();
    assert_eq!(end, wrapped.len() - 1);
    toks.iter().map(|t| text(&wrapped, t).to_string()).collect()
}

#[test]
fn a_slash_divides_after_a_value_and_starts_a_pattern_elsewhere() {
    assert_eq!(words("a=b/c/d"), ["a", "=", "b", "/", "c", "/", "d"]);
    assert_eq!(
        words("a=/c\\/[/]d/gi.test(x)"),
        ["a", "=", "/c\\/[/]d/gi", ".", "test", "(", "x", ")"]
    );
    assert_eq!(
        words("return/x/.test(y)"),
        ["return", "/x/", ".", "test", "(", "y", ")"]
    );
    assert_eq!(words("(a)/2"), ["(", "a", ")", "/", "2"]);
    assert_eq!(words("x/=2"), ["x", "/=", "2"]);
}

#[test]
fn strings_comments_and_templates_hide_their_brackets() {
    assert_eq!(
        words("a='}{\\''+\"]\" // }\n/* { */b"),
        ["a", "=", "'}{\\''", "+", "\"]\"", "b"]
    );
    assert_eq!(
        words("t=`x${ {a:1}.a + `in${2}` }y`;"),
        [
            "t", "=", "`x${", "{", "a", ":", "1", "}", ".", "a", "+", "`in${", "2", "}`", "}y`",
            ";"
        ]
    );
    assert_eq!(
        words("n=1E-6+0x1F+.5+a?.b+c?.5:1"),
        [
            "n", "=", "1E-6", "+", "0x1F", "+", ".5", "+", "a", "?.", "b", "+", "c", "?", ".5",
            ":", "1"
        ]
    );
}

#[test]
fn an_unended_string_or_a_stray_bracket_is_an_error() {
    assert!(tokens("a='x", 0).is_err());
    assert!(tokens("a=/x", 0).is_err());
    assert!(tokens("a=(1", 0).is_err());
    let src = "a=1;b=(2;}";
    let (toks, _) = tokens(src, 0).unwrap();
    assert!(statements(src, &toks).is_err());
}

fn split(src: &str) -> Vec<String> {
    let wrapped = format!("{src}}}");
    let (toks, _) = tokens(&wrapped, 0).unwrap();
    statements(&wrapped, &toks)
        .unwrap()
        .into_iter()
        .map(|r| {
            let st = &toks[r];
            wrapped[st[0].start..st[st.len() - 1].end].to_string()
        })
        .collect()
}

#[test]
fn statements_end_at_a_semicolon_or_a_blocks_brace() {
    assert_eq!(
        split("var a=1,b=function(){return 2};function f(){}g.x=function(){if(a){}};if(a){b()}else{c()}try{}catch(e){}do{}while(a);x()"),
        [
            "var a=1,b=function(){return 2};",
            "function f(){}",
            "g.x=function(){if(a){}};",
            "if(a){b()}else{c()}",
            "try{}catch(e){}",
            "do{}while(a);",
            "x()",
        ]
    );
}

#[test]
fn calls_made_while_loading_are_left_out_and_definitions_kept() {
    let wrapped = "'use strict';var a=f();g.b=function(){};c.prototype.d=1;e[f]=2;h(1);new g.Xo;(0,g.y)();a=1,b();x&&y();!function(){}();lbl:for(;;){}g.z(1).w=3;}";
    let (toks, _) = tokens(wrapped, 0).unwrap();
    let kept: Vec<&str> = statements(wrapped, &toks)
        .unwrap()
        .into_iter()
        .filter(|r| !left_out(wrapped, &toks[r.clone()]))
        .map(|r| {
            let st = &toks[r];
            &wrapped[st[0].start..st[st.len() - 1].end]
        })
        .collect();
    assert_eq!(
        kept,
        [
            "'use strict';",
            "var a=f();",
            "g.b=function(){};",
            "c.prototype.d=1;",
            "e[f]=2;",
            "lbl:for(;;){}",
            "g.z(1).w=3;"
        ]
    );
}

/// A player in the real one's shape, small: its names behind a string table, its address
/// class, its signature steps in a dispatcher picked by number (as `a5c2aafe`'s
/// `gm(18,2147,$g(6,5915,c))`), its `n` method, and calls at load that would fail where
/// there is no page.
pub(crate) const PLAYER: &str = r#"var _yt_player={};(function(g){var window=this;/*
 a licence */
'use strict';var ca="set;get;n;split;reverse;join".split(";"),Xa;
g.ch=function(a,b){this.url=a;this.p={}};
g.ch.prototype.set=function(a,b){this.p[a]=b};
g.ch.prototype.get=function(a){return this.p[a]};
g.ch.prototype.tN=function(){var a=this.get(ca[2]);a&&this.set(ca[2],a.split("").reverse().join("")+"_"+Xa.length)};
g.ch.prototype.clone=function(){return new g.ch(this.url)};
Xa=[1,2,3];
var Ys=function(a,b,c){switch(a){case 6:return c[ca[3]]("");case 18:return c[ca[4]]();case 13:return c[ca[5]]("").toUpperCase()}return c};
om=function(a,b,c){b=b===void 0?"":b;c=c===void 0?"":c;a=new g.ch(a,!0);a.set("alr","yes");c&&(c=Ys(18,2147,Ys(6,5915,decodeURIComponent(c))),a[ca[0]](b,encodeURIComponent(Ys(13,2385,c))));return a};
pm=function(a){a.set("alr","yes")||a.get("x")};
window.document.createElement("div").appendChild(null);
g.Vx(om);
})(_yt_player);
"#;

/// [`PLAYER`] as a player of November 2026 has it (gaps row 209): `alr` and `yes` behind a
/// table split on `{` and read through a key, `e`, worked out from a global its callers set
/// first (`g.B[25]=…;om(…)`). Uncalled, `g.B[25]` is nothing and the key wrong.
fn player_behind_a_table() -> String {
    PLAYER
        .replace(
            "'use strict';",
            "'use strict';g.d='set{alr{yes'.split(\"{\");g.B=[];",
        )
        .replace(
            "om=function(a,b,c){",
            "om=function(a,b,c){var e=g.B[25]%53723;",
        )
        .replace(
            "a.set(\"alr\",\"yes\");c&&",
            "a[g.d[e^4094]](g.d[e^4095],g.d[e^4092]);c&&",
        )
        .replace("pm=function(a){a.set(\"alr\",\"yes\")||a.get(\"x\")};", "")
}

#[test]
fn a_player_s_string_table_and_its_global_key_are_read() {
    let player = player_behind_a_table();
    assert!(
        !player.contains("\"alr\""),
        "the test player still says alr in quotes"
    );
    assert_eq!(
        string_table(&player),
        Some(Table {
            name: "g.d".into(),
            set: 0,
            alr: 1,
            yes: 2,
        })
    );
    let program = program(&player).unwrap();
    assert!(
        program
            .contains(";__dm.makers = [function (a, b, c) { g.B[25]=4094; return om(a, b, c); }];"),
        "{program}"
    );
    // A key from the function's own arguments is the caller's call: nothing set first.
    let by_arguments = player.replace("var e=g.B[25]%53723;", "var e=4094;");
    assert!(program_makers(&by_arguments).contains(";__dm.makers = [om];"));
    // A remainder the key cannot be is not set.
    let too_small = player.replace("%53723", "%9");
    assert!(program_makers(&too_small).contains(";__dm.makers = [om];"));
}

fn program_makers(player: &str) -> String {
    program(player).unwrap()
}

#[tokio::test]
async fn a_player_behind_a_table_answers_as_the_plain_one_does() {
    let program = program(&player_behind_a_table()).unwrap();
    let got = crate::javascript::run(
        &program,
        &call(&["abc"], &["a/b+c"]),
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap();
    let answer: serde_json::Value = serde_json::from_str(&got).unwrap();
    assert_eq!(answer["n"]["abc"], "cba_3");
    assert_eq!(answer["sig"]["a/b+c"], "C+B/A");
}

#[test]
fn the_player_is_cut_to_its_definitions_and_its_address_function_found() {
    let program = program(PLAYER).unwrap();
    assert!(program.contains(";__dm.makers = [om];"), "{program}");
    assert!(
        !program.contains("createElement"),
        "a call at load was kept"
    );
    assert!(!program.contains("g.Vx(om)"), "a call at load was kept");
    assert!(program.contains("(function (g) {\nvar window=this;\n'use strict';"));
    assert!(program.contains("Xa=[1,2,3];"));
    assert_eq!(
        call(&["abc"], &["x%2Fy"]),
        r#"__dm.solve({"n":["abc"],"sig":["x%2Fy"]})"#
    );
}

/// Whether an engine is here to run a program in: dm-js, always.
fn an_engine_is_here() -> bool {
    true
}

#[tokio::test]
async fn the_cut_player_answers_both_checks_in_the_computers_own_engine() {
    if !an_engine_is_here() {
        eprintln!("no JavaScript engine here: not run");
        return;
    }
    let program = program(PLAYER).unwrap();
    let got = crate::javascript::run(
        &program,
        &call(&["abc", "xyz"], &["a/b+c"]),
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap();
    let answer: serde_json::Value = serde_json::from_str(&got).unwrap();
    assert_eq!(answer["n"]["abc"], "cba_3");
    assert_eq!(answer["n"]["xyz"], "zyx_3");
    assert_eq!(answer["sig"]["a/b+c"], "C+B/A");
    // A player whose `n` method fails says so, rather than answering with what it was given.
    let failing = PLAYER.replace("var a=this.get(ca[2]);", "throw new Error('changed');");
    let why = crate::javascript::run(
        &super::program(&failing).unwrap(),
        &call(&["abc"], &[]),
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap_err();
    assert!(
        why.contains("no answer for n abc") && why.contains("changed"),
        "{why}"
    );
}

/// The TV's player, in its own shape: a function with no parameter called on the window,
/// strict, its parts declared as functions.
const TV_PLAYER: &str = r#"(function(){'use strict';var h="set;get;n".split(";");
function Ba(a,b){this.url=a;this.p={}}
Ba.prototype.set=function(a,b){this.p[a]=b};Ba.prototype.get=function(a){return this.p[a]};
Ba.prototype.vN=function(){var a=this.get(h[2]);a&&this.set(h[2],a+"!")};
Ba.prototype.clone=function(){return new Ba(this.url)};
function Oa(a,b,c){return a===17?c.slice(1):c}
function JS(a,b,c){b=b===void 0?"":b;c=c===void 0?"":c;a=new Ba(a,!0);a.set("alr","yes");c&&(c=Oa(17,2144,c),a[h[0]](b,c));return a}
document.body.appendChild(null);}).call(this);
"#;

#[tokio::test]
async fn the_tvs_player_is_cut_and_answers_too() {
    let program = program(TV_PLAYER).unwrap();
    assert!(program.contains(";__dm.makers = [JS];"), "{program}");
    assert!(!program.contains("appendChild"));
    if !an_engine_is_here() {
        eprintln!("no JavaScript engine here: not run");
        return;
    }
    let got = crate::javascript::run(
        &program,
        &call(&["abc"], &["xyz"]),
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap();
    assert_eq!(got, r#"{"n":{"abc":"abc!"},"sig":{"xyz":"yz"}}"#);
}

#[test]
fn a_player_with_no_address_function_or_not_one_function_is_said_to_have_changed() {
    let none = PLAYER.replace(r#"a.set("alr","yes");"#, "");
    let why = program(&none).unwrap_err();
    assert!(
        why.contains("no function in it builds a stream's address"),
        "{why}"
    );
    let why = program("var a = 1;").unwrap_err();
    assert!(
        why.contains("YouTube's player is not as dm knows it"),
        "{why}"
    );
    let why = program("(function(g){var a='x;").unwrap_err();
    assert!(why.contains("never ends"), "{why}");
}

/// A player of another shape, whose address function is not found, still answers `n`
/// through a function of the `n` function's own shape: one that takes the value apart letter
/// by letter and puts it back (gaps row Y3). A signature it cannot answer says so.
#[tokio::test]
async fn a_player_of_another_shape_answers_n_by_its_n_function() {
    const OTHER: &str = r#"var _yt_player={};(function(g){var window=this;
var Xy=function(a){var b=a.split(""),c=b.length;b.reverse();b.push(String(c));return b.join("")};
var Qz=function(a){return a+1};
document.body.appendChild(null);})(_yt_player);
"#;
    let program = program(OTHER).unwrap();
    assert!(program.contains("__dm.nfuncs = [Xy];"), "{program}");
    if !an_engine_is_here() {
        eprintln!("no JavaScript engine here: not run");
        return;
    }
    let got = crate::javascript::run(
        &program,
        &call(&["abc"], &[]),
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap();
    assert_eq!(got, r#"{"n":{"abc":"cba3"},"sig":{}}"#);
    let why = crate::javascript::run(
        &program,
        &call(&[], &["xyz"]),
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap_err();
    assert!(why.contains("no answer for sig xyz"), "{why}");
}
/// The live player (a file named by `DM_JS_PLAYER`, YouTube's `base.js`) answers both
/// checks in dm's own interpreter: each `n` and signature given back changed (gaps row F15,
/// which found its answers the same as JavaScriptCore's).
#[tokio::test]
async fn the_live_player_answers_in_dm_js() {
    let Some(path) = std::env::var_os("DM_JS_PLAYER") else {
        eprintln!("DM_JS_PLAYER not set: not run");
        return;
    };
    let player = std::fs::read_to_string(path).unwrap();
    let program = program(&player).unwrap();
    let (n, sig) = ("qx4bZJdOsBf7pXcM", "AOq0QJ8wRQIhAJOPkbz9cO6FpXmDfaA8n3qYhRjM2iVxw7BtZuS1LP5mAiAF0eHgYpTr9rkSvCdLa3UzNjXqWs6Ei2Jf1oKbwcBmQ==");
    let started = std::time::Instant::now();
    let got = crate::javascript::run(
        &program,
        &call(&[n], &[sig]),
        std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();
    eprintln!("dm-js: {got} in {:?}", started.elapsed());
    let answer: serde_json::Value = serde_json::from_str(&got).unwrap();
    let (got_n, got_sig) = (
        answer["n"][n].as_str().unwrap(),
        answer["sig"][sig].as_str().unwrap(),
    );
    assert!(
        !got_n.is_empty() && !got_n.ends_with(n),
        "n not answered: {got_n}"
    );
    assert!(
        got_sig != sig && got_sig.len() + 4 >= sig.len(),
        "signature not answered: {got_sig}"
    );
}
