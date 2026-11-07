// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The toolbar popup is written in Rust (browser/src/popup.rs), compiled to
// WebAssembly; this loads it.
import init, * as dm from './pkg/dm_catcher.js';

const ext = globalThis.browser ?? globalThis.chrome;
init().then(() => dm.popup_start(ext)).catch((e) => console.error('dm:', e));
