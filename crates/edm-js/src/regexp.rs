// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! JavaScript's patterns (`/…/flags`): a pattern compiled to a small program of steps, run by
//! backtracking over UTF-16, as the language matches. Groups (named, non-capturing), lookahead
//! and lookbehind, greedy and lazy quantifiers, backreferences, classes, and the flags `i`, `g`,
//! `m`, `s`, `y` (and `u`, `d`, read). A match that takes too many steps is stopped.

mod look;
mod parse;

use crate::lexer::Text;

/// A compiled pattern.
pub struct Regex {
    /// Between the slashes.
    pub source: Text,
    /// Its flags.
    pub flags: String,
    prog: Vec<Inst>,
    classes: Vec<Class>,
    /// How many groups it has (the whole match is group 0).
    pub groups: usize,
    /// Its named groups, by number.
    pub names: Vec<(String, usize)>,
    ignore_case: bool,
    multiline: bool,
    dot_all: bool,
    regs: usize,
}

/// The most steps one match may take (a pattern that backtracks without end is stopped).
const MOST_STEPS: usize = 50_000_000;

#[derive(Clone, Debug)]
enum Inst {
    Char(u16),
    Any,
    Class(usize),
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    LineStart,
    LineEnd,
    WordBoundary(bool),
    BackRef(usize),
    /// A lookaround: its program runs from `start` to the `Match` before `end`; `behind`, and
    /// whether it must not match.
    Look {
        start: usize,
        end: usize,
        behind: bool,
        negate: bool,
    },
    /// Remembers the position in register `n`.
    Mark(usize),
    /// Fails where the position is the one register `n` holds (a loop that matched nothing).
    Progress(usize),
    Match,
}

#[derive(Clone, Debug, Default)]
struct Class {
    ranges: Vec<(u32, u32)>,
    negated: bool,
}

impl Class {
    fn has(&self, c: u32, ignore_case: bool) -> bool {
        let inside = |c: u32| self.ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c));
        let found = inside(c)
            || (ignore_case && {
                let (l, u) = (fold(c, false), fold(c, true));
                inside(l) || inside(u)
            });
        found != self.negated
    }
}

/// `c` in lower case (`upper`: upper case), where it has one form.
fn fold(c: u32, upper: bool) -> u32 {
    let Some(ch) = char::from_u32(c) else {
        return c;
    };
    let mut it: Box<dyn Iterator<Item = char>> = if upper {
        Box::new(ch.to_uppercase())
    } else {
        Box::new(ch.to_lowercase())
    };
    match (it.next(), it.next()) {
        (Some(x), None) if (x as u32) < 0x10000 => x as u32,
        _ => c,
    }
}

/// A pattern's syntax error.
pub type PatternError = String;

/// A match's groups: each one's start and end, none where it matched nothing (group 0 the
/// whole match).
pub type Groups = Vec<Option<(usize, usize)>>;

/// A choice to come back to: where in the program and the text, and the captures and
/// registers then.
type Choice = (usize, usize, Vec<Option<usize>>, Vec<usize>);

// ---- parsing

#[derive(Debug)]
enum Node {
    Empty,
    Char(u16),
    Any,
    Class(Class),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Group(Box<Node>, Option<usize>),
    Repeat {
        node: Box<Node>,
        min: u32,
        max: Option<u32>,
        greedy: bool,
    },
    LineStart,
    LineEnd,
    WordBoundary(bool),
    BackRef(usize),
    NamedRef(String),
    Look {
        node: Box<Node>,
        behind: bool,
        negate: bool,
    },
}

struct Parser<'a> {
    chars: Vec<char>,
    at: usize,
    groups: usize,
    names: Vec<(String, usize)>,
    unicode: bool,
    _src: &'a str,
}

// ---- compiling

struct Compiler {
    prog: Vec<Inst>,
    classes: Vec<Class>,
    regs: usize,
    names: Vec<(String, usize)>,
}

impl Compiler {
    fn emit(&mut self, i: Inst) -> usize {
        self.prog.push(i);
        self.prog.len() - 1
    }

    fn node(&mut self, n: &Node) -> Result<(), PatternError> {
        match n {
            Node::Empty => {}
            Node::Char(c) => {
                self.emit(Inst::Char(*c));
            }
            Node::Any => {
                self.emit(Inst::Any);
            }
            Node::Class(c) => {
                self.classes.push(c.clone());
                let at = self.classes.len() - 1;
                self.emit(Inst::Class(at));
            }
            Node::Seq(items) => {
                for i in items {
                    self.node(i)?;
                }
            }
            Node::Alt(alts) => {
                let mut jumps = Vec::new();
                for (k, alt) in alts.iter().enumerate() {
                    if k + 1 < alts.len() {
                        let split = self.emit(Inst::Split(0, 0));
                        self.node(alt)?;
                        jumps.push(self.emit(Inst::Jmp(0)));
                        let next = self.prog.len();
                        self.prog[split] = Inst::Split(split + 1, next);
                    } else {
                        self.node(alt)?;
                    }
                }
                let end = self.prog.len();
                for j in jumps {
                    self.prog[j] = Inst::Jmp(end);
                }
            }
            Node::Group(inner, n) => match n {
                Some(n) => {
                    self.emit(Inst::Save(2 * n));
                    self.node(inner)?;
                    self.emit(Inst::Save(2 * n + 1));
                }
                None => self.node(inner)?,
            },
            Node::LineStart => {
                self.emit(Inst::LineStart);
            }
            Node::LineEnd => {
                self.emit(Inst::LineEnd);
            }
            Node::WordBoundary(b) => {
                self.emit(Inst::WordBoundary(*b));
            }
            Node::BackRef(n) => {
                self.emit(Inst::BackRef(*n));
            }
            Node::NamedRef(name) => {
                let n = self
                    .names
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, n)| *n)
                    .ok_or("Invalid named capture referenced")?;
                self.emit(Inst::BackRef(n));
            }
            Node::Look {
                node,
                behind,
                negate,
            } => {
                let look = self.emit(Inst::Look {
                    start: 0,
                    end: 0,
                    behind: *behind,
                    negate: *negate,
                });
                let start = self.prog.len();
                self.node(node)?;
                self.emit(Inst::Match);
                let end = self.prog.len();
                self.prog[look] = Inst::Look {
                    start,
                    end,
                    behind: *behind,
                    negate: *negate,
                };
            }
            Node::Repeat {
                node,
                min,
                max,
                greedy,
            } => {
                if *min > 1000 || max.is_some_and(|m| m > 1000) {
                    return Err("a repetition too large for EDM".into());
                }
                for _ in 0..*min {
                    self.node(node)?;
                }
                match max {
                    Some(max) => {
                        // Each optional copy: try it (or not, where lazy), then on.
                        let mut splits = Vec::new();
                        for _ in *min..*max {
                            splits.push(self.emit(Inst::Split(0, 0)));
                            self.node(node)?;
                        }
                        let end = self.prog.len();
                        for s in splits {
                            self.prog[s] = if *greedy {
                                Inst::Split(s + 1, end)
                            } else {
                                Inst::Split(end, s + 1)
                            };
                        }
                    }
                    None => {
                        // A loop, which fails a turn that matched nothing.
                        let reg = self.regs;
                        self.regs += 1;
                        let split = self.emit(Inst::Split(0, 0));
                        self.emit(Inst::Mark(reg));
                        self.node(node)?;
                        self.emit(Inst::Progress(reg));
                        self.emit(Inst::Jmp(split));
                        let end = self.prog.len();
                        self.prog[split] = if *greedy {
                            Inst::Split(split + 1, end)
                        } else {
                            Inst::Split(end, split + 1)
                        };
                    }
                }
            }
        }
        Ok(())
    }
}

impl Regex {
    /// `source` with `flags`, compiled.
    pub fn new(source: &str, flags: &str) -> Result<Regex, PatternError> {
        for (n, f) in flags.char_indices() {
            if !"dgimsuyv".contains(f) || flags[..n].contains(f) {
                return Err(format!(
                    "Invalid flags supplied to RegExp constructor '{flags}'"
                ));
            }
        }
        let mut parser = Parser {
            chars: source.chars().collect(),
            at: 0,
            groups: 0,
            names: Vec::new(),
            unicode: flags.contains('u') || flags.contains('v'),
            _src: source,
        };
        // Named groups are known before the pattern is read, for `\k<name>` before its group.
        let tree = parser.alternation()?;
        if parser.at < parser.chars.len() {
            return Err("Unmatched ')'".into());
        }
        let mut c = Compiler {
            prog: Vec::new(),
            classes: Vec::new(),
            regs: 0,
            names: parser.names.clone(),
        };
        c.emit(Inst::Save(0));
        c.node(&tree)?;
        c.emit(Inst::Save(1));
        c.emit(Inst::Match);
        Ok(Regex {
            source: crate::lexer::text(source),
            flags: flags.to_string(),
            prog: c.prog,
            classes: c.classes,
            groups: parser.groups,
            names: parser.names,
            ignore_case: flags.contains('i'),
            multiline: flags.contains('m'),
            dot_all: flags.contains('s'),
            regs: c.regs,
        })
    }

    /// The first match at or after `start` (only at `start` where `sticky`): each group's
    /// start and end, group 0 the whole. `Err` where it took too many steps.
    pub fn exec_at(
        &self,
        input: &[u16],
        start: usize,
        sticky: bool,
    ) -> Result<Option<Groups>, PatternError> {
        let mut steps = 0usize;
        let last = if sticky { start } else { input.len() };
        let mut from = start;
        while from <= last.min(input.len()) {
            let mut caps = vec![None; 2 * (self.groups + 1)];
            let mut regs = vec![usize::MAX; self.regs];
            if let Some(end) = self.run(input, 0, from, &mut caps, &mut regs, &mut steps)? {
                let _ = end;
                let mut out = Vec::with_capacity(self.groups + 1);
                for g in 0..=self.groups {
                    out.push(match (caps[2 * g], caps[2 * g + 1]) {
                        (Some(a), Some(b)) => Some((a, b)),
                        _ => None,
                    });
                }
                return Ok(Some(out));
            }
            from += 1;
        }
        Ok(None)
    }

    fn word_at(&self, input: &[u16], i: usize) -> bool {
        input.get(i).is_some_and(|&c| {
            let c = c as u32;
            (48..=57).contains(&c) || (65..=90).contains(&c) || c == 95 || (97..=122).contains(&c)
        })
    }

    fn same(&self, a: u16, b: u16) -> bool {
        a == b || (self.ignore_case && fold(a as u32, false) == fold(b as u32, false))
    }

    /// Runs the program from `pc` at `pos`; where it reaches `Match`, the position there.
    fn run(
        &self,
        input: &[u16],
        pc: usize,
        pos: usize,
        caps: &mut Vec<Option<usize>>,
        regs: &mut Vec<usize>,
        steps: &mut usize,
    ) -> Result<Option<usize>, PatternError> {
        // Choices to come back to: where, and the captures and registers then.
        let mut stack: Vec<Choice> = Vec::new();
        let (mut pc, mut pos) = (pc, pos);
        loop {
            *steps += 1;
            if *steps > MOST_STEPS {
                return Err("a pattern that takes too long to match".into());
            }
            let ok = match &self.prog[pc] {
                Inst::Match => return Ok(Some(pos)),
                Inst::Char(c) => {
                    let hit = input.get(pos).is_some_and(|x| self.same(*x, *c));
                    if hit {
                        pos += 1;
                        pc += 1;
                    }
                    hit
                }
                Inst::Any => {
                    let hit = input
                        .get(pos)
                        .is_some_and(|&c| self.dot_all || !matches!(c, 10 | 13 | 0x2028 | 0x2029));
                    if hit {
                        pos += 1;
                        pc += 1;
                    }
                    hit
                }
                Inst::Class(n) => {
                    let hit = input
                        .get(pos)
                        .is_some_and(|&c| self.classes[*n].has(c as u32, self.ignore_case));
                    if hit {
                        pos += 1;
                        pc += 1;
                    }
                    hit
                }
                Inst::Split(a, b) => {
                    stack.push((*b, pos, caps.clone(), regs.clone()));
                    pc = *a;
                    true
                }
                Inst::Jmp(to) => {
                    pc = *to;
                    true
                }
                Inst::Save(n) => {
                    caps[*n] = Some(pos);
                    pc += 1;
                    true
                }
                Inst::Mark(r) => {
                    regs[*r] = pos;
                    pc += 1;
                    true
                }
                Inst::Progress(r) => {
                    let moved = regs[*r] != pos;
                    pc += 1;
                    moved
                }
                Inst::LineStart => {
                    let hit = pos == 0
                        || (self.multiline && matches!(input[pos - 1], 10 | 13 | 0x2028 | 0x2029));
                    pc += 1;
                    hit
                }
                Inst::LineEnd => {
                    let hit = pos == input.len()
                        || (self.multiline && matches!(input[pos], 10 | 13 | 0x2028 | 0x2029));
                    pc += 1;
                    hit
                }
                Inst::WordBoundary(want) => {
                    let before = pos > 0 && self.word_at(input, pos - 1);
                    let after = self.word_at(input, pos);
                    pc += 1;
                    (before != after) == *want
                }
                Inst::BackRef(n) => {
                    match (
                        caps.get(2 * n).copied().flatten(),
                        caps.get(2 * n + 1).copied().flatten(),
                    ) {
                        (Some(a), Some(b)) => {
                            let len = b - a;
                            let hit = pos + len <= input.len()
                                && (0..len).all(|k| self.same(input[pos + k], input[a + k]));
                            if hit {
                                pos += len;
                            }
                            pc += 1;
                            hit
                        }
                        // A group that matched nothing matches the empty string.
                        _ => {
                            pc += 1;
                            true
                        }
                    }
                }
                Inst::Look {
                    start,
                    end,
                    behind,
                    negate,
                } => {
                    let look = (*start, *end, *behind, *negate);
                    let hit = self.look(input, pos, caps, regs, steps, look)?;
                    if hit {
                        pc = *end;
                    }
                    hit
                }
            };
            if !ok {
                match stack.pop() {
                    Some((p, at, c, r)) => {
                        pc = p;
                        pos = at;
                        *caps = c;
                        *regs = r;
                    }
                    None => return Ok(None),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::text;

    fn find(p: &str, flags: &str, s: &str) -> Option<String> {
        let r = Regex::new(p, flags).unwrap();
        let input = text(s);
        let m = r.exec_at(&input, 0, false).unwrap()?;
        let (a, b) = m[0]?;
        Some(String::from_utf16_lossy(&input[a..b]))
    }

    fn groups(p: &str, s: &str) -> Vec<Option<String>> {
        let r = Regex::new(p, "").unwrap();
        let input = text(s);
        let m = r.exec_at(&input, 0, false).unwrap().unwrap();
        m.iter()
            .map(|g| g.map(|(a, b)| String::from_utf16_lossy(&input[a..b])))
            .collect()
    }

    #[test]
    fn patterns_match_as_the_language_matches() {
        assert_eq!(find("a+b", "", "xaaab"), Some("aaab".into()));
        assert_eq!(find("a+?", "", "aaa"), Some("a".into()));
        assert_eq!(find("^\\d{2,3}$", "", "123"), Some("123".into()));
        assert_eq!(find("^\\d{2,3}$", "", "1234"), None);
        assert_eq!(find("colou?r", "", "color"), Some("color".into()));
        assert_eq!(find("[a-c]+", "", "xxabcabd"), Some("abcab".into()));
        assert_eq!(find("[^a-c]+", "", "abcxyz"), Some("xyz".into()));
        assert_eq!(find("cat|dog", "", "hotdog"), Some("dog".into()));
        assert_eq!(find("ABC", "i", "xabcx"), Some("abc".into()));
        assert_eq!(find("^b", "m", "a\nb"), Some("b".into()));
        assert_eq!(find("a.c", "", "a\nc"), None);
        assert_eq!(find("a.c", "s", "a\nc"), Some("a\nc".into()));
        assert_eq!(find("\\bfoo\\b", "", "a foo b"), Some("foo".into()));
        assert_eq!(find("x(?=y)", "", "xzxy"), Some("x".into()));
        assert_eq!(find("x(?!y)", "", "xyxz"), Some("x".into()));
        assert_eq!(find("(?<=\\$)\\d+", "", "cost $42"), Some("42".into()));
        assert_eq!(find("(?<!\\$)\\b\\d+", "", "$42 17"), Some("17".into()));
        assert_eq!(find("(a)\\1", "", "xaab"), Some("aa".into()));
        assert_eq!(
            find("(?<q>['\"]).*?\\k<q>", "", "say 'hi' now"),
            Some("'hi'".into())
        );
        assert_eq!(find("(a*)*b", "", "aaab"), Some("aaab".into()));
        assert_eq!(find("[\\w.-]+", "", "  a.b-c "), Some("a.b-c".into()));
        assert_eq!(find("\\u0041\\x42", "", "zAB"), Some("AB".into()));
        assert_eq!(find("a{,2}", "", "a{,2}"), Some("a{,2}".into()));
    }

    #[test]
    fn groups_are_taken_and_an_unused_one_is_none() {
        assert_eq!(
            groups("(\\d+)-(\\d+)?(x)?", "12-34"),
            vec![
                Some("12-34".into()),
                Some("12".into()),
                Some("34".into()),
                None
            ]
        );
        assert_eq!(
            groups("(?:a)(b)", "ab"),
            vec![Some("ab".into()), Some("b".into())]
        );
    }

    #[test]
    fn a_bad_pattern_is_refused() {
        assert!(Regex::new("(a", "").is_err());
        assert!(Regex::new("a)", "").is_err());
        assert!(Regex::new("*a", "").is_err());
        assert!(Regex::new("a", "gg").is_err());
        assert!(Regex::new("a{3,2}", "").is_err());
    }
}
