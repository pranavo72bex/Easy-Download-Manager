// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A pattern's text read into its tree of nodes: alternatives, terms, quantifiers, groups, classes
//! and escapes.

use super::*;

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            return true;
        }
        false
    }

    pub(super) fn alternation(&mut self) -> Result<Node, PatternError> {
        let mut alts = vec![self.sequence()?];
        while self.eat('|') {
            alts.push(self.sequence()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap_or(Node::Empty)
        } else {
            Node::Alt(alts)
        })
    }

    fn sequence(&mut self) -> Result<Node, PatternError> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            let atom = self.quantified(atom)?;
            items.push(atom);
        }
        Ok(Node::Seq(items))
    }

    fn number(&mut self) -> Option<u32> {
        let from = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == from {
            return None;
        }
        self.chars[from..self.at]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }

    fn quantified(&mut self, atom: Node) -> Result<Node, PatternError> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.at += 1;
                (0, None)
            }
            Some('+') => {
                self.at += 1;
                (1, None)
            }
            Some('?') => {
                self.at += 1;
                (0, Some(1))
            }
            Some('{') => {
                // `{n}`, `{n,}`, `{n,m}`; anything else is a plain brace.
                let mark = self.at;
                self.at += 1;
                let Some(min) = self.number() else {
                    self.at = mark;
                    return Ok(atom);
                };
                let max = if self.eat(',') {
                    self.number()
                } else {
                    Some(min)
                };
                if !self.eat('}') {
                    self.at = mark;
                    return Ok(atom);
                }
                if max.is_some_and(|m| m < min) {
                    return Err("numbers out of order in {} quantifier".into());
                }
                (min, max)
            }
            _ => return Ok(atom),
        };
        let greedy = !self.eat('?');
        if matches!(
            atom,
            Node::LineStart | Node::LineEnd | Node::WordBoundary(_)
        ) {
            return Err("Nothing to repeat".into());
        }
        Ok(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
            greedy,
        })
    }

    fn atom(&mut self) -> Result<Node, PatternError> {
        let c = self.peek().ok_or("unexpected end")?;
        self.at += 1;
        Ok(match c {
            '.' => Node::Any,
            '^' => Node::LineStart,
            '$' => Node::LineEnd,
            '(' => self.group()?,
            '[' => Node::Class(self.class()?),
            '\\' => self.escape(false)?,
            '*' | '+' | '?' => return Err("Nothing to repeat".into()),
            c => self.literal(c),
        })
    }

    fn literal(&self, c: char) -> Node {
        let mut buf = [0u16; 2];
        let units = c.encode_utf16(&mut buf);
        if units.len() == 1 {
            Node::Char(units[0])
        } else {
            Node::Seq(units.iter().map(|u| Node::Char(*u)).collect())
        }
    }

    fn group(&mut self) -> Result<Node, PatternError> {
        let node = if self.eat('?') {
            match self.peek() {
                Some(':') => {
                    self.at += 1;
                    Node::Group(Box::new(self.alternation()?), None)
                }
                Some('=') | Some('!') => {
                    let negate = self.peek() == Some('!');
                    self.at += 1;
                    Node::Look {
                        node: Box::new(self.alternation()?),
                        behind: false,
                        negate,
                    }
                }
                Some('<') if matches!(self.chars.get(self.at + 1), Some('=') | Some('!')) => {
                    let negate = self.chars[self.at + 1] == '!';
                    self.at += 2;
                    Node::Look {
                        node: Box::new(self.alternation()?),
                        behind: true,
                        negate,
                    }
                }
                Some('<') => {
                    self.at += 1;
                    let mut name = String::new();
                    while let Some(c) = self.peek() {
                        self.at += 1;
                        if c == '>' {
                            break;
                        }
                        name.push(c);
                    }
                    self.groups += 1;
                    let n = self.groups;
                    self.names.push((name, n));
                    Node::Group(Box::new(self.alternation()?), Some(n))
                }
                _ => return Err("Invalid group".into()),
            }
        } else {
            self.groups += 1;
            let n = self.groups;
            Node::Group(Box::new(self.alternation()?), Some(n))
        };
        if !self.eat(')') {
            return Err("Unterminated group".into());
        }
        Ok(node)
    }

    fn class(&mut self) -> Result<Class, PatternError> {
        let negated = self.eat('^');
        let mut ranges = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek().ok_or("Unterminated character class")?;
            if c == ']' && !(first && false) {
                self.at += 1;
                break;
            }
            first = false;
            let lo = self.class_atom(&mut ranges)?;
            let Some(lo) = lo else { continue };
            if self.peek() == Some('-') && self.chars.get(self.at + 1).is_some_and(|c| *c != ']') {
                self.at += 1;
                let mut more = Vec::new();
                match self.class_atom(&mut more)? {
                    Some(hi) => {
                        if hi < lo {
                            return Err("Range out of order in character class".into());
                        }
                        ranges.push((lo, hi));
                    }
                    None => {
                        // `[\w-x]`: the dash is itself.
                        ranges.push((lo, lo));
                        ranges.push(('-' as u32, '-' as u32));
                        ranges.extend(more);
                    }
                }
            } else {
                ranges.push((lo, lo));
            }
        }
        Ok(Class { ranges, negated })
    }

    /// One character of a class (its code), or a set added to `ranges` (none returned).
    fn class_atom(&mut self, ranges: &mut Vec<(u32, u32)>) -> Result<Option<u32>, PatternError> {
        let c = self.peek().ok_or("Unterminated character class")?;
        self.at += 1;
        if c != '\\' {
            return Ok(Some(c as u32));
        }
        match self.escape(true)? {
            Node::Char(u) => Ok(Some(u as u32)),
            Node::Class(cls) => {
                if cls.negated {
                    // `[\D]`: every code but these.
                    let mut last = 0u32;
                    let mut sorted = cls.ranges.clone();
                    sorted.sort();
                    for (lo, hi) in sorted {
                        if lo > last {
                            ranges.push((last, lo - 1));
                        }
                        last = hi + 1;
                    }
                    ranges.push((last, 0x10FFFF));
                } else {
                    ranges.extend(cls.ranges);
                }
                Ok(None)
            }
            Node::Seq(units) => {
                // A code point beyond the first plane, in a class: its value.
                let us: Vec<u16> = units
                    .iter()
                    .filter_map(|n| match n {
                        Node::Char(u) => Some(*u),
                        _ => None,
                    })
                    .collect();
                let s = String::from_utf16_lossy(&us);
                Ok(s.chars().next().map(|c| c as u32))
            }
            // `[\b]` is a backspace.
            Node::WordBoundary(true) => Ok(Some(8)),
            _ => Ok(None),
        }
    }

    fn hex(&mut self, n: usize) -> Option<u32> {
        let s: String = self.chars.get(self.at..self.at + n)?.iter().collect();
        let v = u32::from_str_radix(&s, 16).ok()?;
        self.at += n;
        Some(v)
    }

    fn escape(&mut self, in_class: bool) -> Result<Node, PatternError> {
        let c = self.peek().ok_or("\\ at end of pattern")?;
        self.at += 1;
        let set = |ranges: &[(u32, u32)], negated: bool| {
            Node::Class(Class {
                ranges: ranges.to_vec(),
                negated,
            })
        };
        const DIGIT: &[(u32, u32)] = &[(48, 57)];
        const WORD: &[(u32, u32)] = &[(48, 57), (65, 90), (95, 95), (97, 122)];
        const SPACE: &[(u32, u32)] = &[
            (9, 13),
            (32, 32),
            (0xa0, 0xa0),
            (0x1680, 0x1680),
            (0x2000, 0x200a),
            (0x2028, 0x2029),
            (0x202f, 0x202f),
            (0x205f, 0x205f),
            (0x3000, 0x3000),
            (0xfeff, 0xfeff),
        ];
        Ok(match c {
            'd' => set(DIGIT, false),
            'D' => set(DIGIT, true),
            'w' => set(WORD, false),
            'W' => set(WORD, true),
            's' => set(SPACE, false),
            'S' => set(SPACE, true),
            'b' if in_class => Node::WordBoundary(true),
            'b' => Node::WordBoundary(true),
            'B' => Node::WordBoundary(false),
            'n' => Node::Char(10),
            'r' => Node::Char(13),
            't' => Node::Char(9),
            'v' => Node::Char(11),
            'f' => Node::Char(12),
            '0' if !self.peek().is_some_and(|c| c.is_ascii_digit()) => Node::Char(0),
            'x' => match self.hex(2) {
                Some(v) => Node::Char(v as u16),
                None => Node::Char('x' as u16),
            },
            'u' => {
                if self.peek() == Some('{') && self.unicode {
                    self.at += 1;
                    let from = self.at;
                    while self.peek().is_some_and(|c| c != '}') {
                        self.at += 1;
                    }
                    let s: String = self.chars[from..self.at].iter().collect();
                    self.at += 1;
                    let v = u32::from_str_radix(&s, 16).map_err(|_| "Invalid Unicode escape")?;
                    self.literal(char::from_u32(v).ok_or("Invalid Unicode escape")?)
                } else {
                    match self.hex(4) {
                        Some(v) => Node::Char(v as u16),
                        None => Node::Char('u' as u16),
                    }
                }
            }
            'c' => match self.peek() {
                Some(l) if l.is_ascii_alphabetic() => {
                    self.at += 1;
                    Node::Char((l as u32 % 32) as u16)
                }
                _ => Node::Char('\\' as u16),
            },
            'k' if self.peek() == Some('<') => {
                self.at += 1;
                let mut name = String::new();
                while let Some(c) = self.peek() {
                    self.at += 1;
                    if c == '>' {
                        break;
                    }
                    name.push(c);
                }
                Node::NamedRef(name)
            }
            '1'..='9' if !in_class => {
                self.at -= 1;
                let n = self.number().unwrap_or(0);
                Node::BackRef(n as usize)
            }
            c => self.literal(c),
        })
    }
}
