// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! UniFFI's generator, built from the same version as the library it reads, so the Kotlin
//! bindings always match `libdm_android.so` (gaps row N3).

fn main() {
    uniffi::uniffi_bindgen_main()
}
