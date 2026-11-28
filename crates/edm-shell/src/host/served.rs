// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The site rules the app serves to the extension in its hello: the document it ships, or a newer
//! `rules.json` in EDM's home folder. Nothing here can fail a hello: a missing, unreadable, too large,
//! malformed or older file is passed over, and the extension keeps its compiled tables regardless.

use std::path::Path;

use edm_protocol::Rules;

/// The document shipped inside the app (`assets/rules/`, gaps row X9).
const SHIPPED: &str = edm_assets::RULES;

/// The file a newer document is put in, in EDM's home folder.
pub const FILE: &str = edm_core::paths::RULES;

/// The rules to serve: the newer of the shipped document and the home folder's.
pub fn served(home: &Path) -> Rules {
    let shipped = Rules::parse(SHIPPED).unwrap_or_default();
    Rules::newest(shipped, found(home))
}

/// Keeps `text`, a rules document fetched by the update check, in the home folder when it is one
/// this side reads and newer than what is served now (the shipped document being the floor).
/// `Ok(true)` when it was kept, `Ok(false)` when it is not newer; the previous file is untouched
/// on any error. Written whole under another name and moved into place.
pub fn adopt(home: &Path, text: &str) -> Result<bool, crate::Error> {
    let rules = Rules::parse(text).map_err(|e| e.to_string())?;
    if rules.revision <= served(home).revision {
        return Ok(false);
    }
    let temp = home.join(format!("{FILE}.new"));
    std::fs::write(&temp, text).map_err(|why| why.to_string())?;
    std::fs::rename(&temp, home.join(FILE)).map_err(|why| {
        let _ = std::fs::remove_file(&temp);
        why.to_string()
    })?;
    Ok(true)
}

/// Hands the served rules' YouTube clients to the engine, asked before its own (gaps row Y1): at
/// start, and again when a newer document is kept.
pub fn apply(home: &Path) {
    let clients = served(home)
        .youtube
        .clients
        .into_iter()
        .map(|c| edm_core::youtube::PlayerClient {
            name: c.name,
            version: c.version,
            id: c.id,
            agent: c.agent,
            context: c.context.into_iter().collect(),
        })
        .collect();
    edm_core::youtube::set_player_clients(clients);
}

/// The home folder's document, if it is there and is one.
fn found(home: &Path) -> Option<Rules> {
    let path = home.join(FILE);
    // Not read past the cap: a file beyond it is refused whole, whatever it holds.
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > edm_protocol::MAX_RULES_BYTES as u64 {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    Rules::parse(&text).ok()
}

#[cfg(test)]
mod tests;
