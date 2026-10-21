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

use std::sync::{Mutex, OnceLock};

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
static WINDOW: OnceLock<egui::Context> = OnceLock::new();

/// Text a drop handed over (a link, or lines of links), for the window to
/// read; the window is woken to do so.
#[allow(dead_code)]
pub(crate) fn arrived(text: String) {
    dm_core::lock(&PENDING).push(text);
    if let Some(ctx) = WINDOW.get() {
        ctx.request_repaint();
    }
}

/// What has been dropped since the last call.
pub fn take() -> Vec<String> {
    std::mem::take(&mut *dm_core::lock(&PENDING))
}

/// Has the window take links dropped on it, where the window system offers
/// them. Called every frame; does its work once a window exists.
pub fn watch(ctx: &egui::Context) {
    let _ = WINDOW.set(ctx.clone());
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
    static ENTERED: OnceLock<Imp> = OnceLock::new();
    static PERFORM: OnceLock<Imp> = OnceLock::new();

    type DragMethod = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool;

    /// The drag's pasteboard.
    unsafe fn pasteboard(sender: *mut AnyObject) -> Option<Retained<AnyObject>> {
        if sender.is_null() {
            return None;
        }
        unsafe { msg_send![sender, draggingPasteboard] }
    }

    /// Whether the drag carries files, which are winit's to take.
    unsafe fn has_files(board: &AnyObject) -> bool {
        let kind = NSString::from_str("NSFilenamesPboardType");
        let files: Option<Retained<AnyObject>> =
            unsafe { msg_send![board, propertyListForType: &*kind] };
        files.is_some()
    }

    /// The link a browser put on the drag: its URL, else its text.
    unsafe fn link_text(board: &AnyObject) -> Option<String> {
        for kind in ["public.url", "public.utf8-plain-text"] {
            let kind = NSString::from_str(kind);
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
        unsafe {
            if let Some(board) = pasteboard(sender) {
                if !has_files(&board) {
                    if let Some(text) = link_text(&board) {
                        super::arrived(text);
                        return Bool::YES;
                    }
                }
            }
            original(&PERFORM, this, cmd, sender)
        }
    }

    /// `perform`, for the tests to call as AppKit would.
    #[cfg(test)]
    pub(super) unsafe fn perform_for_test(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        unsafe { perform(this, cmd, sender) }
    }

    pub(super) fn install() {
        if INSTALLED.load(Ordering::Relaxed) {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let windows: Retained<NSArray<AnyObject>> = unsafe { msg_send![&*app, windows] };
        if windows.count() == 0 {
            return; // a login launch: no window yet
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
        let mut done = false;
        for window in windows.iter() {
            let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*window, delegate] };
            let Some(delegate) = delegate else {
                continue;
            };
            unsafe {
                let _: () = msg_send![&*window, registerForDraggedTypes: &*types];
            }
            if done {
                continue;
            }
            let class: *const AnyClass = unsafe { objc2::ffi::object_getClass(&*delegate) };
            for (name, ours, slot) in [
                (c"draggingEntered:", entered as DragMethod, &ENTERED),
                (c"performDragOperation:", perform as DragMethod, &PERFORM),
            ] {
                let selector = Sel::register(name);
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject, Sel};
    use objc2::{class, define_class, msg_send};
    use objc2_foundation::{NSArray, NSString};

    thread_local! {
        static BOARD: std::cell::RefCell<Option<Retained<AnyObject>>> = const { std::cell::RefCell::new(None) };
    }

    define_class!(
        // A stand-in for AppKit's dragging info: only its pasteboard.
        #[unsafe(super(NSObject))]
        #[name = "DmTestDrag"]
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
        let board: Retained<AnyObject> =
            unsafe { msg_send![class!(NSPasteboard), pasteboardWithUniqueName] };
        let _: isize = unsafe { msg_send![&*board, clearContents] };
        write(&board, &NSString::from_str(kind));
        BOARD.with(|b| *b.borrow_mut() = Some(board));
    }

    fn drop_it() -> Bool {
        let drag: Retained<FakeDrag> =
            unsafe { msg_send![<FakeDrag as objc2::ClassType>::class(), new] };
        let sender = Retained::as_ptr(&drag) as *mut AnyObject;
        let sel = Sel::register(c"performDragOperation:");
        unsafe { super::mac::perform_for_test(std::ptr::null_mut(), sel, sender) }
    }

    #[test]
    fn a_link_dragged_from_a_browser_is_taken_and_a_file_is_left_to_winit() {
        let _ = super::take();
        // A browser's link: its URL on the pasteboard.
        board_with("public.url", |board, kind| {
            let url = NSString::from_str("https://example.com/files/big.zip");
            let _: Bool = unsafe { msg_send![board, setString: &*url, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::YES);
        assert_eq!(super::take(), ["https://example.com/files/big.zip"]);

        // Text that is no link is not taken.
        board_with("public.utf8-plain-text", |board, kind| {
            let text = NSString::from_str("just some words");
            let _: Bool = unsafe { msg_send![board, setString: &*text, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::NO);
        assert!(super::take().is_empty());

        // Files are winit's: passed on (to nothing here), nothing queued.
        board_with("NSFilenamesPboardType", |board, kind| {
            let files = NSArray::from_retained_slice(&[NSString::from_str("/tmp/links.url")]);
            let _: Bool = unsafe { msg_send![board, setPropertyList: &*files, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::NO);
        assert!(super::take().is_empty());
    }
}
