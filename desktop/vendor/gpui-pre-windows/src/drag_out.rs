// dm: files dragged out of the window, into an Explorer folder, a mail or a
// chat (desktop/vendor/README.md). Upstream offers a drag out on macOS and
// Wayland only.
//
// The shell makes the drag: the files become shell items, the items a data
// object (`BHID_DataObject`, what Explorer itself hands over: the paths as
// `CF_HDROP`, the items' own formats, the drag picture), and `SHDoDragDrop`
// runs it with the shell's own drop source. That call holds a modal loop of
// its own until the drop lands, so it is never made inside an update of
// GPUI's: `start` keeps the files and posts the window a message, and the
// window's procedure makes the drag when the message comes, with nothing of
// GPUI's borrowed, then tells GPUI the drag is over.

use std::cell::RefCell;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::{Com::IDataObject, Ole::DROPEFFECT_COPY, Ole::DROPEFFECT_LINK},
    UI::{
        Shell::{
            BHID_DataObject, Common::ITEMIDLIST, ILCreateFromPathW, ILFree, IShellItemArray,
            SHCreateShellItemArrayFromIDLists, SHDoDragDrop,
        },
        WindowsAndMessaging::{PostMessageW, WM_USER},
    },
};
use windows::core::PCWSTR;

/// The message a window is posted to make its drag out.
pub(crate) const WM_DM_DRAG_OUT: u32 = WM_USER + 20;

thread_local! {
    /// The files of the drag about to be made, and the window making it.
    /// One at a time: a person drags one thing.
    static WAITING: RefCell<Option<(isize, Vec<PathBuf>)>> = const { RefCell::new(None) };
}

/// Keeps `files` for `hwnd`'s drag and asks the window to make it; false
/// when there is nothing to drag or the message could not be posted.
pub(crate) fn start(hwnd: HWND, validation_number: usize, files: &gpui::FileDragPaths) -> bool {
    let files: Vec<PathBuf> = files.entries().iter().map(|(path, _)| path.clone()).collect();
    if files.is_empty() {
        return false;
    }
    WAITING.with(|waiting| *waiting.borrow_mut() = Some((hwnd.0 as isize, files)));
    let posted = unsafe {
        PostMessageW(
            Some(hwnd),
            WM_DM_DRAG_OUT,
            WPARAM(validation_number),
            LPARAM(0),
        )
    };
    if posted.is_err() {
        WAITING.with(|waiting| waiting.borrow_mut().take());
        return false;
    }
    true
}

/// Makes the drag `start` asked of `hwnd`, until it lands or is given up.
pub(crate) fn run(hwnd: HWND) {
    let Some(files) = WAITING.with(|waiting| {
        let mut waiting = waiting.borrow_mut();
        match waiting.as_ref() {
            Some((of, _)) if *of == hwnd.0 as isize => waiting.take().map(|(_, files)| files),
            _ => None,
        }
    }) else {
        return;
    };
    // Each file as the shell names it; a file gone meanwhile is left out.
    let items: Vec<*mut ITEMIDLIST> = files
        .iter()
        .filter_map(|path| {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let item = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
            (!item.is_null()).then_some(item)
        })
        .collect();
    if !items.is_empty() {
        let list: Vec<*const ITEMIDLIST> = items.iter().map(|item| item.cast_const()).collect();
        let data = unsafe { SHCreateShellItemArrayFromIDLists(&list) }.and_then(
            |array: IShellItemArray| unsafe {
                array.BindToHandler::<_, IDataObject>(None, &BHID_DataObject)
            },
        );
        if let Ok(data) = data {
            // The shell's own drop source; copied, or a shortcut made where
            // the target asks for one. Never moved: the file stays dm's.
            let _ = unsafe { SHDoDragDrop(Some(hwnd), &data, None, DROPEFFECT_COPY | DROPEFFECT_LINK) };
        }
    }
    for item in items {
        unsafe { ILFree(Some(item.cast_const())) };
    }
}
