// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A logout, `kill`, a service manager stopping EDM or Ctrl+C in a terminal
//! ends EDM as Program › Exit does: the first asks it to quit, a second
//! while it is closing ends it there.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use edm_shell::app::backend::Backend;
use edm_shell::settings;

#[cfg(windows)]
mod session_end;

/// Set when the system asks EDM to end: SIGTERM, SIGINT or SIGHUP (a
/// logout, `kill`, a service manager stopping it, Ctrl+C in a terminal),
/// or Windows' console close, break, logoff and shutdown events, and its
/// session ending (`on_session_end`).
pub static SIGNALLED: AtomicBool = AtomicBool::new(false);

/// Ends EDM as Program › Exit does when the system asks, rather than being
/// cut off where it stood: its downloads were stopped by nothing, a
/// video run under it went on alone, and `control.port` was left naming a
/// port nobody listened on. A second signal while closing ends it at once.
pub fn on_signal(runtime: &tokio::runtime::Runtime, wake: impl Fn() + Send + Sync + 'static) {
    let wake = Arc::new(wake);
    // The handlers are installed here, now: a stream made inside the task
    // would leave the default action in place until the task first ran.
    let _inside = runtime.enter();
    let mut asked: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>> =
        Vec::new();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        for kind in [
            SignalKind::terminate(),
            SignalKind::interrupt(),
            SignalKind::hangup(),
        ] {
            if let Ok(mut stream) = signal(kind) {
                asked.push(Box::pin(async move {
                    while stream.recv().await.is_some() {
                        if quit_on_signal() {
                            return;
                        }
                    }
                }));
            }
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        macro_rules! listen {
            ($make:expr) => {
                if let Ok(mut stream) = $make {
                    asked.push(Box::pin(async move {
                        while stream.recv().await.is_some() {
                            if quit_on_signal() {
                                return;
                            }
                        }
                    }));
                }
            };
        }
        listen!(windows::ctrl_c());
        // What a process manager sends a console program it stops (Python's
        // `send_signal(CTRL_BREAK_EVENT)`): Windows' nearest to SIGTERM.
        listen!(windows::ctrl_break());
        listen!(windows::ctrl_close());
        listen!(windows::ctrl_logoff());
        listen!(windows::ctrl_shutdown());
    }
    for each in asked {
        let wake = Arc::clone(&wake);
        runtime.spawn(async move {
            each.await;
            wake();
        });
    }
}

/// Windows ending the session (a logoff, a shutdown or restart, an
/// installer's Restart Manager) ends EDM as Exit does, the downloads stopped
/// and awaited before Windows is answered, since it may end the process
/// then (gaps row 186). Windows asks windows, not with a signal, and never
/// tells a program with the window system loaded through its console: a
/// hidden window of EDM's own hears it, under GPUI's window and under
/// `--headless`, which has none. Nothing elsewhere: a logout there is a
/// signal.
pub fn on_session_end(backend: &Backend, wake: impl Fn() + Send + Sync + 'static) {
    #[cfg(windows)]
    {
        let backend = backend.clone();
        let _ = session_end::watch(Arc::new(move || {
            // Here, whatever the window's thread is doing (a dialog of the
            // system's holds it); then EDM is asked to end, as by a signal,
            // and its own Exit finds nothing left running.
            backend.shut_down();
            crate::control::closing();
            crate::control::forget_port(&settings::home());
            SIGNALLED.store(true, Ordering::SeqCst);
            wake();
        }));
    }
    #[cfg(not(windows))]
    let _ = (backend, wake);
}

/// The first signal asks EDM to quit; a second, while it is still closing,
/// ends it there. Says whether this one asked.
pub fn quit_on_signal() -> bool {
    if SIGNALLED.swap(true, Ordering::SeqCst) {
        crate::control::forget_port(&settings::home());
        std::process::exit(130);
    }
    true
}
