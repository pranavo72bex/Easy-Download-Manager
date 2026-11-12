// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What is dropped on dm's windows, for every window alike: a link dragged
//! from a browser (an address, not a file) or a file, queued here by the
//! system's drop target and read by the window as it reads its own drops
//! (`take`), the window woken to do so (`on_arrival`). A toolkit hands a
//! program only dropped files; where the system offers the link, a target
//! of dm's own takes it: on Windows the OLE `IDropTarget` here (`win`), put
//! in the toolkit's place on the window, taking files as it did and a
//! browser's link (`UniformResourceLocatorW`, else plain text); on macOS
//! each window's own drag methods (the windows' crates).

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// What a drop handed over: text (a link, or lines of links), or a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    /// Text, holding a link or lines of links.
    Text(String),
    /// A file's path.
    File(PathBuf),
}

static PENDING: Mutex<Vec<Dropped>> = Mutex::new(Vec::new());
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// How the window is woken when something arrives; the first one given
/// stands.
pub fn on_arrival(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

/// A drop for the window to read; the window is woken to do so.
pub fn arrived(what: Dropped) {
    dm_core::lock(&PENDING).push(what);
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

/// What has been dropped since the last call.
pub fn take() -> Vec<Dropped> {
    std::mem::take(&mut *dm_core::lock(&PENDING))
}

#[cfg(windows)]
pub mod win {
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

    // No `unsafe impl Sync` is needed, and none is wanted: `DropTarget` holds one shared
    // reference to `VTBL`, a static table of function pointers that is never written, and
    // the compiler makes a shared reference to such a table `Sync` on its own. OLE may
    // call the table's functions from any thread it registered the target on; they
    // touch only `TAKES`, an atomic, and the data object they are handed.

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
        // SAFETY: `riid` is non-null (checked above); OLE passes a pointer to a GUID that
        // is valid for the whole call.
        let id = unsafe { *riid };
        let same = |a: &GUID, b: &GUID| {
            a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
        };
        if same(&id, &IID_IUNKNOWN) || same(&id, &IID_IDROPTARGET) {
            // SAFETY: `out` is non-null (checked above) and is OLE's slot for the interface
            // pointer; `this` is the static drop target, valid for ever.
            unsafe { *out = this };
            S_OK
        } else {
            // SAFETY: `out` is non-null and writable, as above.
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
        // SAFETY: `name` is NUL-terminated (the `\0` is in the string) and alive for the call.
        unsafe { RegisterClipboardFormatW(name.as_ptr()) as u16 }
    }

    unsafe fn vtbl(data: *mut c_void) -> &'static DataObjectVtbl {
        // SAFETY: a COM interface pointer points at an object whose first field is a
        // pointer to its table of functions, the layout `DataObjectVtbl` mirrors; the
        // caller guarantees `data` is a live data object, so the table lives as long as
        // the reference is used (the `'static` is a simplification: it never outlives the
        // call that was handed `data`).
        unsafe { &**(data as *mut *const DataObjectVtbl) }
    }

    unsafe fn offers(data: *mut c_void, kind: u16) -> bool {
        if data.is_null() {
            return false;
        }
        let wanted = format(kind);
        // SAFETY: `data` is non-null (checked above) and, by this function's contract, a
        // live data object; `wanted` is valid for the call.
        unsafe { (vtbl(data).query_get_data)(data, &wanted) == S_OK }
    }

    /// The UTF-16 text the data object holds in `kind`, up to its first NUL.
    unsafe fn text(data: *mut c_void, kind: u16) -> Option<String> {
        let wanted = format(kind);
        // SAFETY: STGMEDIUM is plain C data (integers, pointers, a union of handles);
        // all zero is the empty medium OLE expects for an out parameter.
        let mut medium: STGMEDIUM = unsafe { std::mem::zeroed() };
        // SAFETY: `data` is a live data object (this function's contract); `wanted` and
        // `medium` are valid for the call, which fills `medium`.
        if unsafe { (vtbl(data).get_data)(data, &wanted, &mut medium) } != S_OK {
            return None;
        }
        // SAFETY: `TYMED_HGLOBAL` was asked for and `get_data` answered `S_OK`, so the
        // union holds a global memory handle.
        let global = unsafe { medium.u.hGlobal };
        // SAFETY: `global` is that handle, owned by `medium` until it is released below.
        let size = unsafe { GlobalSize(global) } / 2;
        // SAFETY: as above; a failed lock gives null, which is checked next.
        let at = unsafe { GlobalLock(global) } as *const u16;
        let text = (!at.is_null()).then(|| {
            // SAFETY: `at` is non-null and points at the locked block of `GlobalSize`
            // bytes, `size` being that in 16-bit units; global memory is at least 8-aligned
            // and the block stays locked, and unchanged, until `GlobalUnlock` below.
            let units = unsafe { std::slice::from_raw_parts(at, size) };
            let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
            String::from_utf16_lossy(&units[..end])
        });
        // SAFETY: `global` was locked above (unlocking a block that is not locked only
        // fails); `medium` came from `get_data` and is released exactly once, here, after
        // the last use of `at`.
        unsafe {
            GlobalUnlock(global);
            ReleaseStgMedium(&mut medium);
        }
        text
    }

    /// The paths of the files the data object holds.
    unsafe fn files(data: *mut c_void) -> Vec<PathBuf> {
        let wanted = format(CF_HDROP);
        // SAFETY: STGMEDIUM is plain C data; all zero is the empty medium OLE expects.
        let mut medium: STGMEDIUM = unsafe { std::mem::zeroed() };
        // SAFETY: `data` is a live data object (this function's contract); `wanted` and
        // `medium` are valid for the call, which fills `medium`.
        if unsafe { (vtbl(data).get_data)(data, &wanted, &mut medium) } != S_OK {
            return Vec::new();
        }
        // SAFETY: `CF_HDROP` over `TYMED_HGLOBAL` was asked for and answered `S_OK`: the
        // union holds the handle of a file list, which is what an `HDROP` is.
        let drop = unsafe { medium.u.hGlobal } as HDROP;
        // SAFETY: `drop` is valid until `ReleaseStgMedium` below; an index of `u32::MAX`
        // with a null buffer asks for the number of files.
        let count = unsafe { DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0) };
        let mut paths = Vec::new();
        for i in 0..count {
            // SAFETY: `drop` as above, `i < count`; a null buffer asks for the length.
            let len = unsafe { DragQueryFileW(drop, i, std::ptr::null_mut(), 0) } as usize;
            let mut name = vec![0u16; len + 1];
            // SAFETY: `name` holds `len + 1` units and that size is passed, so at most
            // that many are written.
            unsafe { DragQueryFileW(drop, i, name.as_mut_ptr(), name.len() as u32) };
            name.truncate(len);
            use std::os::windows::ffi::OsStringExt;
            paths.push(std::ffi::OsString::from_wide(&name).into());
        }
        // SAFETY: `medium` came from `get_data` above and is released once, after the
        // last use of `drop`.
        unsafe { ReleaseStgMedium(&mut medium) };
        paths
    }

    /// What a drop over the window holds that dm takes, read.
    pub(super) unsafe fn read(data: *mut c_void) -> Vec<Dropped> {
        if data.is_null() {
            return Vec::new();
        }
        // SAFETY: `data` is non-null (checked above) and, by this function's contract, a
        // live data object for the whole call; the helpers need nothing more.
        unsafe {
            if offers(data, CF_HDROP) {
                return files(data).into_iter().map(Dropped::File).collect();
            }
            for kind in [url_format(), CF_UNICODETEXT] {
                if let Some(text) =
                    text(data, kind).filter(|t| !dm_shell::app::links::links_in(t).is_empty())
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
        // SAFETY: `data` is the data object OLE holds for this drag, live for the call;
        // `offers` and `read` return at once for null.
        let takes = unsafe { offers(data, CF_HDROP) || !read(data).is_empty() };
        TAKES.store(takes, Ordering::Relaxed);
        if !effect.is_null() {
            // SAFETY: `effect` is non-null (checked) and is the DWORD OLE reads back.
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
            // SAFETY: `effect` is non-null (checked) and is the DWORD OLE reads back.
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
        // SAFETY: `data` is the data object OLE hands this drop, live for the call; `read`
        // returns at once for null.
        let taken = unsafe { read(data) };
        if !effect.is_null() {
            // SAFETY: `effect` is non-null (checked) and is the DWORD OLE reads back.
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

    /// dm's drop target on the window `hwnd`, in place of the toolkit's;
    /// once a window.
    pub fn install(hwnd: isize) {
        let mut on = dm_core::lock(&ON);
        if on.contains(&hwnd) {
            return;
        }
        let window = hwnd as HWND;
        // winit's own, which takes files only, out; dm's in. OLE was
        // started on this thread by winit for its own.
        // SAFETY: `window` is a window handle of this process (from the window system or
        // `FindWindowW`); both calls only fail for one that is not valid. OLE was started
        // on this thread (see above), and `target` is a static that outlives the
        // registration, whose reference counting is a no-op (`add_ref`, `release`).
        unsafe {
            RevokeDragDrop(window);
            let target = &TARGET as *const DropTarget as *mut c_void;
            if RegisterDragDrop(window, target) == S_OK {
                on.push(hwnd);
            }
        }
    }

    /// dm's drop target on the window with this title, where there is one.
    pub fn install_titled(title: &str) {
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
        // SAFETY: `target` is the static drop target; `data` is the caller's live data
        // object (the tests' own); `effect` is a local that outlives the call.
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
            // SAFETY: the drop target under test passes a valid FORMATETC.
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
            // SAFETY: the same pointers the caller gave `get`, valid for the call.
            if unsafe { query(this, wanted) } != S_OK {
                return -2147221404;
            }
            let text = HOLDS
                .with(|h| h.borrow().as_ref().map(|(_, t)| t.clone()))
                .unwrap_or_default();
            let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
            // SAFETY: `global` is a fresh block of `units.len() * 2` bytes, locked for the
            // copy and unlocked after (this stand-in for a test does not handle a failed
            // allocation); `medium` is the caller's valid STGMEDIUM, which takes the block
            // and releases it with `ReleaseStgMedium` in the code under test.
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
            // SAFETY: `data` is a `#[repr(C)]` object whose first field points at
            // `DATA_VTBL`, the layout the drop target reads; it outlives the call.
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
