// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control port, as the egui window opens it: the same server
//! (`dm_desktop_base::control`), its requests handed to the window's thread
//! through the window's own wake-up, and answered there (`list::Downloads`);
//! `queue.list`, which only reads, answered on the port's own thread.

use std::sync::{mpsc, Arc, Mutex};

use dm_desktop_base::boot::Wake;
use dm_desktop_base::control::{Reads, Request};
use dm_shell::app::backend::Backend;

/// Opens the port (`0`: one the system picks) and writes where it is for
/// clients; the requests, or why it could not open. `wake` is called as
/// each request arrives that the window is to answer.
pub fn open(
    port: u16,
    wake: std::sync::Arc<dyn Fn(Wake) + Send + Sync>,
    backend: Backend,
    assistants_only: bool,
) -> Result<mpsc::Receiver<Request>, String> {
    let home = dm_shell::settings::home();
    let token = dm_desktop_base::control::fresh_token(&home).map_err(|e| e.to_string())?;
    let (to_window, requests) = mpsc::channel();
    let port = dm_desktop_base::control::listen_reading(
        port,
        token,
        to_window,
        move || wake(Wake::Now),
        Some(reads(backend, assistants_only)),
    )
    .map_err(|e| e.to_string())?;
    dm_desktop_base::control::write_port(&home, port).map_err(|e| e.to_string())?;
    Ok(requests)
}

/// `queue.list` answered from the queue as it stands, on the port's own
/// thread: a script asking ten times a second for the list woke the window
/// for each and drew it, and waited behind whatever it was drawing (gaps row
/// 176). Answered as the window answers it: who may ask as the application
/// says (`control_api::refused`), each finished row by its file's size on
/// disk (`SavedSizes`, as the window's read gives it).
fn reads(backend: Backend, assistants_only: bool) -> Reads {
    let saved = Mutex::new(dm_shell::app::snapshot::SavedSizes::default());
    Arc::new(move |method, params, via_mcp| {
        if method != "queue.list" {
            return None;
        }
        let allow_mcp = dm_core::lock(&backend.settings_handle()).allow_mcp;
        if let Some(why) = dm_shell::app::control_api::refused(via_mcp, assistants_only, allow_mcp)
        {
            return Some(Err(why));
        }
        let mut rows = backend.list_shared();
        dm_core::lock(&saved).apply(&mut rows);
        dm_shell::app::control_api::answer(&backend, &rows, method, params)
    })
}
