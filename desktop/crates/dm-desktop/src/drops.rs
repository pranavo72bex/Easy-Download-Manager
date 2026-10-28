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
use std::sync::{Mutex, OnceLock};

/// What a drop handed over: text (a link, or lines of links), or a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    Text(String),
    File(PathBuf),
}

static PENDING: Mutex<Vec<Dropped>> = Mutex::new(Vec::new());
static WINDOW: OnceLock<egui::Context> = OnceLock::new();

/// A drop for the window to read; the window is woken to do so.
// Called by the macOS and Windows drop targets; Linux takes drops through
// winit.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub(crate) fn arrived(what: Dropped) {
    dm_core::lock(&PENDING).push(what);
    if let Some(ctx) = WINDOW.get() {
        ctx.request_repaint();
    }
}

/// What has been dropped since the last call.
pub fn take() -> Vec<Dropped> {
    std::mem::take(&mut *dm_core::lock(&PENDING))
}

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

    /// `perform`, for the tests to call as AppKit would.
    #[cfg(test)]
    pub(super) unsafe fn perform_for_test(
        this: *mut AnyObject,
        cmd: Sel,
        sender: *mut AnyObject,
    ) -> Bool {
        // SAFETY: the caller's obligations are those of `perform`, which this
        // forwards to unchanged.
        unsafe { perform(this, cmd, sender) }
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
mod win {
    //! An OLE drop target of dm's own. The interfaces are written out as
    //! their vtables, as winit's own drop target is.

    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::core::{GUID, HRESULT};
    use windows_sys::Win32::Foundation::{E_NOINTERFACE, E_POINTER, HWND, POINTL, S_OK};
    use windows_sys::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL};
    use windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows_sys::Win32::System::Ole::{
        RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP, CF_UNICODETEXT,
        DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

    use super::{arrived, Dropped};

    #[repr(C)]
    pub(super) struct DataObjectVtbl {
        query_interface: usize,
        add_ref: usize,
        release: usize,
        pub(super) get_data:
            unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut STGMEDIUM) -> HRESULT,
        get_data_here: usize,
        pub(super) query_get_data:
            unsafe extern "system" fn(*mut c_void, *const FORMATETC) -> HRESULT,
    }

    #[repr(C)]
    struct DropTargetVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        drag_enter:
            unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINTL, *mut u32) -> HRESULT,
        drag_over: unsafe extern "system" fn(*mut c_void, u32, POINTL, *mut u32) -> HRESULT,
        drag_leave: unsafe extern "system" fn(*mut c_void) -> HRESULT,
        drop: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINTL, *mut u32) -> HRESULT,
    }

    #[repr(C)]
    pub(super) struct DropTarget {
        vtbl: &'static DropTargetVtbl,
    }

    // SAFETY: a pointer to a static table of functions; it holds no state.
    unsafe impl Sync for DropTarget {}

    static VTBL: DropTargetVtbl = DropTargetVtbl {
        query_interface,
        add_ref,
        release,
        drag_enter,
        drag_over,
        drag_leave,
        drop,
    };
    pub(super) static TARGET: DropTarget = DropTarget { vtbl: &VTBL };
    /// Whether what is being dragged over the window is something dm takes.
    static TAKES: AtomicBool = AtomicBool::new(false);

    const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
    const IID_IDROPTARGET: GUID = GUID::from_u128(0x00000122_0000_0000_c000_000000000046);

    unsafe extern "system" fn query_interface(
        this: *mut c_void,
        riid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        if out.is_null() || riid.is_null() {
            return E_POINTER;
        }
        let id = unsafe { *riid };
        let same = |a: &GUID, b: &GUID| {
            a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
        };
        if same(&id, &IID_IUNKNOWN) || same(&id, &IID_IDROPTARGET) {
            unsafe { *out = this };
            S_OK
        } else {
            unsafe { *out = std::ptr::null_mut() };
            E_NOINTERFACE
        }
    }

    // A static object: counting references keeps nothing alive.
    unsafe extern "system" fn add_ref(_this: *mut c_void) -> u32 {
        1
    }

    unsafe extern "system" fn release(_this: *mut c_void) -> u32 {
        1
    }

    fn format(format: u16) -> FORMATETC {
        FORMATETC {
            cfFormat: format,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT,
            lindex: -1,
            tymed: TYMED_HGLOBAL as u32,
        }
    }

    /// The registered format a browser puts a link's address in.
    fn url_format() -> u16 {
        let name: Vec<u16> = "UniformResourceLocatorW\0".encode_utf16().collect();
        unsafe { RegisterClipboardFormatW(name.as_ptr()) as u16 }
    }

    unsafe fn vtbl(data: *mut c_void) -> &'static DataObjectVtbl {
        unsafe { &**(data as *mut *const DataObjectVtbl) }
    }

    unsafe fn offers(data: *mut c_void, kind: u16) -> bool {
        if data.is_null() {
            return false;
        }
        let wanted = format(kind);
        unsafe { (vtbl(data).query_get_data)(data, &wanted) == S_OK }
    }

    /// The UTF-16 text the data object holds in `kind`, up to its first NUL.
    unsafe fn text(data: *mut c_void, kind: u16) -> Option<String> {
        let wanted = format(kind);
        let mut medium: STGMEDIUM = unsafe { std::mem::zeroed() };
        if unsafe { (vtbl(data).get_data)(data, &wanted, &mut medium) } != S_OK {
            return None;
        }
        let global = unsafe { medium.u.hGlobal };
        let size = unsafe { GlobalSize(global) } / 2;
        let at = unsafe { GlobalLock(global) } as *const u16;
        let text = (!at.is_null()).then(|| {
            let units = unsafe { std::slice::from_raw_parts(at, size) };
            let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
            String::from_utf16_lossy(&units[..end])
        });
        unsafe {
            GlobalUnlock(global);
            ReleaseStgMedium(&mut medium);
        }
        text
    }

    /// The paths of the files the data object holds.
    unsafe fn files(data: *mut c_void) -> Vec<PathBuf> {
        let wanted = format(CF_HDROP);
        let mut medium: STGMEDIUM = unsafe { std::mem::zeroed() };
        if unsafe { (vtbl(data).get_data)(data, &wanted, &mut medium) } != S_OK {
            return Vec::new();
        }
        let drop = unsafe { medium.u.hGlobal } as HDROP;
        let count = unsafe { DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0) };
        let mut paths = Vec::new();
        for i in 0..count {
            let len = unsafe { DragQueryFileW(drop, i, std::ptr::null_mut(), 0) } as usize;
            let mut name = vec![0u16; len + 1];
            unsafe { DragQueryFileW(drop, i, name.as_mut_ptr(), name.len() as u32) };
            name.truncate(len);
            use std::os::windows::ffi::OsStringExt;
            paths.push(std::ffi::OsString::from_wide(&name).into());
        }
        unsafe { ReleaseStgMedium(&mut medium) };
        paths
    }

    /// What a drop over the window holds that dm takes, read.
    pub(super) unsafe fn read(data: *mut c_void) -> Vec<Dropped> {
        if data.is_null() {
            return Vec::new();
        }
        unsafe {
            if offers(data, CF_HDROP) {
                return files(data).into_iter().map(Dropped::File).collect();
            }
            for kind in [url_format(), CF_UNICODETEXT] {
                if let Some(text) = text(data, kind).filter(|t| !crate::app::links_in(t).is_empty())
                {
                    return vec![Dropped::Text(text)];
                }
            }
        }
        Vec::new()
    }

    unsafe extern "system" fn drag_enter(
        _this: *mut c_void,
        data: *mut c_void,
        _keys: u32,
        _at: POINTL,
        effect: *mut u32,
    ) -> HRESULT {
        let takes = unsafe { offers(data, CF_HDROP) || !read(data).is_empty() };
        TAKES.store(takes, Ordering::Relaxed);
        if !effect.is_null() {
            unsafe {
                *effect = if takes {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                }
            };
        }
        S_OK
    }

    unsafe extern "system" fn drag_over(
        _this: *mut c_void,
        _keys: u32,
        _at: POINTL,
        effect: *mut u32,
    ) -> HRESULT {
        if !effect.is_null() {
            let takes = TAKES.load(Ordering::Relaxed);
            unsafe {
                *effect = if takes {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                }
            };
        }
        S_OK
    }

    unsafe extern "system" fn drag_leave(_this: *mut c_void) -> HRESULT {
        TAKES.store(false, Ordering::Relaxed);
        S_OK
    }

    unsafe extern "system" fn drop(
        _this: *mut c_void,
        data: *mut c_void,
        _keys: u32,
        _at: POINTL,
        effect: *mut u32,
    ) -> HRESULT {
        let taken = unsafe { read(data) };
        if !effect.is_null() {
            unsafe {
                *effect = if taken.is_empty() {
                    DROPEFFECT_NONE
                } else {
                    DROPEFFECT_COPY
                }
            };
        }
        for what in taken {
            arrived(what);
        }
        TAKES.store(false, Ordering::Relaxed);
        S_OK
    }

    /// The windows dm's drop target is on: the main one, and the floating
    /// drop target where it is shown.
    static ON: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());

    pub(super) fn install(hwnd: isize) {
        let mut on = dm_core::lock(&ON);
        if on.contains(&hwnd) {
            return;
        }
        let window = hwnd as HWND;
        // winit's own, which takes files only, out; dm's in. OLE was
        // started on this thread by winit for its own.
        unsafe {
            RevokeDragDrop(window);
            let target = &TARGET as *const DropTarget as *mut c_void;
            if RegisterDragDrop(window, target) == S_OK {
                on.push(hwnd);
            }
        }
    }

    /// dm's drop target on the window with this title, where there is one.
    pub(super) fn install_titled(title: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
        let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated title, alive for the call.
        let found = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
        if !found.is_null() {
            install(found as isize);
        }
    }

    /// The drop target, for the tests to call as OLE would.
    #[cfg(test)]
    pub(super) unsafe fn drop_for_test(data: *mut c_void) -> u32 {
        let mut effect = 0u32;
        let target = &TARGET as *const DropTarget as *mut c_void;
        unsafe { (TARGET.vtbl.drop)(target, data, 0, POINTL { x: 0, y: 0 }, &mut effect) };
        effect
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::System::Memory::{GlobalAlloc, GMEM_MOVEABLE};

        thread_local! {
            /// What the stand-in data object holds: its format and text.
            static HOLDS: std::cell::RefCell<Option<(u16, String)>> = const { std::cell::RefCell::new(None) };
        }

        unsafe extern "system" fn query(_this: *mut c_void, wanted: *const FORMATETC) -> HRESULT {
            let kind = unsafe { (*wanted).cfFormat };
            let holds = HOLDS.with(|h| h.borrow().as_ref().map(|(k, _)| *k));
            if holds == Some(kind) {
                S_OK
            } else {
                -2147221404
            } // DV_E_FORMATETC
        }

        unsafe extern "system" fn get(
            this: *mut c_void,
            wanted: *const FORMATETC,
            medium: *mut STGMEDIUM,
        ) -> HRESULT {
            if unsafe { query(this, wanted) } != S_OK {
                return -2147221404;
            }
            let text = HOLDS
                .with(|h| h.borrow().as_ref().map(|(_, t)| t.clone()))
                .unwrap_or_default();
            let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
            unsafe {
                let global = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
                let at = GlobalLock(global) as *mut u16;
                std::ptr::copy_nonoverlapping(units.as_ptr(), at, units.len());
                GlobalUnlock(global);
                (*medium).tymed = TYMED_HGLOBAL as u32;
                (*medium).u.hGlobal = global;
                (*medium).pUnkForRelease = std::ptr::null_mut();
            }
            S_OK
        }

        static DATA_VTBL: DataObjectVtbl = DataObjectVtbl {
            query_interface: 0,
            add_ref: 0,
            release: 0,
            get_data: get,
            get_data_here: 0,
            query_get_data: query,
        };

        #[repr(C)]
        struct Data {
            vtbl: &'static DataObjectVtbl,
        }

        fn dropped(kind: u16, text: &str) -> (u32, Vec<Dropped>) {
            let _ = super::super::take();
            HOLDS.with(|h| *h.borrow_mut() = Some((kind, text.to_string())));
            let mut data = Data { vtbl: &DATA_VTBL };
            let effect = unsafe { drop_for_test(&mut data as *mut Data as *mut c_void) };
            (effect, super::super::take())
        }

        #[test]
        fn a_link_dragged_from_a_browser_is_taken_and_words_are_not() {
            let (effect, taken) = dropped(url_format(), "https://example.com/files/big.zip");
            assert_eq!(effect, DROPEFFECT_COPY);
            assert_eq!(
                taken,
                [Dropped::Text("https://example.com/files/big.zip".into())]
            );
            // A browser that hands over only text.
            let (_, taken) = dropped(CF_UNICODETEXT, "magnet:?xt=urn:btih:abc");
            assert_eq!(taken, [Dropped::Text("magnet:?xt=urn:btih:abc".into())]);
            let (effect, taken) = dropped(CF_UNICODETEXT, "just some words");
            assert_eq!(effect, DROPEFFECT_NONE);
            assert!(taken.is_empty());
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
        // `draggingPasteboard`; no original method is installed in a test, so the
        // null `this` is never used.
        unsafe { super::mac::perform_for_test(std::ptr::null_mut(), sel, sender) }
    }

    #[test]
    fn a_link_dragged_from_a_browser_is_taken_and_a_file_is_left_to_winit() {
        let _ = super::take();
        // A browser's link: its URL on the pasteboard.
        board_with("public.url", |board, kind| {
            let url = NSString::from_str("https://example.com/files/big.zip");
            // SAFETY: `board` is the live pasteboard; `setString:forType:` takes two
            // NSStrings and returns a BOOL.
            let _: Bool = unsafe { msg_send![board, setString: &*url, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::YES);
        assert_eq!(
            super::take(),
            [super::Dropped::Text(
                "https://example.com/files/big.zip".into()
            )]
        );

        // Text that is no link is not taken.
        board_with("public.utf8-plain-text", |board, kind| {
            let text = NSString::from_str("just some words");
            // SAFETY: as above.
            let _: Bool = unsafe { msg_send![board, setString: &*text, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::NO);
        assert!(super::take().is_empty());

        // Files are winit's: passed on (to nothing here), nothing queued.
        board_with("NSFilenamesPboardType", |board, kind| {
            let files = NSArray::from_retained_slice(&[NSString::from_str("/tmp/links.url")]);
            // SAFETY: `setPropertyList:forType:` takes a property list (an NSArray of
            // NSStrings here) and a type name, and returns a BOOL.
            let _: Bool = unsafe { msg_send![board, setPropertyList: &*files, forType: kind] };
        });
        assert_eq!(drop_it(), Bool::NO);
        assert!(super::take().is_empty());
    }
}
