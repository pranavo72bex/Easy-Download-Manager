// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A question's sheet on macOS, begun and ended by dm rather than by GPUI, so
//! that dm keeps working while the sheet slides (gaps row 194).
//!
//! AppKit slides a sheet in and out in a run loop of its own
//! (`_NSMoveTimerRunLoopMode`, about 0.26 s each way on this Mac), and in it
//! the main dispatch queue, where GPUI runs everything, is not served. Begun
//! by GPUI (`WindowKind::Dialog`, inside its window's making), every window
//! stood still and the control port waited out the slide, twice for each
//! question: measured, `ui.texts` asked every 20 ms waited 247 to 293 ms as a
//! question opened and again as it closed, the window's thread idle meanwhile.
//!
//! Here the sheet is begun and ended from a run loop timer of dm's own, where
//! no GPUI work is under way (outside its `App`, and outside any task of the
//! main queue, whose work a run loop inside it cannot serve). While the sheet
//! slides, a timer in AppKit's mode runs the default mode's waiting work every
//! 4 ms: GPUI's tasks, its frames and the control port's answers go on as
//! between any two frames. The slide itself is AppKit's own, unchanged; the
//! question's first frame is drawn before it starts, so it slides in drawn.
//!
//! One sheet moves at a time, in the order asked: a question asked while
//! another slides out begins once that one has gone, as AppKit asks it to.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::NSWindow;
use objc2_foundation::NSString;

/// The mode AppKit runs its window slides in. Private: were it renamed, the
/// timer below would never run and a slide would hold the window as before,
/// nothing worse.
const SLIDE_MODE: &str = "_NSMoveTimerRunLoopMode";

/// How often the waiting work is run while a sheet slides.
const DRAIN_EVERY: f64 = 0.004;

type CFRef = *const c_void;
type TimerCallout = extern "C" fn(timer: CFRef, info: *mut c_void);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: CFRef;
    fn CFRunLoopGetMain() -> CFRef;
    fn CFRunLoopRunInMode(mode: CFRef, seconds: f64, return_after_source_handled: u8) -> i32;
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopTimerCreate(
        allocator: CFRef,
        fire_date: f64,
        interval: f64,
        flags: usize,
        order: isize,
        callout: TimerCallout,
        context: *mut c_void,
    ) -> CFRef;
    fn CFRunLoopAddTimer(run_loop: CFRef, timer: CFRef, mode: CFRef);
    fn CFRunLoopTimerInvalidate(timer: CFRef);
    fn CFRelease(cf: CFRef);
}

/// A sheet to move: shown on the window in front, or taken off its window
/// (then `done`, which closes it in GPUI).
enum Move {
    Begin(Retained<NSWindow>),
    End(Retained<NSWindow>, Box<dyn FnOnce()>),
}

thread_local! {
    /// The moves asked for, in order.
    static MOVES: RefCell<VecDeque<Move>> = const { RefCell::new(VecDeque::new()) };
    /// A sheet is sliding: the next move waits for it.
    static SLIDING: Cell<bool> = const { Cell::new(false) };
    /// The waiting work is being run: a slide begun by that work (inside
    /// GPUI, its `App` in use) runs none of it again.
    static DRAINING: Cell<bool> = const { Cell::new(false) };
    /// The timer that makes the moves is set.
    static SET: Cell<bool> = const { Cell::new(false) };
}

/// `sheet`, made but not shown, is shown as a sheet on the window in front
/// (on its sheet, where it has one), as GPUI's `Dialog` shows one; where no
/// window is in front, as a window of its own.
pub fn begin(sheet: Retained<NSWindow>) {
    // Not over the other windows, as a GPUI `Floating` window is made: a
    // sheet stands with its window, as GPUI's `Dialog` stood.
    sheet.setLevel(objc2_app_kit::NSNormalWindowLevel);
    sheet.setAcceptsMouseMovedEvents(false);
    ask(Move::Begin(sheet));
}

/// `sheet` slides off its window, then `done` closes it. One not shown yet
/// is never shown: `done` at once.
pub fn end(sheet: Retained<NSWindow>, done: impl FnOnce() + 'static) {
    if unshown(&sheet) {
        done();
        return;
    }
    ask(Move::End(sheet, Box::new(done)));
}

/// `sheet` is taken off its window now, without sliding the window's work
/// along: dm quitting, which closes every window at once after it (macOS
/// holds its quitting while a sheet is up).
pub fn end_now(sheet: &NSWindow) {
    if unshown(sheet) || SLIDING.with(Cell::get) {
        return;
    }
    if let Some(parent) = sheet.sheetParent() {
        parent.endSheet(sheet);
    }
}

/// Whether a sheet is sliding now: what runs meanwhile runs inside AppKit's
/// slide, where dm cannot quit (macOS drops a `terminate:` asked inside it).
pub fn sliding() -> bool {
    SLIDING.with(Cell::get)
}

/// Every sheet asked to slide off and not yet gone is taken off its window
/// now, and closed: dm quitting.
pub fn end_all_now() {
    if SLIDING.with(Cell::get) {
        return;
    }
    let moves = MOVES.with(|moves| std::mem::take(&mut *moves.borrow_mut()));
    for next in moves {
        if let Move::End(sheet, done) = next {
            if let Some(parent) = sheet.sheetParent() {
                parent.endSheet(&sheet);
            }
            done();
        }
    }
}

/// Whether `sheet` was still waiting to be shown; it no longer is.
fn unshown(sheet: &NSWindow) -> bool {
    MOVES.with(|moves| {
        let mut moves = moves.borrow_mut();
        let before = moves.len();
        moves.retain(|m| !matches!(m, Move::Begin(w) if std::ptr::eq(&**w, sheet)));
        moves.len() != before
    })
}

fn ask(next: Move) {
    MOVES.with(|moves| moves.borrow_mut().push_back(next));
    if SET.with(|set| set.replace(true)) {
        return;
    }
    // SAFETY: a one-shot timer on the main run loop, where this runs (GPUI's
    // thread); the run loop keeps it, so dm's own reference goes at once.
    unsafe {
        let timer = CFRunLoopTimerCreate(
            std::ptr::null(),
            CFAbsoluteTimeGetCurrent(),
            0.0,
            0,
            0,
            moved,
            std::ptr::null_mut(),
        );
        CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopDefaultMode);
        CFRelease(timer);
    }
}

/// The moves asked for, one after another; none while a sheet slides (the
/// one sliding makes the rest when it is done).
extern "C" fn moved(_timer: CFRef, _info: *mut c_void) {
    SET.with(|set| set.set(false));
    if SLIDING.with(Cell::get) {
        return;
    }
    while let Some(next) = MOVES.with(|moves| moves.borrow_mut().pop_front()) {
        SLIDING.with(|s| s.set(true));
        match next {
            Move::Begin(sheet) => {
                if let Some(main) = main_window() {
                    let parent = main.attachedSheet().unwrap_or(main);
                    // SAFETY: `-[NSWindow beginSheet:completionHandler:]` on
                    // the main thread, both windows alive (held here), with
                    // no handler (nil), as GPUI's `Dialog` begins one.
                    slide_working(|| unsafe {
                        let _: () = msg_send![
                            &*parent,
                            beginSheet: &*sheet,
                            completionHandler: None::<&block2::DynBlock<dyn Fn(isize)>>
                        ];
                    });
                }
                sheet.makeKeyAndOrderFront(None);
            }
            Move::End(sheet, done) => {
                if let Some(parent) = sheet.sheetParent() {
                    slide_working(|| parent.endSheet(&sheet));
                }
                done();
            }
        }
        SLIDING.with(|s| s.set(false));
    }
}

/// The application's main window (`NSApp.mainWindow`): the window in front.
fn main_window() -> Option<Retained<NSWindow>> {
    // SAFETY: `+[NSApplication sharedApplication]` and its `mainWindow`, read
    // on the main thread, where AppKit is asked everything; the window comes
    // back retained.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        msg_send![app, mainWindow]
    }
}

/// Runs `slide`, an AppKit call that slides a sheet, with the default mode's
/// waiting work run every [`DRAIN_EVERY`] meanwhile.
fn slide_working(slide: impl FnOnce()) {
    let mode = NSString::from_str(SLIDE_MODE);
    // SAFETY: a repeating timer on the main run loop (this thread), in the
    // slide's mode only, taken off again once the slide is over.
    let timer = unsafe {
        let timer = CFRunLoopTimerCreate(
            std::ptr::null(),
            CFAbsoluteTimeGetCurrent() + DRAIN_EVERY,
            DRAIN_EVERY,
            0,
            0,
            drain,
            std::ptr::null_mut(),
        );
        CFRunLoopAddTimer(CFRunLoopGetMain(), timer, Retained::as_ptr(&mode).cast());
        timer
    };
    slide();
    // SAFETY: the timer made above, still dm's own reference.
    unsafe {
        CFRunLoopTimerInvalidate(timer);
        CFRelease(timer);
    }
}

/// The default mode's waiting work, run once: GPUI's tasks (the main
/// dispatch queue, served in the default mode), its frames, the timers.
extern "C" fn drain(_timer: CFRef, _info: *mut c_void) {
    if DRAINING.with(|d| d.replace(true)) {
        return;
    }
    // SAFETY: a pass of the main run loop on its own thread, inside a timer
    // of its own, which CoreFoundation allows; nothing of GPUI's is in use
    // here (`moved` runs from the run loop itself).
    unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.0, 0) };
    DRAINING.with(|d| d.set(false));
}
