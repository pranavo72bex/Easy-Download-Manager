// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The native messaging port to the app, and the handshake that finds out what it is.

use super::*;

use std::cell::Cell;

use edm_protocol::LEGACY_NATIVE_HOST;

thread_local! {
    /// The host name the browser found last: [`NATIVE_HOST`], or [`LEGACY_NATIVE_HOST`]
    /// while the app on this computer is one from before the rename.
    static HOST: Cell<&'static str> = const { Cell::new(NATIVE_HOST) };
}

pub(super) enum Heard {
    Frame(Value),
    Gone(String),
}

/// One connection to the host: a process the browser starts for it, and
/// stops when it is dropped.
pub(super) struct Port {
    heard: UnboundedReceiver<Heard>,
    line: Line,
}

/// How the messages reach the host.
enum Line {
    /// A native messaging port (Chrome, Edge, Firefox).
    Port {
        port: JsValue,
        on_message: Closure<dyn FnMut(JsValue)>,
        on_disconnect: Closure<dyn FnMut(JsValue)>,
    },
    /// Safari: each message on its own (`runtime.sendNativeMessage`), to EDM's extension inside
    /// EDM.app, which hands it to the same host and gives back its answer, heard here as a port's
    /// would be (`decide::one_message_at_a_time`).
    Messages { tell: UnboundedSender<Heard> },
}

impl Port {
    pub(super) fn open() -> Result<Port, String> {
        let base = call_on(&api("runtime"), "getURL", &["".into()])
            .ok()
            .and_then(|b| b.as_string())
            .unwrap_or_default();
        if decide::one_message_at_a_time(&base) {
            let (tell, heard) = unbounded();
            return Ok(Port {
                heard,
                line: Line::Messages { tell },
            });
        }
        let host = HOST.with(Cell::get);
        let port = call_on(&api("runtime"), "connectNative", &[host.into()])?;
        let (tell, heard) = unbounded();
        let told = tell.clone();
        let on_message = Closure::<dyn FnMut(JsValue)>::new(move |message: JsValue| {
            let _ = tell.unbounded_send(Heard::Frame(from_js(&message)));
        });
        let on_disconnect = Closure::<dyn FnMut(JsValue)>::new(move |port: JsValue| {
            // Firefox says why on the port; Chrome in lastError.
            let why = get(&port, "error.message")
                .as_string()
                .or_else(|| api("runtime.lastError.message").as_string())
                .unwrap_or_else(|| "the EDM app closed the connection".into());
            let _ = told.unbounded_send(Heard::Gone(why));
        });
        add_listener(&get(&port, "onMessage"), on_message.as_ref())?;
        add_listener(&get(&port, "onDisconnect"), on_disconnect.as_ref())?;
        Ok(Port {
            heard,
            line: Line::Port {
                port,
                on_message,
                on_disconnect,
            },
        })
    }

    pub(super) fn send(&self, request: &Request) -> Result<(), String> {
        let frame = to_js(&request.to_frame());
        match &self.line {
            Line::Port { port, .. } => call_on(port, "postMessage", &[frame]).map(|_| ()),
            Line::Messages { tell } => {
                let asked = call_on(
                    &api("runtime"),
                    "sendNativeMessage",
                    &[NATIVE_HOST.into(), frame],
                )?
                .dyn_into::<Promise>()
                .map_err(|_| "the browser's sendNativeMessage gave no promise".to_string())?;
                let tell = tell.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let heard = match JsFuture::from(asked).await {
                        // A message the host answers nothing to comes back empty.
                        Ok(reply) if reply.is_undefined() || reply.is_null() => return,
                        Ok(reply) => Heard::Frame(from_js(&reply)),
                        Err(why) => Heard::Gone(say(&why)),
                    };
                    let _ = tell.unbounded_send(heard);
                });
                Ok(())
            }
        }
    }

    /// The next thing the host says, or why there will be nothing.
    pub(super) async fn next(&mut self, deadline: f64) -> Heard {
        let left = deadline - now();
        if left <= 0.0 {
            return Heard::Gone("the EDM app did not answer in time".into());
        }
        let heard = Box::pin(self.heard.next());
        let timer = Box::pin(sleep(left));
        match select(heard, timer).await {
            Either::Left((Some(heard), _)) => heard,
            Either::Left((None, _)) => Heard::Gone("the connection to EDM was lost".into()),
            Either::Right(_) => Heard::Gone("the EDM app did not answer in time".into()),
        }
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        let Line::Port {
            port,
            on_message,
            on_disconnect,
        } = &self.line
        else {
            return;
        };
        // Listeners first, so nothing calls into closures about to be freed.
        let _ = call_on(
            &get(port, "onMessage"),
            "removeListener",
            &[on_message.as_ref().clone()],
        );
        let _ = call_on(
            &get(port, "onDisconnect"),
            "removeListener",
            &[on_disconnect.as_ref().clone()],
        );
        let _ = call_on(port, "disconnect", &[]);
    }
}

/// Asks the host whether it is there and what it wants caught. The port is
/// returned open when it answered, so a hand-off can follow on it rather
/// than start the host a second time.
pub(super) async fn probe() -> (App, Option<Port>) {
    let asked = HOST.with(Cell::get);
    let (app, port) = probe_once().await;
    let App::Absent { reason } = &app else {
        return (app, port);
    };
    if !decide::not_connected(reason) {
        return (app, port);
    }
    // The browser knows no host by this name: an app from before the rename, or one
    // since updated, may answer to the other.
    let other = if asked == NATIVE_HOST {
        LEGACY_NATIVE_HOST
    } else {
        NATIVE_HOST
    };
    HOST.with(|h| h.set(other));
    let (again, port) = probe_once().await;
    if matches!(again, App::Present { .. }) {
        return (again, port);
    }
    HOST.with(|h| h.set(NATIVE_HOST));
    (app, None)
}

async fn probe_once() -> (App, Option<Port>) {
    let mut port = match Port::open() {
        Ok(port) => port,
        Err(why) => return (App::Absent { reason: why }, None),
    };
    if let Err(why) = port
        .send(&Request::Hello(decide::greeting()))
        .and_then(|()| port.send(&Request::Settings))
    {
        return (App::Absent { reason: why }, None);
    }

    let mut probe = Probe::default();
    let mut deadline = now() + decide::HELLO_WITHIN_MS;
    loop {
        match port.next(deadline).await {
            Heard::Frame(frame) => {
                let had_hello = probe.has_hello();
                probe.absorb(&frame);
                if let Some(app) = probe.finished() {
                    let open = matches!(app, App::Present { .. }).then_some(port);
                    return (app, open);
                }
                if probe.has_hello() && !had_hello {
                    deadline = deadline.min(now() + decide::SETTINGS_WITHIN_MS);
                }
            }
            // Gone or silent: a hand-off that follows opens a port of its own.
            Heard::Gone(why) => return (probe.give_up(&why), None),
        }
    }
}

/// What is known of the app, asked again when the answer has gone stale.
pub(super) async fn app() -> (App, Option<Port>) {
    let at = now();
    if let Some(known) = KNOWN.with(|k| k.borrow().clone()) {
        if known.fresh(at) {
            return (known.app, None);
        }
    }
    let (app, port) = probe().await;
    if let App::Absent { reason } = &app {
        log("info", &format!("no app to hand downloads to: {reason}"));
    }
    if let App::Present { rules, hello } = &app {
        remember_rules(rules).await;
        // The site rules the app serves are added to the built-in tables; an app
        // that serves none (an older one) leaves the tables as they were built.
        decide::rules::install(hello.rules.clone());
        remember_served(decide::rules::held().as_ref()).await;
        // The right-click menu the app serves, made again where it changed.
        remember_menus(hello.menus.as_deref()).await;
    }
    show_badge(decide::badge(&app));
    KNOWN.with(|k| {
        *k.borrow_mut() = Some(Known {
            app: app.clone(),
            at_ms: at,
        })
    });
    (app, port)
}

/// The toolbar icon's badge set to `text` ("!" while the app does not answer, `decide::badge`).
/// A browser without badges says nothing, and nothing else changes.
fn show_badge(text: &str) {
    let action = api("action");
    if !text.is_empty() {
        let colour = serde_json::json!({ "color": decide::BADGE_COLOUR });
        let _ = call_on(&action, "setBadgeBackgroundColor", &[to_js(&colour)]);
    }
    let _ = call_on(
        &action,
        "setBadgeText",
        &[to_js(&serde_json::json!({ "text": text }))],
    );
}
