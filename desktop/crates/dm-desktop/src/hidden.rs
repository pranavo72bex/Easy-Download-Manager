use super::*;

/// A login launch: dm waits in the tray with no window until one is asked
/// for — the tray's Show, a second launch, the Dock icon, a hand-off that
/// opens a dialog, or no tray to wait in. The window and its GL context
/// are about 40 MB (`qa/after.md`) that a dm waiting in the tray all day has
/// no use for; eframe's own start made them at once and hid them, after
/// showing them for a frame.
///
/// Until then dm's logic runs on a stand-in context with no fonts and no
/// drawing (`run_logic`), woken by what wakes the window: its own timers,
/// the queue, the tray, a second launch. Once shown, eframe's context takes
/// over, and every wake the stand-in is asked for is passed on to it.
pub(super) fn run_hidden(
    options: eframe::NativeOptions,
    make: impl FnOnce(&egui::Context) -> Window + 'static,
) -> Result<(), String> {
    use winit::event_loop::EventLoop;
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let real: Arc<std::sync::OnceLock<egui::Context>> = Arc::default();
    let real_ctx = Arc::clone(&real);
    let made: std::rc::Rc<std::cell::RefCell<Option<Window>>> = std::rc::Rc::default();
    let eframe_app = eframe::create_native(
        "dm",
        options,
        Box::new({
            let (real, made) = (Arc::clone(&real), std::rc::Rc::clone(&made));
            move |cc| {
                let _ = real.set(cc.egui_ctx.clone());
                match made.borrow_mut().take() {
                    Some(window) => Ok(Box::new(window)),
                    None => Err("the window was asked for before dm had started".into()),
                }
            }
        }),
        &event_loop,
    );
    let ctx = egui::Context::default();
    let due: Arc<Mutex<Option<std::time::Instant>>> = Arc::default();
    let ticking = Arc::new(AtomicBool::new(false));
    ctx.set_repaint_observer({
        let (due, ticking, proxy) = (
            Arc::clone(&due),
            Arc::clone(&ticking),
            event_loop.create_proxy(),
        );
        move |info| {
            if let Some(real) = real.get() {
                real.request_repaint_after(info.delay);
                return;
            }
            let at = std::time::Instant::now() + info.delay;
            {
                let mut due = dm_core::lock(&due);
                if due.is_none_or(|d| at < d) {
                    *due = Some(at);
                }
            }
            // From another thread, or a timer's: the loop is woken to look
            // at the new time. A tick's own asks are read when it ends.
            if !ticking.load(Ordering::Relaxed) {
                let _ = proxy.send_event(eframe::UserEvent::RequestRepaint {
                    viewport_id: egui::ViewportId::ROOT,
                    when: at,
                    cumulative_pass_nr: info.current_cumulative_pass_nr,
                });
            }
        }
    });
    let mut lazy = Lazy {
        eframe: eframe_app,
        make: Some(Box::new(make)),
        made,
        ctx,
        real: real_ctx,
        asked: Vec::new(),
        due,
        ticking,
        shown: false,
    };
    event_loop.run_app(&mut lazy).map_err(|e| e.to_string())
}

/// What makes the window's state around a context.
pub(super) type MakeWindow<'a> = Box<dyn FnOnce(&egui::Context) -> Window + 'a>;

/// The event loop of a login launch: dm's own until the window is shown,
/// eframe's from then on.
pub(super) struct Lazy<'a> {
    eframe: eframe::EframeWinitApplication<'a>,
    make: Option<MakeWindow<'a>>,
    /// The window's state, made on the loop's start and handed to eframe's
    /// creator when shown.
    made: std::rc::Rc<std::cell::RefCell<Option<Window>>>,
    ctx: egui::Context,
    /// eframe's context, once the window is made.
    real: Arc<std::sync::OnceLock<egui::Context>>,
    /// What dm asked of a window that was not there yet — a control
    /// request's screenshot, the focus — given to the window once it is.
    asked: Vec<egui::ViewportCommand>,
    /// When dm's logic next asked to run.
    due: Arc<Mutex<Option<std::time::Instant>>>,
    ticking: Arc<AtomicBool>,
    shown: bool,
}

impl Lazy<'_> {
    pub(super) fn tick(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let now = std::time::Instant::now();
        *dm_core::lock(&self.due) = None;
        let (quitting, show) = {
            let mut made = self.made.borrow_mut();
            let Some(window) = made.as_mut() else {
                return;
            };
            self.ticking.store(true, Ordering::Relaxed);
            let out = self
                .ctx
                .run_logic(&egui::RawInput::default(), |ctx| window.tick(ctx));
            self.ticking.store(false, Ordering::Relaxed);
            for (_, commands) in out.viewport_commands {
                self.asked.extend(commands);
            }
            (window.quitting, !window.hidden)
        };
        if quitting {
            event_loop.exit();
        } else if show {
            self.shown = true;
            winit::application::ApplicationHandler::resumed(&mut self.eframe, event_loop);
            if let Some(real) = self.real.get() {
                for command in self.asked.drain(..) {
                    real.send_viewport_cmd(command);
                }
                real.request_repaint();
            }
        } else {
            // Not straight round again on an ask for "now": dm's headless
            // loop looks twenty times a second at most, and so does this.
            let mut due = dm_core::lock(&self.due);
            let floor = now + std::time::Duration::from_millis(50);
            *due = Some(due.map_or(now + std::time::Duration::from_secs(30), |d| d.max(floor)));
        }
    }
}

impl winit::application::ApplicationHandler<eframe::UserEvent> for Lazy<'_> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.resumed(event_loop);
            return;
        }
        // On the loop's start, as eframe's creator would be: the tray and
        // the Dock's hook need the application winit has made by now.
        if let Some(make) = self.make.take() {
            *self.made.borrow_mut() = Some(make(&self.ctx));
            *dm_core::lock(&self.due) = Some(std::time::Instant::now());
        }
    }

    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if self.shown {
            self.eframe.new_events(event_loop, cause);
        }
    }

    fn user_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: eframe::UserEvent,
    ) {
        if self.shown {
            self.eframe.user_event(event_loop, event);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if self.shown {
            self.eframe.window_event(event_loop, window_id, event);
        }
    }

    fn device_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if self.shown {
            self.eframe.device_event(event_loop, device_id, event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !self.shown {
            let due = *dm_core::lock(&self.due);
            if due.is_some_and(|d| d <= std::time::Instant::now()) {
                self.tick(event_loop);
            }
        }
        if self.shown {
            self.eframe.about_to_wait(event_loop);
        } else if let Some(due) = *dm_core::lock(&self.due) {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(due));
        }
    }

    fn suspended(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.suspended(event_loop);
        }
    }

    fn exiting(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.exiting(event_loop);
        }
    }

    fn memory_warning(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.shown {
            self.eframe.memory_warning(event_loop);
        }
    }
}
