// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A promise's state. Its jobs run when the program's own code has run (stage 3 of gaps
//! row F15).

use crate::object::Obj;
use crate::value::Value;

/// Where a promise stands, and what waits on it.
pub enum PromiseState {
    /// Not yet settled: what runs when it is (each a function to fulfil and one to reject).
    Pending(Vec<(Option<Obj>, Option<Obj>, Obj)>),
    /// Fulfilled with a value.
    Fulfilled(Value),
    /// Rejected with a reason.
    Rejected(Value),
}
