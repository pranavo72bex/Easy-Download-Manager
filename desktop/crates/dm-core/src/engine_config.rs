// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the engine is told for the whole program rather than for one queue, in one place (gaps
//! row A11): the name dm gives itself, the language of a dubbed video's sound, the connection
//! every download waits for, the second network, the folder YouTube's cut players are kept in
//! and the one network downloads are held to. [`configure`] is the one place each of them is
//! put in force; the setters it calls are left public, hidden, for the tests that set one part
//! alone. [`trust_bench_ca`] comes before all of it: the benchmark's certificate, trusted before
//! the first client is built.

use std::path::{Path, PathBuf};

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
    /// The one network every download is held to (Options › Advanced › Network › Only through,
    /// gaps row W12), by its adapter's name; empty for any. Chosen, the second network is off:
    /// a connection on it would leave by another network.
    pub only_through: String,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("identify_as_dm", &self.identify_as_dm)
            .field("sound_language", &self.sound_language)
            .field("connect_first", &self.connect_first.is_some())
            .field("second_path", &self.second_path)
            .field("kept_players", &self.kept_players)
            .field("only_through", &self.only_through)
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
    // Downloads held to one network: no second one beside it (gaps row W12).
    let only = config.only_through.trim();
    crate::only_through::set_only_through(Some(only));
    let second = config.second_path.as_deref().filter(|_| only.is_empty());
    crate::second_path::set_second_path(second)
}

/// Trusts the certificate file `DM_BENCH_CA` names beside the public roots, for the benchmark's
/// `wan` case, served over TLS with a certificate of its own (bench/README.md, "The line cases").
/// Called before the first client is built, which sets dm's TLS up once for every client and
/// stream protocol. A file that cannot be read or holds no certificate is noted, and dm runs on
/// with the public roots alone.
pub fn trust_bench_ca() {
    let Some(path) = crate::env::get_os("DM_BENCH_CA").filter(|path| !path.is_empty()) else {
        return;
    };
    if let Err(why) = trust_file(Path::new(&path)) {
        crate::note!(warn, "dm: DM_BENCH_CA is not trusted: {why}");
    }
}

/// Trusts the certificates of the file at `path` beside the public roots; why not, in words.
fn trust_file(path: &Path) -> Result<(), String> {
    let pem = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    dm_net::trust::trust_also(&pem).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_missing_or_with_no_certificate_is_refused_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.pem");
        let said = trust_file(&missing).unwrap_err();
        assert!(said.contains("missing.pem"), "{said}");
        let empty = dir.path().join("empty.pem");
        std::fs::write(&empty, b"no certificate here").unwrap();
        let said = trust_file(&empty).unwrap_err();
        assert!(said.contains("empty.pem"), "{said}");
    }
}
