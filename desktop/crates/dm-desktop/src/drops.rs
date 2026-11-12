// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Links dragged onto the window from a browser.
//!
//! winit hands a program only dropped files: a link dragged from a page
//! (an address, not a file) never reached dm, which could only say so.
//! Where the window system offers the link, it is taken here and queued,
//! and the window reads it as it reads a dropped file (`take`). File drops
//! still go through winit as before.
//!
//! - macOS: the window also registers for URLs and text, and the two drag
//!   methods of winit's window delegate are swapped at run time for ones
//!   that read a link off the drag's pasteboard and pass every file drag
//!   on to winit's own unchanged (as `dock.rs` adds a method to winit's
//!   application delegate).
//! - Windows: winit's drop target (an OLE `IDropTarget` taking files only)
//!   is replaced on the window by dm's, which takes files as it did and a
//!   browser's link (`UniformResourceLocatorW`, else plain text).

use std::path::{Path, PathBuf};

// The queue and Windows' drop target are every window's
// (`dm_desktop_base::drops`); macOS's drag methods are winit's window's, here.
#[cfg(target_os = "macos")]
use dm_desktop_base::drops::arrived;
pub use dm_desktop_base::drops::{take, Dropped};

/// The window is told how to be woken once.
static WATCHED: std::sync::Once = std::sync::Once::new();

/// A file dropped through dm's own drop target, read as one winit hands
/// over is.
#[derive(Debug)]
pub struct DroppedPath(pub PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Puts dm's drop target on the window whose native handle this is, where
/// the window system's own needs replacing (Windows). Once; later calls do
/// nothing.
pub fn attach(window: raw_window_handle::RawWindowHandle) {
    #[cfg(windows)]
    if let raw_window_handle::RawWindowHandle::Win32(handle) = window {
        win::install(handle.hwnd.get());
    }
    let _ = window;
}

/// The floating drop target's window title, by which Windows finds it.
pub const DROP_TARGET_TITLE: &str = "dm drop target";

/// Puts dm's drop target on the floating drop target's window too.
pub fn attach_floating() {
    #[cfg(windows)]
    win::install_titled(DROP_TARGET_TITLE);
}

/// Has the window take links dropped on it, where the window system offers
/// them. Called every frame; does its work once a window exists.
pub fn watch(ctx: &egui::Context) {
    WATCHED.call_once(|| {
        let ctx = ctx.clone();
        dm_desktop_base::drops::on_arrival(move || ctx.request_repaint());
    });
    #[cfg(target_os = "macos")]
    mac::install();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::CStr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{msg_send, MainThreadMarker};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSString};

    static INSTALLED: AtomicBool = AtomicBool::new(false);
    /// How many windows have been registered for links: a window opened
    /// since (the floating drop target) is registered too.
    static REGISTERED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static ENTERED: OnceLock<Imp> = OnceLock::new();
    static PERFORM: OnceLock<Imp> = OnceLock::new();

    type DragMethod = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool;

    /// The drag's pasteboard.
    unsafe fn pasteboard(sender: *mut AnyObject) -> Option<Retained<AnyObject>> {
        if sender.is_null() {
            return None;
        }
        // SAFETY: `sender` was checked to be non-null, and AppKit hands a drag
        // method its NSDraggingInfo, which answers `draggingPasteboard` with an
        // object or nil (the `Option`).
        unsafe { msg_send![sender, draggingPasteboard] }
    }

    /// Whether the drag carries files, which are winit's to take.
    unsafe fn has_files(board: &AnyObject) -> bool {
        let kind = NSString::from_str("NSFilenamesPboardType");
        // SAFETY: `board` is a live NSPasteboard for the call; `propertyListForType:`
        // takes an NSString and returns an object or nil.
        let files: Option<Retained<AnyObject>> =
            unsafe { msg_send![board, propertyListForType: &*kind] };
        files.is_some()
    }

    /// The link a browser put on the drag: its URL, else its text.
    unsafe fn link_text(board: &AnyObject) -> Option<String> {
        for kind in ["public.url", "public.utf8-plain-text"] {
            let kind = NSString::from_str(kind);
            // SAFETY: as in `has_files`; `stringForType:` takes an NSString and
            // returns an NSString or nil.
            let text: Option<Retained<NSString>> =
                unsafe { msg_send![board, stringForType: &*kind] };
            if let Some(text) = text.map(|t| t.to_string()) {
                if !dm_desktop_links(&text).is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }

    fn dm_desktop_links(text: &str) -> Vec<String> {
        crate::app::links_in(text)
    }

    unsafe fn original(
        slot: &OnceLock<Imp>,
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        match slot.get() {
            // SAFETY: `imp` was returned by `class_replaceMethod` for the very
            // method this slot is for, whose signature is `DragMethod`, so calling
            // it as one with the arguments AppKit gave us is what it expects.
            Some(imp) => unsafe {
                let method: DragMethod = std::mem::transmute(*imp);
                method(this, cmd, sender)
            },
            None => Bool::NO,
        }
    }

    /// `- (NSDragOperation)draggingEntered:(id<NSDraggingInfo>)sender`
    unsafe extern "C-unwind" fn entered(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        // SAFETY: AppKit calls this in place of `draggingEntered:`, so `sender` is
        // null or a live NSDraggingInfo, which is all `pasteboard` and `original`
        // ask of it.
        unsafe {
            match pasteboard(sender) {
                Some(board) if !has_files(&board) && link_text(&board).is_some() => Bool::YES,
                _ => original(&ENTERED, this, cmd, sender),
            }
        }
    }

    /// `- (BOOL)performDragOperation:(id<NSDraggingInfo>)sender`
    unsafe extern "C-unwind" fn perform(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        // SAFETY: AppKit calls this in place of `performDragOperation:`; as in
        // `entered`, `sender` is null or a live NSDraggingInfo.
        unsafe {
            if let Some(board) = pasteboard(sender) {
                if !has_files(&board) {
                    if let Some(text) = link_text(&board) {
                        super::arrived(super::Dropped::Text(text));
                        return Bool::YES;
                    }
                }
            }
            original(&PERFORM, this, cmd, sender)
        }
    }

    pub(super) fn install() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: `app` is the live shared NSApplication, and this runs on the
        // main thread (the marker above); `windows` returns an NSArray.
        let windows: Retained<NSArray<AnyObject>> = unsafe { msg_send![&*app, windows] };
        if windows.count() == 0 {
            return; // a login launch: no window yet
        }
        if INSTALLED.load(Ordering::Relaxed)
            && REGISTERED.load(Ordering::Relaxed) == windows.count()
        {
            return;
        }
        let types = NSArray::from_retained_slice(&[
            NSString::from_str("NSFilenamesPboardType"),
            NSString::from_str("public.url"),
            NSString::from_str("public.utf8-plain-text"),
        ]);
        // BOOL is `bool` on Apple silicon and `signed char` on Intel.
        let encoding: &CStr = if cfg!(target_arch = "aarch64") {
            c"B@:@"
        } else {
            c"c@:@"
        };
        let mut done = INSTALLED.load(Ordering::Relaxed);
        REGISTERED.store(windows.count(), Ordering::Relaxed);
        for window in windows.iter() {
            // SAFETY: `window` is a live NSWindow from the array read above;
            // `delegate` returns its delegate or nil.
            let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*window, delegate] };
            let Some(delegate) = delegate else {
                continue;
            };
            // SAFETY: `registerForDraggedTypes:` takes an NSArray of type names,
            // which `types` is, and returns nothing.
            unsafe {
                let _: () = msg_send![&*window, registerForDraggedTypes: &*types];
            }
            if done {
                continue;
            }
            // SAFETY: `delegate` is a live object; `object_getClass` only reads its
            // class pointer.
            let class: *const AnyClass = unsafe { objc2::ffi::object_getClass(&*delegate) };
            for (name, ours, slot) in [
                (c"draggingEntered:", entered as DragMethod, &ENTERED),
                (c"performDragOperation:", perform as DragMethod, &PERFORM),
            ] {
                let selector = Sel::register(name);
                // SAFETY: `ours` has the signature of the Objective-C method it
                // replaces (`DragMethod`), `encoding` is that method's type encoding
                // on this architecture, and `class` is the delegate's class, not an
                // instance. The implementation it replaces is kept to pass file
                // drags on to.
                unsafe {
                    let imp: Imp = std::mem::transmute(ours);
                    // The class's own, kept to pass file drags on to.
                    if let Some(previous) = objc2::ffi::class_replaceMethod(
                        class as *mut AnyClass,
                        selector,
                        imp,
                        encoding.as_ptr(),
                    ) {
                        let _ = slot.set(previous);
                    }
                }
            }
            done = true;
        }
        INSTALLED.store(done, Ordering::Relaxed);
    }
}

#[cfg(windows)]
use dm_desktop_base::drops::win;
