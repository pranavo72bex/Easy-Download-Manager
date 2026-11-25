// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Each network's own allowance on the Advanced page, under its speed limit (gaps row K5e).

use super::*;

/// A network's own allowance under its speed limit, in the speed limiter's words: at most so
/// many MB in each period of so many hours over it (gaps row K5e). `keys` are its draft's.
pub(super) fn allowance(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    keys: [&'static str; 2],
    on: bool,
) {
    let [mb, hours] = keys;
    let third = mb.starts_with("third");
    for (key, words) in [
        (mb, "Download at most this many MB (0 for no limit)"),
        (hours, "in each period of this many hours"),
    ] {
        let field = input(o, window, cx, key, words, NUMBER, on, false, |o| {
            let s = &o.draft;
            match (third, key == mb) {
                (false, true) => s.second_limit_mb,
                (false, false) => s.second_limit_hours.max(1),
                (true, true) => s.third_limit_mb,
                (true, false) => s.third_limit_hours.max(1),
            }
            .to_string()
        });
        p.setting(t(words), on, field);
    }
}

/// The third network's own speed limit and allowance, under its choice (gaps rows K5b, K5e).
pub(super) fn third_limits(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let words = "Third network's speed limit (KB/s, 0 for none)";
    let field = input(
        o,
        window,
        cx,
        "third_limit_kb",
        words,
        NUMBER,
        true,
        false,
        |o| o.draft.third_limit_kb.to_string(),
    );
    let on = o.draft.advanced.only_through.is_empty();
    p.setting(t(words), on, field);
    allowance(
        o,
        p,
        window,
        cx,
        ["third_limit_mb", "third_limit_hours"],
        on,
    );
}

/// A name of the person's own for the network `interface`, where one is chosen (gaps row K4):
/// said in the progress window and the pop-ups in place of the system's.
pub(super) fn name_field(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    interface: &str,
    words: &'static str,
) {
    if interface.is_empty() {
        return;
    }
    let key = format!("network_names.{interface}");
    let owned = interface.to_string();
    let field = input(o, window, cx, &key, words, WIDE, true, false, move |o| {
        o.draft
            .network_names
            .get(&owned)
            .cloned()
            .unwrap_or_default()
    });
    p.labelled(t(words), true, field);
}
