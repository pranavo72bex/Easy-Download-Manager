//! The native messaging port to the app, and the handshake that finds out what it is.

use super::*;

pub(super) enum Heard {
    Frame(Value),
    Gone(String),
}

/// One connection to the host: a process the browser starts for it, and
/// stops when it is dropped.
pub(super) struct Port {
    port: JsValue,
    heard: UnboundedReceiver<Heard>,
    on_message: Closure<dyn FnMut(JsValue)>,
    on_disconnect: Closure<dyn FnMut(JsValue)>,
}

impl Port {
    pub(super) fn open() -> Result<Port, String> {
        let port = call_on(&api("runtime"), "connectNative", &[NATIVE_HOST.into()])?;
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
                .unwrap_or_else(|| "the dm app closed the connection".into());
            let _ = told.unbounded_send(Heard::Gone(why));
        });
        add_listener(&get(&port, "onMessage"), on_message.as_ref())?;
        add_listener(&get(&port, "onDisconnect"), on_disconnect.as_ref())?;
        Ok(Port {
            port,
            heard,
            on_message,
            on_disconnect,
        })
    }

    pub(super) fn send(&self, request: &Request) -> Result<(), String> {
        call_on(&self.port, "postMessage", &[to_js(&request.to_frame())]).map(|_| ())
    }

    /// The next thing the host says, or why there will be nothing.
    pub(super) async fn next(&mut self, deadline: f64) -> Heard {
        let left = deadline - now();
        if left <= 0.0 {
            return Heard::Gone("the dm app did not answer in time".into());
        }
        let heard = Box::pin(self.heard.next());
        let timer = Box::pin(sleep(left));
        match select(heard, timer).await {
            Either::Left((Some(heard), _)) => heard,
            Either::Left((None, _)) => Heard::Gone("the connection to dm was lost".into()),
            Either::Right(_) => Heard::Gone("the dm app did not answer in time".into()),
        }
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        // Listeners first, so nothing calls into closures about to be freed.
        let _ = call_on(
            &get(&self.port, "onMessage"),
            "removeListener",
            &[self.on_message.as_ref().clone()],
        );
        let _ = call_on(
            &get(&self.port, "onDisconnect"),
            "removeListener",
            &[self.on_disconnect.as_ref().clone()],
        );
        let _ = call_on(&self.port, "disconnect", &[]);
    }
}

/// Asks the host whether it is there and what it wants caught. The port is
/// returned open when it answered, so a hand-off can follow on it rather
/// than start the host a second time.
pub(super) async fn probe() -> (App, Option<Port>) {
    let mut port = match Port::open() {
        Ok(port) => port,
        Err(why) => return (App::Absent { reason: why }, None),
    };
    if let Err(why) = port
        .send(&Request::Hello)
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
    if let App::Present { rules, .. } = &app {
        remember_rules(rules).await;
    }
    KNOWN.with(|k| {
        *k.borrow_mut() = Some(Known {
            app: app.clone(),
            at_ms: at,
        })
    });
    (app, port)
}
