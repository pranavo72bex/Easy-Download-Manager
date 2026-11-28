// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The settings a backup holds, read in place of these (File › Restore…, gaps row W16).

use super::Settings;

impl Settings {
    /// Settings read from a backup's `text` in place of these: read as the settings file is
    /// (an odd entry costs only itself; a newer EDM's fields are passed over), each value
    /// brought into range, with the names of what could not be read and a sentence for each
    /// thing that had to be changed to be used. As a settings file imported: the passwords this
    /// computer has for the same site and name, for the same proxy and for the same dial-up
    /// connection are kept (a backup's own, where it is opened, are put in after:
    /// `crate::backup`), and so is this computer's history (the addresses typed, the folders
    /// last chosen). The error, where `text` is not settings at all.
    pub fn restored(&self, text: &str) -> Result<(Settings, Vec<String>), String> {
        let (read, mut said) =
            Self::parse(text).map_err(|e| format!("its settings could not be read ({e})"))?;
        let (mut read, notes) = read.readable();
        said.extend(notes);
        for login in &mut read.site_logins {
            login.password = self
                .site_logins
                .iter()
                .find(|k| k.host == login.host && k.username == login.username)
                .map(|k| k.password.clone())
                .unwrap_or_default();
        }
        let (proxy, mine) = (&mut read.advanced.proxy, &self.advanced.proxy);
        proxy.password = if (&proxy.host, proxy.port, &proxy.username)
            == (&mine.host, mine.port, &mine.username)
        {
            mine.password.clone()
        } else {
            String::new()
        };
        let (dialup, mine) = (&mut read.advanced.dialup, &self.advanced.dialup);
        dialup.password =
            if (&dialup.connection, &dialup.username) == (&mine.connection, &mine.username) {
                mine.password.clone()
            } else {
                String::new()
            };
        read.recent_urls.clone_from(&self.recent_urls);
        read.last_folders.clone_from(&self.last_folders);
        Ok((read, said))
    }
}
