// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A pattern's lookahead and lookbehind (`(?=…)`, `(?!…)`, `(?<=…)`, `(?<!…)`): steps of the
//! pattern's own program, run again from where the match has come to.

use super::{PatternError, Regex};

impl Regex {
    /// Whether a lookaround holds at `pos`. `look` is its steps' start and end in the program,
    /// whether it looks behind, and whether it is negated; where one that is not negated holds,
    /// `caps` takes the groups it matched.
    pub(super) fn look(
        &self,
        input: &[u16],
        pos: usize,
        caps: &mut Vec<Option<usize>>,
        regs: &[usize],
        steps: &mut usize,
        look: (usize, usize, bool, bool),
    ) -> Result<bool, PatternError> {
        let (start, end, behind, negate) = look;
        let mut inner = caps.clone();
        let mut inner_regs = regs.to_vec();
        let found = if behind {
            // Some start before here from which it matches up to exactly here.
            let mut hit = false;
            for k in (0..=pos).rev() {
                let mut c = caps.clone();
                let mut r = regs.to_vec();
                let sub = Regex {
                    prog: self.prog[..end].to_vec(),
                    ..self.sub()
                };
                if let Some(e) = sub.run_to(input, start, k, &mut c, &mut r, steps, pos)? {
                    let _ = e;
                    inner = c;
                    hit = true;
                    break;
                }
            }
            hit
        } else {
            self.run(input, start, pos, &mut inner, &mut inner_regs, steps)?
                .is_some()
        };
        if found != negate {
            if !negate {
                *caps = inner;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// A copy of the pattern's settings, its program left empty (a lookbehind's own run).
    fn sub(&self) -> Regex {
        Regex {
            source: self.source.clone(),
            flags: self.flags.clone(),
            prog: Vec::new(),
            classes: self.classes.clone(),
            groups: self.groups,
            names: Vec::new(),
            ignore_case: self.ignore_case,
            multiline: self.multiline,
            dot_all: self.dot_all,
            regs: self.regs,
        }
    }

    /// Runs as [`Regex::run`], a match counting only where it ends at `must_end`.
    #[allow(clippy::too_many_arguments)]
    fn run_to(
        &self,
        input: &[u16],
        pc: usize,
        pos: usize,
        caps: &mut Vec<Option<usize>>,
        regs: &mut Vec<usize>,
        steps: &mut usize,
        must_end: usize,
    ) -> Result<Option<usize>, PatternError> {
        let limited = &input[..must_end];
        match self.run(limited, pc, pos, caps, regs, steps)? {
            Some(e) if e == must_end => Ok(Some(e)),
            _ => Ok(None),
        }
    }
}
