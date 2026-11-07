// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Links and files the system opens with dm, on macOS: a `magnet:` link
//! clicked in a browser or another program, a `.torrent` file opened from
//! Finder. The bundle's Info.plist (`macos/Info.plist`) names both; the
//! system then sends the running app an Apple event, never a command line:
//! `GURL` for a link, `odoc` for files.
//!
//! dm takes both events itself, from the Apple event manager. Adding
//! `application:openURLs:` to winit's delegate (the way `dock.rs` adds the
//! Dock's method) was tried first and never heard a link (gaps row 105,
//! 2026-11-07), most likely because AppKit reads which methods its delegate
//! answers when the delegate is set, and winit sets it before dm adds one. The handlers
//! are put in when AppKit says the app will finish launching, as Apple
//! documents: after AppKit's own defaults, which would otherwise replace
//! them, and before the launch's own events are read, so the link or file
//! that starts dm is not lost.

use std::ffi::{c_char, CStr};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, define_class, msg_send, sel, ClassType, MainThreadMarker};
use objc2_foundation::NSString;

static OPENED: OnceLock<Box<dyn Fn(Vec<String>) + Send + Sync>> = OnceLock::new();

/// A four-character Apple event code as the number the event manager takes.
const fn code(text: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*text)
}

/// `kInternetEventClass` and `kAEGetURL`: a link opened with dm.
const GET_URL: u32 = code(b"GURL");
/// `kCoreEventClass`: the class of `kAEOpenDocuments`.
const CORE: u32 = code(b"aevt");
/// `kAEOpenDocuments`: files opened with dm.
const OPEN_DOCUMENTS: u32 = code(b"odoc");
/// `keyDirectObject`: the event's link, or its list of files.
const DIRECT_OBJECT: u32 = code(b"----");

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class has
    // no `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "DmOpenedObserver"]
    struct Observer;

    impl Observer {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: *mut AnyObject) {
            handle_events(self);
        }

        /// `GURL`: the event's direct object is the link, as text.
        #[unsafe(method(getUrl:withReplyEvent:))]
        fn get_url(&self, event: *mut AnyObject, _reply: *mut AnyObject) {
            // SAFETY: `event` is the NSAppleEventDescriptor the event manager
            // passes its handler; each message is one that class answers.
            let words = unsafe { event.as_ref().map(|e| link_of(e)).unwrap_or_default() };
            hand(words);
        }

        /// `odoc`: the event's direct object is a list of file addresses.
        #[unsafe(method(openDocuments:withReplyEvent:))]
        fn open_documents(&self, event: *mut AnyObject, _reply: *mut AnyObject) {
            // SAFETY: as in `get_url`.
            let words = unsafe { event.as_ref().map(|e| files_of(e)).unwrap_or_default() };
            hand(words);
        }
    }
);

/// The text of an NSString, copied out before the string is let go.
///
/// # Safety
/// `text` is null or an NSString.
unsafe fn text_of(text: *mut NSString) -> Option<String> {
    let text = unsafe { text.as_ref() }?;
    let utf8: *const c_char = unsafe { msg_send![text, UTF8String] };
    (!utf8.is_null()).then(|| {
        unsafe { CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned()
    })
}

/// The link a `GURL` event carries.
///
/// # Safety
/// `event` is an NSAppleEventDescriptor.
unsafe fn link_of(event: &AnyObject) -> Vec<String> {
    unsafe {
        let direct: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT];
        let Some(direct) = direct.as_ref() else {
            return Vec::new();
        };
        let text: *mut NSString = msg_send![direct, stringValue];
        text_of(text).into_iter().collect()
    }
}

/// The paths of the files an `odoc` event carries: one descriptor, or a
/// list of them (counted from 1), each read as a file's address.
///
/// # Safety
/// `event` is an NSAppleEventDescriptor.
unsafe fn files_of(event: &AnyObject) -> Vec<String> {
    unsafe {
        let direct: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT];
        let Some(direct) = direct.as_ref() else {
            return Vec::new();
        };
        let path_of = |item: &AnyObject| -> Option<String> {
            let url: *mut AnyObject = msg_send![item, fileURLValue];
            let url = url.as_ref()?;
            let path: *mut NSString = msg_send![url, path];
            text_of(path)
        };
        let count: isize = msg_send![direct, numberOfItems];
        if count <= 0 {
            return path_of(direct).into_iter().collect();
        }
        (1..=count)
            .filter_map(|i| {
                let item: *mut AnyObject = msg_send![direct, descriptorAtIndex: i];
                item.as_ref().and_then(path_of)
            })
            .collect()
    }
}

fn hand(words: Vec<String>) {
    if words.is_empty() {
        return;
    }
    if let Some(opened) = OPENED.get() {
        opened(words);
    }
}

/// Makes `observer` the event manager's handler for links and files.
fn handle_events(observer: &Observer) {
    // SAFETY: plain Foundation messages to the shared event manager; the
    // observer lives for the life of the process (`listen` keeps it), and
    // each selector takes the event and its reply, as the manager sends.
    unsafe {
        let manager: *mut AnyObject =
            msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        let Some(manager) = manager.as_ref() else {
            return;
        };
        let _: () = msg_send![
            manager,
            setEventHandler: observer,
            andSelector: sel!(getUrl:withReplyEvent:),
            forEventClass: GET_URL,
            andEventID: GET_URL
        ];
        let _: () = msg_send![
            manager,
            setEventHandler: observer,
            andSelector: sel!(openDocuments:withReplyEvent:),
            forEventClass: CORE,
            andEventID: OPEN_DOCUMENTS
        ];
    }
}

/// Calls `opened` with each link or file the system opens with dm. Must run
/// on the main thread before the window's event loop starts.
pub fn listen(opened: impl Fn(Vec<String>) + Send + Sync + 'static) {
    if MainThreadMarker::new().is_none() || OPENED.set(Box::new(opened)).is_err() {
        return;
    }
    // SAFETY: plain Foundation messages. The observer is kept for the life
    // of the process (neither the notification centre nor the event manager
    // keeps it), and its selector takes the one NSNotification argument the
    // centre sends.
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
