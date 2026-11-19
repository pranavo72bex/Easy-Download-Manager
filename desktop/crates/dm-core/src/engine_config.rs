// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the engine is told for the whole program rather than for one queue, in one place (gaps
//! row A11): the name dm gives itself, the language of a dubbed video's sound, the connection
//! every download waits for, the second network and the folder YouTube's cut players are kept
//! in. [`configure`] is the one place each of them is put in force; the setters it calls are
//! left public, hidden, for the tests that set one part alone.

use std::path::PathBuf;

use crate::ConnectFirst;

/// The engine's program-wide settings. The default is the engine as it starts: a browser's name
/// first, each video's own sound, no connection to wait for, one network and cut players kept in
/// memory only.
#[derive(Clone, Default)]
pub struct EngineConfig {
    /// Whether a request goes out with dm's own name first rather than a browser's (Options ›
    /// Advanced › Identify as dm only). A site that refuses the first name is still asked again
    /// with the others.
    pub identify_as_dm: bool,
    /// The language a dubbed video's sound is taken in, as Options keeps it (`es`, `pt-BR`);
    /// empty, or `original`, for each video's own sound (gaps row Y10).
    pub sound_language: String,
    /// What every download waits on before it goes out (the dial-up or VPN connection a person
    /// chose); `None` for nothing.
    pub connect_first: Option<ConnectFirst>,
    /// The second network downloads may also go out on, by its interface's name; `None`, or an
    /// empty name, for none.
    pub second_path: Option<String>,
    /// The folder YouTube's cut players are kept in between sessions (gaps row Y13); `None` keeps
    /// them in memory only.
    pub kept_players: Option<PathBuf>,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("identify_as_dm", &self.identify_as_dm)
            .field("sound_language", &self.sound_language)
            .field("connect_first", &self.connect_first.is_some())
            .field("second_path", &self.second_path)
            .field("kept_players", &self.kept_players)
            .finish()
    }
}

/// Puts `config` in force for every download from now on: each part of it, whatever was in force
/// before. The second network is put in force last, and one that cannot be used (not connected,
/// or a client that cannot be built on it) is left off, with why in words; the other parts are in
/// force all the same.
pub fn configure(config: &EngineConfig) -> Result<(), String> {
    crate::agents::set_identify_as_dm(config.identify_as_dm);
    crate::sound_language::set_sound_language(&config.sound_language);
    crate::connect_first::set_connect_first(config.connect_first.clone());
    crate::youtube::set_kept_players(config.kept_players.clone());
    crate::second_path::set_second_path(config.second_path.as_deref())
}
