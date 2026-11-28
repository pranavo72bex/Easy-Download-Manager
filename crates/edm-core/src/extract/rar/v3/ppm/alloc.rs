// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! PPMd variant H's memory: one block of the size the stream asks for, in 12-byte units,
//! as the model's description lays it out, since when the model runs out of memory, and so
//! restarts, depends on exactly how it is used. The bytes of the text seen grow from the
//! bottom; contexts are taken from the top down; arrays of states from the bottom of the
//! units up, or from 38 lists of freed blocks by size, which are glued together when they
//! run dry.
//!
//! Places are offsets into the block, 0 for none (the block starts a little above 0). Every
//! read or write out of bounds reads 0 or does nothing: damaged data makes wrong bytes,
//! which the file's CRC refuses, never a panic.

/// A unit's size.
pub(super) const UNIT: u32 = 12;
/// The lists of free blocks, by size.
const INDEXES: usize = 38;
/// Where the block starts, so that no place in it is 0.
const BASE: u32 = UNIT;
/// A free block's mark, where a used one has its first two bytes.
const FREE: u16 = 0xffff;

/// The bytes from `at`, `n` of them, as a range of the memory.
fn span(at: u32, n: usize) -> Option<std::ops::Range<usize>> {
    let at = at as usize;
    at.checked_add(n).map(|end| at..end)
}

/// The memory and where its parts are.
pub(super) struct Alloc {
    mem: Vec<u8>,
    size: u32,
    free: [u32; INDEXES],
    /// The units between `lo` and `hi` are not used yet.
    pub lo: u32,
    pub hi: u32,
    /// Where the units start (lower as the text's room is taken for them) and where the
    /// text goes next.
    pub units_start: u32,
    pub text: u32,
    /// Where the block ends.
    pub end: u32,
    /// The list head the free blocks are gathered on while they are glued.
    head: u32,
    glue: u32,
    /// Units in each list's blocks, and the list for blocks of a number of units.
    index_units: [u8; INDEXES],
    units_index: [u8; 128],
}

impl Alloc {
    /// `size` bytes of memory (at most 256 MiB), not yet laid out.
    pub(super) fn new(size: u32) -> Alloc {
        let mut index_units = [0u8; INDEXES];
        let mut k = 0u32;
        for (i, slot) in index_units.iter_mut().enumerate() {
            k += match i {
                0..=3 => 1,
                4..=7 => 2,
                8..=11 => 3,
                _ => 4,
            };
            *slot = k as u8;
        }
        let mut units_index = [0u8; 128];
        let mut i = 0usize;
        for (k, slot) in units_index.iter_mut().enumerate() {
            if usize::from(index_units[i]) < k + 1 {
                i += 1;
            }
            *slot = i as u8;
        }
        // Two units more than asked, as the format's own reader has, and one for the head.
        let alloc = size / UNIT * UNIT + 2 * UNIT;
        let head = BASE + alloc;
        Alloc {
            mem: vec![0; (head + UNIT) as usize],
            size,
            free: [0; INDEXES],
            lo: 0,
            hi: 0,
            units_start: 0,
            text: 0,
            end: head - UNIT,
            head,
            glue: 0,
            index_units,
            units_index,
        }
    }

    pub(super) fn size(&self) -> u32 {
        self.size
    }

    /// Lays the memory out afresh: everything in it is forgotten.
    pub(super) fn init(&mut self) {
        self.free = [0; INDEXES];
        self.text = BASE;
        let units = UNIT * (self.size / 8 / UNIT * 7);
        self.units_start = BASE + (self.size - units);
        self.lo = self.units_start;
        self.hi = self.lo + units;
        self.glue = 0;
    }

    pub(super) fn u8(&self, at: u32) -> u8 {
        self.mem.get(at as usize).copied().unwrap_or(0)
    }

    pub(super) fn set_u8(&mut self, at: u32, v: u8) {
        if let Some(slot) = self.mem.get_mut(at as usize) {
            *slot = v;
        }
    }

    pub(super) fn u16(&self, at: u32) -> u16 {
        match span(at, 2).and_then(|r| self.mem.get(r)) {
            Some(b) => u16::from_le_bytes([b[0], b[1]]),
            None => 0,
        }
    }

    pub(super) fn set_u16(&mut self, at: u32, v: u16) {
        if let Some(b) = span(at, 2).and_then(|r| self.mem.get_mut(r)) {
            b.copy_from_slice(&v.to_le_bytes());
        }
    }

    pub(super) fn u32(&self, at: u32) -> u32 {
        match span(at, 4).and_then(|r| self.mem.get(r)) {
            Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            None => 0,
        }
    }

    pub(super) fn set_u32(&mut self, at: u32, v: u32) {
        if let Some(b) = span(at, 4).and_then(|r| self.mem.get_mut(r)) {
            b.copy_from_slice(&v.to_le_bytes());
        }
    }

    /// Copies `len` bytes from `from` to `to`.
    pub(super) fn copy(&mut self, to: u32, from: u32, len: u32) {
        let (Some(src), Some(dst)) = (span(from, len as usize), span(to, len as usize)) else {
            return;
        };
        if src.end <= self.mem.len() && dst.end <= self.mem.len() {
            self.mem.copy_within(src, dst.start);
        }
    }

    fn units(&self, index: usize) -> u32 {
        u32::from(self.index_units.get(index).copied().unwrap_or(128))
    }

    fn index(&self, units: u32) -> usize {
        let i = units.wrapping_sub(1) as usize;
        usize::from(
            self.units_index
                .get(i)
                .copied()
                .unwrap_or(INDEXES as u8 - 1),
        )
    }

    fn insert(&mut self, p: u32, index: usize) {
        if let Some(&first) = self.free.get(index) {
            self.set_u32(p, first);
            self.free[index] = p;
        }
    }

    fn remove(&mut self, index: usize) -> u32 {
        let p = self.free.get(index).copied().unwrap_or(0);
        if p != 0 {
            self.free[index] = self.u32(p);
        }
        p
    }

    /// The block at `p`, of list `old`'s size, cut to list `new`'s; the rest freed.
    fn split(&mut self, p: u32, old: usize, new: usize) {
        let mut diff = self.units(old).saturating_sub(self.units(new));
        let mut p = p + UNIT * self.units(new);
        let i = self.index(diff);
        if self.units(i) != diff {
            let i = i.saturating_sub(1);
            self.insert(p, i);
            let k = self.units(i);
            p += UNIT * k;
            diff -= k;
        }
        let i = self.index(diff);
        self.insert(p, i);
    }

    /// A free block's list links, while they are glued: next at 4, before at 8.
    fn next(&self, p: u32) -> u32 {
        self.u32(p.wrapping_add(4))
    }

    fn before(&self, p: u32) -> u32 {
        self.u32(p.wrapping_add(8))
    }

    fn link(&mut self, a: u32, b: u32) {
        self.set_u32(a.wrapping_add(4), b);
        self.set_u32(b.wrapping_add(8), a);
    }

    fn unlink(&mut self, p: u32) {
        let (a, b) = (self.before(p), self.next(p));
        self.link(a, b);
    }

    /// Every free block joined with the free blocks right after it, then put back on the
    /// lists by its new size.
    fn glue_blocks(&mut self) {
        let head = self.head;
        // The untouched units are no free block to join.
        if self.lo != self.hi {
            self.set_u8(self.lo, 0);
        }
        self.link(head, head);
        for i in 0..INDEXES {
            while self.free[i] != 0 {
                let p = self.remove(i);
                let after = self.next(head);
                self.link(head, p);
                self.link(p, after);
                self.set_u16(p, FREE);
                self.set_u16(p + 2, self.units(i) as u16);
            }
        }
        let mut p = self.next(head);
        while p != head {
            loop {
                let nu = u32::from(self.u16(p + 2));
                let q = p + nu * UNIT;
                let nu2 = u32::from(self.u16(q + 2));
                if self.u16(q) != FREE || nu + nu2 >= 0x1_0000 {
                    break;
                }
                self.unlink(q);
                self.set_u16(p + 2, (nu + nu2) as u16);
            }
            p = self.next(p);
        }
        loop {
            let mut p = self.next(head);
            if p == head {
                break;
            }
            self.unlink(p);
            let mut nu = u32::from(self.u16(p + 2));
            while nu > 128 {
                self.insert(p, INDEXES - 1);
                nu -= 128;
                p += 128 * UNIT;
            }
            let mut i = self.index(nu);
            if self.units(i) != nu {
                i = i.saturating_sub(1);
                let k = nu.saturating_sub(self.units(i));
                self.insert(p + (nu - k) * UNIT, (k as usize).saturating_sub(1));
            }
            self.insert(p, i);
        }
    }

    /// A block of list `index`'s size when its list and the untouched units have none:
    /// from a larger free block, after gluing, or from the text's room. 0: no memory left.
    fn alloc_rare(&mut self, index: usize) -> u32 {
        if self.glue == 0 {
            self.glue = 255;
            self.glue_blocks();
            if self.free[index] != 0 {
                return self.remove(index);
            }
        }
        let mut i = index;
        loop {
            i += 1;
            if i == INDEXES {
                self.glue = self.glue.saturating_sub(1);
                let bytes = UNIT * self.units(index);
                if self.units_start.saturating_sub(self.text) > bytes {
                    self.units_start -= bytes;
                    return self.units_start;
                }
                return 0;
            }
            if self.free[i] != 0 {
                break;
            }
        }
        let p = self.remove(i);
        self.split(p, i, index);
        p
    }

    /// A block of `units` units (1 to 128); 0: no memory left.
    pub(super) fn alloc_units(&mut self, units: u32) -> u32 {
        let index = self.index(units);
        if self.free[index] != 0 {
            return self.remove(index);
        }
        let bytes = UNIT * self.units(index);
        if self.hi - self.lo >= bytes {
            let p = self.lo;
            self.lo += bytes;
            return p;
        }
        self.alloc_rare(index)
    }

    /// One unit for a context, from the top; 0: no memory left.
    pub(super) fn alloc_context(&mut self) -> u32 {
        if self.hi != self.lo {
            self.hi -= UNIT;
            return self.hi;
        }
        if self.free[0] != 0 {
            return self.remove(0);
        }
        self.alloc_rare(0)
    }

    /// The block at `old` of `units` units made one unit larger, moved if it must be; 0: no
    /// memory left.
    pub(super) fn expand(&mut self, old: u32, units: u32) -> u32 {
        let (i0, i1) = (self.index(units), self.index(units + 1));
        if i0 == i1 {
            return old;
        }
        let p = self.alloc_units(units + 1);
        if p != 0 {
            self.copy(p, old, UNIT * units);
            self.insert(old, i0);
        }
        p
    }

    /// The block at `old` of `units` units made `fewer` units, moved if a block of that
    /// size is free.
    pub(super) fn shrink(&mut self, old: u32, units: u32, fewer: u32) -> u32 {
        let (i0, i1) = (self.index(units), self.index(fewer));
        if i0 == i1 {
            return old;
        }
        if self.free[i1] != 0 {
            let p = self.remove(i1);
            self.copy(p, old, UNIT * fewer);
            self.insert(old, i0);
            p
        } else {
            self.split(old, i0, i1);
            old
        }
    }

    /// Frees the block at `p` of `units` units.
    pub(super) fn free_units(&mut self, p: u32, units: u32) {
        let i = self.index(units);
        self.insert(p, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_sizes_go_by_ones_twos_threes_then_fours() {
        let a = Alloc::new(1 << 20);
        assert_eq!(a.index_units[..9], [1, 2, 3, 4, 6, 8, 10, 12, 15]);
        assert_eq!(a.index_units[INDEXES - 1], 128);
        assert_eq!(
            (a.index(1), a.index(5), a.index(6), a.index(128)),
            (0, 4, 4, 37)
        );
    }

    #[test]
    fn blocks_freed_are_taken_again_and_glued_when_their_lists_run_dry() {
        let mut a = Alloc::new(1 << 20);
        a.init();
        assert!(a.text < a.units_start && a.lo == a.units_start && a.hi > a.lo);
        let (x, y) = (a.alloc_units(2), a.alloc_units(2));
        assert_eq!(y, x + 2 * UNIT);
        a.free_units(x, 2);
        assert_eq!(a.alloc_units(2), x);
        let c = a.alloc_context();
        assert_eq!(c, a.hi);
        // Two neighbours of two units freed, the untouched units used up: a block of four
        // is glued from them.
        a.free_units(x, 2);
        a.free_units(y, 2);
        a.lo = a.hi;
        assert_eq!(a.alloc_units(4), x);
        // A block grown keeps its bytes.
        a.set_u32(x, 0xdead_beef);
        let grown = a.expand(x, 4);
        assert_eq!(a.u32(grown), 0xdead_beef);
        // Out of bounds reads nothing and writes nothing.
        assert_eq!(a.u32(u32::MAX - 1), 0);
        a.set_u32(u32::MAX - 1, 7);
    }
}
