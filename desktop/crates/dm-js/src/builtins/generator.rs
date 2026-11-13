// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Generators (`function*`). dm's programs define them but do not call them while dm reads a
//! player: calling one says so plainly rather than running it wrongly.

use std::rc::Rc;

use crate::interp::{Interp, Res};
use crate::object::{Obj, ScriptFunc};
use crate::value::Value;

/// Installs what generators share.
pub fn install(_interp: &mut Interp) {}

/// A generator function called: not run here.
pub fn start(
    interp: &mut Interp,
    _f: &Obj,
    _code: &Rc<ScriptFunc>,
    _this: Value,
    _args: &[Value],
) -> Res<Value> {
    Err(interp.type_error("generator functions are not supported by dm-js"))
}
