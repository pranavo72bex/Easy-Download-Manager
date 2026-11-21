// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The harness's own control port, and calls made as the assistant.

use super::*;

impl Harness {
    /// The control port opened on the loopback over this window with `token`, as `--control`
    /// opens it (`port::open`), and its number: what the window answers is answered as the
    /// harness steps it, the list and pause, resume and cancel by id on the port's own thread.
    /// `assistants_only`: opened because AI assistants are allowed, for them alone.
    pub(crate) fn open_port(&mut self, token: &str, assistants_only: bool) -> u16 {
        let backend = self.view(|view| {
            view.port.assistants_only = assistants_only;
            view.backend.clone()
        });
        dm_desktop_base::control::listen_reading(
            0,
            token.into(),
            self.asks.clone(),
            || {},
            Some(crate::port::reads(backend, assistants_only)),
        )
        .unwrap()
    }

    /// [`Harness::call`] as an AI assistant's client asks it, through `dm mcp`.
    pub(crate) fn call_as_assistant(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let (reply, answer) = mpsc::channel();
        self.asks
            .send(dm_desktop_base::control::Request {
                via_mcp: true,
                method: method.into(),
                params,
                reply,
            })
            .unwrap();
        for _ in 0..100 {
            self.step();
            if let Ok(answered) = answer.try_recv() {
                return answered;
            }
        }
        Err(format!("{method} was not answered"))
    }

    /// Where the window's requests come in, for a real control port opened over it
    /// (`dm_desktop_base::control::listen`).
    pub(crate) fn requests(&self) -> mpsc::Sender<dm_desktop_base::control::Request> {
        self.asks.clone()
    }

    /// Whether the window takes requests from AI assistants alone, as `dm --mcp-only` opens it.
    pub(crate) fn assistants_only(&mut self, only: bool) {
        self.view(|view| view.port.assistants_only = only);
    }
}
