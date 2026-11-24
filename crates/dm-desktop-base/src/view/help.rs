// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A dialog's help page, one click from it and offline (gaps row E11): the pages of
//! `docs/guide/` are built into dm, and the one asked for is written as a plain web page into
//! dm's home and opened in the browser. dm's own small reader of the Markdown the guide uses
//! (headings, paragraphs, lists, bold, code, links); nothing is fetched.

use std::path::{Path, PathBuf};

/// The guide's pages dm carries, by name.
const PAGES: &[(&str, &str)] = &[
    (
        "download-details",
        include_str!("../../../../docs/guide/download-details.md"),
    ),
    (
        "scheduler",
        include_str!("../../../../docs/guide/scheduler.md"),
    ),
    ("options", include_str!("../../../../docs/guide/options.md")),
    (
        "options-general",
        include_str!("../../../../docs/guide/options-general.md"),
    ),
    (
        "options-downloads",
        include_str!("../../../../docs/guide/options-downloads.md"),
    ),
    (
        "options-folders",
        include_str!("../../../../docs/guide/options-folders.md"),
    ),
    (
        "options-advanced",
        include_str!("../../../../docs/guide/options-advanced.md"),
    ),
    (
        "options-browser",
        include_str!("../../../../docs/guide/options-browser.md"),
    ),
    (
        "properties",
        include_str!("../../../../docs/guide/properties.md"),
    ),
    (
        "download-window",
        include_str!("../../../../docs/guide/download-window.md"),
    ),
    ("grabber", include_str!("../../../../docs/guide/grabber.md")),
    (
        "speed-limiter",
        include_str!("../../../../docs/guide/speed-limiter.md"),
    ),
    ("add-url", include_str!("../../../../docs/guide/add-url.md")),
    (
        "failures",
        include_str!("../../../../docs/guide/failures.md"),
    ),
];

/// `text` with the characters HTML reads as its own escaped.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// One line's inline marks: `code`, **bold**, and [words](link) as their words alone (a link
/// to another page of the guide is not there offline).
fn inline(line: &str) -> String {
    let mut out = String::new();
    let mut rest = escaped(line);
    // Links first, so their words get the other marks.
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|c| open + c) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|e| close + e) else {
            break;
        };
        let words = rest[open + 1..close].to_string();
        rest = format!("{}{}{}", &rest[..open], words, &rest[end + 1..]);
    }
    let mut code = false;
    for (i, part) in rest.split('`').enumerate() {
        if i > 0 {
            out.push_str(if code { "</code>" } else { "<code>" });
            code = !code;
        }
        if code {
            out.push_str(part);
        } else {
            let mut bold = false;
            for (j, piece) in part.split("**").enumerate() {
                if j > 0 {
                    out.push_str(if bold { "</b>" } else { "<b>" });
                    bold = !bold;
                }
                out.push_str(piece);
            }
            if bold {
                out.push_str("</b>");
            }
        }
    }
    if code {
        out.push_str("</code>");
    }
    out
}

/// The guide's Markdown `text` as an HTML page.
pub fn page_html(text: &str) -> String {
    let mut body = String::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut in_list = false;
    let flush = |body: &mut String, paragraph: &mut Vec<String>| {
        if !paragraph.is_empty() {
            body.push_str(&format!("<p>{}</p>\n", inline(&paragraph.join(" "))));
            paragraph.clear();
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            flush(&mut body, &mut paragraph);
            if !in_list {
                body.push_str("<ul>\n");
                in_list = true;
            }
            body.push_str(&format!("<li>{}", inline(item)));
            continue;
        }
        // A list item's next lines, indented, go on in it.
        if in_list && line.starts_with("  ") && !trimmed.is_empty() {
            body.push(' ');
            body.push_str(&inline(trimmed));
            continue;
        }
        if in_list {
            body.push_str("</li>\n</ul>\n");
            in_list = false;
        }
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=4).contains(&level) && trimmed[level..].starts_with(' ') {
            flush(&mut body, &mut paragraph);
            body.push_str(&format!(
                "<h{level}>{}</h{level}>\n",
                inline(trimmed[level..].trim())
            ));
        } else if trimmed.is_empty() {
            flush(&mut body, &mut paragraph);
        } else {
            paragraph.push(trimmed.to_string());
        }
    }
    flush(&mut body, &mut paragraph);
    if in_list {
        body.push_str("</li>\n</ul>\n");
    }
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>dm help</title>\n\
         <style>body{{font:16px/1.5 system-ui,sans-serif;max-width:46em;margin:2em auto;\
         padding:0 1em;color:#1b1b1b;background:#fff}}code{{background:#f0f0f0;\
         padding:0 .2em;border-radius:3px}}@media(prefers-color-scheme:dark){{body{{\
         color:#e8e8e8;background:#1e1e1e}}code{{background:#333}}}}</style></head>\n\
         <body>\n{body}</body></html>\n"
    )
}

/// Writes the help page `name` into `home`'s `help` folder as a web page, giving its path; an
/// error for a page dm does not carry, or one it could not write.
pub fn write_page(home: &Path, name: &str) -> Result<PathBuf, String> {
    let (_, text) = PAGES
        .iter()
        .find(|(page, _)| *page == name)
        .ok_or_else(|| format!("dm has no help page called {name}"))?;
    let folder = home.join("help");
    std::fs::create_dir_all(&folder).map_err(|e| format!("the help could not be written: {e}"))?;
    let path = folder.join(format!("{name}.html"));
    std::fs::write(&path, page_html(text))
        .map_err(|e| format!("the help could not be written: {e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guides_markdown_reads_as_a_page() {
        let html = page_html(
            "# Download details\n\nA **bold** word and `code` and [a link](x.md).\n\n\
             - one\n  more of one\n- two\n\n## After <b>",
        );
        assert!(html.contains("<h1>Download details</h1>"));
        assert!(html.contains("<p>A <b>bold</b> word and <code>code</code> and a link.</p>"));
        assert!(html.contains("<li>one more of one"));
        assert!(html.contains("<li>two"));
        assert!(html.contains("<h2>After &lt;b&gt;</h2>"), "{html}");
    }

    #[test]
    fn every_page_carried_is_written_and_an_unknown_one_is_refused() {
        let home = tempfile::tempdir().unwrap();
        for (name, _) in PAGES {
            let path = write_page(home.path(), name).unwrap();
            assert!(
                std::fs::read_to_string(path).unwrap().contains("<h1>"),
                "{name}"
            );
        }
        assert!(write_page(home.path(), "nothing").is_err());
    }
}
