// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Options' Downloads page, its sections in the page's order, each adding
//! its rows under the one before.

use super::*;

pub(super) fn downloads(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    connections(o, p, window, cx);
    files_and_quality(o, p, window, cx);
    sound_language(o, p);
    let s = o.draft.clone();
    switches(p, cx, &s);
    queue_limits(o, p, window, cx, &s);
    queue_starts(o, p, cx, &s);
    watching_and_after(o, p, window, cx);
    forgetting(o, p, window, cx);
    metered_and_battery(o, p, window, cx);
    // A phone's own lines (gaps row N10): never on a computer's Options.
    #[cfg(target_os = "android")]
    phone_rules::phone_rules(p, cx, &s);
}

/// Connections per file, and its presets.
fn connections(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
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
        |o| o.draft.downloads.connections.to_string(),
    );
    p.setting(t("Connections per file"), true, connections);
    p.hint(t("The most parts of a file EDM fetches at once. It opens more only while they make it faster; some sites limit it."));
    // The presets (B56): one sets the box; the one matching it is chosen.
    let now = o.draft.downloads.connections;
    let presets = options::CONNECTION_PRESETS.iter().map(|(words, count)| {
        let count = *count;
        let on = now == count as usize;
        div()
            .id(SharedString::from(format!("preset-{count}")))
            .map(|c| crate::widgets::native::faced(c, theme.control, on, theme))
            .role(gpui::Role::RadioButton)
            .aria_label(SharedString::from(t(words)))
            .aria_toggled(if on {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .px(px(space::M))
            // A finger's least on a phone (gaps row E26).
            .h(px(if crate::platform::ON_A_PHONE {
                native::CONTROL
            } else {
                20.0
            }))
            .flex()
            .items_center()
            .rounded(px(native::RADIUS))
            .text_size(px(native::SMALL_TEXT))
            .child(screen.label(t(words)))
            .on_click(cx.listener(move |o, _, _, cx| {
                o.draft.downloads.connections = count as usize;
                o.forget_field("connections");
                cx.notify();
            }))
    });
    // The page's width, so the presets wrap where it has no room for them, from the right in a
    // language read so: at 400 points in Arabic the row ran past the window's left edge.
    p.add(
        div()
            .w_full()
            .flex()
            .map(crate::window::mirror::row)
            .flex_wrap()
            .items_center()
            .gap(px(space::S))
            .child(native::note(t("Presets:"), theme.muted, &screen))
            .children(presets),
    );
}

/// Files at once, and the videos' quality.
fn files_and_quality(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let files = input(
        o,
        window,
        cx,
        "concurrency",
        "Files at once",
        NUMBER,
        true,
        false,
        |o| o.draft.downloads.concurrency.to_string(),
    );
    p.setting(t("Files at once"), true, files);
    p.hint(t("How many files download at the same time."));
    let qualities = edm_desktop_base::file_info::QUALITIES;
    let quality_at = qualities
        .iter()
        .position(|(v, _)| *v == o.draft.downloads.stream_quality);
    let quality_words = quality_at
        .map(|i| t(qualities[i].1))
        .unwrap_or_else(|| t("Best available"));
    p.setting(
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
}

/// The language a video's sound is taken in, under its quality (gaps row Y10): the video's own
/// first, then each language by its own name. A value `settings.json` has that the list lacks
/// is shown as itself, and kept until another is picked.
fn sound_language(o: &Options, p: &mut Page) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let now = &o.draft.downloads.sound_language;
    let at = options::sound_language_at(now);
    let words: SharedString = match at {
        Some(0) => t("The video’s own").into(),
        Some(i) => options::SOUND_LANGUAGES
            .get(i - 1)
            .map_or_else(|| now.clone(), |(_, name)| name.to_string())
            .into(),
        None => now.clone().into(),
    };
    let choices: Vec<SharedString> = std::iter::once(SharedString::from(t("The video’s own")))
        .chain(
            options::SOUND_LANGUAGES
                .iter()
                .map(|(_, name)| SharedString::from(*name)),
        )
        .collect();
    p.setting(
        t("Sound language"),
        true,
        popup::button("sound-language", words, choices, at, theme, &screen),
    );
    p.hint(t("Where a video has its sound in several languages, this one is taken; where none speaks it, the video’s own."));
}

/// The switches: the Main queue at launch, subtitles and streams as MP4.
fn switches(p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    check(
        p,
        cx,
        "start-queue-on-launch",
        t("Start the Main queue when EDM opens"),
        s.downloads.start_queue_on_launch,
        true,
        |s| s.downloads.start_queue_on_launch = !s.downloads.start_queue_on_launch,
    );
    p.hint(t(
        "Downloads waiting in the Main queue start without a click.",
    ));
    check(
        p,
        cx,
        "subtitles",
        t("Embed subtitles in videos from player pages"),
        s.downloads.subtitles,
        true,
        |s| s.downloads.subtitles = !s.downloads.subtitles,
    );
    p.hint(t(
        "A video’s subtitles are saved inside its file, to turn on in your player.",
    ));
    check(
        p,
        cx,
        "remux-ts",
        t("Save streams that arrive as .ts as MP4"),
        s.downloads.remux_ts,
        true,
        |s| s.downloads.remux_ts = !s.downloads.remux_ts,
    );
    p.hint(t(
        "Streamed videos are saved as MP4 files, which more players open.",
    ));
}

/// Queues with their own limit, and the button adding one.
fn queue_limits(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
    s: &Settings,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // Queues with their own limit, one limit a queue.
    p.section(t("Queues with their own limit"));
    for i in 0..s.downloads.queue_limits.len() {
        let offered = limit_queues(&o.queues, s, i);
        let queue = s.downloads.queue_limits[i].queue.clone();
        let at = offered.iter().position(|q| *q == queue);
        let pick = popup::button_of(
            "queue-limit",
            i,
            edm_desktop_base::i18n::name(&queue).into(),
            offered
                .iter()
                .map(|q| edm_desktop_base::i18n::name(q).into())
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
            move |o| o.draft.downloads.queue_limits[i].files.to_string(),
        );
        let delete = destructive(p, ("delete-queue-limit", i), t("Delete queue limit")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.downloads.queue_limits.len() {
                    o.draft.downloads.queue_limits.remove(i);
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
        .find(|q| !s.downloads.queue_limits.iter().any(|l| &l.queue == *q))
        .cloned();
    let add_limit = button(p, "add-queue-limit", t("Add a queue limit")).on_click(cx.listener(
        move |o, _, _, cx| {
            if let Some(queue) = unlimited.clone() {
                o.draft
                    .downloads
                    .queue_limits
                    .push(edm_shell::settings::QueueLimit { queue, files: 1 });
            }
            cx.notify();
        },
    ));
    p.add(native::enabled(
        o.queues
            .iter()
            .any(|q| !s.downloads.queue_limits.iter().any(|l| &l.queue == q)),
        div().flex().child(add_limit),
    ));
}

/// Queues that start at a set time, and the button adding one.
fn queue_starts(o: &mut Options, p: &mut Page, cx: &mut Context<Options>, s: &Settings) {
    let (theme, screen) = (p.theme, p.screen.clone());
    // Queues that start on their own, each at its own time.
    p.section(t("Queues that start at a set time"));
    for i in 0..s.downloads.queue_starts.len() {
        let queue = s.downloads.queue_starts[i].queue.clone();
        let pick = popup::button_of(
            "queue-start",
            i,
            edm_desktop_base::i18n::name(&queue).into(),
            o.queues
                .iter()
                .map(|q| edm_desktop_base::i18n::name(q).into())
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
            &o.draft.downloads.queue_starts[i].at,
            theme,
            &screen,
        );
        let delete = destructive(p, ("delete-start", i), t("Delete start time")).on_click(
            cx.listener(move |o, _, _, cx| {
                if i < o.draft.downloads.queue_starts.len() {
                    o.draft.downloads.queue_starts.remove(i);
                    o.forget("queue_starts");
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
                .unwrap_or_else(|| edm_core::MAIN_QUEUE.to_string());
            o.draft
                .downloads
                .queue_starts
                .push(edm_shell::settings::QueueStart {
                    queue,
                    at: "02:00".into(),
                });
            cx.notify();
        }));
    p.add(div().flex().child(add_start));
}

/// The folder watched, recording, rechecking and the command run after a
/// download.
fn watching_and_after(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    folder(
        o,
        p,
        window,
        cx,
        "watch_folder",
        "Watch a folder: links (.url, .webloc or a text file of links) and .torrent files saved there are added, then moved to its “done” folder. Leave it empty to turn this off.",
        true,
        |o| o.draft.downloads.watch_folder.clone(),
        |o, folder| o.draft.downloads.watch_folder = folder,
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
        |o| o.draft.downloads.record_minutes.to_string(),
    );
    p.setting(
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
        |o| o.draft.downloads.recheck_hours.to_string(),
    );
    p.setting(t("Recheck every (hours, 0 for manual)"), true, recheck);
    // A phone runs no command of its own after a download.
    if crate::platform::ON_A_PHONE {
        return;
    }
    p.words(t("When a download finishes, run this command (a virus checker or a script). The file’s path is added at the end:"));
    let command = input(
        o,
        window,
        cx,
        "on_complete",
        "Command",
        400.0,
        true,
        false,
        |o| o.draft.downloads.on_complete.clone(),
    );
    p.labelled(t("Command"), true, command);
}

/// Finished downloads forgotten once they are so many days old, their files kept (gaps row W17).
fn forgetting(o: &mut Options, p: &mut Page, window: &mut Window, cx: &mut Context<Options>) {
    let days = input(
        o,
        window,
        cx,
        "forget_after_days",
        "Forget finished downloads after (days, 0 for never)",
        NUMBER,
        true,
        false,
        |o| o.draft.downloads.forget_after_days.to_string(),
    );
    p.setting(
        t("Forget finished downloads after (days, 0 for never)"),
        true,
        days,
    );
    p.hint(t("A finished download leaves the list once it is that many days old. Its file stays where it was saved."));
}

/// Every download waits on a metered connection or a low battery, as chosen here (gaps row W11):
/// go on, ask or wait on a metered connection, and the battery's level to wait below.
fn metered_and_battery(
    o: &mut Options,
    p: &mut Page,
    window: &mut Window,
    cx: &mut Context<Options>,
) {
    let (theme, screen) = (p.theme, p.screen.clone());
    let at = options::on_metered_at(&o.draft.downloads.on_metered);
    let words: Vec<SharedString> = options::ON_METERED_WORDS
        .iter()
        .map(|w| SharedString::from(t(w)))
        .collect();
    p.setting(
        t("On a metered connection"),
        true,
        popup::button(
            "on-metered",
            words[at].clone(),
            words,
            Some(at),
            theme,
            &screen,
        ),
    );
    p.hint(t("Where the system marks the connection as metered: a phone’s hotspot, a capped plan. Downloads that wait go on by themselves once it is not."));
    let level = input(
        o,
        window,
        cx,
        "battery_below",
        "On battery: wait below (%, 0 for never)",
        NUMBER,
        true,
        false,
        |o| o.draft.downloads.battery_below.to_string(),
    );
    p.setting(t("On battery: wait below (%, 0 for never)"), true, level);
    p.hint(if crate::platform::ON_A_PHONE {
        t("Downloads wait while the phone’s battery is below this level, and go on once it charges or is above it again.")
    } else {
        t("Downloads wait while the computer runs on its battery below this level, and go on once it charges or is above it again.")
    });
}
