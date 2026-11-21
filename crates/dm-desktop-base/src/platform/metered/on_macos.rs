// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! IOKit's word on the battery (gaps row W11): its power sources, each a dictionary of
//! Core Foundation values. Whether the connection is metered is not read here (the module's
//! own words say why).

use std::ffi::{c_char, c_void};

use super::Battery;

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFIndex = isize;

/// `kCFStringEncodingUTF8`.
const UTF8: u32 = 0x0800_0100;
/// `kCFNumberSInt32Type`.
const SINT32: CFIndex = 3;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, source: CFTypeRef) -> CFDictionaryRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> *const c_void;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        text: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    fn CFNumberGetValue(number: *const c_void, kind: CFIndex, out: *mut c_void) -> u8;
}

/// A Core Foundation object this module made or copied, released when dropped.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a non-null object this module was given ownership of (a Create or Copy
            // call's), released once.
            unsafe { CFRelease(self.0) }
        }
    }
}

/// A Core Foundation string of `text`.
fn cf_string(text: &std::ffi::CStr) -> Owned {
    // SAFETY: a nul-terminated string, the default allocator (null) and UTF-8.
    Owned(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) })
}

/// The internal battery's state, from the power sources IOKit lists: discharging where its
/// "Power Source State" is "Battery Power", and its "Current Capacity" out of "Max Capacity"
/// (a percent on every Mac). `None` for a Mac with no battery.
pub(super) fn battery() -> Option<Battery> {
    let state_key = cf_string(c"Power Source State");
    let on_battery = cf_string(c"Battery Power");
    let type_key = cf_string(c"Type");
    let internal = cf_string(c"InternalBattery");
    let current_key = cf_string(c"Current Capacity");
    let max_key = cf_string(c"Max Capacity");
    // SAFETY: the copy is owned here and released by `Owned`.
    let blob = Owned(unsafe { IOPSCopyPowerSourcesInfo() });
    if blob.0.is_null() {
        return None;
    }
    // SAFETY: as above, from the blob just copied.
    let list = Owned(unsafe { IOPSCopyPowerSourcesList(blob.0) });
    if list.0.is_null() {
        return None;
    }
    // SAFETY: a valid array.
    let count = unsafe { CFArrayGetCount(list.0) };
    for i in 0..count {
        // SAFETY: an index within the array; the value and the description are borrowed
        // from the blob and the list, which outlive them here (the Get rule).
        let description = unsafe {
            let source = CFArrayGetValueAtIndex(list.0, i);
            IOPSGetPowerSourceDescription(blob.0, source)
        };
        if description.is_null() {
            continue;
        }
        // SAFETY: a valid dictionary and keys; the values are borrowed from it.
        let (kind, state, current, max) = unsafe {
            (
                CFDictionaryGetValue(description, type_key.0),
                CFDictionaryGetValue(description, state_key.0),
                number(CFDictionaryGetValue(description, current_key.0)),
                number(CFDictionaryGetValue(description, max_key.0)),
            )
        };
        // SAFETY: two valid objects, or a null `kind` passed over.
        if kind.is_null() || unsafe { CFEqual(kind, internal.0) } == 0 {
            continue;
        }
        // SAFETY: as above.
        let discharging = !state.is_null() && unsafe { CFEqual(state, on_battery.0) } != 0;
        let percent = match (current, max) {
            (Some(now), Some(full)) if full > 0 => {
                u8::try_from(now * 100 / full).ok().filter(|p| *p <= 100)
            }
            _ => None,
        };
        return Some(Battery {
            discharging,
            percent,
        });
    }
    None
}

/// A CFNumber's value as a 32-bit integer; `None` for a null. `value` is null or a CFNumber
/// (a power source's capacity).
fn number(value: *const c_void) -> Option<i32> {
    if value.is_null() {
        return None;
    }
    let mut out: i32 = 0;
    // SAFETY: a CFNumber, read into a local of the size asked for.
    let read = unsafe { CFNumberGetValue(value, SINT32, (&mut out as *mut i32).cast()) };
    (read != 0).then_some(out)
}
