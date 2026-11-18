// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The extension program as Safari runs it: Foundation's `NSExtensionMain`, with the handler
//! class named in the extension's Info.plist (`DMSafariHandler`) made at run time through the
//! Objective-C runtime, in Rust. Safari gives the handler each message the extension sends
//! (`runtime.sendNativeMessage`) as a dictionary under `message`; the handler turns it into
//! JSON, hands it to dm's host (`relay::ask`) and gives Safari the host's answer the same way.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::PathBuf;

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, NSObject, Sel};
use objc2::{msg_send, sel, ClassType};

use crate::relay;

#[link(name = "Foundation", kind = "framework")]
extern "C" {
    /// The extension point's own main: it runs until the system stops the extension, and asks
    /// the principal class for each request.
    fn NSExtensionMain(argc: c_int, argv: *mut *mut c_char) -> c_int;
    /// The system log, which `log show --predicate 'process == "dm-safari"'` reads.
    fn NSLog(format: *mut AnyObject, ...);
}

/// The class the extension's Info.plist names as `NSExtensionPrincipalClass`.
const HANDLER: &CStr = c"DMSafariHandler";

/// The key Safari keeps an extension's message under (`SFExtensionMessageKey`).
const MESSAGE_KEY: &CStr = c"message";

/// Makes the handler class and hands the program to the extension point; returns its code.
pub fn run() -> i32 {
    register();
    let args: Vec<CString> = std::env::args_os()
        .filter_map(|a| CString::new(a.into_encoded_bytes()).ok())
        .collect();
    let mut argv: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let argc = c_int::try_from(args.len()).unwrap_or(0);
    // SAFETY: argv is `argc` strings that live until the program ends (NSExtensionMain does
    // not return while the extension runs), then a null, as a C main's are.
    unsafe { NSExtensionMain(argc, argv.as_mut_ptr()) }
}

/// The handler class: an NSObject that answers `beginRequestWithExtensionContext:`.
fn register() {
    let Some(mut builder) = ClassBuilder::new(HANDLER, NSObject::class()) else {
        return;
    };
    if let Some(protocol) = AnyProtocol::get(c"NSExtensionRequestHandling") {
        builder.add_protocol(protocol);
    }
    // SAFETY: the method's types are the selector's: an object (the context), returning nothing.
    unsafe {
        builder.add_method(
            sel!(beginRequestWithExtensionContext:),
            begin_request as extern "C-unwind" fn(&'static NSObject, Sel, *mut AnyObject),
        );
    }
    builder.register();
}

/// An object handed to another thread: NSExtensionContext may be completed from any thread.
struct Sendable(Retained<AnyObject>);

// SAFETY: only an NSExtensionContext is wrapped, whose completion Foundation allows from any
// thread, and nothing else is done with it.
unsafe impl Send for Sendable {}

extern "C-unwind" fn begin_request(_this: &'static NSObject, _cmd: Sel, context: *mut AnyObject) {
    // SAFETY: the extension point passes a live NSExtensionContext (or nil, which gives None).
    let Some(context) = (unsafe { Retained::retain(context) }) else {
        return;
    };
    let context = Sendable(context);
    // Off Safari's thread: the host may ask a server and start dm before it answers.
    std::thread::spawn(move || {
        let context = context;
        autoreleasepool(|_| answer(&context.0));
    });
}

/// Answers one request: the message to dm's host, and its answer to Safari.
fn answer(context: &AnyObject) {
    let reply = message_of(context)
        .ok_or_else(|| "Safari sent no message".to_string())
        .and_then(|request| {
            let program = dm_program().ok_or("dm.app was not found around the extension")?;
            relay::ask(&program, real_home().as_deref(), &request, relay::WITHIN)
        });
    match reply {
        Ok(reply) => complete(context, reply.as_deref()),
        Err(why) => {
            say(&format!("dm-safari: {why}"));
            fail(context, &why);
        }
    }
}

/// The JSON of the request's `message`, or `None` where Safari sent none that is JSON.
fn message_of(context: &AnyObject) -> Option<Vec<u8>> {
    // SAFETY: each call is one NSExtensionContext, NSArray, NSExtensionItem or NSDictionary
    // method with these types; a nil anywhere answers nil (or 0) and ends it below.
    unsafe {
        let items: *mut AnyObject = msg_send![context, inputItems];
        if items.is_null() {
            return None;
        }
        let item: *mut AnyObject = msg_send![items, firstObject];
        if item.is_null() {
            return None;
        }
        let info: *mut AnyObject = msg_send![item, userInfo];
        if info.is_null() {
            return None;
        }
        let message: *mut AnyObject = msg_send![info, objectForKey: string(MESSAGE_KEY)];
        json_of(message)
    }
}

/// Safari told the host's answer: an item whose `message` is the answer's object, or no item
/// where the host answered nothing.
fn complete(context: &AnyObject, reply: Option<&[u8]>) {
    // SAFETY: Foundation's classes and methods with their own types; a reply that is not JSON
    // is no object, and the item then carries none.
    unsafe {
        let Some(items_class) = AnyClass::get(c"NSMutableArray") else {
            return;
        };
        let items: *mut AnyObject = msg_send![items_class, array];
        if let Some(object) = reply.and_then(object_of) {
            if let (Some(item_class), Some(dict_class)) = (
                AnyClass::get(c"NSExtensionItem"),
                AnyClass::get(c"NSDictionary"),
            ) {
                let item: Option<Retained<AnyObject>> = msg_send![item_class, new];
                if let Some(item) = item {
                    let info: *mut AnyObject = msg_send![
                        dict_class,
                        dictionaryWithObject: object,
                        forKey: string(MESSAGE_KEY)
                    ];
                    let _: () = msg_send![&*item, setUserInfo: info];
                    let _: () = msg_send![items, addObject: &*item];
                }
            }
        }
        let none: *mut c_void = std::ptr::null_mut();
        let _: () =
            msg_send![context, completeRequestReturningItems: items, completionHandler: none];
    }
}

/// Safari told why there is no answer: the extension's promise is refused with it.
fn fail(context: &AnyObject, why: &str) {
    // SAFETY: Foundation's classes and methods with their own types.
    unsafe {
        let (Some(error_class), Some(dict_class)) =
            (AnyClass::get(c"NSError"), AnyClass::get(c"NSDictionary"))
        else {
            return;
        };
        let why = CString::new(why.replace('\0', " ")).unwrap_or_default();
        let info: *mut AnyObject = msg_send![
            dict_class,
            dictionaryWithObject: string(&why),
            forKey: string(c"NSLocalizedDescription")
        ];
        let error: *mut AnyObject = msg_send![
            error_class,
            errorWithDomain: string(c"dm"),
            code: 1isize,
            userInfo: info
        ];
        let _: () = msg_send![context, cancelRequestWithError: error];
    }
}

/// An NSString of `text`, autoreleased.
fn string(text: &CStr) -> *mut AnyObject {
    let Some(class) = AnyClass::get(c"NSString") else {
        return std::ptr::null_mut();
    };
    // SAFETY: `text` is a NUL-ended UTF-8 string that outlives the call, which copies it.
    unsafe { msg_send![class, stringWithUTF8String: text.as_ptr()] }
}

/// The JSON Foundation writes of `object`, or `None` where it is no JSON object (Foundation
/// would throw, which cannot be caught here, so it is asked first).
///
/// # Safety
///
/// `object` is nil or a live object.
unsafe fn json_of(object: *mut AnyObject) -> Option<Vec<u8>> {
    if object.is_null() {
        return None;
    }
    let class = AnyClass::get(c"NSJSONSerialization")?;
    // SAFETY: NSJSONSerialization's own methods with their own types, `object` live.
    unsafe {
        let valid: Bool = msg_send![class, isValidJSONObject: object];
        if !valid.as_bool() {
            return None;
        }
        let none: *mut c_void = std::ptr::null_mut();
        let data: *mut AnyObject =
            msg_send![class, dataWithJSONObject: object, options: 0usize, error: none];
        bytes_of(data)
    }
}

/// The object Foundation reads from `json`, or `None` where it is not JSON.
fn object_of(json: &[u8]) -> Option<*mut AnyObject> {
    let class = AnyClass::get(c"NSJSONSerialization")?;
    let data_class = AnyClass::get(c"NSData")?;
    // SAFETY: NSData copies `json`, which lives for the call; NSJSONSerialization answers nil
    // (not a throw) for data that is not JSON. Fragments (a bare string or number) are read too.
    unsafe {
        let data: *mut AnyObject = msg_send![data_class, dataWithBytes: json.as_ptr().cast::<c_void>(), length: json.len()];
        let none: *mut c_void = std::ptr::null_mut();
        let object: *mut AnyObject =
            msg_send![class, JSONObjectWithData: data, options: 4usize, error: none];
        (!object.is_null()).then_some(object)
    }
}

/// The bytes of an NSData.
///
/// # Safety
///
/// `data` is nil or a live NSData.
unsafe fn bytes_of(data: *mut AnyObject) -> Option<Vec<u8>> {
    if data.is_null() {
        return None;
    }
    // SAFETY: NSData's own methods; `bytes` is `length` bytes long while `data` lives.
    unsafe {
        let len: usize = msg_send![data, length];
        let at: *const c_void = msg_send![data, bytes];
        if at.is_null() {
            return Some(Vec::new());
        }
        Some(std::slice::from_raw_parts(at.cast::<u8>(), len).to_vec())
    }
}

/// dm's program in the app around this extension, as that app's Info.plist names it.
fn dm_program() -> Option<PathBuf> {
    let app = relay::container_app(&std::env::current_exe().ok()?)?;
    let class = AnyClass::get(c"NSBundle")?;
    let path = CString::new(app.into_os_string().into_encoded_bytes()).ok()?;
    // SAFETY: NSBundle's own methods with their own types; nil where the folder is no bundle.
    unsafe {
        let bundle: *mut AnyObject = msg_send![class, bundleWithPath: string(&path)];
        if bundle.is_null() {
            return None;
        }
        let program: *mut AnyObject = msg_send![bundle, executablePath];
        if program.is_null() {
            return None;
        }
        let text: *const c_char = msg_send![program, UTF8String];
        if text.is_null() {
            return None;
        }
        Some(PathBuf::from(
            CStr::from_ptr(text).to_string_lossy().into_owned(),
        ))
    }
}

/// The person's own home folder, from the user database: inside the sandbox `HOME` is the
/// extension's container.
fn real_home() -> Option<PathBuf> {
    // SAFETY: getpwuid's entry is read at once, before anything else could ask for another;
    // only this thread asks in this program's handling of a request.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() || (*entry).pw_dir.is_null() {
            return None;
        }
        let dir = CStr::from_ptr((*entry).pw_dir)
            .to_string_lossy()
            .into_owned();
        (!dir.is_empty()).then(|| PathBuf::from(dir))
    }
}

/// A line in the system log.
fn say(line: &str) {
    let Ok(line) = CString::new(line.replace('\0', " ")) else {
        return;
    };
    // SAFETY: a `%@` format with one NSString argument, as NSLog takes them.
    unsafe { NSLog(string(c"%@"), string(&line)) }
}
