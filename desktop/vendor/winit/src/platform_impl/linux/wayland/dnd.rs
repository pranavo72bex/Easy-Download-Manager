//! dm: links and files dropped on a window under Wayland, which winit 0.30
//! does not take at all. The drop is offered as `text/uri-list` (files,
//! and links from most browsers), `text/x-moz-url` (Firefox's link) or text,
//! read from the source client through a pipe on the event loop, and given
//! to the window as `WindowEvent::DroppedFile` for each item: a file's path,
//! or a link's address as a path, as dm's X11 patch gives it (`x11/dnd.rs`).
//!
//! And files dragged out of a window: `request_drag_out` is called while
//! the button that began the drag is still down; the event loop starts the
//! drag on its next turn (`start_drag_out`), with that press's serial, and
//! hands the target the files as `text/uri-list`. `take_drag_ended` says
//! when it is over, since the window never sees the button let go.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use sctk::data_device_manager::data_device::DataDeviceHandler;
use sctk::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use sctk::data_device_manager::data_source::DataSourceHandler;
use sctk::data_device_manager::WritePipe;
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_data_source::WlDataSource;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};

use crate::event::WindowEvent;
use crate::platform_impl::wayland::make_wid;
use sctk::shell::WaylandSurface;

use crate::platform_impl::wayland::seat::WinitPointerDataExt;
use crate::platform_impl::wayland::state::WinitState;

/// A drag out asked for, as the `text/uri-list` it hands over.
static DRAG_OUT: Mutex<Option<String>> = Mutex::new(None);

/// Set when a drag out has ended, however it ended.
static DRAG_ENDED: AtomicBool = AtomicBool::new(false);

/// What wakes the event loop, so a drag asked for starts at once rather
/// than on whatever turn comes next.
static WAKER: std::sync::OnceLock<calloop::ping::Ping> = std::sync::OnceLock::new();

pub(crate) fn set_waker(ping: calloop::ping::Ping) {
    let _ = WAKER.set(ping);
}

/// Asks for `uri_list` to be dragged out of the window under the pointer.
pub fn request_drag_out(uri_list: String) {
    if let Ok(mut pending) = DRAG_OUT.lock() {
        *pending = Some(uri_list);
    }
    if let Some(ping) = WAKER.get() {
        ping.ping();
    }
}

/// Whether a drag out has ended since this was last asked.
pub fn take_drag_ended() -> bool {
    DRAG_ENDED.swap(false, Ordering::Relaxed)
}

impl WinitState {
    /// Starts the drag out asked for, if one was: from the window the
    /// pointer is on, with the serial of the press that began it.
    pub(crate) fn start_drag_out(&mut self, queue_handle: &QueueHandle<Self>) {
        let Some(list) = DRAG_OUT.lock().ok().and_then(|mut pending| pending.take()) else {
            return;
        };
        let Some(manager) = &self.data_device_manager else {
            DRAG_ENDED.store(true, Ordering::Relaxed);
            return;
        };
        for seat in self.seats.values() {
            let (Some(pointer), Some(device)) = (&seat.pointer, &seat.data_device) else {
                continue;
            };
            let data = pointer.pointer().winit_data();
            let Some(window_id) = data.focused_window() else {
                continue;
            };
            let surface = match self.windows.borrow().get(&window_id) {
                Some(window) => window.lock().unwrap().window.wl_surface().clone(),
                None => continue,
            };
            let source =
                manager.create_drag_and_drop_source(queue_handle, ["text/uri-list"], DndAction::Copy);
            source.start_drag(device, &surface, None, data.latest_button_serial());
            self.drag_out = Some((source, list));
            return;
        }
        DRAG_ENDED.store(true, Ordering::Relaxed);
    }

    fn drag_out_over(&mut self) {
        self.drag_out = None;
        DRAG_ENDED.store(true, Ordering::Relaxed);
    }
}

/// The formats a drop is read in, best first.
const WANTED: [&str; 5] =
    ["text/uri-list", "text/x-moz-url", "text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

/// The most a drop's text may be: a list of a few thousand links.
const MOST: usize = 4 * 1024 * 1024;

/// The format this drop will be read in, if it offers one dm reads.
fn chosen(offer: &DragOffer) -> Option<String> {
    offer.with_mime_types(|offered| {
        WANTED.iter().find(|w| offered.iter().any(|o| o == *w)).map(|w| (*w).to_string())
    })
}

/// The items a drop's bytes name, in `mime`: each file or address once.
pub(crate) fn items(mime: &str, bytes: &[u8]) -> Vec<PathBuf> {
    let text = if mime == "text/x-moz-url" {
        // UTF-16: the address, a newline, the link's title.
        let units: Vec<u16> =
            bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let lines = text
        .split(['\r', '\n'])
        .map(|line| line.trim_matches(char::from(0)).trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    if mime == "text/uri-list" {
        lines.map(path_of).collect()
    } else {
        // A link's text, or the first line of words that hold one.
        lines.take(1).map(path_of).collect()
    }
}

/// A `file://` address as its path; anything else as the address itself.
fn path_of(item: &str) -> PathBuf {
    match item.strip_prefix("file://") {
        Some(rest) => {
            let path = rest.find('/').map_or(rest, |at| &rest[at..]);
            PathBuf::from(percent_decoded(path))
        },
        None => PathBuf::from(item),
    }
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[at + 1..at + 3], 16) {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl DataDeviceHandler for WinitState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
        _surface: &WlSurface,
    ) {
        let Some(offer) = data_device
            .data::<sctk::data_device_manager::data_device::DataDeviceData>()
            .and_then(|data| data.drag_offer())
        else {
            return;
        };
        let mime = chosen(&offer);
        offer.accept_mime_type(offer.serial, mime.clone());
        if mime.is_some() {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {
    }

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        let Some(offer) = data_device
            .data::<sctk::data_device_manager::data_device::DataDeviceData>()
            .and_then(|data| data.drag_offer())
        else {
            return;
        };
        let Some(mime) = chosen(&offer) else {
            offer.finish();
            offer.destroy();
            return;
        };
        let window_id = make_wid(&offer.surface);
        let Ok(pipe) = offer.receive(mime.clone()) else {
            offer.destroy();
            return;
        };
        // Read on the event loop as the source writes it, whole before the
        // window hears of it.
        let mut bytes = Vec::new();
        let read = self.loop_handle.insert_source(pipe, move |(), file, state| {
            let mut chunk = [0u8; 8192];
            // SAFETY: the file is only read here, never dropped.
            match unsafe { file.get_mut() }.read(&mut chunk) {
                Ok(n) if n > 0 && bytes.len() + n <= MOST => {
                    bytes.extend_from_slice(&chunk[..n]);
                    calloop::PostAction::Continue
                },
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    calloop::PostAction::Continue
                },
                _ => {
                    for item in items(&mime, &bytes) {
                        state
                            .events_sink
                            .push_window_event(WindowEvent::DroppedFile(item), window_id);
                    }
                    offer.finish();
                    offer.destroy();
                    calloop::PostAction::Remove
                },
            }
        });
        if read.is_err() {
            data_device
                .data::<sctk::data_device_manager::data_device::DataDeviceData>()
                .and_then(|data| data.drag_offer())
                .inspect(DragOffer::destroy);
        }
    }
}

impl DataOfferHandler for WinitState {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        offer: &mut DragOffer,
        actions: DndAction,
    ) {
        if actions.contains(DndAction::Copy) {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

// The files of a drag out, as the target asks for them.
impl DataSourceHandler for WinitState {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        mime: String,
        mut pipe: WritePipe,
    ) {
        if let Some((_, list)) = &self.drag_out {
            if mime == "text/uri-list" {
                let _ = pipe.write_all(list.as_bytes());
            }
        }
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        self.drag_out_over();
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        // The button is up; the target may still be reading.
        DRAG_ENDED.store(true, Ordering::Relaxed);
    }

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        self.drag_out_over();
    }

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

sctk::delegate_data_device!(WinitState);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_names_its_files_and_links() {
        let list = b"# a comment\r\nfile:///home/me/a%20b.txt\r\nhttps://e.com/x.zip\r\n";
        assert_eq!(
            items("text/uri-list", list),
            vec![PathBuf::from("/home/me/a b.txt"), PathBuf::from("https://e.com/x.zip")]
        );
        let moz: Vec<u8> = "https://e.com/y.zip\nA title"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(items("text/x-moz-url", &moz), vec![PathBuf::from("https://e.com/y.zip")]);
        assert_eq!(items("text/plain", b"https://e.com/z.zip\n"), vec![PathBuf::from(
            "https://e.com/z.zip"
        )]);
    }
}
