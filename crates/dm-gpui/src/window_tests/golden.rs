// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's goldens: each scene below (the main window at a width, a dialog open over it)
//! is drawn, and every word it shows, with the window it is in and where it is seen, is held to
//! the scene's file in `docs/behaviour-baseline/layout/<target>/`. A word moved, cut, added or
//! taken away, a dialog grown or shrunk, fails, with the lines that differ.
//!
//! The words are measured by GPUI's test text system (every letter 0.6 of the text's size), not
//! the system's fonts, so a scene draws the same on every machine of one system; each system
//! has its own files, as dm's measures are each system's own (`native::pick`). `android` is the
//! phone's layout built on Linux (`--features phone-layout`, as CI's Android goldens run it).
//!
//! The files are changed by a person only: a bot may not change the window (CLAUDE.md), and
//! `docs/behaviour-baseline/` is among the bots' guardrails. After a change to the window that
//! was meant, write them again and read the difference before committing it:
//!
//! ```sh
//! DM_GOLDEN=write cargo test -p dm-gpui golden
//! DM_GOLDEN=write cargo test -p dm-gpui --features phone-layout golden   # on Linux: android
//! ```
//!
//! A system with no files yet (a new one) is not judged: its files are written to
//! `target/goldens-new/<target>/` for a person to read and commit, and the test says so.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::harness::world::{world, World};
use crate::harness::{settings_file, Harness};

/// Whose files these are: the system's, or the phone's layout. Android's build takes the
/// branches Linux's does where dm's code asks which system it is on, so the phone's layout is
/// judged on Linux; built elsewhere it has no files, and is not.
const TARGET: &str = if cfg!(all(feature = "phone-layout", target_os = "linux")) {
    "android"
} else if cfg!(feature = "phone-layout") {
    "android-not-on-linux"
} else if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(windows) {
    "windows"
} else {
    "linux"
};

/// A phone held upright, in points (as `window_tests/narrow.rs`), and on its side.
const PHONE: (f32, f32) = (412.0, 915.0);
const PHONE_ON_ITS_SIDE: (f32, f32) = (915.0, 412.0);

/// One picture of the window: its name (its file's), the main window's size, and what is done
/// to the window before its words are read.
struct Scene {
    name: &'static str,
    size: (f32, f32),
    language: &'static str,
    text_size: f32,
    /// A change to the settings the window opens with.
    set: fn(&mut dm_shell::Settings),
    open: fn(&mut Harness, &World),
}

const fn scene(name: &'static str, size: (f32, f32), open: fn(&mut Harness, &World)) -> Scene {
    Scene {
        name,
        size,
        language: "en",
        text_size: 1.0,
        set: |_| {},
        open,
    }
}

/// Nothing done: the window as it opens.
fn nothing(_: &mut Harness, _: &World) {}

/// Three downloads listed for later, fetching nothing: what a row shows, with no clock in it.
fn rows(h: &mut Harness, world: &World) {
    let names = ["report.pdf", "holiday.mp4", "archive.zip"];
    let urls: Vec<String> = names.iter().map(|n| world.url(n)).collect();
    h.call(
        "downloads.add",
        serde_json::json!({ "urls": urls, "later": true, "again": true }),
    )
    .unwrap();
    h.until("the rows", |h| h.rows().len() == names.len());
    h.until_shown("archive.zip");
}

/// A dialog opened by its command, and shown with `words` on it.
fn dialog(h: &mut Harness, command: &str, words: &'static str) {
    h.command(command);
    h.until_shown(dm_desktop_base::i18n::t(words));
}

/// The rows, the first chosen, and a dialog of it opened by its command.
fn rows_then(h: &mut Harness, world: &World, command: &str, words: &'static str) {
    rows(h, world);
    let id = h.rows()[0].id;
    h.call("ui.select", serde_json::json!({ "ids": [id] }))
        .unwrap();
    dialog(h, command, words);
}

/// Options, on the page called `page`.
fn options_on(h: &mut Harness, page: &'static str) {
    dialog(h, "downloads.options", "Language");
    if page != "General" {
        h.click(dm_desktop_base::i18n::t(page));
        h.until("the page", |h| {
            h.shows_in_dialog(dm_desktop_base::i18n::t(page))
        });
    }
}

/// The scenes of a computer's window.
fn desktop_scenes() -> Vec<Scene> {
    vec![
        scene("main-empty-1280", (1280.0, 720.0), nothing),
        scene("main-empty-840", (840.0, 600.0), nothing),
        scene("main-empty-600", (600.0, 600.0), nothing),
        scene("main-empty-360", (360.0, 720.0), nothing),
        scene("main-rows-1280", (1280.0, 720.0), rows),
        scene("main-rows-840", (840.0, 600.0), rows),
        scene("main-rows-360", (360.0, 720.0), rows),
        Scene {
            text_size: 2.0,
            ..scene("main-rows-1280-text-200", (1280.0, 720.0), rows)
        },
        Scene {
            language: "de",
            ..scene("main-rows-1280-de", (1280.0, 720.0), rows)
        },
        Scene {
            language: "he",
            ..scene("main-rows-1280-he", (1280.0, 720.0), rows)
        },
        scene("add-url", (1280.0, 720.0), |h, _| {
            dialog(h, "program.add_url", "Address")
        }),
        scene("options-general", (1280.0, 720.0), |h, _| {
            options_on(h, "General")
        }),
        scene("options-downloads", (1280.0, 720.0), |h, _| {
            options_on(h, "Downloads")
        }),
        scene("options-folders", (1280.0, 720.0), |h, _| {
            options_on(h, "Folders")
        }),
        scene("options-advanced", (1280.0, 720.0), |h, _| {
            options_on(h, "Advanced")
        }),
        Scene {
            // Its date is today's where none is set: one is, so the scene keeps no clock.
            set: |s| s.schedule.once_at = Some("2030-06-15 01:00".into()),
            ..scene("scheduler", (1280.0, 720.0), |h, _| {
                dialog(h, "downloads.scheduler", "Stop at")
            })
        },
        scene("speed-limiter", (1280.0, 720.0), |h, _| {
            dialog(
                h,
                "downloads.speed_limiter.settings",
                "Slower between set hours",
            )
        }),
        scene("properties", (1280.0, 720.0), |h, w| {
            rows_then(h, w, "file.properties", "Will be saved to")
        }),
        scene("about", (1280.0, 720.0), |h, _| {
            dialog(h, "help.about", "dm")
        }),
    ]
}

/// The scenes of a phone's window: the same, at a phone's size.
fn phone_scenes() -> Vec<Scene> {
    let mut scenes: Vec<Scene> = desktop_scenes()
        .into_iter()
        .filter(|s| s.size.0 == 1280.0)
        .map(|s| Scene {
            name: s.name.trim_end_matches("-1280"),
            size: PHONE,
            ..s
        })
        .map(|s| Scene {
            name: match s.name {
                "main-rows-1280-text-200" => "main-rows-text-200",
                "main-rows-1280-de" => "main-rows-de",
                "main-rows-1280-he" => "main-rows-he",
                other => other,
            },
            ..s
        })
        .collect();
    scenes.push(scene("main-rows-on-its-side", PHONE_ON_ITS_SIDE, rows));
    scenes
}

fn scenes() -> Vec<Scene> {
    if cfg!(feature = "phone-layout") {
        phone_scenes()
    } else {
        desktop_scenes()
    }
}

/// The repository's root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Where this system's files are kept.
fn kept() -> PathBuf {
    root().join("docs/behaviour-baseline/layout").join(TARGET)
}

/// A number as the files write it: to a tenth of a point, never "-0.0".
fn at(x: gpui::Pixels) -> String {
    let x = (f32::from(x) * 10.0).round() / 10.0;
    format!("{}", if x == 0.0 { 0.0 } else { x })
}

/// Where every scene downloads to: the same on every machine of a system, so a path's words
/// are as long on each (a test's own folder's are not).
fn downloads_folder() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\dm-golden\Downloads")
    } else {
        PathBuf::from("/tmp/dm-golden/Downloads")
    }
}

/// What changes from one run to the next, put as the same words: the server's port, the
/// test's own folders. With whether the words' place is kept: dm's version moves as it grows.
fn steady(text: &str, world: &World) -> (String, bool) {
    let version = env!("CARGO_PKG_VERSION");
    if text.contains(version) {
        return (text.replace(version, "<version>"), false);
    }
    let mut text = text.replace(&world.addr.to_string(), "server");
    for (path, said) in [
        (world.dir.path().to_path_buf(), "<folder>"),
        (crate::harness::home().to_path_buf(), "<home>"),
    ] {
        let path = path.display().to_string();
        text = text.replace(&path, said);
        // macOS's temporary folder is also reached through /private.
        if let Some(short) = path.strip_prefix("/private") {
            text = text.replace(short, said);
        }
    }
    (text.replace(['\n', '\r'], "⏎"), true)
}

/// The scene as its file writes it: each window (the main one first, then each dialog by its
/// name), its size, and each word it draws, in the order laid out, with where it is seen.
fn picture(h: &mut Harness, world: &World, scene: &Scene) -> String {
    // A frame drawn after the last change, so every word is where it rests.
    h.update(|_, cx| cx.notify());
    let windows = h.cx.update(|cx| {
        crate::window::screen::Screens::all(cx)
            .into_iter()
            .map(|(handle, screen, dialog)| {
                let name = crate::window::screen::Screens::name_of(handle, cx)
                    .or_else(|| crate::window::screen::Screens::title_of(handle, cx))
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let size = handle
                    .update(cx, |_, window, _| window.viewport_size())
                    .unwrap_or_default();
                (dialog, name, size, screen.drawn())
            })
            .collect::<Vec<_>>()
    });
    let mut windows = windows;
    // The main window first, then the dialogs by name: the order they were opened in is not
    // what a person sees.
    windows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# dm's window, scene {} ({TARGET}, {}x{}, {}, text at {}%), drawn by \
         crates/dm-gpui/src/window_tests/golden.rs.",
        scene.name,
        scene.size.0,
        scene.size.1,
        scene.language,
        scene.text_size * 100.0,
    );
    let _ = writeln!(
        out,
        "# Each word: left top right bottom, where it is seen; \"hidden\" where nothing of it is."
    );
    for (dialog, name, size, drawn) in windows {
        let kind = if dialog { "dialog" } else { "main" };
        let _ = writeln!(
            out,
            "\n[{kind} {:?} {}x{}]",
            steady(&name, world).0,
            at(size.width),
            at(size.height)
        );
        for d in drawn {
            if d.text.trim().is_empty() {
                continue;
            }
            let (text, placed) = steady(&d.text, world);
            let b = d.bounds;
            if !placed {
                let _ = writeln!(out, "{text:?} somewhere");
            } else if b.size.width <= gpui::px(0.0) || b.size.height <= gpui::px(0.0) {
                let _ = writeln!(out, "{text:?} hidden");
            } else {
                let _ = writeln!(
                    out,
                    "{text:?} {} {} {} {}",
                    at(b.left()),
                    at(b.top()),
                    at(b.right()),
                    at(b.bottom())
                );
            }
        }
    }
    out
}

/// The scene drawn in a window of its own.
fn draw(scene: &Scene) -> String {
    let world = world();
    let mut settings = world.settings();
    settings.general.language = scene.language.into();
    settings.text_size = scene.text_size;
    settings.folders.download_dir = downloads_folder();
    (scene.set)(&mut settings);
    let mut h = world.harness_with(settings);
    dm_desktop_base::i18n::set(scene.language);
    h.act(|_, cx| crate::window::zoom::start(scene.text_size, cx));
    h.resize(scene.size.0, scene.size.1);
    (scene.open)(&mut h, &world);
    let drawn = picture(&mut h, &world, scene);
    h.act(|_, cx| crate::window::zoom::start(1.0, cx));
    dm_desktop_base::i18n::set("en");
    drawn
}

/// The lines of `was` and `now` that differ, as a person reads a change: `-` gone, `+` new.
fn difference(was: &str, now: &str) -> String {
    let was: Vec<&str> = was.lines().collect();
    let now: Vec<&str> = now.lines().collect();
    // The longest run the two share, line by line (each file is a few hundred lines).
    let mut same = vec![vec![0u32; now.len() + 1]; was.len() + 1];
    for i in (0..was.len()).rev() {
        for j in (0..now.len()).rev() {
            same[i][j] = if was[i] == now[j] {
                same[i + 1][j + 1] + 1
            } else {
                same[i + 1][j].max(same[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out, mut shown) = (0, 0, String::new(), 0);
    while (i < was.len() || j < now.len()) && shown < 60 {
        if i < was.len() && j < now.len() && was[i] == now[j] {
            i += 1;
            j += 1;
        } else if i < was.len() && (j == now.len() || same[i + 1][j] >= same[i][j + 1]) {
            let _ = writeln!(out, "- {}", was[i]);
            i += 1;
            shown += 1;
        } else {
            let _ = writeln!(out, "+ {}", now[j]);
            j += 1;
            shown += 1;
        }
    }
    if shown == 60 {
        out.push_str("… and more\n");
    }
    out
}

/// Every scene draws as its golden file has it, and every golden file has its scene.
#[test]
fn the_window_draws_as_its_goldens() {
    // The language and the text size are the process's.
    let _file = settings_file();
    let write = dm_core::env::get("DM_GOLDEN").is_some_and(|v| v == "write");
    let kept = kept();
    let new = root().join("target/goldens-new").join(TARGET);
    let first = !kept.is_dir();
    let scenes = scenes();
    let mut failed = Vec::new();
    for scene in &scenes {
        let now = draw(scene);
        // Drawn twice, the same: a golden of a scene that changes by itself judges nothing.
        let again = draw(scene);
        assert_eq!(
            now,
            again,
            "scene {} draws differently each time:\n{}",
            scene.name,
            difference(&now, &again)
        );
        let file = kept.join(format!("{}.txt", scene.name));
        if write {
            std::fs::create_dir_all(&kept).unwrap();
            std::fs::write(&file, &now).unwrap();
            continue;
        }
        let was = std::fs::read_to_string(&file).ok();
        if was.as_deref() != Some(now.as_str()) {
            std::fs::create_dir_all(&new).unwrap();
            std::fs::write(new.join(format!("{}.txt", scene.name)), &now).unwrap();
            if first {
                continue;
            }
            failed.push(match was {
                Some(was) => format!(
                    "scene {} differs from {}:\n{}",
                    scene.name,
                    file.display(),
                    difference(&was, &now)
                ),
                None => format!("scene {} has no golden file {}", scene.name, file.display()),
            });
        }
    }
    if write {
        eprintln!("goldens written to {}", kept.display());
        return;
    }
    if first {
        eprintln!(
            "no goldens for {TARGET} yet: not judged; written to {} for a person to read and \
             copy to {}",
            new.display(),
            kept.display()
        );
        return;
    }
    // A golden file no scene draws: a scene taken away (or renamed) leaves its file, and a
    // person, not a bot, takes the file away with it.
    for entry in std::fs::read_dir(&kept).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".txt") else {
            continue;
        };
        if !scenes.iter().any(|s| s.name == stem) {
            failed.push(format!("{name} in {} has no scene", kept.display()));
        }
    }
    assert!(
        failed.is_empty(),
        "the window does not draw as its goldens ({} scene(s)); the new pictures are in {}. \
         A change to the window that was meant: DM_GOLDEN=write (window_tests/golden.rs).\n\n{}",
        failed.len(),
        new.display(),
        failed.join("\n")
    );
}

/// The difference names the lines gone and the lines new, and nothing the two share.
#[test]
fn the_difference_names_what_changed() {
    let was = "a\nb\nc\nd\n";
    let now = "a\nc\nd\ne\n";
    assert_eq!(difference(was, now), "- b\n+ e\n");
    assert_eq!(difference(was, was), "");
    // A line changed in place reads as a person reads it: what was, then what is.
    assert_eq!(difference("a\nb\n", "a\nc\n"), "- b\n+ c\n");
}
