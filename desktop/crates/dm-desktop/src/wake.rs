//! Waking a sleeping computer for the schedule: the window's start, or a
//! queue's start time, where the Scheduler says to. Windows lets any
//! program set a wake timer (where the power plan allows them, as it does
//! by default); macOS and Linux keep that to the administrator, so there
//! this does nothing and the Scheduler does not offer it.

/// Keeps a wake timer set for the next start `next` says (minutes from
/// now), looked at again every minute so a change in the Scheduler is
/// taken up. `None` clears it.
pub fn watch(next: impl Fn() -> Option<u32> + Send + 'static) {
    #[cfg(windows)]
    {
        let _ = std::thread::Builder::new()
            .name("dm-wake".into())
            .spawn(move || win::run(next));
    }
    #[cfg(not(windows))]
    let _ = next;
}

/// Whether this system lets dm wake the computer.
pub fn possible() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::System::Threading::{
        CancelWaitableTimer, CreateWaitableTimerW, SetWaitableTimer, WaitForSingleObject,
    };

    pub(super) fn run(next: impl Fn() -> Option<u32>) {
        // SAFETY: no attributes and no name; the handle is kept for the
        // life of the thread, which is the life of dm.
        let timer = unsafe { CreateWaitableTimerW(std::ptr::null(), 0, std::ptr::null()) };
        if timer.is_null() {
            return;
        }
        loop {
            match next() {
                // A minute early, so the queue is ready when its time comes.
                Some(minutes) if minutes >= 2 => {
                    // Relative times are negative, in 100-nanosecond units.
                    let due: i64 = -(i64::from(minutes - 1) * 60 * 10_000_000);
                    // SAFETY: `due` lives for the call; no completion routine.
                    unsafe {
                        SetWaitableTimer(timer, &due, 0, None, std::ptr::null(), 1);
                    }
                }
                _ => {
                    // SAFETY: the timer this thread made.
                    unsafe {
                        CancelWaitableTimer(timer);
                    }
                }
            }
            // SAFETY: the timer this thread made; a minute at most.
            unsafe {
                WaitForSingleObject(timer, 60_000);
            }
        }
    }
}
