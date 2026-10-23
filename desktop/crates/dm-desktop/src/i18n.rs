//! The window's words in the language a person picked in View › Language.
//!
//! Keyed by the English text itself: English needs no catalog, a string
//! with no translation yet reads in English rather than as a key, and the
//! tests, which find controls by their English names, run unchanged. Each
//! catalog is a JSON object from English to the translation, compiled in.
//! `tests/i18n.rs` holds every string the window passes to `t` or `tf`, and
//! every word it chooses at run time, to be in every catalog and drawable in
//! the window's fonts.
//!
//! Only scripts egui draws correctly are offered: Latin and Cyrillic.
//! Right-to-left and joined scripts (Arabic, Hindi) need text shaping egui
//! does not do, and would be drawn wrong.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::sync::OnceLock;

/// Each language offered: its code, as the settings keep it, and its name
/// in itself, as a list of languages should show it.
pub const LANGUAGES: [(&str, &str); 14] = [
    ("en", "English"),
    ("es", "Español"),
    ("pt-BR", "Português (Brasil)"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("it", "Italiano"),
    ("nl", "Nederlands"),
    ("pl", "Polski"),
    ("ru", "Русский"),
    ("id", "Bahasa Indonesia"),
    ("tr", "Türkçe"),
    ("zh-CN", "简体中文"),
    ("ja", "日本語"),
    ("ko", "한국어"),
];

/// The catalogs, as written.
pub const CATALOGS: [(&str, &str); 13] = [
    ("es", include_str!("../i18n/es.json")),
    ("pt-BR", include_str!("../i18n/pt-BR.json")),
    ("fr", include_str!("../i18n/fr.json")),
    ("de", include_str!("../i18n/de.json")),
    ("it", include_str!("../i18n/it.json")),
    ("nl", include_str!("../i18n/nl.json")),
    ("pl", include_str!("../i18n/pl.json")),
    ("ru", include_str!("../i18n/ru.json")),
    ("id", include_str!("../i18n/id.json")),
    ("tr", include_str!("../i18n/tr.json")),
    ("zh-CN", include_str!("../i18n/zh-CN.json")),
    ("ja", include_str!("../i18n/ja.json")),
    ("ko", include_str!("../i18n/ko.json")),
];

/// English to translation. Held in a static, so what it holds is `'static`
/// without leaking each string.
type Catalog = HashMap<String, String>;

/// Names dm itself gives, which a person cannot tell from its own: the two
/// queues and the categories' default folders. Only these are translated
/// where a name is shown; a name a person chose stays as typed.
pub const BUILT_IN_NAMES: [&str; 9] = [
    dm_core::MAIN_QUEUE,
    dm_core::RECHECK_QUEUE,
    "Videos",
    "Audio",
    "Documents",
    "Apps",
    "Archives",
    "Pictures",
    "Other",
];

/* ---------------------------------------------------------- languages */

/// A language's catalog, read the first time that language is put in
/// force: English reads none, and a session in one language reads one. A
/// catalog that does not parse is empty, so the window falls back to
/// English rather than failing to open; the tests hold each to parsing.
fn catalog(code: &str) -> Option<&'static Catalog> {
    static READ: [OnceLock<Catalog>; CATALOGS.len()] = [const { OnceLock::new() }; CATALOGS.len()];
    let at = CATALOGS.iter().position(|(c, _)| *c == code)?;
    let (_, text) = CATALOGS.get(at)?;
    Some(
        READ.get(at)?
            .get_or_init(|| serde_json::from_str(text).unwrap_or_default()),
    )
}

thread_local! {
    /// The language in force on this thread. The window is drawn on one
    /// thread, so this is the window's; tests, each on a thread of its own,
    /// can put one window in German while another is in English.
    static NOW: Cell<Option<&'static Catalog>> = const { Cell::new(None) };

    /// The code of the language in force on this thread, for its plurals.
    static CODE: Cell<&'static str> = const { Cell::new("en") };

    /// What was asked for and had no translation, on this thread, while a
    /// test is recording; `None`, and nothing kept, otherwise.
    static MISSED: RefCell<Option<BTreeSet<&'static str>>> = const { RefCell::new(None) };
}

/// Puts a language in force on this thread; an unknown code, or English,
/// is English.
pub fn set(code: &str) {
    NOW.with(|now| now.set(catalog(code)));
    let known = CATALOGS
        .iter()
        .map(|(c, _)| *c)
        .find(|c| *c == code)
        .unwrap_or("en");
    CODE.with(|now| now.set(known));
}

/// The code of the language in force on this thread ("en" for English).
pub fn code() -> &'static str {
    CODE.with(Cell::get)
}

/// The code of a language dm offers, as `LANGUAGES` spells it.
fn offered(code: &str) -> Option<&'static str> {
    LANGUAGES.iter().map(|(c, _)| *c).find(|c| *c == code)
}

/// The language in force for a setting: the one it names, or the system's
/// where it names none.
pub fn chosen(setting: &str) -> &'static str {
    offered(setting).unwrap_or_else(system)
}

/// The languages to choose from, the one in force marked; the one picked,
/// if any. View › Language and Options › General both offer it.
pub fn pick(ui: &mut egui::Ui, now: &str) -> Option<&'static str> {
    // Each language's name in its own script: 日本語 and 한국어 need the
    // computer's fonts for them, found the first time the list is shown.
    crate::theme::cover(ui.ctx(), LANGUAGES.iter().map(|(_, name)| *name));
    let mut picked = None;
    for (code, name) in LANGUAGES {
        if crate::dialogs::radio_flag(ui, now == code, name.to_string()).clicked() {
            picked = Some(code);
        }
    }
    picked
}

/// The language a code names, for the settings' choice; English if unknown.
pub fn name_of(code: &str) -> &'static str {
    LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or("English", |(_, name)| name)
}

/// The language the system is set to, where it is one dm has; English
/// otherwise. The first launch starts in it.
pub fn system() -> &'static str {
    // It cannot change while dm runs, and the Language menu asks each frame.
    static FOUND: OnceLock<&'static str> = OnceLock::new();
    FOUND.get_or_init(|| {
        let named = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok());
        resolve(named.as_deref(), sys_locale::get_locale().as_deref())
    })
}

/// The language a locale setting comes to, as a C library reads it: the
/// environment's first non-empty setting, else the system's own (an app
/// opened from the Dock has no environment); "C" or "POSIX" (C.UTF-8 too)
/// is English; `de_DE.UTF-8@euro` is German; a region dm lacks takes the
/// language's other (pt-PT is pt-BR); anything else is English.
fn resolve(environment: Option<&str>, os: Option<&str>) -> &'static str {
    let locale = environment
        .filter(|v| !v.is_empty())
        .or(os)
        .unwrap_or_default();
    let wanted = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    if wanted == "C" || wanted == "POSIX" {
        return "en";
    }
    let base = |code: &str| code.split('-').next().unwrap_or_default().to_owned();
    LANGUAGES
        .iter()
        .map(|(c, _)| *c)
        .find(|c| c.eq_ignore_ascii_case(&wanted))
        .or_else(|| {
            let language = base(&wanted);
            LANGUAGES
                .iter()
                .map(|(c, _)| *c)
                .find(|c| base(c).eq_ignore_ascii_case(&language))
        })
        .unwrap_or("en")
}

/* ------------------------------------------------------------- words */

/// The words in the language in force.
pub fn t(english: &'static str) -> &'static str {
    let Some(catalog) = NOW.with(Cell::get) else {
        return english;
    };
    match catalog.get(english) {
        Some(translated) => translated,
        None => {
            MISSED.with(|missed| {
                if let Some(missed) = missed.borrow_mut().as_mut() {
                    missed.insert(english);
                }
            });
            english
        }
    }
}

/// Starts recording, on this thread, what `t` is asked for and lacks.
pub fn record_missing() {
    MISSED.with(|missed| *missed.borrow_mut() = Some(BTreeSet::new()));
}

/// What was recorded since `record_missing`, which stops recording.
pub fn take_missing() -> Vec<&'static str> {
    MISSED.with(|missed| {
        missed
            .borrow_mut()
            .take()
            .unwrap_or_default()
            .into_iter()
            .collect()
    })
}

/// A name as shown: translated if dm gave it, as typed otherwise.
pub fn name(text: &str) -> String {
    match BUILT_IN_NAMES.iter().find(|n| **n == text) {
        Some(built_in) => t(built_in).to_owned(),
        None => text.to_owned(),
    }
}

/// The queue a name typed or picked means: dm's own queues by their name
/// in the language in force (or in English), as stored; any other as
/// typed, trimmed. The inverse of `name` for the queues.
pub fn queue_named(typed: &str) -> String {
    let typed = typed.trim();
    [dm_core::MAIN_QUEUE, dm_core::RECHECK_QUEUE]
        .into_iter()
        .find(|q| *q == typed || t(q) == typed)
        .unwrap_or(typed)
        .to_owned()
}

/* ------------------------------------------------------ placeholders */

/// A piece of a template: words as written, or a value's place, `{}` in
/// turn or `{N}` by position, with the placeholder as written.
#[derive(Debug, PartialEq)]
enum Piece<'a> {
    Words(&'a str),
    Value(usize, &'a str),
}

/// A template cut into its words and its values' places, lazily: `fill`
/// runs for many strings every frame, and allocates nothing for this. A
/// brace that is not a placeholder (`{`, `{name}`, `}`) is words.
struct Pieces<'a> {
    rest: &'a str,
    /// The position the next `{}` takes.
    next: usize,
    /// A placeholder found after words, handed out on the following call.
    held: Option<Piece<'a>>,
}

fn pieces(template: &str) -> Pieces<'_> {
    Pieces {
        rest: template,
        next: 0,
        held: None,
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        if let Some(held) = self.held.take() {
            return Some(held);
        }
        if self.rest.is_empty() {
            return None;
        }
        let mut from = 0;
        while let Some(found) = self.rest.get(from..).and_then(|r| r.find('{')) {
            let open = from + found;
            let after = self.rest.get(open + 1..).unwrap_or_default();
            let inside = after.find('}').and_then(|c| after.get(..c));
            let index = match inside {
                Some("") => Some(self.next),
                // Digits only: `{+1}` and `{ 1}` are words, as in Rust's own.
                Some(n) if n.bytes().all(|b| b.is_ascii_digit()) => n.parse().ok(),
                _ => None,
            };
            if let (Some(index), Some(inside)) = (index, inside) {
                if inside.is_empty() {
                    self.next += 1;
                }
                let end = open + inside.len() + 2;
                let words = self.rest.get(..open).unwrap_or_default();
                let raw = self.rest.get(open..end).unwrap_or_default();
                self.rest = self.rest.get(end..).unwrap_or_default();
                let value = Piece::Value(index, raw);
                if words.is_empty() {
                    return Some(value);
                }
                self.held = Some(value);
                return Some(Piece::Words(words));
            }
            // Not a placeholder: the brace is words; look past it.
            from = open + 1;
        }
        let words = self.rest;
        self.rest = "";
        Some(Piece::Words(words))
    }
}

/* ------------------------------------------------------------ plurals */

/// What divides a sentence's forms for a count: "{} file||{} files". A
/// language has as many as its grammar wants — English two, Polish and
/// Russian three, Indonesian and Turkish one — in the order `form` numbers
/// them.
pub const FORMS: &str = "||";

/// Which of a language's forms a count takes: 0 for one, then the rest, as
/// each language's grammar has them (CLDR's cardinal rules for integers).
pub fn form(code: &str, n: u64) -> usize {
    let (ten, hundred) = (n % 10, n % 100);
    let few = (2..=4).contains(&ten) && !(12..=14).contains(&hundred);
    match code {
        // One form whatever the number.
        "id" | "tr" | "zh-CN" | "ja" | "ko" => 0,
        // 0 and 1 are both singular.
        "fr" | "pt-BR" => usize::from(n > 1),
        "ru" => {
            if ten == 1 && hundred != 11 {
                0
            } else if few {
                1
            } else {
                2
            }
        }
        "pl" => {
            if n == 1 {
                0
            } else if few {
                1
            } else {
                2
            }
        }
        _ => usize::from(n != 1),
    }
}

/// The form of `template` (forms divided by `FORMS`) that `n` takes in
/// language `code`; the last where it has fewer.
pub fn form_of<'a>(template: &'a str, code: &str, n: u64) -> &'a str {
    let forms: Vec<&str> = template.split(FORMS).collect();
    let at = form(code, n).min(forms.len().saturating_sub(1));
    forms.get(at).copied().unwrap_or(template)
}

/// A sentence about a count, in the form the count takes in the language
/// in force: `english` holds its English forms divided by `FORMS`, and its
/// translation the language's own.
pub fn tn(english: &'static str, n: u64, values: &[&dyn Display]) -> String {
    let translated = t(english);
    let code = if std::ptr::eq(translated, english) {
        "en"
    } else {
        CODE.with(Cell::get)
    };
    fill(form_of(translated, code, n), values)
}

/// A sentence with values in it. Each `{}` takes the next value in turn;
/// a translation whose word order differs names them instead, `{0}` for the
/// first and `{1}` for the second, in whatever order it needs.
pub fn tf(english: &'static str, values: &[&dyn Display]) -> String {
    fill(t(english), values)
}

/// A template's placeholders filled with values: `{}` in turn, `{N}` by
/// position. What `tf` does once the template is in the language in force.
/// A placeholder with no value is kept as written; a value is never read
/// for placeholders of its own.
pub fn fill(template: &str, values: &[&dyn Display]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(template.len() + 16);
    for piece in pieces(template) {
        match piece {
            Piece::Words(words) => out.push_str(words),
            Piece::Value(index, raw) => match values.get(index) {
                // Writing to a String does not fail.
                Some(value) => drop(write!(out, "{value}")),
                None => out.push_str(raw),
            },
        }
    }
    out
}

/// The values a template takes, by position, for the tests to hold a
/// translation to the English's values.
pub fn placeholders(template: &str) -> Vec<usize> {
    let mut found: Vec<usize> = pieces(template)
        .filter_map(|piece| match piece {
            Piece::Value(index, _) => Some(index),
            Piece::Words(_) => None,
        })
        .collect();
    found.sort_unstable();
    found
}

/* ------------------------------------------------------- status line */

/// A sentence for the status line, kept in English with its values and put
/// into the language in force only where it is shown: a person reads it in
/// theirs, and the control protocol and MCP hand scripts and agents the same
/// English on every machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Said(Sentence);

#[derive(Debug, Clone, PartialEq)]
enum Sentence {
    /// Made elsewhere (an engine's error, already worded): shown as it is.
    Plain(String),
    /// One of the window's own, with its values.
    Worded {
        template: &'static str,
        values: Vec<String>,
        /// Whether the values are names dm may give (a queue's), worded
        /// with the sentence; a file name or a number never is.
        names: bool,
        /// The count whose form the sentence takes, where it has forms.
        count: Option<u64>,
    },
}

impl Default for Said {
    fn default() -> Self {
        Said(Sentence::Plain(String::new()))
    }
}

impl Said {
    /// One of the window's sentences, its values shown as they are.
    pub fn new(template: &'static str, values: &[&dyn Display]) -> Self {
        Said::worded(template, values, false)
    }

    /// One whose values are names: those dm gave (its queues, its
    /// categories) are shown in the language in force, as the tree shows
    /// them, and a person's own as typed.
    pub fn naming(template: &'static str, values: &[&dyn Display]) -> Self {
        Said::worded(template, values, true)
    }

    /// One about a count, worded in the form the count takes (`tn`).
    pub fn counting(template: &'static str, n: u64, values: &[&dyn Display]) -> Self {
        Said::counted(template, values, false, Some(n))
    }

    /// One about a count whose values are names, as `naming`.
    pub fn counting_naming(template: &'static str, n: u64, values: &[&dyn Display]) -> Self {
        Said::counted(template, values, true, Some(n))
    }

    fn worded(template: &'static str, values: &[&dyn Display], names: bool) -> Self {
        Said::counted(template, values, names, None)
    }

    fn counted(
        template: &'static str,
        values: &[&dyn Display],
        names: bool,
        count: Option<u64>,
    ) -> Self {
        Said(Sentence::Worded {
            template,
            values: values.iter().map(ToString::to_string).collect(),
            names,
            count,
        })
    }

    /// In the language in force, for a person.
    pub fn shown(&self) -> String {
        match &self.0 {
            Sentence::Plain(plain) => plain.clone(),
            Sentence::Worded {
                template,
                values,
                names,
                count,
            } => {
                let shown: Vec<String> = if *names {
                    values.iter().map(|v| name(v)).collect()
                } else {
                    values.clone()
                };
                match count {
                    Some(n) => {
                        let shown: Vec<&dyn Display> =
                            shown.iter().map(|v| v as &dyn Display).collect();
                        tn(template, *n, &shown)
                    }
                    None => filled(t(template), &shown),
                }
            }
        }
    }

    /// In English, for a script or an agent.
    pub fn english(&self) -> String {
        match &self.0 {
            Sentence::Plain(plain) => plain.clone(),
            Sentence::Worded {
                template,
                values,
                count,
                ..
            } => match count {
                Some(n) => filled(form_of(template, "en", *n), values),
                None => filled(template, values),
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.0 {
            Sentence::Plain(plain) => plain.is_empty(),
            Sentence::Worded { template, .. } => template.is_empty(),
        }
    }

    /// Whether it says the speed limiter was turned on, which the status
    /// bar already says standing.
    pub fn is_limiter(&self) -> bool {
        matches!(&self.0, Sentence::Worded { template, .. } if *template == LIMITER_ON)
    }
}

impl Said {
    /// Whether it says a held download waits for the schedule, which is
    /// true only while one does.
    pub fn is_schedule_wait(&self) -> bool {
        matches!(&self.0, Sentence::Worded { template, .. }
            if template.starts_with("Waiting for the schedule")
                || template.contains("is waiting for the schedule"))
    }
}

/// What turning the speed limiter on says.
pub const LIMITER_ON: &str = "Speed limiter on: {} KB/s.";

/// `fill` with values already turned to text.
fn filled(template: &str, values: &[String]) -> String {
    let values: Vec<&dyn Display> = values.iter().map(|v| v as &dyn Display).collect();
    fill(template, &values)
}

impl From<String> for Said {
    fn from(plain: String) -> Self {
        Said(Sentence::Plain(plain))
    }
}

impl From<&str> for Said {
    fn from(plain: &str) -> Self {
        Said(Sentence::Plain(plain.to_owned()))
    }
}

#[cfg(test)]
mod tests;
