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
    dm_shell::app::lookups::probe_later(
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
