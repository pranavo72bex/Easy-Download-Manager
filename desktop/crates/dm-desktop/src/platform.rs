use super::*;

/// The desktop's own services.
pub(super) struct Desktop;

impl Platform for Desktop {
    /// Only when the setting and the system disagree — or when the login
    /// item starts another copy of dm that is gone (the app moved, or was
    /// removed and installed elsewhere) and this one may claim it
    /// (`autostart::takes_over`, `autostart::may_claim`: a release build,
    /// not a test's dm). A copy still there keeps it, so two installed
    /// copies do not take it from each other at every launch. Registering
    /// writes the path of the running binary, so a copy that will not be
    /// there at the next login, or that macOS keeps from starting there, is
    /// refused (`autostart::refused_at_login`). Not being able to read the
    /// state is fatal only for registering: a login item that cannot be
    /// read must still be one a person can get rid of.
    fn set_login_item(&self, wanted: bool) -> Result<(), String> {
        // Linux's entry is dm's own: see `autostart`.
        if cfg!(target_os = "linux") {
            return dm_desktop::autostart::set(wanted);
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if wanted {
            let home = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            if let Some(why) = dm_desktop::autostart::refused_at_login(
                &exe,
                &home,
                cfg!(target_os = "macos"),
                dm_shell::register::mounted_read_only,
            ) {
                return Err(why);
            }
        }
        let launcher = login_launcher(&exe)?;
        match launcher.is_enabled() {
            Ok(now) if now == wanted => {
                let gone = wanted
                    && login_item_program().is_some_and(|found| {
                        dm_desktop::autostart::takes_over(
                            &found,
                            &exe,
                            dm_desktop::autostart::may_claim(),
                            |p| p.exists(),
                        )
                    });
                if !gone {
                    return Ok(());
                }
                // Enabled for a copy that is gone: written again for this one.
            }
            Ok(_) => {}
            Err(e) if wanted => return Err(e.to_string()),
            Err(_) => {}
        }
        if wanted {
            launcher.enable()
        } else {
            launcher.disable()
        }
        .map_err(|e| e.to_string())
    }

    /// To the Recycle Bin or the Trash, not erased: a file deleted by a
    /// mistake can be put back.
    fn trash(&self, path: &str) -> Result<(), String> {
        trash::delete(path).map_err(|e| e.to_string())
    }

    /// Posted from a thread of its own: on macOS, `show` waits on the
    /// system for about two seconds, and called from the window's thread
    /// every finished download froze the window that long (several at once,
    /// 25 seconds). Found by `scripts/ui-shot.py` and a `sample` of the
    /// stuck process.
    fn notify(&self, title: &str, body: &str) {
        // Held while a fullscreen app, a presentation or Focus Assist has
        // the person's attention, and said when it does not.
        let note = dm_desktop::quiet::Note {
            title: title.to_string(),
            body: body.to_string(),
            path: None,
        };
        if let Some(note) = dm_desktop::quiet::offer(note) {
            say(note);
        }
    }

    fn tick(&self) {
        for note in dm_desktop::quiet::due_notes() {
            say(note);
        }
    }

    /// As `notify`, and a click on it opens the file, where the system
    /// says which was clicked: Linux's notification servers and Windows'
    /// toasts do. macOS says so only to a sender that waits on it in step,
    /// which can hold a thread for as long as the notification is kept, so
    /// there a click brings dm forward, as the system does by itself.
    fn notify_file(&self, title: &str, body: &str, path: &str) {
        let note = dm_desktop::quiet::Note {
            title: title.to_string(),
            body: body.to_string(),
            path: Some(path.to_string()),
        };
        if let Some(note) = dm_desktop::quiet::offer(note) {
            say(note);
        }
    }

    fn chime(&self) {
        // A sound that comes late is only noise: not while the person is busy.
        if !dm_desktop::quiet::quiet_now() {
            dm_desktop::power::chime();
        }
    }

    fn alert(&self) {
        if !dm_desktop::quiet::quiet_now() {
            dm_desktop::power::alert();
        }
    }

    fn clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn reveal(&self, path: &str) -> Result<(), String> {
        shell::reveal_in_folder(path)
    }

    fn open(&self, path: &str) -> Result<(), String> {
        shell::open_file(path)
    }

    fn open_later(&self, path: &str, report: dm_desktop::power::Report) {
        let path = path.to_string();
        std::thread::spawn(move || {
            if let Err(e) = shell::open_file(&path) {
                report(e);
            }
        });
    }

    fn reveal_later(&self, path: &str, report: dm_desktop::power::Report) {
        let path = path.to_string();
        std::thread::spawn(move || {
            // The file manager's own "show this file", selected in its
            // folder, where one answers on the session bus; otherwise the
            // folder alone.
            #[cfg(target_os = "linux")]
            if std::path::Path::new(&path).exists() && show_item(&path) {
                return;
            }
            if let Err(e) = shell::reveal_in_folder(&path) {
                report(e);
            }
        });
    }

    fn power(
        &self,
        action: dm_desktop::power::WhenDone,
        report: dm_desktop::power::Report,
    ) -> Result<(), String> {
        dm_desktop::power::run(action, report)
    }

    fn choose_app(&self) -> Option<std::path::PathBuf> {
        let dialog = rfd::FileDialog::new().set_title("Open with");
        #[cfg(target_os = "macos")]
        let dialog = dialog
            .set_directory("/Applications")
            .add_filter("Applications", &["app"]);
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Apps", &["exe"]);
        dialog.pick_file()
    }

    fn open_with(&self, path: &str, app: &std::path::Path) -> Result<(), String> {
        shell::open_with(path, app)
    }

    fn choose_folder(&self) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    /// On Linux the chooser is the desktop portal's, else zenity's; with
    /// neither, rfd answers as if cancelled. Elsewhere the system has one.
    fn can_choose_folder(&self) -> bool {
        #[cfg(target_os = "linux")]
        return dm_desktop::backend::chooser_here(
            file_chooser_portal(),
            std::env::var_os("PATH").as_deref(),
        );
        #[cfg(not(target_os = "linux"))]
        true
    }

    fn choose_file(&self, save: bool, suggested: &str) -> Option<std::path::PathBuf> {
        // What is asked for decides the filter: a settings file is JSON,
        // and was hidden behind the download lists' filter.
        let dialog = if suggested.ends_with(".json") {
            rfd::FileDialog::new().add_filter("dm settings", &["json"])
        } else {
            rfd::FileDialog::new().add_filter("Download lists", &["dmlist", "txt"])
        };
        if save {
            dialog.set_file_name(suggested).save_file()
        } else {
            dialog.pick_file()
        }
    }
}

/// Starts the system's drag of finished files out of the window, into
/// Finder or Explorer, a mail or a chat. Called while the pointer is down,
/// from the frame that saw the drag begin.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn drag_out(
    frame: &eframe::Frame,
    files: Vec<std::path::PathBuf>,
) -> Result<(), String> {
    drag::start_drag(
        frame,
        drag::DragItem::Files(files),
        drag::Image::Raw(DRAG_ICON.to_vec()),
        |_, _| {},
        drag::Options::default(),
    )
    .map_err(|e| format!("the file could not be dragged out: {e}"))
}

/// On Linux the drag library needs a GTK window, and dm's is not one: on
/// X11 dm speaks the drag protocol itself (`xdnd`); on Wayland the drag has
/// to begin on winit's own connection, with the press's serial, so dm's
/// patched winit starts it (`dm_request_drag_out`).
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn drag_out(
    frame: &eframe::Frame,
    files: Vec<std::path::PathBuf>,
) -> Result<(), String> {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    #[cfg(target_os = "linux")]
    if let Ok(handle) = frame.display_handle() {
        match handle.as_raw() {
            RawDisplayHandle::Xlib(xlib) => {
                let display = xlib.display.map_or(std::ptr::null_mut(), |d| d.as_ptr());
                return dm_desktop::xdnd::start(display, files);
            }
            RawDisplayHandle::Wayland(_) => {
                winit::platform::wayland::dm_request_drag_out(dm_desktop::xdnd::uri_list(&files));
                return Ok(());
            }
            _ => {}
        }
    }
    let _ = (frame, files);
    Err(
        dm_desktop::i18n::t("Dragging out isn't available here. Use right-click › Open folder.")
            .into(),
    )
}

/// What a dragged file looks like under the pointer: dm's icon, small.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) const DRAG_ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// Shows `path` selected in its folder through the freedesktop file
/// manager interface, which the usual file managers answer (and are
/// started for). False where none is there to ask.
#[cfg(target_os = "linux")]
pub(super) fn show_item(path: &str) -> bool {
    const NAME: &str = "org.freedesktop.FileManager1";
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let there = zbus::blocking::fdo::DBusProxy::new(&connection).is_ok_and(|bus| {
        let Ok(name) = zbus::names::BusName::try_from(NAME) else {
            return false;
        };
        bus.name_has_owner(name).unwrap_or(false)
            || bus
                .list_activatable_names()
                .is_ok_and(|names| names.iter().any(|n| n.as_str() == NAME))
    });
    if !there {
        return false;
    }
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination(NAME)
        .and_then(|b| b.path("/org/freedesktop/FileManager1"))
        .and_then(|b| b.interface(NAME))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    let Ok(proxy) = proxy else {
        return false;
    };
    proxy
        .call::<_, _, ()>("ShowItems", &(vec![dm_desktop::format::file_uri(path)], ""))
        .is_ok()
}

/// Whether the desktop portal offers its file chooser on the session bus.
#[cfg(target_os = "linux")]
pub(super) fn file_chooser_portal() -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination("org.freedesktop.portal.Desktop")
        .and_then(|b| b.path("/org/freedesktop/portal/desktop"))
        .and_then(|b| b.interface("org.freedesktop.portal.FileChooser"))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    proxy.is_ok_and(|p| p.get_property::<u32>("version").is_ok())
}

/// Says a toast: one whose click opens a finished file, or a plain one.
fn say(note: dm_desktop::quiet::Note) {
    let dm_desktop::quiet::Note { title, body, path } = note;
    let Some(path) = path else {
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .appname("dm")
                .summary(&title)
                .body(&body)
                .show();
        });
        return;
    };
    /// Notifications waited on at once: each holds a thread until it is
    /// clicked or closed, and some servers keep them a long time.
    static WAITING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    const MOST_WAITING: usize = 8;
    std::thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification.appname("dm").summary(&title).body(&body);
        #[cfg(all(unix, not(target_os = "macos")))]
        if !path.is_empty() {
            // The body's click, as the servers name it, and a button for
            // the folder: Open File and Open Folder on the toast itself.
            notification.action("default", "Open");
            notification.action("folder", "Open folder");
        }
        let Ok(handle) = notification.show() else {
            return;
        };
        #[cfg(target_os = "macos")]
        let _ = (handle, &path, &WAITING, MOST_WAITING);
        #[cfg(not(target_os = "macos"))]
        {
            use std::sync::atomic::Ordering;
            if path.is_empty() || WAITING.fetch_add(1, Ordering::AcqRel) >= MOST_WAITING {
                WAITING.fetch_sub(1, Ordering::AcqRel);
                return;
            }
            let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                if response.is_default_action() {
                    let _ = shell::open_file(&path);
                } else if matches!(response, notify_rust::NotificationResponse::Action(key) if key == "folder")
                {
                    let _ = shell::reveal_in_folder(&path);
                }
            });
            WAITING.fetch_sub(1, Ordering::AcqRel);
        }
    });
}
