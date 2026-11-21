// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
