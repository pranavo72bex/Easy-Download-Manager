// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! PPMd variant H's model after each byte: the byte counted once more in the next shorter
//! context, added to every longer context it was escaped from, and the contexts that follow
//! it made (`create_successors`) from the text the model keeps, as the model's description
//! has them. Memory running out starts the model again.

use super::{Model, St, MAX_FREQ, MAX_ORDER, STATE};

impl Model {
    /// The model taught the byte just found.
    pub(super) fn update_model(&mut self) {
        let fs = self.state(self.found);
        let fs_freq = u32::from(fs.freq);
        let mut p = 0u32;
        let suffix = self.suffix(self.min_ctx);
        if fs_freq < MAX_FREQ / 4 && suffix != 0 {
            let pc = suffix;
            if self.ns(pc) != 1 {
                p = self.find(pc, fs.sym);
                if p == 0 {
                    return self.restart_full();
                }
                let stats = self.stats(pc);
                if p != stats && self.freq(p) >= self.freq(p - STATE) {
                    self.swap(p, p - STATE);
                    p -= STATE;
                }
                if self.freq(p) < MAX_FREQ - 9 {
                    self.set_freq(p, self.freq(p) + 2);
                    self.set_summ(pc, self.summ(pc) + 2);
                }
            } else {
                p = pc.wrapping_add(2);
                let f = self.freq(p);
                self.set_freq(p, f + u32::from(f < 32));
            }
        }
        if self.order_fall == 0 {
            let s = self.create_successors(true, p);
            self.min_ctx = s;
            self.max_ctx = s;
            self.set_succ(self.found, s);
            if s == 0 {
                self.restart_full();
            }
            return;
        }
        let text = self.a.text;
        self.a.set_u8(text, fs.sym);
        self.a.text += 1;
        let mut successor = self.a.text;
        if self.a.text >= self.a.units_start {
            return self.restart_full();
        }
        let mut fs_succ = fs.succ;
        if fs_succ != 0 {
            if fs_succ <= self.a.text {
                fs_succ = self.create_successors(false, p);
                if fs_succ == 0 {
                    return self.restart_full();
                }
            }
            self.order_fall -= 1;
            if self.order_fall == 0 {
                successor = fs_succ;
                if self.max_ctx != self.min_ctx {
                    self.a.text -= 1;
                }
            }
        } else {
            self.set_succ(self.found, successor);
            fs_succ = self.min_ctx;
        }
        if !self.add_to_longer(fs, successor) {
            return self.restart_full();
        }
        self.max_ctx = fs_succ;
        self.min_ctx = fs_succ;
    }

    /// The byte `fs` added, with `successor`, to each context from the longest down to the
    /// one it was found in; false when memory runs out.
    fn add_to_longer(&mut self, fs: St, successor: u32) -> bool {
        let fs_freq = u32::from(fs.freq);
        let ns = self.ns(self.min_ctx);
        let s0 = self
            .summ(self.min_ctx)
            .wrapping_sub(ns)
            .wrapping_sub(fs_freq.wrapping_sub(1));
        let mut pc = self.max_ctx;
        while pc != self.min_ctx {
            if !self.valid(pc) {
                return false;
            }
            let ns1 = self.ns(pc);
            if ns1 != 1 {
                if ns1 & 1 == 0 {
                    let moved = self.a.expand(self.stats(pc), ns1 >> 1);
                    if moved == 0 {
                        return false;
                    }
                    self.set_stats(pc, moved);
                }
                let sf = self.summ(pc);
                let add =
                    u32::from(2 * ns1 < ns) + 2 * u32::from((4 * ns1 <= ns) & (sf <= 8 * ns1));
                self.set_summ(pc, sf + add);
            } else {
                let s = self.a.alloc_units(1);
                if s == 0 {
                    return false;
                }
                let mut st = self.state(pc.wrapping_add(2));
                st.freq = if u32::from(st.freq) < MAX_FREQ / 4 - 1 {
                    st.freq * 2
                } else {
                    (MAX_FREQ - 4) as u8
                };
                self.put_state(s, st);
                self.set_stats(pc, s);
                let summ = u32::from(st.freq) + self.init_esc + u32::from(ns > 3);
                self.set_summ(pc, summ);
            }
            let summ = self.summ(pc);
            let mut cf = 2 * fs_freq * (summ + 6);
            let sf = s0.wrapping_add(summ);
            if cf < 6 * sf {
                cf = 1 + u32::from(cf > sf) + u32::from(cf >= 4 * sf);
                self.set_summ(pc, summ + 3);
            } else {
                cf = 4
                    + u32::from(cf >= 9 * sf)
                    + u32::from(cf >= 12 * sf)
                    + u32::from(cf >= 15 * sf);
                self.set_summ(pc, summ + cf);
            }
            let s = self.stats(pc).wrapping_add(STATE * ns1);
            let new = St {
                sym: fs.sym,
                freq: cf as u8,
                succ: successor,
            };
            self.put_state(s, new);
            self.set_ns(pc, ns1 + 1);
            pc = self.suffix(pc);
        }
        true
    }

    /// The contexts that follow the byte just found, made from the text where they are not
    /// yet contexts; `p1` is its state in the next shorter context when that is known
    /// already. 0 when memory runs out or the model does not hold together.
    fn create_successors(&mut self, skip: bool, p1: u32) -> u32 {
        let mut pc = self.min_ctx;
        let up_branch = self.succ(self.found);
        let found_sym = self.sym(self.found);
        let mut ps = [0u32; MAX_ORDER as usize];
        let mut n = 0usize;
        let mut walk = true;
        if !skip {
            ps[0] = self.found;
            n = 1;
            walk = self.suffix(pc) != 0;
        }
        if walk {
            let mut given = p1;
            loop {
                pc = self.suffix(pc);
                if !self.valid(pc) {
                    return 0;
                }
                let p = if given != 0 {
                    std::mem::take(&mut given)
                } else if self.ns(pc) != 1 {
                    self.find(pc, found_sym)
                } else {
                    pc.wrapping_add(2)
                };
                if p == 0 {
                    return 0;
                }
                if self.succ(p) != up_branch {
                    pc = self.succ(p);
                    break;
                }
                if n >= ps.len() {
                    return 0;
                }
                ps[n] = p;
                n += 1;
                if self.suffix(pc) == 0 {
                    break;
                }
            }
        }
        if n == 0 {
            return pc;
        }
        // The state every new context starts with: the byte the text has next, counted as
        // the shorter context counts it.
        let up_sym = self.a.u8(up_branch);
        let up_freq = if self.ns(pc) != 1 {
            if pc <= self.a.text {
                return 0;
            }
            let p = self.find(pc, up_sym);
            if p == 0 {
                return 0;
            }
            let cf = self.freq(p).wrapping_sub(1);
            let s0 = self.summ(pc).wrapping_sub(self.ns(pc)).wrapping_sub(cf);
            1 + if 2 * cf <= s0 {
                u32::from(5 * cf > s0)
            } else {
                (2 * cf + 3 * s0 - 1) / (2 * s0).max(1)
            }
        } else {
            self.freq(pc.wrapping_add(2))
        };
        let up = St {
            sym: up_sym,
            freq: up_freq as u8,
            succ: up_branch.wrapping_add(1),
        };
        while n > 0 {
            n -= 1;
            let c = self.a.alloc_context();
            if c == 0 {
                return 0;
            }
            self.set_ns(c, 1);
            self.put_state(c.wrapping_add(2), up);
            self.set_suffix(c, pc);
            self.set_succ(ps[n], c);
            pc = c;
        }
        pc
    }
}
