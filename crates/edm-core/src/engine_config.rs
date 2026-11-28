// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What the engine is told for the whole program rather than for one queue, in one place (gaps
//! row A11): the name EDM gives itself, the language of a dubbed video's sound, the connection
//! every download waits for, the second network, the folder YouTube's cut players are kept in
//! and the one network downloads are held to. [`configure`] is the one place each of them is
//! put in force; the setters it calls are left public, hidden, for the tests that set one part
//! alone. [`trust_bench_ca`] and [`use_bench_dns`] come before all of it: the benchmark's
//! certificate and its DNS server, put in force before the first client is built.

use std::path::{Path, PathBuf};

use crate::ConnectFirst;

/// The engine's program-wide settings. The default is the engine as it starts: a browser's name
/// first, each video's own sound, no connection to wait for, one network and cut players kept in
/// memory only.
#[derive(Clone, Default)]
pub struct EngineConfig {
    /// Whether a request goes out with EDM's own name first rather than a browser's (Options ›
    /// Advanced › Identify as EDM only). A site that refuses the first name is still asked again
    /// with the others.
    pub identify_as_dm: bool,
    /// The language a dubbed video's sound is taken in, as Options keeps it (`es`, `pt-BR`);
    /// empty, or `original`, for each video's own sound (gaps row Y10).
    pub sound_language: String,
    /// A video's picture taken in H.264 where it has one within the quality asked, rather than
    /// a taller AV1 or VP9 one (gaps row Y8c).
    pub prefer_h264: bool,
    /// What every download waits on before it goes out (the dial-up or VPN connection a person
    /// chose); `None` for nothing.
    pub connect_first: Option<ConnectFirst>,
    /// The second network downloads may also go out on, by its interface's name; `None`, or an
    /// empty name, for none.
    pub second_path: Option<String>,
    /// A third network beside the second, by its interface's name (gaps row K3).
    pub third_path: Option<String>,
    /// More networks beyond the third, by name, in order (gaps row K3c).
    pub more_paths: Vec<String>,
    /// The second network's own speed limit, in bytes a second, shared by its connections; 0
    /// for none (gaps row K5).
    pub second_limit: u64,
    /// The third network's own speed limit, as `second_limit` is the second's (gaps row K5b).
    pub third_limit: u64,
    /// Each network's own allowance, the second's first, then the third's (gaps row K5d); one
    /// of no bytes, or none given, is none.
    pub network_allowances: Vec<crate::quota::Allowance>,
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
            .field("prefer_h264", &self.prefer_h264)
            .field("connect_first", &self.connect_first.is_some())
            .field("second_path", &self.second_path)
            .field("kept_players", &self.kept_players)
            .field("only_through", &self.only_through)
            .field("second_limit", &self.second_limit)
            .field("third_limit", &self.third_limit)
            .field("network_allowances", &self.network_allowances)
            .field("third_path", &self.third_path)
            .field("more_paths", &self.more_paths)
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
    crate::prefer_h264::set_prefer_h264(config.prefer_h264);
    crate::connect_first::set_connect_first(config.connect_first.clone());
    crate::youtube::set_kept_players(config.kept_players.clone());
    // Downloads held to one network: no second one beside it (gaps row W12).
    let only = config.only_through.trim();
    crate::only_through::set_only_through(Some(only));
    let second = config.second_path.as_deref().filter(|_| only.is_empty());
    crate::second_path::set_second_limit(config.second_limit);
    crate::second_path::set_third_limit(config.third_limit);
    for network in 1..=2u8 {
        let given = config.network_allowances.get(usize::from(network) - 1);
        crate::network_allowance::set_allowance(network, given.copied().unwrap_or_default());
    }
    // The third and any more, in order (gaps row K3c), none while held to one network.
    let extra: Vec<String> = config
        .third_path
        .iter()
        .chain(&config.more_paths)
        .filter(|_| only.is_empty())
        .cloned()
        .collect();
    // Their failure is said after the second's own: the second is in force either way.
    let set_third = crate::second_path::set_extra_paths(&extra);
    crate::second_path::set_second_path(second).and(set_third)
}

/// Trusts the certificate file `EDM_BENCH_CA` names beside the public roots, for the benchmark's
/// `wan` case, served over TLS with a certificate of its own (bench/README.md, "The line cases").
/// Called before the first client is built, which sets EDM's TLS up once for every client and
/// stream protocol. A file that cannot be read or holds no certificate is noted, and EDM runs on
/// with the public roots alone.
pub fn trust_bench_ca() {
    let Some(path) = crate::env::get_os("EDM_BENCH_CA").filter(|path| !path.is_empty()) else {
        return;
    };
    if let Err(why) = trust_file(Path::new(&path)) {
        crate::note!(warn, "EDM: EDM_BENCH_CA is not trusted: {why}");
    }
}

/// Has names under `.test` asked of the DNS server `EDM_BENCH_DNS` names (`127.0.0.1:<port>`), in
/// place of the system, for the benchmark's `slowname` case: a name that answers slowly
/// (bench/README.md, "The line cases"). Called before the first client is built. Every other
/// name goes to the system as before; an address that cannot be read is noted, and every name
/// goes to the system.
pub fn use_bench_dns() {
    let Some(said) = crate::env::get("EDM_BENCH_DNS").filter(|said| !said.trim().is_empty()) else {
        return;
    };
    match bench_dns(&said) {
        Ok(server) => {
            if !edm_net::dns::ask_test_names_of(server) {
                crate::note!(
                    warn,
                    "EDM: EDM_BENCH_DNS was given twice: the first is in force"
                );
            }
        }
        Err(why) => crate::note!(warn, "EDM: EDM_BENCH_DNS is not used: {why}"),
    }
}

/// The DNS server `said` names: an address and a port (`127.0.0.1:5353`); why not, in words.
fn bench_dns(said: &str) -> Result<std::net::SocketAddr, String> {
    said.trim()
        .parse()
        .map_err(|_| format!("{said:?} is not an address and a port, as 127.0.0.1:5353"))
}

/// Trusts the certificates of the file at `path` beside the public roots; why not, in words.
fn trust_file(path: &Path) -> Result<(), String> {
    let pem = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    edm_net::trust::trust_also(&pem).map_err(|e| format!("{}: {e}", path.display()))
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

    #[test]
    fn the_bench_dns_server_is_an_address_and_a_port() {
        assert_eq!(
            bench_dns(" 127.0.0.1:5353 "),
            Ok(std::net::SocketAddr::from(([127, 0, 0, 1], 5353)))
        );
        for wrong in ["127.0.0.1", "localhost:53", "bench.test"] {
            let said = bench_dns(wrong).unwrap_err();
            assert!(said.contains(wrong), "{said}");
        }
    }
}
