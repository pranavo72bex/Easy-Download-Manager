//! The Dock icon, on macOS: a click on it brings back a window hidden in the
//! tray, the way every Mac app's does.
//!
//! winit does not pass that click on. Its documentation says a program may
//! register an application delegate of its own, but 0.30 installs one and
//! panics on any other ("tried to get a delegate that was not the one Winit
//! has registered") — so rather than replacing winit's delegate, the one
//! method this needs is added to its class at run time. winit's delegate
//! does not implement it, so nothing of winit's is overridden.

use std::ffi::CStr;
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;

static REOPEN: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// `- (BOOL)applicationShouldHandleReopen:(NSApplication *)app
///                      hasVisibleWindows:(BOOL)visible`
unsafe extern "C-unwind" fn should_handle_reopen(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    _visible: Bool,
) -> Bool {
    if let Some(reopen) = REOPEN.get() {
        reopen();
    }
    Bool::YES
}

/// Calls `reopen` whenever the Dock icon is clicked. Must run on the main
/// thread once winit's delegate is in place — inside eframe's app creator.
pub fn on_reopen(reopen: impl Fn() + Send + Sync + 'static) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if REOPEN.set(Box::new(reopen)).is_err() {
        return;
    }
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        return;
    };
    let object: &AnyObject = delegate.as_ref();
    let class: *const AnyClass = unsafe { objc2::ffi::object_getClass(object) };
    let selector = Sel::register(c"applicationShouldHandleReopen:hasVisibleWindows:");
    // BOOL is `bool` on Apple silicon and `signed char` on Intel.
    let types: &CStr = if cfg!(target_arch = "aarch64") {
        c"B@:@B"
    } else {
        c"c@:@c"
    };
    unsafe {
        let imp: objc2::runtime::Imp = std::mem::transmute(
            should_handle_reopen
                as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool,
        );
        // Fails, harmlessly, only if the class already answers it.
        objc2::ffi::class_addMethod(class as *mut AnyClass, selector, imp, types.as_ptr());
    }
}
