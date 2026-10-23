use super::*;
/// A new install's welcome: what the extension does, and a button for each
/// browser on the computer that opens its extension page there.
pub(super) fn welcome(app: &mut DmApp, ctx: &egui::Context) {
    let Some(found) = app.dialogs.welcome.clone() else {
        return;
    };
    let mut open = true;
    let mut later = false;
    let mut page: Option<&'static str> = None;
    let mut at_login = app.applied.start_on_login;
    window(ctx, "Add dm to your browser")
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(t(
                "Downloads you click in your browser come to dm once its extension is added. It takes a minute, once.",
            ));
            ui.add_space(8.0);
            use dm_shell::register::Store;
            let link = |store: Store| match store {
                Store::Edge => crate::app::EXTENSION_EDGE,
                Store::Firefox => crate::app::EXTENSION_FIREFOX,
                Store::Chrome => crate::app::EXTENSION_CHROME,
            };
            if found.is_empty() {
                ui.label(RichText::new(t("No browser was found; each one's page is here:")).weak());
                for (name, store) in [
                    ("Microsoft Edge", Store::Edge),
                    ("Firefox", Store::Firefox),
                    (t("Chrome and others"), Store::Chrome),
                ] {
                    if ui.button(name).clicked() {
                        page = Some(link(store));
                    }
                }
            } else {
                for browser in &found {
                    if ui.button(tf("Add to {}", &[&browser.name])).clicked() {
                        page = Some(link(browser.store));
                    }
                }
            }
            // What dm changed on the computer, said where it can be undone:
            // it starts with the computer, in the tray, so a download
            // clicked in the browser always has dm to go to.
            ui.add_space(8.0);
            check(ui, &mut at_login, t("Start dm with the computer, in the tray"));
            ui.label(
                RichText::new(t(
                    "So a download clicked in the browser always has dm to go to. Options › General changes it later.",
                ))
                .small()
                .weak(),
            );
            footer(ui, |ui| {
                if ui.button(t("Later")).clicked() {
                    later = true;
                }
            });
        });
    if at_login != app.applied.start_on_login {
        let mut wanted = app.applied.clone();
        wanted.start_on_login = at_login;
        if let Err(e) = app.apply_settings(wanted) {
            app.said = e.into();
        }
    }
    if let Some(page) = page {
        app.open_file(page);
    }
    if !open || later {
        app.dialogs.welcome = None;
    }
}

pub(super) fn about(app: &mut DmApp, ctx: &egui::Context) {
    if !app.dialogs.about_open {
        return;
    }
    let mut open = true;
    window(ctx, "About dm").open(&mut open).show(ctx, |ui| {
        ui.heading("dm");
        ui.label(tf("Version {}", &[&env!("CARGO_PKG_VERSION")]));
        ui.label(t("A free download manager, written in Rust."));
        ui.horizontal_wrapped(|ui| {
            for (words, page) in [
                (t("Source code"), crate::update::SOURCE),
                (t("Licence (MIT)"), crate::update::LICENCE),
                (t("Privacy"), crate::update::PRIVACY),
                (t("Code signing policy"), crate::update::SIGNING),
            ] {
                if ui.link(words).on_hover_text(page).clicked() {
                    if let Err(e) = app.backend.platform.open(page) {
                        app.said = e.into();
                    }
                }
            }
        });
        ui.separator();
        if ui.button(t("Check for updates")).clicked()
            || std::mem::take(&mut app.dialogs.check_update)
        {
            let client = app.client();
            crate::update::check(
                &app.backend.runtime,
                client,
                Arc::clone(&app.update),
                ctx.clone(),
            );
        }
        let state = dm_core::lock(&app.update).clone();
        ui.label(RichText::new(state.shown()).weak());
        // What changed in it, on the release's own page.
        if matches!(state, crate::update::Update::Newer(..)) {
            if let Some(notes) = dm_core::lock(&crate::update::LATEST_NOTES).clone() {
                if ui.button(t("What's new")).clicked() {
                    if let Err(e) = app.backend.platform.open(&notes) {
                        app.said = e.into();
                    }
                }
            }
        }
        if let crate::update::Update::Newer(_, Some(installer)) = state {
            if ui.button(t("Download the update")).clicked() {
                // Checked against the digest the release published; one that
                // does not match fails and is removed.
                let with = AddOptions {
                    checksum: dm_core::Checksum::parse(&format!("sha256:{}", installer.sha256)),
                    ..AddOptions::default()
                };
                app.said = match app.backend.add_with(&installer.url, with) {
                    Ok(id) => {
                        app.update_row = Some(id);
                        crate::i18n::Said::new(
                            "The new installer is downloading; dm offers to install it once it is checked.",
                            &[],
                        )
                    }
                    Err(e) => e.into(),
                };
            }
        }
    });
    app.dialogs.about_open = open;
}

/// A folder name every computer can make: what a category made or renamed
/// in Options is held to, since settings travel between computers.
pub(super) fn portable_folder(name: &str) -> bool {
    dm_core::category::portable_folder(name).is_ok()
}
