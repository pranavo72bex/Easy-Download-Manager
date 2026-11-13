// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! dm's own JavaScript interpreter (gaps row F15).
//!
//! dm runs a little JavaScript: the program that answers YouTube's player checks (built from
//! YouTube's own player script, `dm-core`'s `youtube::player`) and a proxy configuration script
//! a network gives. Until now the computer's own engine ran it (JavaScriptCore on macOS and
//! Linux, Chakra on Windows), in a process of its own behind the system's sandbox (gaps row 201).
//! This crate runs it in Rust, with nothing outside the language in reach: no files, programs,
//! network, clock or page, and limits on time and memory that a program cannot pass.
//!
//! The parts, each in a file of its own:
//! - [`lexer`]: the text cut into tokens.
//! - `parser` and `ast`: the tokens read as a program (stage 1 of row F15).
//! - The interpreter, its objects and its patterns follow in later stages.

pub mod ast;
pub mod lexer;
pub mod number;
pub mod parser;
