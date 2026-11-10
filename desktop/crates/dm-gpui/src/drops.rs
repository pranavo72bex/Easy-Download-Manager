// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What is dropped on dm's windows, taken as the egui window takes it: a
//! link dragged from a browser (an address, not a file), and files (a
//! `.torrent` is itself the download; a `.url`, `.webloc` or text file of
//! links is read for them: `dm_shell::app::links::links_at`). GPUI's windows
//! take only dragged files, so: on macOS their drag methods are given dm's
//! own, which take a link off the drag's pasteboard and pass every file
//! drag on to GPUI's unchanged, as the egui window does with winit's; on
//! Windows dm's OLE drop target takes GPUI's place on the window and takes
//! both (`dm_desktop_base::drops::win`, the egui window's too). Either way
//! what arrives waits in `dm_desktop_base::drops` for the window's next
//! look; on Linux GPUI's own drop takes the files.

use dm_desktop_base::drops::Dropped;
use dm_desktop_base::i18n::t;
use gpui::Context;

use crate::list::Downloads;

/// What a drop with no link in it is answered with.
const DROP_HINT: &str = "Drop a link, .torrent, .url, .webloc or text file of links here.";

/// A link's text taken off a drag, for the window's next look.
#[cfg(target_os = "macos")]
fn arrived(text: String) {
    dm_desktop_base::drops::arrived(Dropped::Text(text));
}

/// The links dropped files hold: a `.torrent` is itself the download; a
/// `.url`, `.webloc` or text file of links is read for them.
fn links_in_files(paths: &[std::path::PathBuf]) -> Vec<String> {
    paths
        .iter()
        .flat_map(|path| dm_shell::app::links::links_at(path, || std::fs::read(path).ok()))
        .collect()
}

impl Downloads {
    /// Files dropped on a window: the links they hold added, or said where
    /// there are none.
    pub(crate) fn dropped_files(&mut self, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        self.add_dropped(links_in_files(paths), cx);
    }

    /// What dm's own drop targets took since the last look (a browser's
    /// links, and on Windows files too), added.
    pub(crate) fn take_dropped_links(&mut self, cx: &mut Context<Self>) {
        let dropped = dm_desktop_base::drops::take();
        if dropped.is_empty() {
            return;
        }
        let mut links = Vec::new();
        for what in dropped {
            match what {
                Dropped::Text(text) => links.extend(dm_shell::app::links::links_in(&text)),
                Dropped::File(path) => links.extend(links_in_files(&[path])),
            }
        }
        self.add_dropped(links, cx);
    }

    fn add_dropped(&mut self, links: Vec<String>, cx: &mut Context<Self>) {
        if links.is_empty() {
            // A drop with no link in it was ignored without a word.
            self.say(t(DROP_HINT).into());
            cx.notify();
            return;
        }
        if let Err(e) = self.add(links, dm_core::AddOptions::default(), cx) {
            self.say(e);
        }
        cx.notify();
    }
}

/// `window` takes a browser's link dragged onto it, as well as files.
#[cfg(target_os = "macos")]
pub fn take_links(window: &gpui::Window) {
    mac::install(window);
}

/// On Windows dm's OLE drop target takes GPUI's place on `window`.
#[cfg(windows)]
pub fn take_links(window: &gpui::Window) {
    if let Some(hwnd) = crate::syswindow::hwnd(window) {
        dm_desktop_base::drops::win::install(hwnd as isize);
    }
}

/// On Linux GPUI's own drop takes files (X11's and Wayland's offer of a
/// browser's link is not yet taken).
#[cfg(not(any(target_os = "macos", windows)))]
pub fn take_links(_: &gpui::Window) {}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::CStr;
    use std::sync::OnceLock;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2_foundation::{NSArray, NSString};

    /// GPUI's own methods, kept to pass file drags on to (its window and
    /// panel classes share them).
    static ENTERED: OnceLock<Imp> = OnceLock::new();
    static UPDATED: OnceLock<Imp> = OnceLock::new();
    static PERFORM: OnceLock<Imp> = OnceLock::new();
    /// The classes given dm's methods, each once: given twice, a method
    /// would keep dm's own as "GPUI's" and call itself.
    static CLASSES: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

    /// `draggingEntered:` and `draggingUpdated:` answer an `NSDragOperation`.
    type Operation = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize;
    /// `performDragOperation:` answers a `BOOL`.
    type Perform = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool;
    /// `NSDragOperationCopy`.
    const COPY: usize = 1;

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

    /// Whether the drag carries files, which are GPUI's to take.
    unsafe fn has_files(board: &AnyObject) -> bool {
        let kind = NSString::from_str("NSFilenamesPboardType");
        // SAFETY: `board` is a live NSPasteboard for the call;
        // `propertyListForType:` takes an NSString and returns an object or nil.
        let files: Option<Retained<AnyObject>> =
            unsafe { msg_send![board, propertyListForType: &*kind] };
        files.is_some()
    }

    /// The link a browser put on the drag: its URL, else its text, where it
    /// holds one.
    unsafe fn link_text(board: &AnyObject) -> Option<String> {
        for kind in ["public.url", "public.utf8-plain-text"] {
            let kind = NSString::from_str(kind);
            // SAFETY: as in `has_files`; `stringForType:` takes an NSString and
            // returns an NSString or nil.
            let text: Option<Retained<NSString>> =
                unsafe { msg_send![board, stringForType: &*kind] };
            if let Some(text) = text.map(|t| t.to_string()) {
                if !dm_shell::app::links::links_in(&text).is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }

    /// A link on the drag, and no files.
    unsafe fn a_link(sender: *mut AnyObject) -> Option<String> {
        // SAFETY: `sender` is what AppKit gave the drag method.
        let board = unsafe { pasteboard(sender) }?;
        // SAFETY: `board` is the live pasteboard just read.
        if unsafe { has_files(&board) } {
            return None;
        }
        // SAFETY: as above.
        unsafe { link_text(&board) }
    }

    /// GPUI's own `draggingEntered:` or `draggingUpdated:`.
    unsafe fn operation(
        slot: &OnceLock<Imp>,
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> usize {
        match slot.get() {
            // SAFETY: `imp` was returned by `class_replaceMethod` for the very
            // method this slot is for, whose signature is `Operation`.
            Some(imp) => unsafe {
                let method: Operation = std::mem::transmute(*imp);
                method(this, cmd, sender)
            },
            None => 0,
        }
    }

    /// `- (NSDragOperation)draggingEntered:(id<NSDraggingInfo>)sender`
    unsafe extern "C-unwind" fn entered(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> usize {
        // SAFETY: AppKit calls this in place of `draggingEntered:`, so `sender`
        // is null or a live NSDraggingInfo.
        unsafe {
            if a_link(sender).is_some() {
                COPY
            } else {
                operation(&ENTERED, this, cmd, sender)
            }
        }
    }

    /// `- (NSDragOperation)draggingUpdated:(id<NSDraggingInfo>)sender`
    unsafe extern "C-unwind" fn updated(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> usize {
        // SAFETY: as in `entered`.
        unsafe {
            if a_link(sender).is_some() {
                COPY
            } else {
                operation(&UPDATED, this, cmd, sender)
            }
        }
    }

    /// `- (BOOL)performDragOperation:(id<NSDraggingInfo>)sender`
    unsafe extern "C-unwind" fn perform(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        // SAFETY: as in `entered`.
        unsafe {
            if let Some(text) = a_link(sender) {
                super::arrived(text);
                // The window wakes as at any change, and takes it.
                dm_shell::app::changes::changed();
                return Bool::YES;
            }
            match PERFORM.get() {
                // SAFETY: as in `operation`, for `Perform`.
                Some(imp) => {
                    let method: Perform = std::mem::transmute(*imp);
                    method(this, cmd, sender)
                }
                None => Bool::NO,
            }
        }
    }

    /// `perform`, for the tests to call as AppKit would.
    #[cfg(test)]
    pub(super) unsafe fn perform_for_test(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        // SAFETY: the caller's obligations are those of `perform`.
        unsafe { perform(this, cmd, sender) }
    }

    /// Registers `window` for links and text as well as files, and, once,
    /// gives its class dm's drag methods.
    pub(super) fn install(window: &gpui::Window) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return;
        };
        // SAFETY: `ns_view` is the live content view of `window`, which this
        // borrow keeps open; only its window is read, on the main thread.
        let view: &objc2_app_kit::NSView = unsafe { appkit.ns_view.cast().as_ref() };
        let Some(ns_window) = view.window() else {
            return;
        };
        let types = NSArray::from_retained_slice(&[
            NSString::from_str("NSFilenamesPboardType"),
            NSString::from_str("public.url"),
            NSString::from_str("public.utf8-plain-text"),
        ]);
        // SAFETY: `registerForDraggedTypes:` takes an NSArray of type names,
        // which `types` is, and returns nothing.
        unsafe {
            let _: () = msg_send![&*ns_window, registerForDraggedTypes: &*types];
        }
        let object: *const AnyObject = (&*ns_window as *const objc2_app_kit::NSWindow).cast();
        // SAFETY: `object` points at `ns_window`, a live object for the call;
        // `object_getClass` only reads its class pointer.
        let class: *const AnyClass = unsafe { objc2::ffi::object_getClass(object) };
        {
            let mut done = dm_core::lock(&CLASSES);
            if done.contains(&(class as usize)) {
                return;
            }
            done.push(class as usize);
        }
        // NSUInteger and BOOL as each architecture encodes them.
        let operation: &CStr = c"Q@:@";
        let boolean: &CStr = if cfg!(target_arch = "aarch64") {
            c"B@:@"
        } else {
            c"c@:@"
        };
        // SAFETY: each replacement has exactly the signature of the method it
        // replaces (`Operation` or `Perform`), its encoding is that method's
        // on this architecture, and `class` is GPUI's window class, not an
        // instance. The implementation it replaces is kept to pass file drags
        // on to.
        unsafe {
            for (name, imp, encoding, slot) in [
                (
                    c"draggingEntered:",
                    std::mem::transmute::<Operation, Imp>(entered as Operation),
                    operation,
                    &ENTERED,
                ),
                (
                    c"draggingUpdated:",
                    std::mem::transmute::<Operation, Imp>(updated as Operation),
                    operation,
                    &UPDATED,
                ),
                (
                    c"performDragOperation:",
                    std::mem::transmute::<Perform, Imp>(perform as Perform),
                    boolean,
                    &PERFORM,
                ),
            ] {
                let selector = Sel::register(name);
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
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject, Sel};
    use objc2::{class, define_class, msg_send};
    use objc2_foundation::{NSArray, NSString};

    thread_local! {
        static BOARD: std::cell::RefCell<Option<Retained<AnyObject>>> =
            const { std::cell::RefCell::new(None) };
    }

    define_class!(
        // A stand-in for AppKit's dragging info: only its pasteboard.
        #[unsafe(super(NSObject))]
        #[name = "DmGpuiTestDrag"]
        struct FakeDrag;

        impl FakeDrag {
            #[unsafe(method(draggingPasteboard))]
            fn board(&self) -> *mut AnyObject {
                BOARD.with(|b| {
                    b.borrow()
                        .as_ref()
                        .map_or(std::ptr::null_mut(), |b| Retained::as_ptr(b) as *mut AnyObject)
                })
            }
        }
    );

    fn board_with(kind: &str, write: impl FnOnce(&AnyObject, &NSString)) {
        // SAFETY: a class method of NSPasteboard that returns a new pasteboard.
        let board: Retained<AnyObject> =
            unsafe { msg_send![class!(NSPasteboard), pasteboardWithUniqueName] };
        // SAFETY: `board` is that live pasteboard; `clearContents` returns the
        // change count.
        let _: isize = unsafe { msg_send![&*board, clearContents] };
        write(&board, &NSString::from_str(kind));
        BOARD.with(|b| *b.borrow_mut() = Some(board));
    }

    fn drop_it() -> Bool {
        // SAFETY: `FakeDrag` is this test's own NSObject subclass, and `new`
        // allocates and initialises one.
        let drag: Retained<FakeDrag> =
            unsafe { msg_send![<FakeDrag as objc2::ClassType>::class(), new] };
        let sender = Retained::as_ptr(&drag) as *mut AnyObject;
        let sel = Sel::register(c"performDragOperation:");
        // SAFETY: `sender` is the live FakeDrag just made, which answers
        // `draggingPasteboard`; no method of GPUI's is installed in a test, so
        // the null `this` is never used.
        unsafe { super::mac::perform_for_test(std::ptr::null_mut(), sel, sender) }
    }

    /// The texts dropped since the last call.
    fn take() -> Vec<String> {
        dm_desktop_base::drops::take()
            .into_iter()
            .filter_map(|what| match what {
                super::Dropped::Text(text) => Some(text),
                super::Dropped::File(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_link_dragged_from_a_browser_is_taken_and_a_file_is_left_to_gpui() {
        let _ = take();
        // A browser's link: its URL on the pasteboard.
        board_with("public.url", |board, kind| {
            let url = NSString::from_str("https://example.com/files/big.zip");
            // SAFETY: `board` is the live pasteboard; `setString:forType:`
            // takes two NSStrings and returns a BOOL.
            let _: Bool = unsafe { msg_send![board, setString: &*url, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::YES);
        assert_eq!(take(), ["https://example.com/files/big.zip"]);

        // Text that is no link is not taken.
        board_with("public.utf8-plain-text", |board, kind| {
            let text = NSString::from_str("just some words");
            // SAFETY: as above.
            let _: Bool = unsafe { msg_send![board, setString: &*text, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::NO);
        assert!(take().is_empty());

        // Files are GPUI's: passed on (to nothing here), nothing queued.
        board_with("NSFilenamesPboardType", |board, kind| {
            let files = NSArray::from_retained_slice(&[NSString::from_str("/tmp/links.url")]);
            // SAFETY: `setPropertyList:forType:` takes a property list (an
            // NSArray of NSStrings here) and a type name, and returns a BOOL.
            let _: Bool = unsafe { msg_send![board, setPropertyList: &*files, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::NO);
        assert!(take().is_empty());
    }
}
