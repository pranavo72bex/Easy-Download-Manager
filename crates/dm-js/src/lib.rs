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
//! - `interp`, `object`, `value`, `realm`: the interpreter, its values and objects, and the
//!   world a program starts in (stage 2).
//! - `builtins`: the language's built-in objects (stage 3); `regexp`: its patterns (stage 4).

pub mod ast;
pub mod builtins;
pub mod interp;
pub mod lexer;
pub mod number;
pub mod object;
pub mod parser;
pub mod promise;
pub mod realm;
pub mod regexp;
pub mod typed;
pub mod value;

/// The stack a program runs on: deep enough for the depth the interpreter allows its calls,
/// in a debug build too (only what is used is ever touched).
const STACK: usize = 512 << 20;

/// Runs `program`, then `call` (an expression), within `within`, on a thread of its own: the
/// call's value as a string, or what was thrown, in words. What dm asks of its JavaScript:
/// `program` defines functions, `call` asks one for an answer (`__dm.solve({...})`).
pub fn evaluate(program: &str, call: &str, within: std::time::Duration) -> Result<String, String> {
    let (program, call) = (program.to_string(), call.to_string());
    let worker = std::thread::Builder::new()
        .name("dm-js".into())
        .stack_size(STACK)
        .spawn(move || {
            let mut interp = interp::Interp::new();
            interp.deadline = Some(std::time::Instant::now() + within);
            let said = |interp: &mut interp::Interp, e: value::Value| -> String {
                interp
                    .to_rust_string(&e)
                    .unwrap_or_else(|_| "an error that cannot be said".into())
            };
            if let Err(e) = interp.run(&program) {
                return Err(said(&mut interp, e));
            }
            match interp.run(&call) {
                Ok(v) => interp.to_rust_string(&v).map_err(|e| said(&mut interp, e)),
                Err(e) => Err(said(&mut interp, e)),
            }
        })
        .map_err(|e| format!("the JavaScript thread could not start: {e}"))?;
    worker
        .join()
        .unwrap_or_else(|_| Err("the JavaScript interpreter stopped unexpectedly".into()))
}
