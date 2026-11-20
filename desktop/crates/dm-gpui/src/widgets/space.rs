// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's spacing, lines and corners, named once (gaps row A19): every gap, padding and
//! margin is a step of this scale, so two parts of the window that should sit alike do, and a
//! step changed here moves every place that uses it. What the system's own dialogs measure
//! (their margins, a row's gap, a control's height) is in [`super::native`], per system; a
//! control's own width or height stays beside the control.
//!
//! In points, at 100 %; `crate::window::zoom::px` scales them with the window's zoom.

/// Two points: a hairline's room, text a little off its neighbour.
pub const XXS: f32 = 2.0;
/// Four points: an icon and its label, the rows of a small list.
pub const XS: f32 = 4.0;
/// Six points.
pub const S: f32 = 6.0;
/// Eight points: the common gap between the parts of a bar or a row.
pub const M: f32 = 8.0;
/// Ten points.
pub const M_L: f32 = 10.0;
/// Twelve points: a pill's or a button's sides.
pub const L: f32 = 12.0;
/// Sixteen points.
pub const XL: f32 = 16.0;
/// Twenty points.
pub const XXL: f32 = 20.0;
/// Twenty-four points.
pub const XXXL: f32 = 24.0;

/// Forty-eight points: a finger's least target at a phone's width (gaps row N7), as phones'
/// own guidelines ask (44 to 48).
pub const TOUCH: f32 = 48.0;

/// A rule's or a border's thickness, and the gap that shows one between two parts.
pub const LINE: f32 = 1.0;

/// A small part's corners (a badge, a pill, a menu's row).
pub const CORNER_S: f32 = 4.0;
/// A panel's corners.
pub const CORNER_M: f32 = 6.0;
/// A popup's corners.
pub const CORNER_L: f32 = 8.0;
