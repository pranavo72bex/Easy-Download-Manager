// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The update manifest's key and signature (gaps row F6).
//!
//! `cargo xtask update-key` makes a new Ed25519 pair: the public half is pasted into dm
//! (`SIGNING_KEY` in `dm-shell/src/app/update.rs`), the private half becomes the repository
//! secret `DM_UPDATE_SIGNING_KEY` and is never written into a tracked file.
//! `cargo xtask sign-manifest <latest.json>` signs a manifest with the key in that variable,
//! writing `latest.json.sig` beside it, and refuses a key that is not the one dm carries: a
//! release signed by any other key is a release every dm would refuse.

use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::path::Path;

/// The environment variable `sign-manifest` reads the private key from: base64 of its PKCS#8.
pub const ENV: &str = "DM_UPDATE_SIGNING_KEY";

/// Where dm carries the public key, from the repository's root.
pub const CARRIED_IN: &str = "desktop/crates/dm-shell/src/app/update.rs";

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// A new pair: (public key, private key), each in base64 (32 bytes; PKCS#8 v2).
pub fn new_pair() -> Result<(String, String), String> {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).map_err(|e| e.to_string())?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|e| e.to_string())?;
    Ok((
        b64().encode(pair.public_key().as_ref()),
        b64().encode(pkcs8.as_ref()),
    ))
}

/// What `cargo xtask update-key` prints: the new pair and where each half goes.
pub fn update_key() -> Result<String, String> {
    let (public, private) = new_pair()?;
    Ok(format!(
        "public key (paste as SIGNING_KEY in {CARRIED_IN}):\n{public}\n\n\
         private key (the repository secret {ENV}; keep a copy offline, never commit it):\n{private}"
    ))
}

/// The public key `source` (the text of [`CARRIED_IN`]) carries as `SIGNING_KEY`.
pub fn carried_key(source: &str) -> Option<&str> {
    let line = source.lines().find(|l| {
        l.trim_start()
            .starts_with("pub const SIGNING_KEY: &str = \"")
    })?;
    let rest = line.split_once('"')?.1;
    rest.split_once('"').map(|(key, _)| key)
}

/// The base64 signature of `manifest` by `private` (base64 PKCS#8, as [`new_pair`] makes or
/// `openssl genpkey -algorithm ed25519` writes), refused when its public half is not `carried`.
pub fn sign(private: &str, carried: &str, manifest: &[u8]) -> Result<String, String> {
    let der = b64()
        .decode(private.trim())
        .map_err(|_| format!("{ENV} is not base64"))?;
    let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&der)
        .map_err(|_| format!("{ENV} is not an Ed25519 private key (PKCS#8)"))?;
    let public = b64().encode(pair.public_key().as_ref());
    if public != carried.trim() {
        return Err(format!(
            "the key in {ENV} is not the one dm carries ({public} is not {carried}): paste the \
             public line of `cargo xtask update-key` as SIGNING_KEY in {CARRIED_IN}, or set the \
             secret to that key's private half"
        ));
    }
    let signature = b64().encode(pair.sign(manifest).as_ref());
    // Checked as dm checks it, so a release is never published with a signature dm refuses.
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, pair.public_key());
    key.verify(
        manifest,
        &b64().decode(&signature).map_err(|e| e.to_string())?,
    )
    .map_err(|_| "the signature made does not check".to_string())?;
    Ok(signature)
}

/// `cargo xtask sign-manifest <file>`: `<file>.sig` written beside `<file>`.
pub fn sign_manifest(root: &Path, file: Option<&str>) -> Result<String, String> {
    let file = file.ok_or("usage: cargo xtask sign-manifest <latest.json>")?;
    let private = std::env::var(ENV).map_err(|_| format!("{ENV} is not set"))?;
    let source =
        std::fs::read_to_string(root.join(CARRIED_IN)).map_err(|e| format!("{CARRIED_IN}: {e}"))?;
    let carried = carried_key(&source).ok_or(format!("{CARRIED_IN} has no SIGNING_KEY"))?;
    let manifest = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let signature = sign(&private, carried, &manifest)?;
    let out = format!("{file}.sig");
    std::fs::write(&out, signature).map_err(|e| format!("{out}: {e}"))?;
    Ok(format!("signed {file}: {out}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_is_signed_only_with_the_key_dm_carries() {
        let (public, private) = new_pair().unwrap();
        let manifest = br#"{"version":"9.9.9"}"#;
        let signature = sign(&private, &public, manifest).unwrap();
        let key = ring::signature::UnparsedPublicKey::new(
            &ring::signature::ED25519,
            b64().decode(&public).unwrap(),
        );
        assert!(key
            .verify(manifest, &b64().decode(&signature).unwrap())
            .is_ok());
        let (other, _) = new_pair().unwrap();
        let refused = sign(&private, &other, manifest).unwrap_err();
        assert!(refused.contains("not the one dm carries"), "{refused}");
        assert!(sign("not base64!", &public, manifest).is_err());
        assert!(sign(&b64().encode([7u8; 40]), &public, manifest).is_err());
    }

    #[test]
    fn the_key_dm_carries_is_read_from_its_source() {
        let source = std::fs::read_to_string(crate::root().join(CARRIED_IN)).unwrap();
        let key = carried_key(&source).expect("update.rs carries SIGNING_KEY");
        assert_eq!(b64().decode(key).unwrap().len(), 32);
        assert_eq!(carried_key("no key here"), None);
    }
}
