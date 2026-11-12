// dm: files dragged out of a window on X11, into a file manager, a mail or a
// chat (`PlatformWindow::start_external_drag`, which upstream has on macOS and
// Wayland only). XDND is spoken as the drag's source on a connection of its
// own, on a thread of its own, so the window's event loop goes on drawing
// while the pointer is out: the press's grab the window's connection holds is
// let go of, the pointer taken on the root window, the window under it told a
// file is coming and asked whether it takes one, and on release it is handed
// the files as `text/uri-list`. When the drag is over, the window that started
// it is sent `_DM_DRAG_ENDED`, which the client turns into
// `FileDropEvent::Ended`, as the other systems end a platform drag.

use std::time::{Duration, Instant};

use gpui::FileDragPaths;
use x11rb::{
    CURRENT_TIME, NONE,
    connection::Connection,
    protocol::{
        Event,
        xinput::ConnectionExt as _,
        xproto::{
            self, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask,
            GrabMode, GrabStatus, PropMode, SelectionNotifyEvent, WindowClass,
        },
    },
    wrapper::ConnectionExt as _,
    xcb_ffi::XCBConnection,
};

/// The master pointer, whose XInput 2 grab the press that began the drag
/// holds on the window's connection.
const MASTER_POINTER: u16 = 2;

/// The `text/uri-list` a drop hands over: each file as a `file://` address.
pub(crate) fn uri_list(paths: &FileDragPaths) -> String {
    paths
        .entries()
        .iter()
        .filter_map(|(path, _)| url::Url::from_file_path(path).ok())
        .fold(String::new(), |mut list, url| {
            list.push_str(url.as_str());
            list.push_str("\r\n");
            list
        })
}

/// Starts the drag of `list` out of `from`, whose connection `window_xcb`
/// holds the press's grab. False when the display cannot be reached, so the
/// drag stays inside the window.
pub(crate) fn start(window_xcb: &XCBConnection, from: xproto::Window, list: String) -> bool {
    if list.is_empty() {
        return false;
    }
    let Ok((conn, screen)) = XCBConnection::connect(None) else {
        log::warn!("drag out: the display could not be reached");
        return false;
    };
    // Both grabs the press made: the core pointer's and XInput 2's.
    window_xcb.ungrab_pointer(CURRENT_TIME).ok();
    window_xcb
        .xinput_xi_ungrab_device(CURRENT_TIME, MASTER_POINTER)
        .ok();
    window_xcb.flush().ok();
    std::thread::Builder::new()
        .name("drag-out".into())
        .spawn(move || {
            if let Err(e) = drag(&conn, screen, &list) {
                log::warn!("drag out: {e}");
            }
            ended(&conn, from);
        })
        .is_ok()
}

/// Tells the window the drag began in that it is over.
fn ended(conn: &XCBConnection, to: xproto::Window) {
    let Ok(atom) = atom(conn, "_DM_DRAG_ENDED") else {
        return;
    };
    send(conn, to, atom, [0; 5]).ok();
}

type Failure = Box<dyn std::error::Error>;

fn atom(conn: &XCBConnection, name: &str) -> Result<u32, Failure> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
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

fn atoms(conn: &XCBConnection) -> Result<Atoms, Failure> {
    Ok(Atoms {
        aware: atom(conn, "XdndAware")?,
        selection: atom(conn, "XdndSelection")?,
        enter: atom(conn, "XdndEnter")?,
        leave: atom(conn, "XdndLeave")?,
        position: atom(conn, "XdndPosition")?,
        status: atom(conn, "XdndStatus")?,
        drop: atom(conn, "XdndDrop")?,
        finished: atom(conn, "XdndFinished")?,
        copy: atom(conn, "XdndActionCopy")?,
        uri_list: atom(conn, "text/uri-list")?,
    })
}

/// The XDND-aware window under the pointer, and its protocol version: down
/// from the root through the windows the point is in, to the first that
/// says it takes drops (a window manager's frame does not; the program's
/// window inside it does).
fn target_at(
    conn: &XCBConnection,
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

fn send(conn: &XCBConnection, to: u32, kind: u32, data: [u32; 5]) -> Result<(), Failure> {
    let event = ClientMessageEvent::new(32, to, kind, data);
    conn.send_event(false, to, EventMask::NO_EVENT, event)?;
    conn.flush()?;
    Ok(())
}

fn drag(conn: &XCBConnection, screen: usize, list: &str) -> Result<(), Failure> {
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
    // The pointer, for as long as the drag lasts; the window's grab was let
    // go of just before, and that may take a moment to land.
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
                if let Some((window, _)) = target
                    && !waiting
                {
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
            Event::ClientMessage(e) if e.type_ == atoms.status => {
                let data = e.data.as_data32();
                accepted = data.get(1).is_some_and(|f| f & 1 == 1);
                waiting = false;
            }
            Event::ClientMessage(e) if e.type_ == atoms.finished => {
                finished = true;
            }
            Event::ButtonRelease(e) if !dropped => match target {
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
            },
            // Escape: the drag is called off.
            Event::KeyPress(e) if !dropped && e.detail == 9 => {
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
