// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The control port, as the egui window opens it: the same server
//! (`dm_desktop_base::control`), its requests handed to the window's thread
//! through the window's own wake-up, and answered there (`list::Downloads`).

use std::sync::mpsc;

use dm_desktop_base::boot::Wake;
use dm_desktop_base::control::Request;

/// Opens the port (`0`: one the system picks) and writes where it is for
/// clients; the requests, or why it could not open.
pub fn open(
    port: u16,
    wakes: async_channel::Sender<Wake>,
) -> Result<mpsc::Receiver<Request>, String> {
    let home = dm_shell::settings::home();
    let token = dm_desktop_base::control::fresh_token(&home).map_err(|e| e.to_string())?;
    let (to_window, requests) = mpsc::channel();
    let port = dm_desktop_base::control::listen(port, token, to_window, move || {
        let _ = wakes.try_send(Wake::Now);
    })
    .map_err(|e| e.to_string())?;
    dm_desktop_base::control::write_port(&home, port).map_err(|e| e.to_string())?;
    Ok(requests)
}
