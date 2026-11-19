// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' Downloads page.

use super::*;

pub(super) fn downloads(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let connections = input(
        o,
        window,
        cx,
        "connections",
        "Connections per file",
        NUMBER,
        true,
        false,
        |o| o.draft.connections.to_string(),
    );
    p.labelled(t("Connections per file"), true, connections);
    p.note(t("How many parts of a file dm fetches at once. More is faster on most sites, but some limit it."));
    // The presets (B56): one sets the box; the one matching it is chosen.
    let now = o.draft.connections;
    let presets = options::CONNECTION_PRESETS.iter().map(|(words, count)| {
        let count = *count;
        let on = now == count as usize;
        div()
            .id(SharedString::from(format!("preset-{count}")))
            .role(gpui::Role::RadioButton)
            .aria_label(SharedString::from(t(words)))
            .aria_toggled(if on {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .px(px(space::M))
            .h(px(20.0))
            .flex()
            .items_center()
            .rounded(px(native::RADIUS))
            .text_size(px(native::SMALL_TEXT))
            .when(on, |d| d.bg(theme.accent).text_color(theme.on_accent))
            .when(!on, |d| d.bg(theme.control))
            .child(screen.label(t(words)))
            .on_click(cx.listener(move |o, _, _, cx| {
                o.draft.connections = count as usize;
                o.forget_field("connections");
                cx.notify();
            }))
    });
    p.add(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(space::S))
            .child(native::note(t("Presets:"), theme.muted, &screen))
            .children(presets),
    );
    let files = input(
        o,
        window,
        cx,
        "concurrency",
        "Files at once",
        NUMBER,
        true,
        false,
        |o| o.draft.concurrency.to_string(),
    );
    p.labelled(t("Files at once"), true, files);
    p.note(t("How many files download at the same time."));
    let qualities = dm_desktop_base::file_info::QUALITIES;
    let quality_at = qualities
        .iter()
        .position(|(v, _)| *v == o.draft.stream_quality);
    let quality_words = quality_at
        .map(|i| t(qualities[i].1))
        .unwrap_or_else(|| t("Best available"));
    p.labelled(
        t("Video quality"),
        true,
        popup::button(
            "quality",
            quality_words.into(),
            qualities
                .iter()
                .map(|(_, l)| SharedString::from(t(l)))
                .collect(),
            quality_at,
            theme,
            &screen,
        ),
    );
    let s = o.draft.clone();
    check(
        p,
        cx,
        "start-queue-on-launch",
        t("Start the Main queue when dm opens"),
        s.start_queue_on_launch,
        true,
        |s| s.start_queue_on_launch = !s.start_queue_on_launch,
    );
    p.hint(t(
        "Downloads waiting in the Main queue start without a click.",
    ));
    check(
        p,
        cx,
        "subtitles",
        t("Embed subtitles in videos from player pages"),
        s.subtitles,
        true,
        |s| s.subtitles = !s.subtitles,
    );
    p.hint(t(
        "A video's subtitles are saved inside its file, to turn on in your player.",
    ));
    check(
        p,
        cx,
        "remux-ts",
        t("Save streams that arrive as .ts as MP4"),
        s.remux_ts,
        true,
        |s| s.remux_ts = !s.remux_ts,
    );
    p.hint(t(
        "Streamed videos are saved as MP4 files, which more players open.",
    ));
    // Queues with their own limit, one limit a queue.
    p.words(t("Queues with their own limit:"));
    for i in 0..s.queue_limits.len() {
        let offered = limit_queues(&o.queues, &s, i);
        let queue = s.queue_limits[i].queue.clone();
        let at = offered.iter().position(|q| *q == queue);
        let pick = popup::button_of(
            "queue-limit",
            i,
            dm_desktop_base::i18n::name(&queue).into(),
            offered
                .iter()
                .map(|q| dm_desktop_base::i18n::name(q).into())
                .collect(),
            at,
            theme,
            &screen,
        );
        let files = input(
            o,
            window,
            cx,
            &format!("queue_limits.{i}.files"),
            "Files at once",
            NUMBER,
            true,
            false,
            move |o| o.draft.queue_limits[i].files.to_string(),
        );
        let delete = button(p, ("delete-queue-limit", i), t("Delete queue limit")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.queue_limits.len() {
                    o.draft.queue_limits.remove(i);
                    o.forget("queue_limits");
                }
                cx.notify();
            }),
        );
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Queue")))
                .child(pick)
                .child(screen.label(t("Files at once")))
                .child(files)
                .child(delete),
        );
    }
    let unlimited = o
        .queues
        .iter()
        .find(|q| !s.queue_limits.iter().any(|l| &l.queue == *q))
        .cloned();
    let add_limit = button(p, "add-queue-limit", t("Add a queue limit")).on_click(cx.listener(
        move |o, _, _, cx| {
            if let Some(queue) = unlimited.clone() {
                o.draft
                    .queue_limits
                    .push(dm_shell::settings::QueueLimit { queue, files: 1 });
            }
            cx.notify();
        },
    ));
    p.add(native::enabled(
        o.queues
            .iter()
            .any(|q| !s.queue_limits.iter().any(|l| &l.queue == q)),
        div().flex().child(add_limit),
    ));
    // Queues that start on their own, each at its own time.
    p.words(t("Queues that start at a set time:"));
    for i in 0..s.queue_starts.len() {
        let queue = s.queue_starts[i].queue.clone();
        let pick = popup::button_of(
            "queue-start",
            i,
            dm_desktop_base::i18n::name(&queue).into(),
            o.queues
                .iter()
                .map(|q| dm_desktop_base::i18n::name(q).into())
                .collect(),
            o.queues.iter().position(|q| *q == queue),
            theme,
            &screen,
        );
        // Hour and minute pop-ups, which hold only times (gaps row U32): typed, a wrong one
        // was said only beside it.
        let at = native::time_picker_of(
            ("start-hour", "start-minute"),
            i,
            &o.draft.queue_starts[i].at,
            theme,
            &screen,
        );
        let delete = button(p, ("delete-start", i), t("Delete start time")).on_click(cx.listener(
            move |o, _, _, cx| {
                if i < o.draft.queue_starts.len() {
                    o.draft.queue_starts.remove(i);
                    o.forget("queue_starts");
                }
                cx.notify();
            },
        ));
        p.add(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(native::LABEL_GAP))
                .child(screen.label(t("Queue")))
                .child(pick)
                .child(screen.label(t("Start at")))
                .child(at)
                .child(delete),
        );
    }
    let add_start =
        button(p, "add-start", t("Add a start time")).on_click(cx.listener(|o, _, _, cx| {
            let queue = o
                .queues
                .first()
                .cloned()
                .unwrap_or_else(|| dm_core::MAIN_QUEUE.to_string());
            o.draft.queue_starts.push(dm_shell::settings::QueueStart {
                queue,
                at: "02:00".into(),
            });
            cx.notify();
        }));
    p.add(div().flex().child(add_start));
    folder(
        o,
        p,
        window,
        cx,
        "watch_folder",
        "Watch a folder: links (.url, .webloc or a text file of links) and .torrent files saved there are added, then moved to its \"done\" folder. Leave it empty to turn this off.",
        true,
        |o| o.draft.watch_folder.clone(),
        |o, folder| o.draft.watch_folder = folder,
    );
    let record = input(
        o,
        window,
        cx,
        "record_minutes",
        "Stop recording after (minutes, 0 for no limit)",
        NUMBER,
        true,
        false,
        |o| o.draft.record_minutes.to_string(),
    );
    p.labelled(
        t("Stop recording after (minutes, 0 for no limit)"),
        true,
        record,
    );
    let recheck = input(
        o,
        window,
        cx,
        "recheck_hours",
        "Recheck every (hours, 0 for manual)",
        NUMBER,
        true,
        false,
        |o| o.draft.recheck_hours.to_string(),
    );
    p.labelled(t("Recheck every (hours, 0 for manual)"), true, recheck);
    p.words(t("When a download finishes, run this command (a virus checker or a script). The file's path is added at the end:"));
    let command = input(
        o,
        window,
        cx,
        "on_complete",
        "Command",
        400.0,
        true,
        false,
        |o| o.draft.on_complete.clone(),
    );
    p.labelled(t("Command"), true, command);
}
