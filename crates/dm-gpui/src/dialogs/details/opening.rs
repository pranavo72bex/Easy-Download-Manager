// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How Download details opens (`open_now`): the row begun at once and the
//! server asked before the window is made, what is done once it is, and
//! what a window that will not open leaves to undo.

use super::*;

/// Begun at once while it asks, where Options says so, before the window
/// is made: its first request then does not wait for a new window (its
/// Direct3D or Metal surface, its fields) to be drawn.
pub(super) fn begun_early(
    backend: &Backend,
    settings: &Settings,
    urls: &[String],
    first: &str,
    handed: &Handed,
) -> Option<Id> {
    let form = Form {
        quality: handed.quality.clone(),
        post: handed.post.clone(),
        ..Form::default()
    };
    let youtube = urls.len() == 1 && dm_core::youtube::video_id(first).is_some();
    file_info::begins_early(settings, urls, &form, youtube)
        .then(|| {
            backend
                .add_with(
                    first,
                    dm_core::AddOptions {
                        referrer: handed.referrer.clone(),
                        title_hint: Some(handed.title.clone()).filter(|t| !t.trim().is_empty()),
                        cookies: handed.cookies.clone(),
                        cookies_once: handed.cookies_once,
                        headers: handed.headers.clone(),
                        keep_no_record: handed.keep_no_record,
                        ..dm_core::AddOptions::default()
                    },
                )
                .ok()
        })
        .flatten()
}

/// Once the dialog is made: Start now given the keyboard, the server's
/// answer heard, a video's qualities asked, the ZIP's files drawn as they
/// are read, and the list read again where a row was begun early.
pub(super) fn first_moves(
    details: &Entity<Details>,
    view: &WeakEntity<Downloads>,
    told: Option<async_channel::Receiver<Probed>>,
    zip_told: async_channel::Receiver<()>,
    early: Option<Id>,
    window: &mut Window,
    cx: &mut App,
) {
    // Start now has the keyboard.
    details.update(cx, |d, cx| window.focus(&d.focus, cx));
    hear_server(details, told, window, cx);
    // A YouTube video's qualities are asked at once, from its own
    // data; a page's once the server says it is one.
    details.update(cx, |d, cx| d.ask_choices(window, cx));
    let weak = details.downgrade();
    window
        .spawn(cx, async move |cx| {
            while zip_told.recv().await.is_ok() {
                if weak.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        })
        .detach();
    if early.is_some() {
        let _ = view.update(cx, |v, cx| {
            v.read();
            cx.notify();
        });
    }
}

/// As Cancel ends it: the row begun for it goes, and the next
/// hand-off is not held back behind one that is never asked.
pub(super) fn never_opened(
    early: Option<Id>,
    gone_with: &Backend,
    gone_view: &WeakEntity<Downloads>,
    was_handed: bool,
    cx: &mut App,
) {
    if let Some(id) = early {
        gone_with.remove_later(id);
    }
    let _ = gone_view.update(cx, |view, cx| {
        if was_handed {
            view.handed_closed();
        }
        view.read();
        cx.notify();
    });
}

/// The server asked what it calls the file and how big it is, once, as the
/// dialog opens (not a torrent's: what the server says of a .torrent is the
/// metainfo's few kilobytes and its file name). Asked before the dialog's
/// window is made, so the request is not held up by it; `None` where there
/// is nothing to ask, which is known at once.
pub(super) fn ask_server(
    backend: &Backend,
    url: &str,
    count: usize,
    remux: bool,
    handed: &Handed,
) -> Option<async_channel::Receiver<Probed>> {
    let web = url.starts_with("http://") || url.starts_with("https://");
    if count != 1 || !web || dm_core::is_torrent(url) {
        return None;
    }
    let (tell, told) = async_channel::bounded(1);
    // A typed address tries its https:// form first (gaps row H6); one a browser handed over
    // is the browser's, as it was sent.
    let probe = if handed.handed {
        dm_shell::app::lookups::probe_later
    } else {
        dm_shell::app::lookups::probe_upgrading
    };
    probe(
        backend,
        url.to_string(),
        handed.referrer.clone(),
        handed.sign_in(&backend.settings(), url),
        remux,
        move |found| {
            let _ = tell.try_send(found);
        },
    );
    Some(told)
}

/// The dialog told what the server answered (`ask_server`), when it does;
/// at once where nothing was asked.
fn hear_server(
    details: &Entity<Details>,
    told: Option<async_channel::Receiver<Probed>>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(told) = told else {
        details.update(cx, |d, cx| d.probed(Probed::default(), window, cx));
        return;
    };
    let details = details.downgrade();
    window
        .spawn(cx, async move |cx| {
            if let Ok(found) = told.recv().await {
                let _ = details.update_in(cx, |d, window, cx| d.probed(found, window, cx));
            }
        })
        .detach();
}

/// Opens Download details for `urls` typed in Add URL (several: a batch,
/// in one).
pub fn open(view: WeakEntity<Downloads>, urls: Vec<String>, cx: &mut App) {
    // After the update that asked: the main window's view is read.
    cx.defer(move |cx| open_now(view, urls, Handed::default(), None, cx));
}

/// Opens Download details for a download the browser handed over, with
/// what the browser sent: the page, the sign-in, the quality picked in its
/// panel and its "Part" line, the browser's name.
pub fn open_handed(view: WeakEntity<Downloads>, request: dm_shell::host::Request, cx: &mut App) {
    // The form for the address it was sent to, before that is moved below.
    let post = dm_shell::shell::posted(&request.url, request.post.as_ref());
    let handed = Handed {
        handed: true,
        referrer: request.referrer,
        title: request.title,
        quality: request.quality,
        sound: request.sound,
        label: request.label,
        cookies: request.cookies,
        cookies_once: request.cookies_once,
        headers: request.headers,
        post,
        browser: request.browser,
        caught: request.caught,
        keep_no_record: request.keep_no_record,
    };
    // The panel's "Part" line: the dialog opens with the part on.
    let part = (!(request.from.is_empty() && request.to.is_empty())).then(|| {
        let from = if request.from.is_empty() && !request.to.is_empty() {
            "0:00".to_string()
        } else {
            request.from
        };
        (from, request.to, request.save_name.unwrap_or_default())
    });
    let urls = vec![request.url];
    cx.defer(move |cx| open_now(view, urls, handed, part, cx));
}

fn open_now(
    view: WeakEntity<Downloads>,
    urls: Vec<String>,
    handed: Handed,
    part: Option<(String, String, String)>,
    cx: &mut App,
) {
    let Some(downloads) = view.upgrade() else {
        return;
    };
    let backend = downloads.read(cx).backend.clone();
    let settings = backend.settings();
    let queues = backend.queues();
    let first = urls.first().cloned().unwrap_or_default();
    let early = opening::begun_early(&backend, &settings, &urls, &first, &handed);
    // What a window that will not open leaves to undo.
    let (gone_with, gone_view, was_handed) = (backend.clone(), view.clone(), handed.handed);
    // What the server calls the file and how big it is, asked now too,
    // before the window is made; the dialog hears the answer once it is.
    let remux = settings.downloads.remux_ts;
    let told = opening::ask_server(&backend, &first, urls.len(), remux, &handed);
    let height = if urls.len() == 1 { 360.0 } else { 300.0 };
    let bounds = Bounds::centered(None, size(px(600.0), px(height)), cx);
    let opened = crate::window::screen::open_window(
        cx,
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(t("Download details").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let hint = file_info::default_folder_words(&settings);
            let titled = handed.quality.is_some();
            let folder = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
            let name = cx.new(|cx| {
                InputState::new(window, cx).placeholder(file_info::name_hint(&first, titled, remux))
            });
            let note = cx.new(|cx| InputState::new(window, cx));
            let part_from = cx.new(|cx| InputState::new(window, cx).placeholder(t("m:ss")));
            let part_to = cx.new(|cx| InputState::new(window, cx).placeholder(t("m:ss")));
            let mut part_on = false;
            if let Some((from, to, save_name)) = &part {
                part_on = true;
                part_from.update(cx, |f, cx| f.set_value(from.clone(), window, cx));
                part_to.update(cx, |f, cx| f.set_value(to.clone(), window, cx));
                if !save_name.is_empty() {
                    name.update(cx, |n, cx| n.set_value(save_name.clone(), window, cx));
                }
            }
            let screen = Screen::default();
            Screens::register(window.window_handle(), screen.clone(), true, cx);
            // A ZIP's files, read once from the end of the archive.
            let web = first.starts_with("http://") || first.starts_with("https://");
            let (zip_tell, zip_told) = async_channel::bounded::<()>(1);
            let zip = (urls.len() == 1 && web && zip_list::is_zip_address(&first)).then(|| {
                dm_shell::app::files::list_zip_at(
                    &backend,
                    first.clone(),
                    handed.page(),
                    handed.sign_in(&backend.settings(), &first),
                    move || {
                        let _ = zip_tell.try_send(());
                    },
                )
            });
            let details = cx.new(|cx| {
                let focus = cx.focus_handle();
                // Return in a field is Start now; what is typed is weighed
                // again (the name's clash).
                for input in [&folder, &name, &note, &part_from, &part_to] {
                    cx.subscribe_in(input, window, |this: &mut Details, _, event, window, cx| {
                        match event {
                            InputEvent::PressEnter { .. } => this.add(false, window, cx),
                            InputEvent::Change => {
                                this.offer_folder(window, cx);
                                cx.notify();
                            }
                            _ => {}
                        }
                    })
                    .detach();
                }
                Details {
                    view: view.clone(),
                    backend: backend.clone(),
                    settings: settings.clone(),
                    urls: urls.clone(),
                    quality: handed.quality.clone(),
                    label: handed.label.clone(),
                    handed: handed.clone(),
                    folder: folder.clone(),
                    name: name.clone(),
                    note,
                    part_from,
                    part_to,
                    probed: None,
                    free: None,
                    folder_offered: false,
                    always_here: false,
                    queue: dm_core::MAIN_QUEUE.into(),
                    queues,
                    delete_after: None,
                    unpack: false,
                    replace: false,
                    keep_no_record: handed.keep_no_record,
                    choices: None,
                    choices_asked: false,
                    part: part_on,
                    shape: None,
                    gif: false,
                    zip,
                    taken: None,
                    taking: String::new(),
                    early,
                    clash_seen: None,
                    clash_looking: false,
                    says: None,
                    done: false,
                    focus,
                    screen,
                }
            });
            opening::first_moves(&details, &view, told, zip_told, early, window, cx);
            crate::widgets::native::close_is_cancel(&details, window, cx);
            cx.new(|cx| gpui_component::Root::new(details, window, cx))
        },
    );
    if let Err(e) = opened {
        dm_core::note!(error, "dm-gpui: Download details could not be opened: {e}");
        opening::never_opened(early, &gone_with, &gone_view, was_handed, cx);
    }
}
