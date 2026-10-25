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

/// The General tab of Options.
pub(super) fn tab_general(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv { platform, .. } = *env;
    let s = &mut d.draft;
    ui.horizontal_wrapped(|ui| {
        let now = crate::i18n::chosen(&s.language);
        // Named by its label, so a screen reader says what it is;
        // the name before the choice, as every number and choice
        // in Options has it (some had it after).
        let name = ui.label(t("Language"));
        combo("language")
            .selected_text(crate::i18n::name_of(now))
            .show_ui(ui, |ui| {
                if let Some(code) = crate::i18n::pick(ui, now) {
                    s.language = code.to_string();
                }
            })
            .response
            .labelled_by(name.id);
    });
    // In groups under headings: one list of a dozen boxes was
    // hard to find a setting in (a design review).
    section(ui, t("Starting and updating"));
    check(ui, &mut s.start_on_login, t("Start dm when I sign in"));
    check(
        ui,
        &mut s.allow_command_line,
        t("Allow the command line to control dm"),
    );
    check(
        ui,
        &mut s.allow_mcp,
        t("Allow AI assistants to control dm (MCP)"),
    );
    check(ui, &mut s.check_updates, t("Check for updates once a week"));
    section(ui, t("Windows and sounds"));
    check(
        ui,
        &mut s.show_progress,
        t("Show progress when a download starts"),
    );
    check(
        ui,
        &mut s.show_complete,
        t("Show a message when a download finishes"),
    );
    check(
        ui,
        &mut s.sound_on_complete,
        t("Play a sound when a download finishes"),
    );
    check(
        ui,
        &mut s.sound_on_failure,
        t("Play a sound when a download fails"),
    );
    // Offered only where it can be drawn.
    if crate::chrome::mica_possible() {
        check(ui, &mut s.mica, t("Use the Mica backdrop"));
    }
    section(ui, t("Adding and keeping files"));
    check(
        ui,
        &mut s.watch_clipboard,
        t("Offer to download links I copy"),
    );
    check(
        ui,
        &mut s.ask_panel_downloads,
        t("Ask where to save videos"),
    );
    check(
        ui,
        &mut s.ask_caught_downloads,
        t("Ask where to save browser downloads"),
    );
    check(
        ui,
        &mut s.server_dates,
        t("Keep the file's date from the server"),
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
    ui.label(t("Look"));
    ui.vertical(|ui| {
        for (word, name) in THEMES {
            radio(ui, &mut s.theme, word.to_string(), t(name));
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
                Some(fallback.clone())
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

/// The FileTypes tab of Options.
pub(super) fn tab_file_types(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv { platform, .. } = *env;
    let s = &mut d.draft;
    // The extension, for someone who said not now to the bar
    // over the list, or has a second browser.
    ui.horizontal_wrapped(|ui| {
        ui.label(t("Add dm to your browser:"));
        for (name, page) in [
            ("Microsoft Edge", crate::app::EXTENSION_EDGE),
            ("Firefox", crate::app::EXTENSION_FIREFOX),
            (t("Chrome and others"), crate::app::EXTENSION_CHROME),
        ] {
            if ui.button(name).clicked() {
                platform.open_later(page, Box::new(|_| {}));
            }
        }
    });
    ui.add_space(8.0);
    let c = &mut s.catch;
    check(
        ui,
        &mut c.enabled,
        t("Take these file types from the browser"),
    );
    ui.add_enabled_ui(c.enabled, |ui| {
                    let name = ui.label(t("File types"));
                    ui.add(egui::TextEdit::multiline(&mut d.types).desired_rows(3).desired_width(ui.available_width().min(480.0)))
                        .labelled_by(name.id);
                    ui.label(RichText::new(t("Separate types with spaces, no dots. Leave empty for all.")).small().weak());
                    number_row(ui, t("Ignore files smaller than (KB)"), |ui, name| {
                        ui.add(number_box(&mut c.min_size_kb).range(0..=1024 * 1024)).labelled_by(name);
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
    check(
        ui,
        &mut c.video_panel,
        t("Show Download video on web videos"),
    );
    check(
        ui,
        &mut c.sign_in_everywhere,
        t("Send the browser's sign-in with downloads"),
    );
    ui.label(RichText::new(t("A file only a signed-in browser can fetch then comes to dm too. The cookies go with that download alone and are forgotten once it is done; without this, only the sites switched on in the extension's popup send them.")).small().weak());
    ui.label(RichText::new(t("Read by the browser extension the next time it talks to dm: a download it catches, or its right-click menu. The right-click menu hands anything over regardless.")).small().weak());
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
    check(
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
                    ui.label(t("Your categories, looked at first: a file of these types, from these sites (any when empty), goes in a folder of the category's name."));
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
    egui::Grid::new("downloads")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            let name = ui.label(t("Connections per file"));
            ui.add(number_box(&mut s.connections).range(1..=32))
                .labelled_by(name.id);
            ui.end_row();
            let name = ui.label(t("Files at once"));
            ui.add(number_box(&mut s.concurrency).range(1..=10))
                .labelled_by(name.id);
            ui.end_row();
            let choices = QUALITIES;
            let current = choices
                .iter()
                .find(|(v, _)| *v == s.stream_quality)
                .map(|(_, l)| *l)
                .unwrap_or("Best available");
            ui.label(t("Video quality"));
            combo("quality")
                .selected_text(t(current))
                .show_ui(ui, |ui| {
                    for (value, label) in choices {
                        pick(ui, &mut s.stream_quality, value.to_string(), t(label));
                    }
                });
            ui.end_row();
        });
    check(
        ui,
        &mut s.start_queue_on_launch,
        t("Start the Main queue when dm opens"),
    );
    check(
        ui,
        &mut s.subtitles,
        t("Embed subtitles in videos from player pages"),
    );
    check(
        ui,
        &mut s.remux_ts,
        t("Save streams that arrive as .ts as MP4"),
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
            if !dm_core::schedule::is_time(&start.at) {
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
        t("Watch a folder for links: a .url, .webloc or text file of links, or a .torrent saved there is added and moved to its \"done\" folder (empty is off)"),
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
    ui.label(t("When a download finishes, run (a virus checker, a script); the file's path is added at the end:"));
    field(ui, t("Command"), &mut s.on_complete, 320.0);
    ui.add_space(8.0);
    check(
        ui,
        &mut s.fetch_tools,
        t("Get the video helper (yt-dlp) when needed"),
    );
    ui.label(RichText::new(t("From its official GitHub release, checked against its published checksum, and kept current weekly. Off, only a yt-dlp installed on this computer is used.")).small().weak());
}

/// The Connection tab of Options.
pub(super) fn tab_connection(ui: &mut egui::Ui, d: &mut OptionsDialog, env: &TabEnv<'_>) {
    let TabEnv {
        first, platform, ..
    } = *env;
    let s = &mut d.draft;
    number_row(ui, t("Speed limit, KB/s (0 for none)"), |ui, name| {
        ui.add(number_box(&mut s.speed_limit).range(0..=1024 * 1024))
            .labelled_by(name);
    });
    number_row(
        ui,
        t("Attempts per connection (0 to keep trying)"),
        |ui, name| {
            ui.add(number_box(&mut s.attempts).range(0..=100))
                .labelled_by(name);
        },
    );
    number_row(
        ui,
        t("Wait between attempts (seconds, 0 for automatic)"),
        |ui, name| {
            ui.add(number_box(&mut s.retry_wait).range(0..=3600))
                .labelled_by(name);
        },
    );
    number_row(
        ui,
        t("Time out a silent connection after (seconds)"),
        |ui, name| {
            ui.add(number_box(&mut s.read_timeout).range(5..=600))
                .labelled_by(name);
        },
    );
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

/// The Proxy tab of Options.
pub(super) fn tab_proxy(ui: &mut egui::Ui, d: &mut OptionsDialog, _env: &TabEnv<'_>) {
    let s = &mut d.draft;
    use dm_core::proxy::Kind;
    let p = &mut s.proxy;
    check(ui, &mut p.enabled, t("Connect through a proxy"));
    ui.add_enabled_ui(p.enabled, |ui| {
                    let kind_name = |kind: Kind| match kind {
                        Kind::System => t("The computer's proxy settings").to_string(),
                        other => format!("{other:?}").to_uppercase(),
                    };
                    ui.horizontal(|ui| {
                        let name = ui.label(t("Proxy type"));
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
                            .labelled_by(name.id);
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
                    });
                    ui.label(RichText::new(t("Every download of dm's own, yt-dlp and a torrent's SOCKS5 connections go through it; FTP through SOCKS or HTTP CONNECT.")).small().weak());
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
    ui.separator();
    ui.label(t(
        "TorBox API key: to fetch a link through your TorBox account (Add URL › Through TorBox, or a download's menu).",
    ));
    ui.horizontal(|ui| {
        let name = ui.label(t("API key"));
        ui.add(
            egui::TextEdit::singleline(&mut s.torbox_key)
                .password(true)
                .desired_width(ui.available_width().min(260.0)),
        )
        .labelled_by(name.id);
    });
    ui.label(
        RichText::new(t(
            "Only a link you send is shared with TorBox, with this key.",
        ))
        .small()
        .weak(),
    );
}
