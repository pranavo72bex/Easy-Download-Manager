//! What the extension decides, with no browser in the way.
//!
//! Everything here is plain Rust: it compiles for the host and is tested
//! there with `cargo test`. The browser side (`web.rs`) only reads events into
//! these types, asks these functions, and does what they say.

// Reads what the browser and the app send: every index is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use dm_protocol::{Candidate, CatchRules, Hello, Reply, CHROME_EXTENSION_ID, EDGE_EXTENSION_ID};
use serde::Deserialize;
use serde_json::Value;

mod answers;
mod app;
mod download;
pub mod excluded;
mod media_panel;
mod page_lines;
mod menu;
pub mod rules;
mod sites;
mod streams;
mod timing;

pub use answers::*;
pub use app::*;
pub use download::*;
pub use media_panel::*;
pub use page_lines::*;
pub use menu::*;
pub use sites::*;
pub use streams::*;
pub use timing::*;

#[cfg(test)]
#[path = "../../desktop/test-support/secrets.rs"]
mod test_secrets;

#[cfg(test)]
mod tests;
