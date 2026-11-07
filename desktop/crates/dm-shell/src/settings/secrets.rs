// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Sign-ins', the proxy's and the dial-up connection's passwords, kept in the operating
//! system's store instead of `settings.json` where the store works (`dm_core::secretstore`; gaps
//! row 39). The dial-up connection's goes there only when "Save password" is on; off, it is never
//! written anywhere and is forgotten when dm closes (`Settings::save_to`, `dialup.rs`).
//!
//! In memory the settings keep their passwords as they always did, so nothing else changes. On
//! the way to the file each password goes to the store under a name of its own and the file keeps
//! the field empty; on the way back an empty field is looked up in the store. A file written
//! earlier, with passwords in clear, is read as it was and its passwords are moved into the store
//! on that first read. Where the store is missing or refuses, the password stays in the file as
//! before and the window is told once.

use dm_core::secretstore::SecretStore;

use super::Settings;

/// The store to use: the system's, never in this crate's own tests (they must leave nothing in a
/// person's store; `dm_core` cannot see this crate's `cfg(test)`).
pub(super) fn store() -> Option<&'static dyn SecretStore> {
    if cfg!(test) {
        None
    } else {
        dm_core::secretstore::system()
    }
}

fn login_name(ns: &str, host: &str, user: &str) -> String {
    format!("dm:login:{ns}:{host}\n{user}")
}

fn proxy_name(ns: &str) -> String {
    format!("dm:proxy:{ns}")
}

fn dialup_name(ns: &str) -> String {
    format!("dm:dialup:{ns}")
}

impl Settings {
    /// Puts every non-empty password into `store` and clears it here, so that what is written to
    /// the file carries none. A password the store refuses stays where it is. Returns what the
    /// store said when it refused, at most once.
    pub(super) fn move_passwords_to(
        &mut self,
        store: &dyn SecretStore,
        ns: &str,
    ) -> Option<String> {
        let mut refused = None;
        let forget = !self.keep_stored_passwords;
        let mut keep = |name: String, password: &mut String| {
            if password.is_empty() {
                // Nothing to keep any more: forget what an earlier password left behind, unless
                // this run never read it back (`keep_stored_passwords`).
                if forget {
                    let _ = store.delete(&name);
                }
                return;
            }
            match store.put(&name, password.as_bytes()) {
                Ok(()) => password.clear(),
                Err(why) => refused = Some(why),
            }
        };
        for login in &mut self.site_logins {
            keep(
                login_name(ns, &login.host, &login.username),
                &mut login.password,
            );
        }
        let proxy = proxy_name(ns);
        keep(proxy, &mut self.proxy.password);
        // Empty here unless it is to be saved (`Settings::save_to`): then what an earlier run
        // kept is forgotten too.
        keep(dialup_name(ns), &mut self.dialup.password);
        refused.map(|why| {
            format!(
                "the operating system's store for passwords could not be used ({why}): the \
                 Sign-ins', the proxy's and the dial-up connection's passwords stay in the \
                 settings file"
            )
        })
    }

    /// Fills the passwords the file left empty from `store`, and moves into it the ones the file
    /// still holds in clear. Returns whether any moved (the file should then be written again,
    /// without them) and what to tell the person when the store refused.
    pub(super) fn take_passwords_from(
        &mut self,
        store: &dyn SecretStore,
        ns: &str,
    ) -> (bool, Option<String>) {
        let mut moved = false;
        let mut refused = None;
        let mut fill = |name: String, password: &mut String| {
            if password.is_empty() {
                match store.get(&name) {
                    Ok(Some(kept)) => {
                        if let Ok(text) = String::from_utf8(kept) {
                            *password = text;
                        }
                    }
                    Ok(None) => {}
                    Err(why) => refused = Some(why),
                }
            } else {
                // A clear password from an earlier file: moved in, kept here until saved.
                match store.put(&name, password.as_bytes()) {
                    Ok(()) => moved = true,
                    Err(why) => refused = Some(why),
                }
            }
        };
        for login in &mut self.site_logins {
            fill(
                login_name(ns, &login.host, &login.username),
                &mut login.password,
            );
        }
        fill(proxy_name(ns), &mut self.proxy.password);
        fill(dialup_name(ns), &mut self.dialup.password);
        let note = refused.map(|why| {
            format!(
                "the operating system's store for passwords could not be read ({why}): a \
                 password that is not in the settings file may be missing; enter it again in \
                 Options"
            )
        });
        (moved, note)
    }
}
