// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Memory dm has finished with goes back to the system (macOS).
//!
//! The system's allocator keeps what a program frees for the program's next
//! blocks and hands it back only when the computer runs short of memory.
//! Measured on macOS 27 (Apple M1, 2026-11-11): 64 MiB freed in blocks of
//! 64 KiB to 1 MiB stayed resident for as long as it was watched, and
//! `malloc_zone_pressure_relief` released none of it. A download's largest
//! blocks are of that kind: each connection's read buffer, which hyper grows
//! a step at a time up to 408 KiB, and each range's write buffers, all freed
//! as its ranges end. After one fast download of 512 MiB dm kept 18 to 22 MiB
//! it no longer used, and kept it (`docs/decisions.md`, 2026-11-12).
//!
//! So [`Allocator`] maps a block of [`APART`] or more on its own and unmaps it
//! as soon as it is freed; the rest stays with the system's allocator. Few
//! blocks are that large (a connection's buffers, the window's arena), so the
//! mapping and the pages the system zeroes for each cost little; the many
//! small ones, where a call to the system each would show, are not touched.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr;

/// From this size a block is a mapping of its own: as large as the write
/// buffer of a range (`ranged/staged.rs`) and every step of a connection's
/// read buffer from there up.
pub const APART: usize = 64 * 1024;

/// What a mapping is made of on every Mac: one page of 16 KiB on Apple
/// silicon, four of 4 KiB on Intel.
const GRAIN: usize = 16 * 1024;

/// The alignment every mapping has at least (a page on Intel); a block that
/// asks for more is left to the system's allocator.
const ALIGNED: usize = 4096;

/// The system's tag for memory an application names itself
/// (`VM_MEMORY_APPLICATION_SPECIFIC_1`, in the place `VM_MAKE_TAG` puts it):
/// `vmmap` lists these blocks as "App-Specific Tag 1", apart from the rest.
const TAG: libc::c_int = (240u32 << 24) as libc::c_int;

/// dm's allocator on macOS: the system's, except that a block of [`APART`]
/// or more is mapped on its own and given back the moment it is freed.
pub struct Allocator;

/// Whether a block of this size and alignment is mapped on its own.
fn apart(size: usize, align: usize) -> bool {
    size >= APART && align <= ALIGNED
}

/// The mapping a block of `size` takes, whole grains; `None` past what an
/// address can hold.
fn span(size: usize) -> Option<usize> {
    size.checked_add(GRAIN - 1).map(|s| s & !(GRAIN - 1))
}

/// A new mapping of at least `size` bytes, zeroed, or null.
fn map(size: usize) -> *mut u8 {
    let Some(len) = span(size) else {
        return ptr::null_mut();
    };
    // SAFETY: an anonymous private mapping at an address of the system's
    // choosing touches no memory of the program's; the result is checked
    // for failure before it is used.
    let at = unsafe {
        libc::mmap(
            ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            TAG,
            0,
        )
    };
    if at == libc::MAP_FAILED {
        ptr::null_mut()
    } else {
        at.cast()
    }
}

/// Gives back the mapping of a block of `size` at `at`.
///
/// # Safety
///
/// `at` is a block [`map`] returned for `size` (or for a size of the same
/// span), not given back before and not used after.
unsafe fn unmap(at: *mut u8, size: usize) {
    if let Some(len) = span(size) {
        // SAFETY: the mapping is the block's own, whole grains, as the caller
        // promises; nothing else lives in it.
        unsafe { libc::munmap(at.cast(), len) };
    }
}

// SAFETY: every block is either the system allocator's, handled by it with
// the same layout it was made with, or a mapping of its own made for at least
// its size and alignment (a mapping starts on a page, at least 4 KiB, and
// `apart` keeps larger alignments with the system). Which of the two a block
// is follows from its layout alone, and the caller passes the same layout
// back, so a block is always freed by the side that made it.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if apart(layout.size(), layout.align()) {
            map(layout.size())
        } else {
            // SAFETY: the caller's layout, as `GlobalAlloc::alloc` requires.
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if apart(layout.size(), layout.align()) {
            // A new mapping is zeroed by the system already.
            map(layout.size())
        } else {
            // SAFETY: the caller's layout, as `GlobalAlloc::alloc_zeroed` requires.
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        if apart(layout.size(), layout.align()) {
            // SAFETY: a block of this layout was mapped by `alloc` (the same
            // test), and the caller gives it back once.
            unsafe { unmap(at, layout.size()) }
        } else {
            // SAFETY: a block of this layout came from the system allocator.
            unsafe { System.dealloc(at, layout) }
        }
    }

    unsafe fn realloc(&self, at: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let align = layout.align();
        let (was, will) = (apart(layout.size(), align), apart(new_size, align));
        if !was && !will {
            // SAFETY: the system allocator's block, resized by it, as the
            // caller's arguments allow.
            return unsafe { System.realloc(at, layout, new_size) };
        }
        if was && will {
            if let (Some(old), Some(new)) = (span(layout.size()), span(new_size)) {
                // The same grains: the mapping already holds it.
                if new == old {
                    return at;
                }
                // Smaller: the grains past the new end are given back, in place.
                if new < old {
                    // SAFETY: the tail is whole grains of this block's own
                    // mapping, past its new end, used by nothing.
                    unsafe { libc::munmap(at.add(new).cast(), old - new) };
                    return at;
                }
            }
        }
        // Across `APART`, or larger: a new block, the bytes copied, the old freed.
        // SAFETY: the caller promises `new_size` with this alignment is a
        // valid layout (`GlobalAlloc::realloc`).
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, align) };
        // SAFETY: a non-zero size (the caller's promise), this allocator's own.
        let moved = unsafe { self.alloc(new_layout) };
        if !moved.is_null() {
            // SAFETY: both blocks are live and distinct, each at least as long
            // as the bytes copied; the old one is freed with its own layout.
            unsafe {
                ptr::copy_nonoverlapping(at, moved, layout.size().min(new_size));
                self.dealloc(at, layout);
            }
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system's page, and how many of the pages of `len` bytes at `at`
    /// are in memory (an unmapped page is not).
    fn resident(at: *mut u8, len: usize) -> (usize, usize) {
        // SAFETY: a query with no arguments to keep alive.
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap();
        let mut pages = vec![0 as libc::c_char; len.div_ceil(page)];
        // SAFETY: `pages` has a byte for every page of the range; `mincore`
        // reads none of the range's memory, only where its pages are.
        let asked = unsafe { libc::mincore(at.cast(), len, pages.as_mut_ptr()) };
        assert_eq!(asked, 0);
        let held = pages
            .iter()
            .filter(|p| libc::c_int::from(**p) & libc::MINCORE_INCORE != 0)
            .count();
        (pages.len(), held)
    }

    fn fill(at: *mut u8, len: usize) {
        for i in 0..len {
            // SAFETY: the block is at least `len` bytes, live, and ours.
            unsafe { *at.add(i) = (i % 251) as u8 };
        }
    }

    fn holds(at: *mut u8, len: usize) -> bool {
        // SAFETY: the block is at least `len` bytes, live, and ours.
        (0..len).all(|i| unsafe { *at.add(i) } == (i % 251) as u8)
    }

    #[test]
    fn a_large_block_is_given_back_when_freed() {
        let layout = Layout::from_size_align(32 * 1024 * 1024, 8).unwrap();
        // SAFETY: a non-zero layout; the block is freed once with it.
        let at = unsafe { Allocator.alloc(layout) };
        assert!(!at.is_null());
        fill(at, layout.size());
        let (pages, held) = resident(at, layout.size());
        assert_eq!(held, pages, "the block is not all in memory");
        // SAFETY: the block just made, with its layout.
        unsafe { Allocator.dealloc(at, layout) };
        // Gone at once. Not none at all: another test's thread may be given
        // part of the same addresses for blocks of its own in the meantime.
        let (pages, held) = resident(at, layout.size());
        assert!(held < pages / 4, "{held} of {pages} pages still held");
    }

    #[test]
    fn a_small_block_stays_with_the_system() {
        let layout = Layout::from_size_align(APART - 1, 8).unwrap();
        // SAFETY: as above.
        let at = unsafe { Allocator.alloc(layout) };
        assert!(!at.is_null());
        fill(at, layout.size());
        assert!(holds(at, layout.size()));
        // SAFETY: as above.
        unsafe { Allocator.dealloc(at, layout) };
    }

    #[test]
    fn a_block_keeps_its_bytes_as_it_grows_and_shrinks_across_the_line() {
        let mut layout = Layout::from_size_align(1000, 16).unwrap();
        // SAFETY: a non-zero layout.
        let mut at = unsafe { Allocator.alloc(layout) };
        fill(at, layout.size());
        // Small to apart, apart to larger, the same grains, smaller in place,
        // and back under the line.
        for size in [
            APART + 10,
            300 * 1024,
            300 * 1024 + 100,
            100 * 1024,
            APART - 1,
            500,
        ] {
            let kept = layout.size().min(size);
            // SAFETY: the live block with its own layout, a non-zero size.
            at = unsafe { Allocator.realloc(at, layout, size) };
            assert!(!at.is_null());
            assert!(holds(at, kept), "lost bytes resizing to {size}");
            layout = Layout::from_size_align(size, 16).unwrap();
            fill(at, size);
        }
        // SAFETY: as above.
        unsafe { Allocator.dealloc(at, layout) };
    }

    #[test]
    fn zeroed_blocks_are_zero_on_both_sides() {
        for size in [64, APART, 3 * APART + 5] {
            let layout = Layout::from_size_align(size, 8).unwrap();
            // SAFETY: as above.
            let at = unsafe { Allocator.alloc_zeroed(layout) };
            assert!(!at.is_null());
            // SAFETY: a live block of `size` bytes.
            assert!((0..size).all(|i| unsafe { *at.add(i) } == 0));
            // SAFETY: as above.
            unsafe { Allocator.dealloc(at, layout) };
        }
    }

    #[test]
    fn a_large_alignment_is_honoured() {
        let layout = Layout::from_size_align(APART * 2, 64 * 1024).unwrap();
        // SAFETY: as above.
        let at = unsafe { Allocator.alloc(layout) };
        assert!(!at.is_null());
        assert_eq!(at as usize % layout.align(), 0);
        fill(at, layout.size());
        // SAFETY: as above.
        unsafe { Allocator.dealloc(at, layout) };
    }
}
