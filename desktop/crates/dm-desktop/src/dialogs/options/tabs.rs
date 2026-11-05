use super::super::*;

/// What every tab of Options draws with besides the settings being edited:
/// the queues that exist, the operating system, and the "focus the row just
/// added" helper.
#[derive(Clone, Copy)]
pub(super) struct TabEnv<'a> {
    pub(super) queues: &'a [String],
    pub(super) platform: &'a dyn crate::backend::Platform,
    pub(super) first: &'a dyn Fn(egui::Response, bool),
}

/// The events a sound file can be chosen for, by the settings' key.
const SOUND_EVENTS: [&str; 4] = ["complete", "failed", "queue_started", "queue_done"];

/// What an event is called beside its sound.
fn sound_words(event: &str) -> &'static str {
    match event {
        "complete" => t("Download complete"),
        "failed" => t("Download failed"),
        "queue_started" => t("Queue started"),
        _ => t("Queue finished"),
    }
}

/// One event's sound: its file's path, Choose… and Play.
fn sound_row(
    ui: &mut egui::Ui,
    sounds: &mut std::collections::BTreeMap<String, String>,
    event: &str,
    words: &str,
    platform: &dyn crate::backend::Platform,
) {
    let mut path = sounds.get(event).cloned().unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        field(ui, words, &mut path, 220.0);
        if ui.button(t("Choose…")).clicked() {
            if let Some(file) = rfd::FileDialog::new()
                .add_filter("WAV", &["wav"])
                .pick_file()
            {
                path = file.display().to_string();
            }
        }
        let has = !path.trim().is_empty();
        if ui.add_enabled(has, egui::Button::new(t("Play"))).clicked() {
            platform.play_file(path.trim());
        }
    });
    if path.trim().is_empty() {
        sounds.remove(event);
    } else {
        sounds.insert(event.to_string(), path);
    }
}

/// The General tab of Options.
pub(super) fn tab_general(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv { platform, .. } = *env;
    let s = &mut d.draft;
    setting_row(ui, t("Language"), |ui, name| {
        let now = crate::i18n::chosen(&s.language);
        // Named by its row, so a screen reader says what it is.
        combo("language")
            .selected_text(crate::i18n::name_of(now))
            .show_ui(ui, |ui| {
                if let Some(code) = crate::i18n::pick(ui, now) {
                    s.language = code.to_string();
                }
            })
            .response
            .labelled_by(name);
    });
    // Appearance, as a modern app lays its settings out: a name at the left
    // and its choice at the right, under a hairline.
    section(ui, t("Appearance"));
    setting_row(ui, t("Theme"), |ui, _| {
        theme_segments(ui, &mut s.theme);
    });
    setting_row(ui, t("Text size"), |ui, name| {
        // The size of everything in the window, the one View › Text size
        // and Ctrl with + and − also change, so it is read from it.
        let now = text_size_named(ui.ctx().zoom_factor());
        combo("text-size")
            .selected_text(t(now))
            .show_ui(ui, |ui| {
                for (label, zoom) in TEXT_SIZES {
                    if option(ui, now == label, t(label)).clicked() {
                        ui.ctx().set_zoom_factor(zoom);
                    }
                }
            })
            .response
            .labelled_by(name);
    });
    // In groups under headings: one list of a dozen boxes was
    // hard to find a setting in (a design review).
    section(ui, t("Starting and updating"));
    toggle_row(ui, &mut s.start_on_login, t("Start dm when I sign in"));
    setting_row(ui, t("When I close the window"), |ui, name| {
        let shown = match s.on_close.as_str() {
            "tray" => t("Keep running in the tray"),
            "exit" => t("Exit dm"),
            _ => t("Ask me"),
        };
        combo("on-close")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                pick(ui, &mut s.on_close, "ask".to_string(), t("Ask me"));
                pick(
                    ui,
                    &mut s.on_close,
                    "tray".to_string(),
                    t("Keep running in the tray"),
                );
                pick(ui, &mut s.on_close, "exit".to_string(), t("Exit dm"));
            })
            .response
            .labelled_by(name);
    });
    // What a caught or typed address already in the list does, asked once
    // and remembered from the question's tick: undone here.
    setting_row(
        ui,
        t("When an address is already in the list"),
        |ui, name| {
            let shown = match s.duplicate_choice.as_str() {
                "show" => t("Show it"),
                "again" => t("Download again"),
                "replace" => t("Replace the old file"),
                _ => t("Ask me"),
            };
            combo("duplicate-choice")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    pick(ui, &mut s.duplicate_choice, String::new(), t("Ask me"));
                    pick(
                        ui,
                        &mut s.duplicate_choice,
                        "show".to_string(),
                        t("Show it"),
                    );
                    pick(
                        ui,
                        &mut s.duplicate_choice,
                        "again".to_string(),
                        t("Download again"),
                    );
                    pick(
                        ui,
                        &mut s.duplicate_choice,
                        "replace".to_string(),
                        t("Replace the old file"),
                    );
                })
                .response
                .labelled_by(name);
        },
    );
    toggle_row(
        ui,
        &mut s.allow_command_line,
        t("Allow the command line to control dm"),
    );
    hint(
        ui,
        t("Lets other programs on this computer add and stop downloads."),
    );
    toggle_row(
        ui,
        &mut s.allow_mcp,
        t("Allow AI assistants to control dm (MCP)"),
    );
    hint(
        ui,
        t("Lets an AI assistant you run on this computer add and manage downloads."),
    );
    toggle_row(ui, &mut s.check_updates, t("Check for updates once a week"));
    section(ui, t("Sounds"));
    toggle_row(
        ui,
        &mut s.sound_on_complete,
        t("Play a sound when a download finishes"),
    );
    toggle_row(
        ui,
        &mut s.sound_on_failure,
        t("Play a sound when a download fails"),
    );
    // A sound of one's own for each event, as reference A offers: empty
    // keeps the system's.
    hint(
        ui,
        t("A sound file of your own for each, or empty for the usual sound (.wav)."),
    );
    for event in SOUND_EVENTS {
        sound_row(ui, &mut s.sounds, event, sound_words(event), platform);
    }
    // Offered only where it can be drawn.
    if crate::chrome::mica_possible() {
        toggle_row(ui, &mut s.mica, t("Use the Mica backdrop"));
        hint(
            ui,
            t("Lets the desktop show faintly through the window, as Windows' own apps do."),
        );
    }
    section(ui, t("Pop-ups"));
    toggle_row(
        ui,
        &mut s.show_progress,
        t("Show progress when a download starts"),
    );
    hint(
        ui,
        t("Off: press Details on a download's row to open its progress."),
    );
    toggle_row(
        ui,
        &mut s.show_complete,
        t("Show a message when a download finishes"),
    );
    toggle_row(
        ui,
        &mut s.watch_clipboard,
        t("Offer to download links I copy"),
    );
    hint(
        ui,
        t("dm looks at what you copy and offers to download any link."),
    );
    ui.add_enabled_ui(s.watch_clipboard, |ui| {
        toggle_row(
            ui,
            &mut s.start_copied,
            t("Take a copied link at once, without offering it first"),
        );
    });
    toggle_row(
        ui,
        &mut s.ask_panel_downloads,
        t("Ask where to save videos"),
    );
    hint(
        ui,
        t("Shows Download details before a video from a page starts."),
    );
    toggle_row(
        ui,
        &mut s.ask_caught_downloads,
        t("Ask where to save browser downloads"),
    );
    hint(
        ui,
        t("Shows Download details before a download from your browser starts."),
    );
    // Reference A's "start downloading while the dialog is open" (gaps row 85).
    toggle_row(
        ui,
        &mut s.start_while_asking,
        t("Start downloading while Download details is open"),
    );
    hint(
        ui,
        t("The file is fetched while you choose where it goes; Start now puts it there, Cancel removes it."),
    );
    toggle_row(
        ui,
        &mut s.offer_new_link,
        t("Offer a new link when a download's link has expired"),
    );
    hint(
        ui,
        t("Asks you to click the file again on its page, to get a link that works."),
    );
    section(ui, t("Adding and keeping files"));
    toggle_row(
        ui,
        &mut s.server_dates,
        t("Keep the file's date from the server"),
    );
    hint(
        ui,
        t("A saved file gets the date the website gives it, not today's."),
    );
    section(ui, t("The list"));
    // Each choice of a group under its name, as WinUI's radio
    // buttons stand.
    ui.label(t("Double-clicking a download"));
    ui.vertical(|ui| {
        for (word, name) in [
            ("properties", t("shows its properties")),
            ("open", t("opens the file")),
            ("folder", t("opens its folder")),
        ] {
            radio(ui, &mut s.double_click, word.to_string(), name);
        }
    });
    // Settings to carry to another computer, never with a
    // password; read back in, they wait here for OK.
    section(ui, t("Settings file"));
    ui.horizontal_wrapped(|ui| {
        // Without a file chooser, the download folder's
        // dm-settings.json, said where: the words asked for a
        // path with nowhere to type one.
        let fallback = s.download_dir.join("dm-settings.json");
        if ui.button(t("Save settings to a file…")).clicked() {
            let path = if platform.can_choose_file() {
                platform.choose_file(true, "dm-settings.json")
            } else {
                Some(fallback.clone())
            };
            d.says = if let Some(path) = path {
                match std::fs::write(&path, s.exported()) {
                    Ok(()) => tf("Saved to {}, without any password.", &[&path.display()]),
                    Err(e) => tf("{} could not be written: {}", &[&path.display(), &e]),
                }
            } else {
                String::new()
            };
        }
        if ui.button(t("Load settings from a file…")).clicked() {
            let path = if platform.can_choose_file() {
                platform.choose_file(false, "dm-settings.json")
            } else {
                Some(fallback)
            };
            if let Some(path) = path {
                d.says = match std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| s.imported(&text))
                {
                    Ok(read) => {
                        *s = read;
                        t("Loaded. Look them over, then OK to use them.").into()
                    }
                    Err(e) => tf("{} could not be read: {}", &[&path.display(), &e]),
                };
            }
        }
    });
}

/// Who opens magnet links and `.torrent` files on this computer, each with
/// a button that makes it dm where it is not, and one that gives it back to
/// the program that had it; read once, beside the window, as the page opens.
#[cfg(target_os = "windows")]
fn handler_rows(ui: &mut egui::Ui, d: &mut OptionsDialog) {
    use dm_shell::register::{self, Owner};
    let read = d.handlers.lock().ok().and_then(|h| *h);
    if read.is_none() && !d.handlers_asked {
        d.handlers_asked = true;
        let slot = std::sync::Arc::clone(&d.handlers);
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            let found = (
                register::magnet_owner(register::run_reg),
                register::torrent_owner(register::run_reg),
            );
            *dm_core::lock(&slot) = Some(found);
            ctx.request_repaint();
        });
    }
    let home = dm_shell::settings::home();
    let mut done: Option<Result<(), String>> = None;
    ui.add_space(8.0);
    for magnet in [true, false] {
        let owner = read.map(|(m, t)| if magnet { m } else { t });
        let backup = if magnet {
            register::magnet_backup(&home)
        } else {
            register::torrent_backup(&home)
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(if magnet {
                t("Magnet links open with:")
            } else {
                t(".torrent files open with:")
            });
            let said = match owner {
                None => t("Looking…"),
                Some(Owner::Dm) => "dm",
                Some(Owner::Other) => t("another program"),
                Some(Owner::Nobody) => t("nothing yet"),
                Some(Owner::WindowsChoice) => t("the app chosen in Windows' Default apps"),
            };
            ui.label(RichText::new(said).strong());
            let take = if magnet {
                t("Use dm for magnet links")
            } else {
                t("Use dm for .torrent files")
            };
            if matches!(owner, Some(Owner::Other | Owner::Nobody)) && ui.button(take).clicked() {
                done = Some(
                    std::env::current_exe()
                        .map_err(|e| e.to_string())
                        .and_then(|exe| {
                            if magnet {
                                register::take_magnet_windows(&exe, &backup, register::run_reg)
                            } else {
                                register::take_torrent_windows(&exe, &backup, register::run_reg)
                            }
                        }),
                );
            }
            let give = if magnet {
                t("Give magnet links back")
            } else {
                t("Give .torrent files back")
            };
            if owner == Some(Owner::Dm) && backup.is_file() && ui.button(give).clicked() {
                done = Some(if magnet {
                    register::give_back_magnet_windows(&backup, register::run_reg)
                } else {
                    register::give_back_torrent_windows(&backup, register::run_reg)
                });
            }
        });
    }
    hint(
        ui,
        t("dm keeps what another program had, and can give it back. Where Windows' Default apps has a choice, that choice decides: change it in Settings, Apps, Default apps."),
    );
    // Windows' own chooser at a first magnet click is its question, not dm's (gaps row 107).
    hint(
        ui,
        t("The first magnet link you click may make Windows ask which app opens it: choose dm there. dm cannot answer for you."),
    );
    // Read again after a change, so the page says what is so now.
    if let Some(result) = done {
        if let Err(why) = result {
            d.says = why;
        }
        *dm_core::lock(&d.handlers) = None;
        d.handlers_asked = false;
    }
}

/// The FileTypes tab of Options.
pub(super) fn tab_file_types(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv { platform, .. } = *env;
    // The extension, for someone who said not now to the bar over the
    // list, or has a second browser: each browser with whether its
    // extension has talked to dm, and Add where it has not.
    ui.label(t("Add dm to your browser:"));
    let seen = dm_shell::host::browsers_seen(&dm_shell::settings::home());
    let tokens = crate::theme::Tokens::get(ui.ctx());
    egui::Grid::new("browsers")
        .num_columns(2)
        .spacing([24.0, 6.0])
        .show(ui, |ui| {
            for (name, page, family) in [
                (
                    "Microsoft Edge",
                    crate::app::EXTENSION_EDGE,
                    &["Microsoft Edge"][..],
                ),
                ("Firefox", crate::app::EXTENSION_FIREFOX, &["Firefox"][..]),
                (
                    t("Chrome and others"),
                    crate::app::EXTENSION_CHROME,
                    &[
                        "Google Chrome",
                        "Brave",
                        "Vivaldi",
                        "Opera",
                        "Arc",
                        "Chromium",
                    ][..],
                ),
            ] {
                ui.label(name);
                if seen.iter().any(|b| family.contains(&b.as_str())) {
                    ui.label(RichText::new(t("Connected")).color(tokens.success));
                } else if ui.button(t("Add")).clicked() {
                    platform.open_later(page, Box::new(|_| {}));
                }
                ui.end_row();
            }
        });
    // Who opens magnet links and .torrent files: Windows keeps that, not
    // the browser.
    #[cfg(target_os = "windows")]
    handler_rows(ui, d);
    let s = &mut d.draft;
    ui.add_space(8.0);
    let c = &mut s.catch;
    // The items dm adds to the browser's right-click menu, each chosen, as
    // reference A's (gaps row 86); the extension makes them again on a change.
    ui.label(t("Items dm adds to the browser's right-click menu"));
    // One under another: a wrapped row takes the whole width it may have,
    // which widened Options on this page alone (at 200 % text OK moved).
    ui.vertical(|ui| {
        for (id, _) in dm_protocol::MENU_ITEMS {
            let words = match id {
                "dm-link" => t("Download link with dm"),
                "dm-media" => t("Download media with dm"),
                "dm-all-links" => t("Download all links with dm"),
                _ => t("Download selected links with dm"),
            };
            let mut on = c.shows_menu(id);
            if check(ui, &mut on, words).changed() {
                c.hidden_menus.retain(|h| h != id);
                if !on {
                    c.hidden_menus.push(id.to_string());
                }
            }
        }
    });
    ui.add_space(4.0);
    toggle_row(
        ui,
        &mut c.enabled,
        t("Take these file types from the browser"),
    );
    hint(
        ui,
        t("Off: the browser keeps every download it starts. Its right-click menu still sends links to dm."),
    );
    ui.add_enabled_ui(c.enabled, |ui| {
                    let name = ui.label(t("File types"));
                    ui.add(egui::TextEdit::multiline(&mut d.types).desired_rows(3).desired_width(ui.available_width().min(480.0)))
                        .labelled_by(name.id);
                    ui.label(RichText::new(t("Separate types with spaces, no dots. Leave empty for all.")).small().weak());
                    number_row(ui, t("Ignore files smaller than (KB)"), |ui, name| {
                        ui.add(number_box(&mut c.min_size_kb).range(0..=1024 * 1024)).labelled_by(name);
                    });
                    // Reference A's list of the browsers it captures from, each ticked
                    // or not (gaps row 86); an unticked one keeps its downloads.
                    ui.label(t("Take downloads from these browsers"));
                    ui.vertical(|ui| {
                        for browser in dm_protocol::BROWSERS {
                            let mut on = c.takes_from(browser);
                            let words = if browser == "Chromium" {
                                t("Other Chromium browsers")
                            } else {
                                browser
                            };
                            if check(ui, &mut on, words).changed() {
                                c.off_browsers.retain(|b| !b.eq_ignore_ascii_case(browser));
                                if !on {
                                    c.off_browsers.push(browser.to_string());
                                }
                            }
                        }
                    });
                    field(ui, t("Never take downloads from these sites"), &mut d.skip, 480.0);
                    ui.label(RichText::new(t("Separate sites with spaces. example.com covers its subdomains.")).small().weak());
                    // A link that works once was spent by dm asking about
                    // it before taking it.
                    field(ui, t("Sites whose links work only once"), &mut d.once, 480.0);
                    ui.label(RichText::new(t("dm takes their downloads without first asking the server whether it can fetch them, which would use the link up. A link that needs the browser's sign-in then fails in dm rather than finishing in the browser.")).small().weak());
                });
    // The keys a click on a link can be held with.
    let keys = [
        ("alt", t("Alt")),
        ("shift", t("key:Shift")),
        ("ctrl", t("Ctrl or Cmd")),
    ];
    let word = |key: &str| {
        keys.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, w)| w.trim_start_matches("key:").to_string())
            .unwrap_or_else(|| t("None").to_string())
    };
    ui.horizontal_wrapped(|ui| {
        let name = ui.label(t("Hold while clicking to leave it with the browser"));
        combo("keep-key")
            .selected_text(word(&c.keep_key))
            .show_ui(ui, |ui| {
                for (key, _) in &keys {
                    pick(ui, &mut c.keep_key, key.to_string(), word(key));
                }
            })
            .response
            .labelled_by(name.id);
    });
    ui.horizontal_wrapped(|ui| {
        let name = ui.label(t(
            "Hold while clicking to send it to dm whatever its type or size",
        ));
        combo("take-key")
            .selected_text(word(&c.take_key))
            .show_ui(ui, |ui| {
                pick(ui, &mut c.take_key, String::new(), word(""));
                for (key, _) in &keys {
                    pick(ui, &mut c.take_key, key.to_string(), word(key));
                }
            })
            .response
            .labelled_by(name.id);
    });
    if !c.take_key.is_empty() && c.take_key == c.keep_key {
        ui.colored_label(
            warn(ui),
            t("The two keys are the same; holding it leaves the download with the browser."),
        );
    }
    toggle_row(
        ui,
        &mut c.video_panel,
        t("Show Download video on web videos"),
    );
    hint(
        ui,
        t("A button over a video on a web page, to download it with dm."),
    );
    toggle_row(
        ui,
        &mut c.sign_in_everywhere,
        t("Send the browser's sign-in with downloads"),
    );
    ui.label(RichText::new(t("Files that need you signed in come to dm too. Your sign-in goes with that one download and is forgotten after it. Off: only the sites you turn on in the extension's menu send it.")).small().weak());
    ui.label(RichText::new(t("Your browser picks this up the next time it sends dm a download. Its right-click menu sends any link either way.")).small().weak());
}

/// The SaveTo tab of Options.
pub(super) fn tab_save_to(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv {
        platform, first, ..
    } = *env;
    let s = &mut d.draft;
    let mut dir = s.download_dir.display().to_string();
    let before = dir.clone();
    folder_field(ui, t("Download folder"), &mut dir, platform);
    if dir != before {
        s.download_dir = dir.into();
    }
    folder_field(
        ui,
        t("Folder for unfinished downloads"),
        &mut s.parts_dir,
        platform,
    );
    ui.label(
        RichText::new(t("A download already begun keeps the folder it began in."))
            .small()
            .weak(),
    );
    toggle_row(
        ui,
        &mut s.categorize,
        t("Sort finished files into folders by kind"),
    );
    // Why what follows is greyed: "Add a category" did nothing,
    // with no word.
    if !s.categorize {
        ui.label(
            RichText::new(t(
                "Turn on sorting to set the kinds' folders and your own categories.",
            ))
            .small()
            .weak(),
        );
    }
    ui.add_enabled_ui(s.categorize, |ui| {
                    // Two kinds a row: seven rows pushed the categories
                    // below out of sight. One where two do not fit: at 200%
                    // text they pushed Options off the window.
                    let per_row = if ui.available_width() >= 600.0 { 2 } else { 1 };
                    egui::Grid::new("folders").num_columns(2 * per_row).spacing([12.0, 6.0]).show(ui, |ui| {
                        for (i, c) in Category::ALL.iter().enumerate() {
                            // In the window's language, as the places
                            // name them; the folder beside it is a name on
                            // disk and stays as it is.
                            let name = ui.label(crate::i18n::name(&format::titled(&category_word(*c))));
                            if let Some(folder) = s.category_folders.get_mut(i) {
                                // Sized to what it holds, and given that
                                // size: a field in a grid took the width its
                                // column had, and the first column's were
                                // cut to two letters.
                                let font = egui::TextStyle::Body.resolve(ui.style());
                                let wide = ui
                                    .painter()
                                    .layout_no_wrap(folder.clone(), font, egui::Color32::PLACEHOLDER)
                                    .size()
                                    .x;
                                let size = egui::vec2(
                                    (wide + 28.0).clamp(140.0, 280.0),
                                    ui.spacing().interact_size.y,
                                );
                                ui.add_sized(size, egui::TextEdit::singleline(folder))
                                    .labelled_by(name.id);
                            }
                            if i % per_row == per_row - 1 {
                                ui.end_row();
                            }
                        }
                    });
                    ui.separator();
                    ui.label(t("Your own categories come first. A file of these types, from these sites (any site when empty), goes into a folder named after the category."));
                    let mut remove = None;
                    let count = d.custom.len();
                    for (i, [name, types, sites]) in d.custom.iter_mut().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            first(field(ui, t("Category"), name, 110.0), i + 1 == count);
                            field(ui, t("Types"), types, 150.0);
                            field(ui, t("Sites"), sites, 150.0);
                            if ui.button(t("Delete category")).clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        d.custom.remove(i);
                    }
                    if ui.button(t("Add a category")).clicked() {
                        d.custom.push(Default::default());
                        d.fresh = true;
                    }
                });
}

/// The Downloads tab of Options.
pub(super) fn tab_downloads(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv {
        queues, platform, ..
    } = *env;
    let s = &mut d.draft;
    number_row(ui, t("Connections per file"), |ui, name| {
        ui.add(number_box(&mut s.connections).range(1..=32))
            .labelled_by(name);
    });
    hint(ui, t("How many parts of a file dm fetches at once. More is faster on most sites, but some limit it."));
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(t("Presets:")).small());
        for (label, count) in [
            (t("Slow (2)"), 2),
            (t("Normal (8)"), 8),
            (t("Fast (16)"), 16),
            (t("Maximum (32)"), 32),
        ] {
            if ui.selectable_label(s.connections == count, label).clicked() {
                s.connections = count;
            }
        }
    });
    number_row(ui, t("Files at once"), |ui, name| {
        ui.add(number_box(&mut s.concurrency).range(1..=10))
            .labelled_by(name);
    });
    hint(ui, t("How many files download at the same time."));
    setting_row(ui, t("Video quality"), |ui, name| {
        let current = QUALITIES
            .iter()
            .find(|(v, _)| *v == s.stream_quality)
            .map(|(_, l)| *l)
            .unwrap_or("Best available");
        combo("quality")
            .selected_text(t(current))
            .show_ui(ui, |ui| {
                for (value, label) in QUALITIES {
                    pick(ui, &mut s.stream_quality, value.to_string(), t(label));
                }
            })
            .response
            .labelled_by(name);
    });
    toggle_row(
        ui,
        &mut s.start_queue_on_launch,
        t("Start the Main queue when dm opens"),
    );
    hint(
        ui,
        t("Downloads waiting in the Main queue start without a click."),
    );
    toggle_row(
        ui,
        &mut s.subtitles,
        t("Embed subtitles in videos from player pages"),
    );
    hint(
        ui,
        t("A video's subtitles are saved inside its file, to turn on in your player."),
    );
    toggle_row(
        ui,
        &mut s.remux_ts,
        t("Save streams that arrive as .ts as MP4"),
    );
    hint(
        ui,
        t("Streamed videos are saved as MP4 files, which more players open."),
    );
    ui.add_space(4.0);
    ui.label(t("Queues with their own limit:"));
    let mut remove = None;
    // One limit a queue: two for Main queue were both kept and
    // the last won without a word. Each offers
    // only the queues no other limit has.
    let limited: Vec<String> = s.queue_limits.iter().map(|l| l.queue.clone()).collect();
    for (i, limit) in s.queue_limits.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            // Chosen from the queues there are, shown in the
            // language in force and kept by dm's own name: a
            // typed name had to be the English one to match.
            let name = ui.label(t("Queue"));
            combo(("queue limit", i))
                .selected_text(crate::i18n::name(&limit.queue))
                .show_ui(ui, |ui| {
                    let mut offered: Vec<String> = queues
                        .iter()
                        .filter(|q| {
                            **q == limit.queue
                                || !limited.iter().enumerate().any(|(j, l)| j != i && l == *q)
                        })
                        .cloned()
                        .collect();
                    if !offered.contains(&limit.queue) {
                        offered.push(limit.queue.clone());
                    }
                    for q in offered {
                        let shown = crate::i18n::name(&q);
                        pick(ui, &mut limit.queue, q, shown);
                    }
                })
                .response
                .labelled_by(name.id);
            let name = ui.label(t("Files at once"));
            ui.add(number_box(&mut limit.files).range(1..=10))
                .labelled_by(name.id);
            if ui.button(t("Delete queue limit")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        s.queue_limits.remove(i);
    }
    let unlimited = queues
        .iter()
        .find(|q| !s.queue_limits.iter().any(|l| &l.queue == *q))
        .cloned();
    if ui
        .add_enabled(
            unlimited.is_some(),
            egui::Button::new(t("Add a queue limit")),
        )
        .clicked()
    {
        if let Some(queue) = unlimited {
            s.queue_limits
                .push(dm_shell::settings::QueueLimit { queue, files: 1 });
        }
    }
    // Queues that start on their own, each at its own time.
    ui.add_space(4.0);
    ui.label(t("Queues that start at a set time:"));
    let mut remove = None;
    for (i, start) in s.queue_starts.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            let name = ui.label(t("Queue"));
            combo(("queue start", i))
                .selected_text(crate::i18n::name(&start.queue))
                .show_ui(ui, |ui| {
                    for q in queues.iter().cloned() {
                        let shown = crate::i18n::name(&q);
                        pick(ui, &mut start.queue, q, shown);
                    }
                })
                .response
                .labelled_by(name.id);
            let name = ui.label(t("Starts at (HH:MM)"));
            ui.add(egui::TextEdit::singleline(&mut start.at).desired_width(60.0))
                .labelled_by(name.id);
            if !dm_core::is_time(&start.at) {
                ui.colored_label(warn(ui), t("Not a time"));
            }
            if ui.button(t("Delete start time")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        s.queue_starts.remove(i);
    }
    if ui.button(t("Add a start time")).clicked() {
        let queue = queues
            .first()
            .cloned()
            .unwrap_or_else(|| MAIN_QUEUE.to_string());
        s.queue_starts.push(dm_shell::settings::QueueStart {
            queue,
            at: "02:00".into(),
        });
    }
    let mut watch = s.watch_folder.clone();
    folder_field(
        ui,
        t("Watch a folder: links (.url, .webloc or a text file of links) and .torrent files saved there are added, then moved to its \"done\" folder. Leave it empty to turn this off."),
        &mut watch,
        platform,
    );
    s.watch_folder = watch;
    number_row(
        ui,
        t("Stop recording after (minutes, 0 for no limit)"),
        |ui, name| {
            ui.add(number_box(&mut s.record_minutes).range(0..=1440))
                .labelled_by(name);
        },
    );
    number_row(ui, t("Recheck every (hours, 0 for manual)"), |ui, name| {
        ui.add(number_box(&mut s.recheck_hours).range(0..=24 * 31))
            .labelled_by(name);
    });
    ui.add_space(8.0);
    ui.label(t("When a download finishes, run this command (a virus checker or a script). The file's path is added at the end:"));
    field(ui, t("Command"), &mut s.on_complete, 320.0);
    ui.add_space(8.0);
    toggle_row(ui, &mut s.fetch_tools, t("Get the video tools when needed"));
    ui.label(RichText::new(t("Downloaded from their official releases, checked, and kept up to date. Off: dm uses only tools you installed yourself.")).small().weak());
}

/// The Advanced page of Options: the settings the Connection and Proxy tabs
/// held, one after the other, each with the words it always had.
pub(super) fn tab_advanced(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    section(ui, t("Network"));
    tab_connection(ui, d, env);
    // The name dm gives servers first, and the switch for dm's own.
    ui.label(t("dm identifies itself to servers as:"));
    let agent = if d.draft.identify_as_dm {
        dm_core::USER_AGENT
    } else {
        dm_core::IE_AGENT
    };
    ui.label(egui::RichText::new(agent).monospace().weak());
    toggle_row(ui, &mut d.draft.identify_as_dm, t("Identify as dm only"));
    hint(
        ui,
        t("Sends dm's own name first. A site that refuses it is asked again with another name."),
    );
    // Where a proxy that cannot be one was said, the page opens at it.
    if std::mem::take(&mut d.to_proxy) {
        ui.scroll_to_cursor(Some(egui::Align::TOP));
    }
    section(ui, t("Proxy"));
    tab_proxy(ui, d, env);
    section(ui, t("BitTorrent"));
    field(
        ui,
        t("Block the peers listed at this address (optional)"),
        &mut d.draft.peer_blocklist,
        360.0,
    );
    hint(
        ui,
        t("A list in the P2P format, plain or gzipped, on the web or on this computer. One on the web is fetched through the proxy and read again once a day; a torrent stops, saying why, if the list cannot be read."),
    );
    // Off unless turned on: sharing sends from the person's connection (gaps row 147).
    toggle_row(
        ui,
        &mut d.draft.torrent_share,
        t("Keep sharing a finished torrent"),
    );
    hint(
        ui,
        t("Its files stay where they are, and other peers are sent them until it has sent the times its size below, or for the minutes below, whichever comes first. Off, a torrent stops once its files are whole."),
    );
    ui.add_enabled_ui(d.draft.torrent_share, |ui| {
        let s = &mut d.draft;
        number_row(ui, t("Until it has sent (times its size)"), |ui, name| {
            ui.add(number_box(&mut s.torrent_share_ratio).range(0.1..=100.0))
                .labelled_by(name);
        });
        number_row(ui, t("For at most (minutes)"), |ui, name| {
            ui.add(number_box(&mut s.torrent_share_minutes).range(1..=7 * 24 * 60))
                .labelled_by(name);
        });
        number_row(ui, t("Send at most (KB/s, 0 for no limit)"), |ui, name| {
            ui.add(number_box(&mut s.torrent_upload_limit).range(0..=1_000_000))
                .labelled_by(name);
        });
    });
    // What Windows' question at the first torrent is for (gaps row 101).
    hint(
        ui,
        t("The first torrent makes Windows ask whether dm may use public and private networks. Allowed, other peers can reach dm and torrents go faster; refused, dm still downloads, reaching peers itself. Change it later in Windows Security, Firewall, Allow an app through firewall."),
    );
}

/// The connection's settings, the first part of the Advanced page.
fn tab_connection(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv {
        first, platform, ..
    } = *env;
    let s = &mut d.draft;
    number_row(ui, t("Speed limit, KB/s (0 for none)"), |ui, name| {
        ui.add(number_box(&mut s.speed_limit).range(0..=1024 * 1024))
            .labelled_by(name);
    });
    hint(ui, t("Keeps dm from using all of your connection."));
    number_row(
        ui,
        t("Attempts per connection (0 to keep trying)"),
        |ui, name| {
            ui.add(number_box(&mut s.attempts).range(0..=100))
                .labelled_by(name);
        },
    );
    hint(
        ui,
        t("How many times dm tries again after a connection fails."),
    );
    number_row(
        ui,
        t("Wait between attempts (seconds, 0 for automatic)"),
        |ui, name| {
            ui.add(number_box(&mut s.retry_wait).range(0..=3600))
                .labelled_by(name);
        },
    );
    hint(ui, t("How long dm waits before it tries again."));
    number_row(
        ui,
        t("Time out a silent connection after (seconds)"),
        |ui, name| {
            ui.add(number_box(&mut s.read_timeout).range(5..=600))
                .labelled_by(name);
        },
    );
    hint(
        ui,
        t("How long a connection may say nothing before dm gives up on it."),
    );
    // A second network beside the one the system routes by: every other
    // connection of a download goes out on it.
    setting_row(ui, t("Second network"), |ui, name| {
        let now = if s.second_path.is_empty() {
            t("None").to_string()
        } else {
            s.second_path.clone()
        };
        combo("second-network")
            .selected_text(now)
            .show_ui(ui, |ui| {
                if option(ui, s.second_path.is_empty(), t("None")).clicked() {
                    s.second_path.clear();
                }
                for found in dm_core::interfaces() {
                    // The system's name for it where it has one ("iPhone
                    // USB (en5)"), else its address.
                    let words = match dm_core::system_name(&found.name) {
                        Some(named) => format!("{named} ({})", found.name),
                        None => format!("{} ({})", found.name, found.address),
                    };
                    if option(ui, s.second_path == found.name, &words).clicked() {
                        s.second_path = found.name;
                    }
                }
            })
            .response
            .labelled_by(name);
    });
    hint(
        ui,
        t("Use a second connection too, such as your phone over USB or its hotspot. Half of each download's connections go over it. Not used while a proxy is on."),
    );
    // How to have a phone there, step by step.
    egui::CollapsingHeader::new(t("How to use your phone"))
        .id_salt("second-network-steps")
        .default_open(false)
        .show(ui, |ui| {
            for step in [
                t("1. Connect your phone to this computer with its USB cable. If it asks whether to trust this computer, say yes."),
                t("2. Turn on its hotspot: on an iPhone, Settings › Personal Hotspot; on Android, USB tethering."),
                t("3. Choose it above. Keep Wi-Fi as this computer's own connection, so only downloads use the phone's data."),
            ] {
                ui.label(RichText::new(step).small());
            }
        });
    ui.add_space(8.0);
    ui.label(t("Sites with their own connection limit:"));
    let mut remove = None;
    let count = s.host_limits.len();
    for (i, limit) in s.host_limits.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            first(field(ui, t("Site"), &mut limit.host, 200.0), i + 1 == count);
            let name = ui.label(t("Connections"));
            ui.add(number_box(&mut limit.connections).range(1..=32))
                .labelled_by(name.id);
            if ui.button(t("Delete site limit")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        s.host_limits.remove(i);
    }
    if ui.button(t("Add a site limit")).clicked() {
        s.host_limits.push(dm_core::HostLimit {
            host: String::new(),
            connections: 1,
        });
        d.fresh = true;
    }
    ui.add_space(8.0);
    ui.label(t("Sites with their own folder and speed:"));
    let mut drop_rule = None;
    let rules = s.site_rules.len();
    for (i, rule) in s.site_rules.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            first(field(ui, t("Site"), &mut rule.host, 160.0), i + 1 == rules);
            folder_field(ui, t("Folder"), &mut rule.folder, platform);
            let name = ui.label(t("Speed limit, KB/s (0 for none)"));
            ui.add(number_box(&mut rule.speed).range(0..=1024 * 1024))
                .labelled_by(name.id);
            if ui.button(t("Delete site rule")).clicked() {
                drop_rule = Some(i);
            }
        });
    }
    if let Some(i) = drop_rule {
        s.site_rules.remove(i);
    }
    if ui.button(t("Add a site rule")).clicked() {
        s.site_rules.push(dm_shell::settings::SiteRule::default());
        d.fresh = true;
    }
    ui.add_space(8.0);
    ui.label(t(
        "For downloads that need a sign-in: a cookies.txt from your browser.",
    ));
    let cookies_before = s.cookies_file.clone();
    ui.horizontal_wrapped(|ui| {
        field(ui, t("Cookies file"), &mut s.cookies_file, 260.0);
        if ui.button(t("Choose…")).clicked() {
            if let Some(file) = rfd::FileDialog::new()
                .add_filter("cookies.txt", &["txt"])
                .pick_file()
            {
                s.cookies_file = file.display().to_string();
            }
        }
        if !s.cookies_file.is_empty() && ui.button(t("Clear")).clicked() {
            s.cookies_file.clear();
        }
    });
    // What was said about the file goes with it: cleared, the
    // red "could not be read" stayed on every tab.
    if s.cookies_file != cookies_before
        && !cookies_before.is_empty()
        && d.says.contains(&cookies_before)
    {
        d.says.clear();
    }
}

/// The proxy settings, the second part of the Advanced page.
fn tab_proxy(ui: &mut egui::Ui, d: &mut OptionsDialog, _env: &TabEnv<'_>) {
    let s = &mut d.draft;
    use dm_core::ProxyKind as Kind;
    let p = &mut s.proxy;
    toggle_row(ui, &mut p.enabled, t("Connect through a proxy"));
    hint(
        ui,
        t("For a network that reaches the internet through a proxy server. Whoever runs the network can give you its address."),
    );
    ui.add_enabled_ui(p.enabled, |ui| {
                    let kind_name = |kind: Kind| match kind {
                        Kind::System => t("The computer's proxy settings").to_string(),
                        other => format!("{other:?}").to_uppercase(),
                    };
                    setting_row(ui, t("Proxy type"), |ui, name| {
                        combo("proxy-kind")
                            .selected_text(kind_name(p.kind))
                            .show_ui(ui, |ui| {
                                for kind in [Kind::System, Kind::Http, Kind::Https, Kind::Socks4, Kind::Socks5] {
                                    let was = p.kind;
                                    if pick(ui, &mut p.kind, kind, kind_name(kind)).changed() {
                                        // The usual port of the kind picked,
                                        // where the port was the last kind's
                                        // usual one: SOCKS5 kept 8080.
                                        // A port typed in stays.
                                        let usual = |k: Kind| match k {
                                            Kind::Socks4 | Kind::Socks5 => Some(1080),
                                            Kind::Http | Kind::Https => Some(8080),
                                            Kind::System => None,
                                        };
                                        if usual(was).is_some_and(|port| port == p.port) || p.port == 0 {
                                            if let Some(port) = usual(kind) {
                                                p.port = port;
                                            }
                                        }
                                    }
                                }
                            })
                            .response
                            .labelled_by(name);
                    });
                    ui.add_enabled_ui(p.kind != Kind::System, |ui| {
                        field(ui, t("Host"), &mut p.host, 260.0);
                        number_row(ui, t("Port"), |ui, name| {
                            ui.add(number_box(&mut p.port).range(1..=65535)).labelled_by(name);
                        });
                        field(ui, t("Username, if it needs one"), &mut p.username, 200.0);
                        let name = ui.label(t("Password"));
                        ui.add(egui::TextEdit::singleline(&mut p.password).password(true).desired_width(ui.available_width().min(200.0))).labelled_by(name.id);
                        field(ui, t("Not for these sites (fetched directly)"), &mut d.bypass, 260.0);
                        // A proxy of its own for one kind of download, as reference A's
                        // per-protocol settings; the rest go through the one above.
                        for (scheme, words) in [
                            ("http", t("A proxy of its own for HTTP downloads")),
                            ("https", t("A proxy of its own for HTTPS downloads")),
                        ] {
                            let mut own = p.per_scheme.contains_key(scheme);
                            if toggle_row(ui, &mut own, words).changed() {
                                if own {
                                    p.per_scheme.insert(
                                        scheme.to_string(),
                                        dm_core::SchemeProxy {
                                            kind: Kind::Http,
                                            host: String::new(),
                                            port: 8080,
                                            username: String::new(),
                                            password: String::new(),
                                        },
                                    );
                                } else {
                                    p.per_scheme.remove(scheme);
                                }
                            }
                            if let Some(entry) = p.per_scheme.get_mut(scheme) {
                                ui.push_id(scheme, |ui| {
                                    ui.indent(scheme, |ui| {
                                        field(ui, t("Its host"), &mut entry.host, 220.0);
                                        number_row(ui, t("Its port"), |ui, name| {
                                            ui.add(number_box(&mut entry.port).range(1..=65535)).labelled_by(name);
                                        });
                                    });
                                });
                            }
                        }
                    });
                    field(ui, t("Automatic configuration script address (it chooses each site's proxy, in place of the above)"), &mut p.script, 260.0);
                    ui.label(RichText::new(t("Every download, the video tools and a torrent's SOCKS5 connections go through it; FTP through SOCKS or HTTP CONNECT.")).small().weak());
                });
}

/// The SiteLogins tab of Options.
pub(super) fn tab_site_logins(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv { first, .. } = *env;
    let s = &mut d.draft;
    ui.label(t(
        "A user and password used for every download from a host.",
    ));
    ui.label(
        RichText::new(t(
            "Sent as a Basic login, or as a Digest where the server asks for one.",
        ))
        .small()
        .weak(),
    );
    let mut remove = None;
    let count = s.site_logins.len();
    for (i, login) in s.site_logins.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            first(field(ui, t("Host"), &mut login.host, 150.0), i + 1 == count);
            field(ui, t("User"), &mut login.username, 110.0);
            let name = ui.label(t("Password"));
            ui.add(
                egui::TextEdit::singleline(&mut login.password)
                    .password(true)
                    .desired_width(ui.available_width().min(110.0)),
            )
            .labelled_by(name.id);
            if ui.button(t("Delete")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        s.site_logins.remove(i);
    }
    if ui.button(t("Add a login")).clicked() {
        s.site_logins.push(SiteLogin::default());
        d.fresh = true;
    }
    ui.label(
        RichText::new(t(
            "Kept in dm's settings file on this computer, readable by your account.",
        ))
        .small()
        .weak(),
    );
}
