use super::*;

/// Set when the system asks dm to end: SIGTERM, SIGINT or SIGHUP (a
/// logout, `kill`, a service manager stopping it, Ctrl+C in a terminal),
/// or Windows' console close, logoff and shutdown events.
pub(super) static SIGNALLED: AtomicBool = AtomicBool::new(false);

/// Ends dm as Program › Exit does when the system asks, rather than being
/// cut off where it stood: its downloads were stopped by nothing, a
/// yt-dlp run under it went on alone, and `control.port` was left naming a
/// port nobody listened on. A second signal while closing ends it at once.
pub(super) fn on_signal(
    runtime: &tokio::runtime::Runtime,
    wake: impl Fn() + Send + Sync + 'static,
) {
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

/// The first signal asks dm to quit; a second, while it is still closing,
/// ends it there. Says whether this one asked.
pub(super) fn quit_on_signal() -> bool {
    if SIGNALLED.swap(true, Ordering::SeqCst) {
        dm_desktop::control::forget_port(&settings::home());
        // Leaving without unwinding: what yt-dlp runs is ended here, or it
        // would go on downloading with nobody to stop it.
        dm_core::ytdlp::end_all();
        std::process::exit(130);
    }
    true
}
