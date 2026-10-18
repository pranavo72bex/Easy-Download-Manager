//! The logins and secrets the tests use, never written in the code.
//!
//! Each is `DM_TEST_<NAME>` from the environment, or from a `.env` file at
//! the repository's root (kept out of git; `.env.example` lists the names),
//! and otherwise made up afresh for this run — so a test never depends on a
//! value anyone could read in the source, and CI needs none set. Included by
//! path (`#[path = …] mod secrets;`) in each crate's tests, which share no
//! crate they could import it from.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The secret called `name`: letters and digits only, the same for every
/// call in this run.
pub fn secret(name: &str) -> String {
    static MADE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let key = format!("DM_TEST_{}", name.to_ascii_uppercase());
    if let Some(value) = std::env::var(&key).ok().filter(|v| !v.is_empty()) {
        return value;
    }
    if let Some(value) = from_dotenv(&key) {
        return value;
    }
    let mut made = MADE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    made.entry(key).or_insert_with(|| fresh(name)).clone()
}

/// [`secret`] with the characters a password must survive in an address
/// and in a proxy's settings: `@` and `:`.
pub fn marked(name: &str) -> String {
    format!("{}@{}:x", secret(name), secret(name))
}

/// `text` as it is written into an address, `@` and `:` escaped.
pub fn escaped(text: &str) -> String {
    text.replace('%', "%25")
        .replace('@', "%40")
        .replace(':', "%3A")
}

/// `KEY=value` from the first `.env` above this crate.
fn from_dotenv(key: &str) -> Option<String> {
    let start = std::env::var_os("CARGO_MANIFEST_DIR")?;
    let mut dir = std::path::PathBuf::from(start);
    loop {
        if let Ok(text) = std::fs::read_to_string(dir.join(".env")) {
            return text.lines().find_map(|line| {
                let (k, v) = line.split_once('=')?;
                (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
            });
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Twelve letters and digits nobody chose.
fn fresh(name: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write(name.as_bytes());
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.write_u32(std::process::id());
    format!("t{:x}", hasher.finish())
}
