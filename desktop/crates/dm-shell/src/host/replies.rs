// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Two of [`answer`]'s replies: to the extension's hello, and to a page's links.

use super::*;

/// The reply to the extension's hello, `greeting`: what this app can do, said up front.
pub(super) fn hello_reply(home: &Path, greeting: dm_protocol::Greeting) -> Reply {
    // Remembered, so the window stops offering the extension to
    // someone who has it (`browser_seen`).
    mark_browser_seen(home);
    // And what the extension said of itself, so the app can work out what an older
    // extension can do rather than ask it to keep up (`extension_heard`).
    note_extension(home, &greeting);
    Reply::Hello(Hello {
        engine_version: env!("CARGO_PKG_VERSION").into(),
        // The extension hands a download the browser holds only to an
        // app that says it judges what it is served.
        protocol: Some(dm_protocol::PROTOCOL_VERSION),
        // The site rules the extension adds to its compiled tables: a fix for
        // a site reaches it with the app, not with a store release.
        rules: Some(served::served(home)),
        renews: true,
        // Abilities by name: an extension that does not know a name ignores it.
        features: [
            feature::RENEWS,
            feature::MENUS,
            feature::JUDGES_CATCH,
            feature::LISTS_STREAMS,
            feature::LISTS_DOWNLOADS,
        ]
        .map(String::from)
        .to_vec(),
        // The right-click menu, made by the extension as the app serves it: an item
        // added to the app works with the extension as released.
        menus: Some(dm_protocol::served_menus(&catch_rules(home))),
        // The extension's own words, in the browser's language, say how the app is.
        status: None,
    })
}

/// The reply to a page's links: filed for the app to ask which of them to download, with the
/// app rung, or started where it is not running.
pub(super) fn links_reply(
    home: &Path,
    Batch {
        id,
        page,
        urls,
        cookies,
        texts,
        names,
    }: Batch,
    world: &mut dyn World,
) -> Reply {
    let links: Vec<String> = urls
        .into_iter()
        .filter_map(|u| page_link(&page, &u))
        .take(2000)
        .collect();
    if links.is_empty() {
        Reply::failed(&id, "there are no links here dm can download")
    } else {
        let filed = deposit(
            home,
            &id,
            &Request {
                url: page.clone(),
                referrer: page,
                ask: true,
                // The words of the links sent, no others.
                texts: texts
                    .into_iter()
                    .filter(|(url, _)| links.contains(url))
                    .collect(),
                names: names
                    .into_iter()
                    .filter(|(url, _)| links.contains(url))
                    .collect(),
                links,
                // Scoped to the page's site by the dialog's sign-in.
                cookies,
                ..Request::default()
            },
        );
        match filed {
            // Download links asks in the app's window.
            Ok(_) if world.app_running() => {
                world.ring();
                Reply::started(&id, true)
            }
            Ok(filed) => match world.launch() {
                Ok(()) => Reply::started(&id, true),
                // Taken back out: the browser keeps it, and a dm
                // started later must not download it a second time.
                Err(e) => {
                    // Only one this hand-off filed: the same request
                    // already waiting was answered Started before.
                    if filed.new {
                        let _ = std::fs::remove_file(&filed.path);
                    }
                    Reply::failed(&id, format!("dm could not be started to take it: {e}"))
                }
            },
            Err(e) => Reply::failed(&id, format!("dm could not take it: {e}")),
        }
    }
}
