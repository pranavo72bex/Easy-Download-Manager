// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask i18n`: for each language, the strings its catalog lacks against the template
//! and the strings no native speaker has read yet (gaps row W23, `docs/guide/translating.md`).
//!
//! - `desktop/assets/i18n/<code>.json` is a language's catalog: the English to its translation.
//! - `desktop/assets/i18n/template/catalog.json` is the file a new language starts from: every
//!   English string the window says, each with an empty translation.
//! - `desktop/assets/i18n/unread/<code>.json` lists the English of each string whose
//!   translation no native speaker has read yet; a string read is taken out of it.
//!
//! A string is missing when the catalog lacks it or leaves it empty. The command fails only on
//! a malformed file: one that is not JSON of the right shape, a string given twice, a
//! translation that takes other values than its English, or an unread list naming a string
//! the catalog does not have. Missing and unread strings are work, not faults.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

/// Where the catalogs are, from the repository's root.
const DIR: &str = "desktop/assets/i18n";
/// The file a new language is copied from.
const TEMPLATE: &str = "desktop/assets/i18n/template/catalog.json";
/// Each language's list of the strings no native speaker has read.
const UNREAD: &str = "desktop/assets/i18n/unread";
/// What divides a sentence's forms for a count (`dm_desktop_base::i18n::FORMS`).
const FORMS: &str = "||";

/// The languages' names in English, for the issue a reader takes; a code not here is named by
/// its code.
const NAMES: [(&str, &str); 13] = [
    ("de", "German"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("id", "Indonesian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("pt-BR", "Portuguese (Brazil)"),
    ("ru", "Russian"),
    ("tr", "Turkish"),
    ("zh-CN", "Chinese (Simplified)"),
];

const USAGE: &str = "usage: cargo xtask i18n [<code> | --issue <code>]";

/// `cargo xtask i18n [<code> | --issue <code>]`: every language's counts, one language's
/// strings, or the text of an issue a reader of that language can take.
pub fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let template = read_template(root)?;
    let languages = read_languages(root, &template)?;
    let malformed: Vec<&Report> = languages
        .iter()
        .filter(|r| !r.problems.is_empty())
        .collect();
    let said = match args {
        [] => summary(&template, &languages),
        [flag, code] if flag == "--issue" => issue(find(&languages, code)?),
        [code] if !code.starts_with('-') => detail(find(&languages, code)?),
        _ => return Err(USAGE.to_string()),
    };
    if malformed.is_empty() {
        return Ok(said);
    }
    let mut why = said;
    for report in malformed {
        let _ = writeln!(why, "\n{} is malformed:", report.code);
        for problem in &report.problems {
            let _ = writeln!(why, "  {problem}");
        }
    }
    Err(why.trim_end().to_string())
}

fn find<'a>(languages: &'a [Report], code: &str) -> Result<&'a Report, String> {
    languages.iter().find(|r| r.code == code).ok_or_else(|| {
        let codes: Vec<&str> = languages.iter().map(|r| r.code.as_str()).collect();
        format!(
            "no catalog {DIR}/{code}.json; the catalogs are {}",
            codes.join(", ")
        )
    })
}

/// The template's English strings, in its order.
fn read_template(root: &Path) -> Result<Vec<String>, String> {
    let text = read(&root.join(TEMPLATE))?;
    let entries = object(&text).map_err(|why| format!("{TEMPLATE}: {why}"))?;
    let mut seen = BTreeSet::new();
    let mut english = Vec::new();
    for (key, _, line) in entries {
        if !seen.insert(key.clone()) {
            return Err(format!("{TEMPLATE}: line {line}: {key:?} is given twice"));
        }
        english.push(key);
    }
    Ok(english)
}

/// Each catalog in `DIR`, checked against the template and its unread list, by code.
fn read_languages(root: &Path, template: &[String]) -> Result<Vec<Report>, String> {
    let dir = root.join(DIR);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut codes: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let code = name.strip_suffix(".json")?.to_string();
            entry.path().is_file().then_some(code)
        })
        .collect();
    codes.sort();
    let mut reports = Vec::new();
    for code in codes {
        let catalog = read(&dir.join(format!("{code}.json")))?;
        let unread_path = root.join(UNREAD).join(format!("{code}.json"));
        let unread = if unread_path.is_file() {
            Some(read(&unread_path)?)
        } else {
            None
        };
        reports.push(check(&code, template, &catalog, unread.as_deref()));
    }
    Ok(reports)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/* ------------------------------------------------------------- the check */

/// One language's catalog held to the template and its unread list.
#[derive(Debug, Default)]
struct Report {
    code: String,
    /// The template's strings, how many.
    wanted: usize,
    /// The template's strings the catalog lacks or leaves empty, in the template's order.
    missing: Vec<String>,
    /// The catalog's strings the template does not have: no longer said, or a template behind.
    extra: Vec<String>,
    /// The strings no native speaker has read, with their translations, in the list's order.
    unread: Vec<(String, String)>,
    /// What makes the catalog or its list malformed.
    problems: Vec<String>,
}

/// `catalog` (and `unread`, the text of its unread list, if it has one) held to `template`.
fn check(code: &str, template: &[String], catalog: &str, unread: Option<&str>) -> Report {
    let mut report = Report {
        code: code.to_string(),
        wanted: template.len(),
        ..Report::default()
    };
    let entries = match object(catalog) {
        Ok(entries) => entries,
        Err(why) => {
            report.problems.push(format!("{code}.json: {why}"));
            return report;
        }
    };
    let mut have: BTreeMap<&str, &str> = BTreeMap::new();
    for (english, translated, line) in &entries {
        if have.insert(english, translated).is_some() {
            report.problems.push(format!(
                "{code}.json: line {line}: {english:?} is given twice"
            ));
            continue;
        }
        if translated.trim().is_empty() {
            continue;
        }
        if let Some(why) = values_differ(english, translated) {
            report
                .problems
                .push(format!("{code}.json: line {line}: {english:?}: {why}"));
        }
    }
    let template_set: BTreeSet<&str> = template.iter().map(String::as_str).collect();
    report.missing = template
        .iter()
        .filter(|english| {
            have.get(english.as_str())
                .is_none_or(|translated| translated.trim().is_empty())
        })
        .cloned()
        .collect();
    report.extra = entries
        .iter()
        .map(|(english, _, _)| english)
        .filter(|english| !template_set.contains(english.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let Some(unread) = unread else {
        return report;
    };
    let listed = match array(unread) {
        Ok(listed) => listed,
        Err(why) => {
            report.problems.push(format!("unread/{code}.json: {why}"));
            return report;
        }
    };
    let mut seen = BTreeSet::new();
    for (english, line) in listed {
        if !seen.insert(english.clone()) {
            report.problems.push(format!(
                "unread/{code}.json: line {line}: {english:?} is listed twice"
            ));
            continue;
        }
        match have.get(english.as_str()) {
            None => report.problems.push(format!(
                "unread/{code}.json: line {line}: {english:?} is not in {code}.json"
            )),
            // Not translated yet: missing, not unread.
            Some(translated) if translated.trim().is_empty() => {}
            Some(translated) => report.unread.push((english, (*translated).to_string())),
        }
    }
    report
}

/// Why `translated` cannot stand for `english`: it takes other values (`{}`, `{0}`) in one of
/// its forms, or it has forms for a count where the English has none.
fn values_differ(english: &str, translated: &str) -> Option<String> {
    if !english.contains(FORMS) && translated.contains(FORMS) {
        return Some(format!(
            "its translation has forms ({FORMS}) its English has not"
        ));
    }
    let first = english.split(FORMS).next().unwrap_or_default();
    let wanted = placeholders(first);
    translated.split(FORMS).find_map(|form| {
        let got = placeholders(form);
        (got != wanted).then(|| {
            format!("its translation takes the values {got:?} where its English takes {wanted:?}")
        })
    })
}

/// The values a template takes, by position, sorted: `{}` takes the next in turn, `{N}` the
/// Nth; any other brace is words (as `dm_desktop_base::i18n::placeholders`).
fn placeholders(template: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut next = 0;
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let inside = after.find('}').map(|close| &after[..close]);
        match inside {
            Some("") => {
                found.push(next);
                next += 1;
                rest = &after[1..];
            }
            Some(n) if n.bytes().all(|b| b.is_ascii_digit()) => match n.parse() {
                Ok(index) => {
                    found.push(index);
                    rest = &after[n.len() + 1..];
                }
                Err(_) => rest = after,
            },
            _ => rest = after,
        }
    }
    found.sort_unstable();
    found
}

/* ------------------------------------------------------------ what it says */

/// Every language's counts.
fn summary(template: &[String], languages: &[Report]) -> String {
    let mut out = format!(
        "{TEMPLATE}: {} English strings\n\n{:<8} {:>11} {:>8} {:>8} {:>17}\n",
        template.len(),
        "language",
        "translated",
        "missing",
        "unread",
        "not in template"
    );
    for r in languages {
        let state = if r.problems.is_empty() {
            ""
        } else {
            "  MALFORMED"
        };
        let _ = writeln!(
            out,
            "{:<8} {:>11} {:>8} {:>8} {:>17}{state}",
            r.code,
            format!("{}/{}", r.wanted - r.missing.len(), r.wanted),
            r.missing.len(),
            r.unread.len(),
            r.extra.len()
        );
    }
    let missing: usize = languages.iter().map(|r| r.missing.len()).sum();
    let unread: usize = languages.iter().map(|r| r.unread.len()).sum();
    let read_by_none = languages.iter().filter(|r| !r.unread.is_empty()).count();
    let _ = write!(
        out,
        "\n{} catalogs: {missing} strings missing; {unread} unread by a native speaker, in {read_by_none} languages.\n\
         `cargo xtask i18n <code>` lists one language's; `cargo xtask i18n --issue <code>` writes the issue a reader takes.",
        languages.len()
    );
    out
}

/// One language's missing, unread and stray strings.
fn detail(r: &Report) -> String {
    let mut out = format!(
        "{}: {} of {} strings translated, {} missing, {} unread by a native speaker, {} not in the template\n",
        r.code,
        r.wanted - r.missing.len(),
        r.wanted,
        r.missing.len(),
        r.unread.len(),
        r.extra.len()
    );
    if !r.missing.is_empty() {
        let _ = writeln!(out, "\nMissing ({}):", r.missing.len());
        for english in &r.missing {
            let _ = writeln!(out, "  {english:?}");
        }
    }
    if !r.unread.is_empty() {
        let _ = writeln!(out, "\nUnread ({}):", r.unread.len());
        for (english, translated) in &r.unread {
            let _ = writeln!(out, "  {english:?}\n    {translated:?}");
        }
    }
    if !r.extra.is_empty() {
        let _ = writeln!(
            out,
            "\nNot in the template ({}): no longer said, or the template is behind:",
            r.extra.len()
        );
        for english in &r.extra {
            let _ = writeln!(out, "  {english:?}");
        }
    }
    out.trim_end().to_string()
}

/// The most strings one issue lists: an issue's text holds 65,536 characters at most, and a
/// reader takes a short list sooner than a long one.
const PER_ISSUE: usize = 100;

/// The text of the issue a reader of `r`'s language takes: what to do, then each unread string
/// with its translation, to tick off. A longer list is cut into several issues, one after the
/// other, each with its own title.
fn issue(r: &Report) -> String {
    let code = r.code.as_str();
    let name = NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(code, |(_, n)| *n);
    if r.unread.is_empty() {
        return format!(
            "{name} ({code}): every string has been read by a native speaker; no issue to post."
        );
    }
    let parts: Vec<&[(String, String)]> = r.unread.chunks(PER_ISSUE).collect();
    let mut issues = Vec::new();
    for (at, part) in parts.iter().enumerate() {
        let of = if parts.len() > 1 {
            format!(" (part {} of {})", at + 1, parts.len())
        } else {
            String::new()
        };
        issues.push(issue_part(code, name, part, &of));
    }
    issues.join("\n\n======== the next issue ========\n\n")
}

fn issue_part(code: &str, name: &str, unread: &[(String, String)], of: &str) -> String {
    let n = unread.len();
    let mut out = format!(
        "Title: {name} ({code}): read {n} of dm's strings that no native speaker has read{of}\n\n\
         dm's window says the {n} strings below in {name}, but no native speaker has read their \
         translation yet. If {name} is your first language, you can take this issue: say so in a \
         comment, so two readers do not do the same work. Reading part of the list helps too.\n\n\
         1. Read [docs/guide/translating.md](https://github.com/pranavo72bex/Easy-Download-Manager/blob/main/docs/guide/translating.md).\n\
         2. In `{DIR}/{code}.json`, mend each translation below that reads wrong, too long or \
         unnatural. Keep each `{{}}`, `{{0}}` and `||` as the English has them.\n\
         3. Take each string you have read, mended or not, out of `{UNREAD}/{code}.json`.\n\
         4. In `desktop/`, run `cargo xtask i18n {code}`: it lists what is still unread and fails \
         on a file it cannot read.\n\
         5. Open a pull request that names this issue.\n\n\
         Each line is the English, then the {name} dm says now.\n\n"
    );
    for (english, translated) in unread {
        let _ = writeln!(
            out,
            "- [ ] {}\n  {}",
            code_span(english),
            code_span(translated)
        );
    }
    out.trim_end().to_string()
}

/// `text` as a Markdown code span, so `<`, `*` and `|` in it read as written; a line break
/// is shown as `\n`.
fn code_span(text: &str) -> String {
    let text = text.replace('\n', "\\n");
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/* ------------------------------------------------------------------ JSON */

/// A JSON object whose values are all strings: each entry with the line it is on, in the
/// file's order, a key given twice kept twice for the caller to name.
fn object(text: &str) -> Result<Vec<(String, String, usize)>, String> {
    let mut json = Reader::new(text);
    let mut entries = Vec::new();
    json.expect('{')?;
    if !json.eat('}') {
        loop {
            json.space();
            let line = json.line();
            let key = json.string()?;
            json.expect(':')?;
            let value = json.string()?;
            entries.push((key, value, line));
            if json.eat('}') {
                break;
            }
            json.expect(',')?;
        }
    }
    json.end()?;
    Ok(entries)
}

/// A JSON array of strings: each with the line it is on.
fn array(text: &str) -> Result<Vec<(String, usize)>, String> {
    let mut json = Reader::new(text);
    let mut items = Vec::new();
    json.expect('[')?;
    if !json.eat(']') {
        loop {
            json.space();
            let line = json.line();
            items.push((json.string()?, line));
            if json.eat(']') {
                break;
            }
            json.expect(',')?;
        }
    }
    json.end()?;
    Ok(items)
}

/// Just enough of a JSON reader for the catalogs: strings, objects and arrays of them, and
/// the line each fault is on, which a person mending a file by hand needs.
struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Reader {
            text: text.strip_prefix('\u{feff}').unwrap_or(text),
            at: 0,
        }
    }

    fn line(&self) -> usize {
        self.text[..self.at].matches('\n').count() + 1
    }

    fn fail(&self, what: &str) -> String {
        format!("line {}: {what}", self.line())
    }

    fn space(&mut self) {
        let rest = &self.text[self.at..];
        self.at += rest.len() - rest.trim_start_matches([' ', '\t', '\n', '\r']).len();
    }

    /// Takes `c` after any space, if it is next.
    fn eat(&mut self, c: char) -> bool {
        self.space();
        if self.text[self.at..].starts_with(c) {
            self.at += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.eat(c) {
            return Ok(());
        }
        let found = self.text[self.at..]
            .chars()
            .next()
            .map_or("the end of the file".to_string(), |f| format!("{f:?}"));
        Err(self.fail(&format!("expected {c:?}, found {found}")))
    }

    /// Nothing but space after what was read.
    fn end(&mut self) -> Result<(), String> {
        self.space();
        if self.at == self.text.len() {
            Ok(())
        } else {
            Err(self.fail("more after the end of the JSON"))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat('"') {
            return Err(self.fail("expected a string in double quotes"));
        }
        let mut out = String::new();
        loop {
            let Some(c) = self.text[self.at..].chars().next() else {
                return Err(self.fail("a string that never ends"));
            };
            self.at += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                c if u32::from(c) < 0x20 => {
                    return Err(
                        self.fail("a line break or control character inside a string (write \\n)")
                    )
                }
                c => out.push(c),
            }
        }
    }

    /// The character an escape after `\` stands for.
    fn escape(&mut self) -> Result<char, String> {
        let Some(c) = self.text[self.at..].chars().next() else {
            return Err(self.fail("a string that never ends"));
        };
        self.at += c.len_utf8();
        Ok(match c {
            '"' => '"',
            '\\' => '\\',
            '/' => '/',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'u' => {
                let high = self.hex()?;
                if (0xD800..0xDC00).contains(&high) {
                    if !self.text[self.at..].starts_with("\\u") {
                        return Err(self.fail("half of a surrogate pair"));
                    }
                    self.at += 2;
                    let low = self.hex()?;
                    if !(0xDC00..0xE000).contains(&low) {
                        return Err(self.fail("half of a surrogate pair"));
                    }
                    let joined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                    char::from_u32(joined).ok_or_else(|| self.fail("a bad \\u escape"))?
                } else {
                    char::from_u32(high).ok_or_else(|| self.fail("a bad \\u escape"))?
                }
            }
            other => return Err(self.fail(&format!("\\{other} is not a JSON escape"))),
        })
    }

    fn hex(&mut self) -> Result<u32, String> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.fail("\\u wants four hexadecimal digits"))?;
        self.at += 4;
        u32::from_str_radix(digits, 16).map_err(|_| self.fail("a bad \\u escape"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template() -> Vec<String> {
        ["Open", "{} of {}", "{} file||{} files", "Close"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn a_complete_catalog_with_nothing_unread_has_nothing_to_do() {
        let catalog = r#"{
 "Open": "Öffnen",
 "{} of {}": "{} von {}",
 "{} file||{} files": "{} Datei||{} Dateien",
 "Close": "Schließen"
}"#;
        let r = check("de", &template(), catalog, None);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(r.missing.is_empty() && r.unread.is_empty() && r.extra.is_empty());
        assert_eq!(r.wanted, 4);
    }

    #[test]
    fn a_string_absent_or_empty_is_missing_and_one_the_template_lacks_is_extra() {
        let catalog = r#"{"Open": "Abrir", "Close": "  ", "Gone": "Ido"}"#;
        let r = check("es", &template(), catalog, None);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.missing, ["{} of {}", "{} file||{} files", "Close"]);
        assert_eq!(r.extra, ["Gone"]);
    }

    #[test]
    fn the_unread_list_names_strings_with_their_translations() {
        let catalog = r#"{"Open": "Ouvrir", "Close": "Fermer", "{} of {}": ""}"#;
        let unread = "[\n \"Close\",\n \"{} of {}\"\n]";
        let r = check("fr", &template(), catalog, Some(unread));
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        // An empty one is missing, not unread.
        assert_eq!(r.unread, [("Close".to_string(), "Fermer".to_string())]);
    }

    #[test]
    fn an_unread_list_naming_a_string_twice_or_one_the_catalog_lacks_is_malformed() {
        let catalog = r#"{"Open": "Apri"}"#;
        let unread = "[\n \"Open\",\n \"Open\",\n \"Shut\"\n]";
        let r = check("it", &template(), catalog, Some(unread));
        assert_eq!(r.problems.len(), 2, "{:?}", r.problems);
        assert!(r.problems[0].contains("line 3") && r.problems[0].contains("twice"));
        assert!(r.problems[1].contains("line 4") && r.problems[1].contains("not in it.json"));
        let r = check("it", &template(), catalog, Some("{\"Open\": \"x\"}"));
        assert!(r.problems[0].starts_with("unread/it.json: line 1: expected '['"));
    }

    #[test]
    fn a_catalog_that_is_not_an_object_of_strings_is_malformed_with_its_line() {
        for (text, said) in [
            ("[]", "line 1: expected '{'"),
            ("{\"Open\": 1}", "expected a string"),
            (
                "{\n\"Open\": \"a\"\n\"Close\": \"b\"}",
                "line 3: expected ','",
            ),
            ("{\"Open\": \"a\"} x", "more after the end"),
            ("{\"Open\": \"a", "never ends"),
            ("{\"Open\": \"a\nb\"}", "control character"),
            ("{\"Open\": \"\\q\"}", "not a JSON escape"),
        ] {
            let r = check("nl", &template(), text, None);
            assert_eq!(r.problems.len(), 1, "{text}");
            assert!(r.problems[0].contains(said), "{text}: {:?}", r.problems);
        }
    }

    #[test]
    fn a_string_given_twice_is_malformed() {
        let r = check(
            "pl",
            &template(),
            "{\"Open\": \"a\",\n\"Open\": \"b\"}",
            None,
        );
        assert_eq!(r.problems, ["pl.json: line 2: \"Open\" is given twice"]);
    }

    #[test]
    fn a_translation_must_take_its_englishs_values_in_each_form() {
        let catalog = r#"{
 "{} of {}": "{1} из {0}",
 "{} file||{} files": "{} файл||{} файла||файлов"
}"#;
        let r = check("ru", &template(), catalog, None);
        assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
        assert!(r.problems[0].contains("line 3"), "{:?}", r.problems);
        let r = check("tr", &template(), r#"{"Open": "Aç||Açın"}"#, None);
        assert!(r.problems[0].contains("has forms"), "{:?}", r.problems);
    }

    #[test]
    fn placeholders_are_read_as_the_window_reads_them() {
        assert_eq!(placeholders("{} of {}"), [0, 1]);
        assert_eq!(placeholders("{1} of {0}"), [0, 1]);
        assert_eq!(placeholders("{name} {+1} { } {"), Vec::<usize>::new());
        assert_eq!(placeholders("{}{}%"), [0, 1]);
    }

    #[test]
    fn json_escapes_are_read() {
        let entries = object(r#"{"a\"\\\/\n\t\u00e9\ud83d\ude00": "b"}"#).unwrap();
        assert_eq!(entries[0].0, "a\"\\/\n\té\u{1F600}");
        assert!(object(r#"{"\ud83d": "b"}"#).is_err());
        assert_eq!(object("\u{feff}{}").unwrap(), []);
        assert_eq!(array(" [ ] ").unwrap(), []);
    }

    #[test]
    fn the_summary_detail_and_issue_say_what_is_left() {
        let catalog = r#"{"Open": "Ouvrir", "Close": "<Fermer> | `x`"}"#;
        let r = check("fr", &template(), catalog, Some(r#"["Close"]"#));
        let all = summary(&template(), std::slice::from_ref(&r));
        assert!(
            all.contains("fr               2/4        2        1"),
            "{all}"
        );
        assert!(
            all.contains("1 catalogs: 2 strings missing; 1 unread"),
            "{all}"
        );
        let one = detail(&r);
        assert!(one.contains("Missing (2):\n  \"{} of {}\""), "{one}");
        assert!(
            one.contains("Unread (1):\n  \"Close\"\n    \"<Fermer> | `x`\""),
            "{one}"
        );
        let text = issue(&r);
        assert!(
            text.starts_with("Title: French (fr): read 1 of dm's strings"),
            "{text}"
        );
        assert!(
            text.contains("- [ ] `Close`\n  `` <Fermer> | `x` ``"),
            "{text}"
        );
        assert!(
            text.contains("desktop/assets/i18n/unread/fr.json"),
            "{text}"
        );
        let done = check("fr", &template(), catalog, None);
        assert!(issue(&done).contains("no issue to post"));
    }

    #[test]
    fn a_long_list_is_cut_into_issues_of_a_hundred() {
        let words: Vec<String> = (0..250).map(|i| format!("Word {i}")).collect();
        let catalog = format!(
            "{{{}}}",
            words
                .iter()
                .map(|w| format!("{w:?}: \"x\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        let unread = format!("{words:?}");
        let r = check("ko", &words, &catalog, Some(&unread));
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let text = issue(&r);
        let titles: Vec<&str> = text.lines().filter(|l| l.starts_with("Title:")).collect();
        assert_eq!(
            titles,
            [
                "Title: Korean (ko): read 100 of dm's strings that no native speaker has read (part 1 of 3)",
                "Title: Korean (ko): read 100 of dm's strings that no native speaker has read (part 2 of 3)",
                "Title: Korean (ko): read 50 of dm's strings that no native speaker has read (part 3 of 3)",
            ]
        );
        assert_eq!(text.matches("- [ ] ").count(), 250);
    }

    #[test]
    fn a_code_span_holds_backticks_and_line_breaks() {
        assert_eq!(code_span("a"), "`a`");
        assert_eq!(code_span("a``b"), "```a``b```");
        assert_eq!(code_span("`a"), "`` `a ``");
        assert_eq!(code_span("a\nb"), "`a\\nb`");
    }

    /// The real catalogs: each well formed, each the template's strings exactly, so a string
    /// added to the window is added to the template a new language starts from, and an
    /// unread list names only strings its catalog has.
    #[test]
    fn the_catalogs_the_template_and_the_unread_lists_agree() {
        let root = crate::root();
        let template = read_template(&root).unwrap();
        assert!(template.len() > 150, "{}", template.len());
        let languages = read_languages(&root, &template).unwrap();
        assert!(languages.len() >= 13, "{}", languages.len());
        for r in &languages {
            assert!(r.problems.is_empty(), "{}: {:?}", r.code, r.problems);
            assert!(
                r.missing.is_empty(),
                "{}: the template has {:?}",
                r.code,
                r.missing
            );
            assert!(
                r.extra.is_empty(),
                "{}: the template lacks {:?}",
                r.code,
                r.extra
            );
        }
        // The template's own translations are empty: a copy starts with every string missing.
        let text = read(&root.join(TEMPLATE)).unwrap();
        assert!(object(&text).unwrap().iter().all(|(_, v, _)| v.is_empty()));
        // Every unread list is a catalog's.
        for entry in std::fs::read_dir(root.join(UNREAD)).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let code = name.strip_suffix(".json").unwrap_or(&name);
            assert!(
                languages.iter().any(|r| r.code == code),
                "unread/{name} is no catalog's"
            );
        }
        assert!(run(&root, &[]).is_ok());
        assert!(run(&root, &["xx".to_string()]).is_err());
        assert!(run(&root, &["--issue".to_string()]).is_err());
    }
}
