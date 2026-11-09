// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Every source file of dm's own says whose it is and under what licence, as
//! the NOTICE's section 7(b) term asks to be kept: four comment lines at its
//! top, in the comment style of its language.
//!
//! `cargo xtask headers` lists the files without them; `cargo xtask headers
//! fix` writes them in. Third-party code (`desktop/vendor/`, under its own
//! licences) and test data (`fixtures/`) are left as they are, and so are
//! formats with no comments (JSON) or that are not code (Markdown, TOML,
//! YAML, media).

use std::path::Path;

/// What a header says, without its comment marks. No web address: the
/// privacy check allows none in shipped code.
pub const LINES: [&str; 4] = [
    "Copyright (C) 2026 pranavo72bex and the dm contributors.",
    "SPDX-License-Identifier: GPL-3.0-or-later",
    "Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,",
    "with the attribution term in NOTICE. See the LICENSE and NOTICE files.",
];

/// The line by which a file is known to carry a header already.
const MARK: &str = "SPDX-License-Identifier: GPL-3.0-or-later";

/// How a language writes a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// `// …`: Rust and JavaScript.
    Slashes,
    /// `# …`: Python, shell, Dockerfiles.
    Hash,
    /// `<!-- … -->`: HTML.
    Markup,
}

/// The style a tracked file's header takes, or `None` for a file that has none:
/// someone else's code, test data, or a format that is not code.
pub fn style_of(path: &str, text: &str) -> Option<Style> {
    let path = path.replace('\\', "/");
    if path.starts_with("desktop/vendor/")
        || path.contains("/fixtures/")
        || path.starts_with("fixtures/")
    {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(&path);
    let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match extension.as_deref() {
        Some("rs" | "js" | "mjs") => Some(Style::Slashes),
        Some("py" | "sh" | "ps1") => Some(Style::Hash),
        Some("html") => Some(Style::Markup),
        _ if name == "Dockerfile" || name == "Makefile" => Some(Style::Hash),
        // A script with no extension is known by its first line.
        None if text.starts_with("#!") => Some(Style::Hash),
        _ => None,
    }
}

/// Whether `text` already carries the header.
pub fn has_header(text: &str) -> bool {
    text.lines().take(12).any(|l| l.contains(MARK))
}

/// `text` with the header written in where its language allows it: after a
/// script's `#!` line or a Dockerfile's parser directive (both must stay
/// first), after HTML's doctype, and otherwise at the very top, before any
/// `//!` documentation or `#![…]` attribute of a Rust file. The file's own
/// line ending is kept.
pub fn with_header(text: &str, style: Style) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let header: String = match style {
        Style::Slashes => LINES.iter().map(|l| format!("// {l}{newline}")).collect(),
        Style::Hash => LINES.iter().map(|l| format!("# {l}{newline}")).collect(),
        Style::Markup => {
            let mut h = format!("<!--{newline}");
            for l in LINES {
                h.push_str(&format!("  {l}{newline}"));
            }
            h.push_str(&format!("-->{newline}"));
            h
        }
    };
    // How many of the first lines must stay where they are.
    let mut keep = 0;
    let first_lines: Vec<&str> = text.split_inclusive('\n').collect();
    for line in &first_lines {
        let bare = line.trim_end();
        let stays = match style {
            Style::Hash => {
                (keep == 0 && bare.starts_with("#!"))
                    || bare.to_ascii_lowercase().starts_with("# syntax=")
                    || bare.to_ascii_lowercase().starts_with("# escape=")
            }
            Style::Markup => keep == 0 && bare.to_ascii_lowercase().starts_with("<!doctype"),
            Style::Slashes => false,
        };
        if !stays {
            break;
        }
        keep += 1;
    }
    let head: String = first_lines[..keep].concat();
    let rest: String = first_lines[keep..].concat();
    // A blank line between the header and what follows it, unless the file ends there.
    let gap = if rest.is_empty() || rest.starts_with(newline) {
        ""
    } else {
        newline
    };
    // A file whose last kept line has no ending (a one-line script) gets one.
    let head = if !head.is_empty() && !head.ends_with('\n') {
        format!("{head}{newline}")
    } else {
        head
    };
    format!("{head}{header}{gap}{rest}")
}

/// The tracked files that should carry a header and do not, as `(path, style)`.
pub fn missing(root: &Path) -> Result<Vec<(String, Style)>, String> {
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    let mut out = Vec::new();
    for name in listed.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name).into_owned();
        // Not text (media, fonts, pictures): no header to look for.
        let Ok(text) = std::fs::read_to_string(root.join(&name)) else {
            continue;
        };
        if let Some(style) = style_of(&name, &text) {
            if !has_header(&text) {
                out.push((name, style));
            }
        }
    }
    Ok(out)
}

/// Writes the header into every file that lacks one; says how many.
pub fn fix(root: &Path) -> Result<usize, String> {
    let todo = missing(root)?;
    for (name, style) in &todo {
        let path = root.join(name);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))?;
        std::fs::write(&path, with_header(&text, *style)).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(todo.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rust_file_takes_it_above_its_module_documentation() {
        let out = with_header("//! The engine.\n\nfn main() {}\n", Style::Slashes);
        assert!(
            out.starts_with("// Copyright (C) 2026 pranavo72bex"),
            "{out}"
        );
        assert!(out.contains(&format!("// {MARK}\n")), "{out}");
        assert!(out.contains("files.\n\n//! The engine.\n"), "{out}");
        assert!(has_header(&out));
    }

    #[test]
    fn a_script_keeps_its_first_line_and_a_dockerfile_its_directive() {
        let out = with_header("#!/usr/bin/env python3\n\"\"\"Doc.\"\"\"\n", Style::Hash);
        assert!(
            out.starts_with("#!/usr/bin/env python3\n# Copyright"),
            "{out}"
        );
        assert!(out.ends_with("files.\n\n\"\"\"Doc.\"\"\"\n"), "{out}");
        let docker = with_header("# syntax=docker/dockerfile:1\nFROM debian\n", Style::Hash);
        assert!(
            docker.starts_with("# syntax=docker/dockerfile:1\n# Copyright"),
            "{docker}"
        );
    }

    #[test]
    fn html_takes_it_after_the_doctype_and_a_windows_file_keeps_its_endings() {
        let out = with_header("<!doctype html>\r\n<html></html>\r\n", Style::Markup);
        assert!(
            out.starts_with("<!doctype html>\r\n<!--\r\n  Copyright"),
            "{out}"
        );
        assert!(
            !out.replace("\r\n", "").contains('\n'),
            "a bare line ending crept in: {out:?}"
        );
    }

    #[test]
    fn someone_elses_code_test_data_and_other_formats_take_none() {
        assert_eq!(style_of("desktop/vendor/winit/src/lib.rs", ""), None);
        assert_eq!(style_of("browser/tests/fixtures/feed.html", ""), None);
        assert_eq!(
            style_of(
                "desktop/crates/dm-core/tests/fixtures/media/hls/h264/seg0.ts",
                ""
            ),
            None
        );
        assert_eq!(style_of("docs/gaps.md", ""), None);
        assert_eq!(style_of("extension/manifest.json", "{}"), None);
        assert_eq!(
            style_of("scripts/pre-commit", "#!/bin/sh\n"),
            Some(Style::Hash)
        );
        assert_eq!(style_of("qa/app/Dockerfile", "FROM x\n"), Some(Style::Hash));
        assert_eq!(style_of("extension/popup.js", ""), Some(Style::Slashes));
    }

    #[test]
    fn a_file_that_has_one_is_not_given_a_second() {
        let once = with_header("fn main() {}\n", Style::Slashes);
        assert!(has_header(&once));
        assert_eq!(once.matches(MARK).count(), 1);
    }

    #[test]
    fn a_one_line_script_gets_a_line_ending_and_no_gap_at_its_end() {
        let out = with_header("#!/bin/sh", Style::Hash);
        assert!(out.starts_with("#!/bin/sh\n# Copyright"), "{out}");
        assert!(out.ends_with("NOTICE files.\n"), "{out}");
        let out = with_header("#!/bin/sh\n", Style::Hash);
        assert!(out.ends_with("NOTICE files.\n"), "nothing after it: {out}");
    }

    /// In a repository of its own: the tracked file with no header is listed
    /// and given one; one that has it, one of another format and one not
    /// tracked are left as they are.
    #[test]
    fn the_tracked_files_without_a_header_are_listed_and_given_one() {
        let dir = super::super::tests::Scratch::new("headers");
        let root = &dir.0;
        let git = |args: &[&str]| {
            let done = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(done.status.success(), "git {args:?}: {done:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(root.join("bare.rs"), "fn main() {}\n").unwrap();
        let held = with_header("print(1)\n", Style::Hash);
        std::fs::write(root.join("held.py"), &held).unwrap();
        std::fs::write(root.join("notes.md"), "# Notes\n").unwrap();
        std::fs::write(root.join("picture.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        git(&["add", "bare.rs", "held.py", "notes.md", "picture.bin"]);
        std::fs::write(root.join("untracked.rs"), "fn x() {}\n").unwrap();

        assert_eq!(
            missing(root).unwrap(),
            [("bare.rs".to_string(), Style::Slashes)]
        );
        assert_eq!(fix(root).unwrap(), 1);
        let fixed = std::fs::read_to_string(root.join("bare.rs")).unwrap();
        assert!(
            has_header(&fixed) && fixed.ends_with("fn main() {}\n"),
            "{fixed}"
        );
        assert_eq!(std::fs::read_to_string(root.join("held.py")).unwrap(), held);
        assert_eq!(
            std::fs::read_to_string(root.join("untracked.rs")).unwrap(),
            "fn x() {}\n"
        );
        assert!(missing(root).unwrap().is_empty());
        assert_eq!(fix(root).unwrap(), 0);
    }
}
