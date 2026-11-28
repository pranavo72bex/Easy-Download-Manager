// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The size gate's counting and its baselines (gaps row A12).

use super::*;

/// Functions as [`functions`] gives them, from names and lengths.
fn named(list: &[(&str, usize)]) -> Vec<(String, usize)> {
    list.iter().map(|(n, l)| (n.to_string(), *l)).collect()
}

/// Sizes from files' and functions' lengths.
fn sizes(files: &[(&str, usize)], functions: &[(&str, &str, usize)]) -> Sizes {
    let mut sizes = Sizes::default();
    for (path, lines) in files {
        sizes.files.insert(path.to_string(), *lines);
    }
    for (path, name, lines) in functions {
        let key = (path.to_string(), name.to_string());
        sizes.functions.entry(key).or_default().push(*lines);
    }
    sizes
}

#[test]
fn braces_in_strings_and_comments_are_not_counted() {
    let text = r#"fn a() {
    let s = "}{ \" }";
    // } a brace in a comment
    /* { /* nested } */ } */
    s
}
fn b() {}
"#;
    assert_eq!(functions(text), named(&[("a", 6), ("b", 1)]));
}

#[test]
fn braces_in_raw_and_byte_strings_are_not_counted() {
    let text = r###"fn a() {
    let s = r#"{ "} "#;
    let t = br"}";
    let u = b"{";
    let v = r"\";
    s
}
"###;
    assert_eq!(functions(text), named(&[("a", 7)]));
}

#[test]
fn braces_in_character_literals_are_not_counted() {
    let text = r#"fn a(c: char) -> bool {
    c == '{' || c == '\'' || c == '\\' || c == '\u{7d}' || c == '"' || b'}' == 0
}
"#;
    assert_eq!(functions(text), named(&[("a", 3)]));
}

#[test]
fn lifetimes_and_labels_are_code_not_characters() {
    let text = r#"fn a<'a, 'b>(s: &'a str, _t: &'b str) -> &'a str {
    'outer: loop {
        break 'outer;
    }
    let _ = '{';
    s
}
fn b() {}
"#;
    assert_eq!(functions(text), named(&[("a", 7), ("b", 1)]));
}

#[test]
fn declarations_and_pointer_types_are_not_functions() {
    let text = "trait T {\n    fn a(&self);\n    fn b(&self) -> [u8; 4] {\n        [0; 4]\n    \
                }\n}\nfn c(f: fn(u8) -> u8) -> u8 {\n    f(1)\n}\n";
    assert_eq!(functions(text), named(&[("b", 3), ("c", 3)]));
}

#[test]
fn a_function_inside_another_is_counted_on_its_own_too() {
    let text = "fn outer() {\n    fn inner() {\n        let _ = 1;\n    }\n    inner()\n}\n";
    assert_eq!(functions(text), named(&[("outer", 6), ("inner", 3)]));
}

#[test]
fn a_function_is_counted_from_its_fn_line_not_its_attributes() {
    let text = "/// Docs.\n#[inline]\npub fn long(\n    a: u8,\n) -> u8 {\n    a\n}\n";
    assert_eq!(functions(text), named(&[("long", 5)]));
}

#[test]
fn blanking_keeps_every_line() {
    let text = "a // b\n\"x\ny\" /* c\nd */ e\n";
    let code: String = code_only(text).into_iter().collect();
    assert_eq!(code.lines().count(), text.lines().count());
    assert_eq!(code.lines().next(), Some("a     "));
    assert!(code.ends_with("    e\n"));
}

#[test]
fn a_file_not_in_the_baseline_may_have_800_lines() {
    let baseline = Sizes::default();
    let verdict = judge(&sizes(&[("a.rs", 800)], &[]), &baseline);
    assert!(verdict.wrong.is_empty() && verdict.notes.is_empty());
    let verdict = judge(&sizes(&[("a.rs", 801)], &[]), &baseline);
    assert_eq!(verdict.wrong.len(), 1);
}

#[test]
fn a_file_in_the_baseline_may_shrink_but_not_grow() {
    let baseline = sizes(&[("a.rs", 1000), ("gone.rs", 900)], &[]);
    let verdict = judge(&sizes(&[("a.rs", 1001)], &[]), &baseline);
    assert_eq!(verdict.wrong.len(), 1, "{verdict:?}");
    let verdict = judge(&sizes(&[("a.rs", 990)], &[]), &baseline);
    assert!(verdict.wrong.is_empty(), "{verdict:?}");
    // One note to lower `a.rs`, one to remove `gone.rs`.
    assert_eq!(verdict.notes.len(), 2, "{verdict:?}");
    let verdict = judge(&sizes(&[("a.rs", 1000), ("gone.rs", 900)], &[]), &baseline);
    assert!(
        verdict.wrong.is_empty() && verdict.notes.is_empty(),
        "{verdict:?}"
    );
}

#[test]
fn no_file_passes_the_ceiling_even_in_the_baseline() {
    let baseline = sizes(&[("a.rs", 3000)], &[]);
    let verdict = judge(&sizes(&[("a.rs", FILE_CEILING + 1)], &[]), &baseline);
    assert_eq!(verdict.wrong.len(), 1, "{verdict:?}");
}

#[test]
fn functions_of_one_name_pair_with_their_entries_longest_first() {
    let baseline = sizes(&[], &[("a.rs", "render", 400), ("a.rs", "render", 200)]);
    let fine = sizes(&[], &[("a.rs", "render", 200), ("a.rs", "render", 390)]);
    let verdict = judge(&fine, &baseline);
    assert!(verdict.wrong.is_empty(), "{verdict:?}");
    assert_eq!(verdict.notes.len(), 1, "{verdict:?}");
    let grown = sizes(&[], &[("a.rs", "render", 201), ("a.rs", "render", 390)]);
    assert_eq!(judge(&grown, &baseline).wrong.len(), 1);
    let third = sizes(
        &[],
        &[
            ("a.rs", "render", 160),
            ("a.rs", "render", 200),
            ("a.rs", "render", 390),
        ],
    );
    assert_eq!(judge(&third, &baseline).wrong.len(), 1);
    let new = sizes(&[], &[("b.rs", "draw", FN_LIMIT + 1)]);
    assert_eq!(judge(&new, &baseline).wrong.len(), 1);
}

#[test]
fn lowering_never_raises_or_adds_an_entry() {
    let baseline = sizes(
        &[("a.rs", 1000), ("b.rs", 1000), ("c.rs", 1000)],
        &[("a.rs", "f", 400), ("a.rs", "f", 200), ("a.rs", "g", 300)],
    );
    let measured = sizes(
        &[("a.rs", 900), ("b.rs", 1100), ("c.rs", 700), ("d.rs", 2000)],
        &[("a.rs", "f", 390), ("a.rs", "h", 500)],
    );
    let expected = sizes(&[("a.rs", 900), ("b.rs", 1000)], &[("a.rs", "f", 390)]);
    assert_eq!(lowered(&measured, &baseline), expected);
}

#[test]
fn a_baseline_reads_back_as_it_is_written() {
    let written = sizes(
        &[("a/b.rs", 900), ("a/b/c.rs", 1200)],
        // A name's lengths as they read back: in the file's order, shortest first.
        &[
            ("a/b.rs", "f", 200),
            ("a/b.rs", "f", 400),
            ("a/b.rs", "e", 160),
        ],
    );
    let (files, functions) = render(&written);
    assert!(functions.ends_with("a/b.rs e 160\na/b.rs f 200\na/b.rs f 400\n"));
    assert_eq!(parse(&files, &functions).unwrap(), written);
    assert!(parse("a.rs many\n", "").is_err());
    assert!(parse("", "a.rs 400\n").is_err());
}

/// The checked-in baselines are in the order and form `cargo xtask size-baseline` writes, so
/// lowering one changes only its own lines.
#[test]
fn the_baselines_are_written_as_the_gate_writes_them() {
    let root = crate::root();
    let files = crate::read(&root.join(FILE_BASELINE))
        .unwrap()
        .replace("\r\n", "\n");
    let functions = crate::read(&root.join(FN_BASELINE))
        .unwrap()
        .replace("\r\n", "\n");
    let baseline = parse(&files, &functions).unwrap();
    assert_eq!(render(&baseline), (files, functions));
    assert!(baseline
        .files
        .values()
        .all(|l| (FILE_LIMIT + 1..=FILE_CEILING).contains(l)));
    assert!(baseline.functions.values().flatten().all(|&l| l > FN_LIMIT));
}
