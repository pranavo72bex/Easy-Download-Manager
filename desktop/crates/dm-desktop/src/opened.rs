// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Links and files the system opens with dm, on macOS: a `magnet:` link
//! clicked in a browser or another program, a `.torrent` file opened from
//! Finder. The bundle's Info.plist (`macos/Info.plist`) names both; the
//! system then sends the running app an Apple event, never a command line.
//!
//! AppKit hands those events to the application delegate's
//! `application:openURLs:`, which winit's delegate does not implement. As
//! in `dock.rs`, the method is added to winit's delegate class at run time.
//! It must be there before the app finishes launching, or the link that
//! started dm is lost, and winit's delegate exists only once eframe has
//! made its event loop: so the method is added when AppKit says the app
//! will finish launching, before it reads the launch's own events.

use std::ffi::{c_char, CStr};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, Sel};
use objc2::{class, define_class, msg_send, sel, ClassType, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;

static OPENED: OnceLock<Box<dyn Fn(Vec<String>) + Send + Sync>> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class has
    // no `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "DmOpenedObserver"]
    struct Observer;

    impl Observer {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: *mut AnyObject) {
            add_open_urls();
        }
    }
);

/// `- (void)application:(NSApplication *)app openURLs:(NSArray<NSURL *> *)urls`
unsafe extern "C-unwind" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    urls: *mut AnyObject,
) {
    let Some(urls) = (unsafe { urls.as_ref() }) else {
        return;
    };
    let mut words = Vec::new();
    // SAFETY: `urls` is the NSArray of NSURL AppKit passes this method;
    // each message is one NSArray and NSURL answer, and the C string an
    // NSString gives is read before the string is let go.
    unsafe {
        let count: usize = msg_send![urls, count];
        for i in 0..count {
            let url: *mut AnyObject = msg_send![urls, objectAtIndex: i];
            let Some(url) = url.as_ref() else { continue };
            let is_file: Bool = msg_send![url, isFileURL];
            // A file as its path, which the command line's reading takes;
            // a link as written.
            let text: *mut NSString = if is_file.as_bool() {
                msg_send![url, path]
            } else {
                msg_send![url, absoluteString]
            };
            let Some(text) = text.as_ref() else { continue };
            let utf8: *const c_char = msg_send![text, UTF8String];
            if !utf8.is_null() {
                words.push(CStr::from_ptr(utf8).to_string_lossy().into_owned());
            }
        }
    }
    if let Some(opened) = OPENED.get() {
        opened(words);
    }
}

fn add_open_urls() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        return;
    };
    let object: &AnyObject = delegate.as_ref();
    // SAFETY: `object` is the application's live delegate, borrowed from `app`
    // for this call; `object_getClass` only reads its class pointer.
    let class: *const AnyClass = unsafe { objc2::ffi::object_getClass(object) };
    let selector = Sel::register(c"application:openURLs:");
    // SAFETY: `open_urls` has exactly the signature of the Objective-C method
    // (self, _cmd, application, urls, returning void), "v@:@@" is its type
    // encoding, and `class` is a class, not an instance.
    unsafe {
        let imp: objc2::runtime::Imp = std::mem::transmute(
            open_urls
                as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
        );
        // Fails, harmlessly, only if the class already answers it.
        objc2::ffi::class_addMethod(class as *mut AnyClass, selector, imp, c"v@:@@".as_ptr());
    }
}

/// Calls `opened` with each link or file the system opens with dm. Must run
/// on the main thread before the window's event loop starts.
pub fn listen(opened: impl Fn(Vec<String>) + Send + Sync + 'static) {
    if MainThreadMarker::new().is_none() || OPENED.set(Box::new(opened)).is_err() {
        return;
    }
    // SAFETY: plain Foundation messages. The observer is kept for the life
    // of the process (the notification centre does not keep it), and its
    // selector takes the one NSNotification argument the centre sends.
    unsafe {
        let observer: Retained<Observer> = msg_send![Observer::class(), new];
        let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
        let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
        let nothing: *const AnyObject = std::ptr::null();
        let _: () = msg_send![
            center,
            addObserver: &*observer,
            selector: sel!(willFinishLaunching:),
            name: &*name,
            object: nothing
        ];
        std::mem::forget(observer);
    }
}
