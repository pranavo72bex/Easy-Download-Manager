//! Finished files dragged out of the window on Linux (X11), into a file
//! manager, a browser, a mail or a chat.
//!
//! On macOS and Windows the `drag` crate starts the system's drag from the
//! window; on Linux it needs a GTK window, and dm's is not one. X11's drag
//! protocol (XDND) is spoken here instead, as the drag's source, on a
//! connection of dm's own: the pointer is taken from winit (which holds it
//! from the press that started the drag), the window under it is told a
//! file is coming and asked whether it takes one, and on release it is
//! handed the files as `text/uri-list`. Wayland has no such route outside
//! winit, which offers none.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    self, AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, GrabMode,
    GrabStatus, PropMode, SelectionNotifyEvent, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

/// Set when a drag has ended, for the window to be told its button is up:
/// winit never sees the release, which went to this drag's grab.
pub static ENDED: AtomicBool = AtomicBool::new(false);

/// The `text/uri-list` a drop hands over: each file as a `file://` address.
pub fn uri_list(files: &[PathBuf]) -> String {
    let mut list = String::new();
    for file in files {
        let path = file.to_string_lossy();
        list.push_str("file://");
        for byte in path.bytes() {
            match byte {
                b'/' | b'-' | b'_' | b'.' | b'~' => list.push(byte as char),
                b if b.is_ascii_alphanumeric() => list.push(byte as char),
                b => list.push_str(&format!("%{b:02X}")),
            }
        }
        list.push_str("\r\n");
    }
    list
}

/// Lets go of the pointer winit holds on its own connection (`display`, an
/// Xlib `Display*`), so this drag can take it.
fn release_winit(display: *mut std::ffi::c_void) {
    if display.is_null() {
        return;
    }
    if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
        // SAFETY: `display` is not null (checked above) and is winit's live Xlib
        // `Display*` for the window being dragged from, which the caller keeps
        // for the call. `XUngrabPointer` takes a display and a time (0 is
        // CurrentTime) and `XFlush` a display; the library `xlib` opened stays
        // loaded while they run.
        unsafe {
            (xlib.XUngrabPointer)(display.cast(), 0);
            (xlib.XFlush)(display.cast());
        }
    }
    // winit reads the pointer through XInput 2, whose own grab of the
    // master pointer (device 2) is let go of too.
    if let Ok(xi) = x11_dl::xinput2::XInput2::open() {
        // SAFETY: as above, `display` is winit's live `Display*`;
        // `XIUngrabDevice` takes a display, a device id (2, the master pointer)
        // and a time (0, CurrentTime), and the library `xi` opened stays loaded.
        unsafe {
            (xi.XIUngrabDevice)(display.cast(), 2, 0);
        }
        if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
            // SAFETY: as above.
            unsafe { (xlib.XFlush)(display.cast()) };
        }
    }
}

/// Starts dragging `files` out, from the window whose Xlib display winit
/// uses. The drag runs on a thread of its own until the button is let go.
pub fn start(display: *mut std::ffi::c_void, files: Vec<PathBuf>) -> Result<(), String> {
    let (conn, screen) = RustConnection::connect(None)
        .map_err(|e| format!("the drag could not reach the display: {e}"))?;
    release_winit(display);
    std::thread::Builder::new()
        .name("dm-drag-out".into())
        .spawn(move || {
            let _ = drag(&conn, screen, &files);
            ENDED.store(true, Ordering::Relaxed);
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

struct Atoms {
    aware: u32,
    selection: u32,
    enter: u32,
    leave: u32,
    position: u32,
    status: u32,
    drop: u32,
    finished: u32,
    copy: u32,
    uri_list: u32,
}

fn atoms(conn: &RustConnection) -> Result<Atoms, Box<dyn std::error::Error>> {
    let atom = |name: &str| -> Result<u32, Box<dyn std::error::Error>> {
        Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    };
    Ok(Atoms {
        aware: atom("XdndAware")?,
        selection: atom("XdndSelection")?,
        enter: atom("XdndEnter")?,
        leave: atom("XdndLeave")?,
        position: atom("XdndPosition")?,
        status: atom("XdndStatus")?,
        drop: atom("XdndDrop")?,
        finished: atom("XdndFinished")?,
        copy: atom("XdndActionCopy")?,
        uri_list: atom("text/uri-list")?,
    })
}

/// The XDND-aware window under the pointer, and its protocol version:
/// down from the root through the windows the point is in, to the first
/// that says it takes drops (a window manager's frame does not; the
/// program's window inside it does).
fn target_at(
    conn: &RustConnection,
    root: u32,
    skip: u32,
    x: i16,
    y: i16,
    aware: u32,
) -> Option<(u32, u32)> {
    let mut window = root;
    for _ in 0..16 {
        if window != root && window != skip {
            let version = conn
                .get_property(false, window, aware, AtomEnum::ATOM, 0, 1)
                .ok()?
                .reply()
                .ok()?;
            if let Some(v) = version.value32().and_then(|mut v| v.next()) {
                return Some((window, v));
            }
        }
        let child = conn
            .translate_coordinates(root, window, x, y)
            .ok()?
            .reply()
            .ok()?
            .child;
        if child == NONE {
            return None;
        }
        window = child;
    }
    None
}

fn send(
    conn: &RustConnection,
    to: u32,
    kind: u32,
    data: [u32; 5],
) -> Result<(), Box<dyn std::error::Error>> {
    let event = ClientMessageEvent::new(32, to, kind, data);
    conn.send_event(false, to, EventMask::NO_EVENT, event)?;
    conn.flush()?;
    Ok(())
}

fn drag(
    conn: &RustConnection,
    screen: usize,
    files: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    let root = conn.setup().roots.get(screen).ok_or("no screen")?.root;
    let atoms = atoms(conn)?;
    let source = conn.generate_id()?;
    conn.create_window(
        0,
        source,
        root,
        -10,
        -10,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        0,
        &CreateWindowAux::new().override_redirect(1),
    )?;
    conn.map_window(source)?;
    conn.set_selection_owner(source, atoms.selection, CURRENT_TIME)?;
    // The pointer, for as long as the drag lasts; the grab winit had is let
    // go of just before, and the press may take a moment to land.
    let mut grabbed = false;
    for _ in 0..50 {
        let reply = conn
            .grab_pointer(
                false,
                root,
                EventMask::POINTER_MOTION | EventMask::BUTTON_RELEASE,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                NONE,
                NONE,
                CURRENT_TIME,
            )?
            .reply()?;
        if reply.status == GrabStatus::SUCCESS {
            grabbed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !grabbed {
        conn.destroy_window(source)?;
        conn.flush()?;
        return Err("the pointer could not be taken".into());
    }
    let _ = conn.grab_keyboard(false, root, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC);
    conn.flush()?;

    let list = uri_list(files);
    let mut target: Option<(u32, u32)> = None;
    let mut accepted = false;
    let mut waiting = false;
    let mut dropped = false;
    let mut finished = false;
    let started = Instant::now();
    let mut drop_at: Option<Instant> = None;
    loop {
        // Bounded: a drag held for minutes, or a target that never says it
        // is done, ends the drag rather than holding the pointer.
        if started.elapsed() > Duration::from_secs(300)
            || drop_at.is_some_and(|at| at.elapsed() > Duration::from_secs(5))
            || finished
        {
            break;
        }
        let Some(event) = conn.poll_for_event()? else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        match event {
            Event::MotionNotify(e) if !dropped => {
                let under = target_at(conn, root, source, e.root_x, e.root_y, atoms.aware);
                if under.map(|t| t.0) != target.map(|t| t.0) {
                    if let Some((old, _)) = target {
                        send(conn, old, atoms.leave, [source, 0, 0, 0, 0])?;
                    }
                    target = under;
                    accepted = false;
                    waiting = false;
                    if let Some((new, version)) = target {
                        let version = version.min(5);
                        send(
                            conn,
                            new,
                            atoms.enter,
                            [source, version << 24, atoms.uri_list, 0, 0],
                        )?;
                    }
                }
                if let Some((window, _)) = target {
                    if !waiting {
                        let at = ((e.root_x as u32) << 16) | (e.root_y as u16 as u32);
                        send(
                            conn,
                            window,
                            atoms.position,
                            [source, 0, at, e.time, atoms.copy],
                        )?;
                        waiting = true;
                    }
                }
            }
            Event::ClientMessage(e) if e.type_ == atoms.status => {
                let data = e.data.as_data32();
                accepted = data.get(1).is_some_and(|f| f & 1 == 1);
                waiting = false;
            }
            Event::ClientMessage(e) if e.type_ == atoms.finished => {
                finished = true;
            }
            Event::ButtonRelease(e) if !dropped => {
                match target {
                    Some((window, _)) if accepted => {
                        send(conn, window, atoms.drop, [source, 0, e.time, 0, 0])?;
                        dropped = true;
                        drop_at = Some(Instant::now());
                        // The pointer goes back while the target asks.
                        conn.ungrab_pointer(CURRENT_TIME)?;
                        conn.ungrab_keyboard(CURRENT_TIME)?;
                        conn.flush()?;
                    }
                    Some((window, _)) => {
                        send(conn, window, atoms.leave, [source, 0, 0, 0, 0])?;
                        break;
                    }
                    None => break,
                }
            }
            Event::KeyPress(e) if !dropped && e.detail == 9 => {
                // Escape: the drag is called off.
                if let Some((window, _)) = target {
                    send(conn, window, atoms.leave, [source, 0, 0, 0, 0])?;
                }
                break;
            }
            Event::SelectionRequest(e) if e.selection == atoms.selection => {
                let property = if e.target == atoms.uri_list {
                    conn.change_property8(
                        PropMode::REPLACE,
                        e.requestor,
                        e.property,
                        atoms.uri_list,
                        list.as_bytes(),
                    )?;
                    e.property
                } else {
                    NONE
                };
                let reply = SelectionNotifyEvent {
                    response_type: xproto::SELECTION_NOTIFY_EVENT,
                    sequence: 0,
                    time: e.time,
                    requestor: e.requestor,
                    selection: e.selection,
                    target: e.target,
                    property,
                };
                conn.send_event(false, e.requestor, EventMask::NO_EVENT, reply)?;
                conn.flush()?;
            }
            _ => {}
        }
    }
    let _ = conn.ungrab_pointer(CURRENT_TIME);
    let _ = conn.ungrab_keyboard(CURRENT_TIME);
    let _ = conn.destroy_window(source);
    let _ = conn.flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_listed_as_addresses_a_drop_target_reads() {
        let list = uri_list(&[
            PathBuf::from("/home/me/Downloads/report 2026.pdf"),
            PathBuf::from("/home/me/Downloads/naïve.zip"),
        ]);
        assert_eq!(
            list,
            "file:///home/me/Downloads/report%202026.pdf\r\nfile:///home/me/Downloads/na%C3%AFve.zip\r\n"
        );
    }
}
