//! The site rules the app serves to the extension in its hello: the document it ships, or a newer
//! `rules.json` in dm's home folder. Nothing here can fail a hello: a missing, unreadable, too large,
//! malformed or older file is passed over, and the extension keeps its compiled tables regardless.

use std::path::Path;

use dm_protocol::Rules;

/// The document shipped inside the app.
const SHIPPED: &str = include_str!("../../resources/rules.json");

/// The file a newer document is put in, in dm's home folder.
pub const FILE: &str = "rules.json";

/// The rules to serve: the newer of the shipped document and the home folder's.
pub fn served(home: &Path) -> Rules {
    let shipped = Rules::parse(SHIPPED).unwrap_or_default();
    Rules::newest(shipped, found(home))
}

/// The home folder's document, if it is there and is one.
fn found(home: &Path) -> Option<Rules> {
    let path = home.join(FILE);
    // Not read past the cap: a file beyond it is refused whole, whatever it holds.
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > dm_protocol::MAX_RULES_BYTES as u64 {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    Rules::parse(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dm-served-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a folder");
        dir
    }

    #[test]
    fn the_shipped_document_is_one_the_extension_reads() {
        let shipped = Rules::parse(SHIPPED).expect("the shipped rules.json parses");
        assert_eq!(shipped.format, dm_protocol::RULES_FORMAT);
        assert!(shipped.revision >= 1);
    }

    #[test]
    fn with_no_file_the_shipped_rules_are_served() {
        let dir = home();
        assert_eq!(served(&dir), Rules::parse(SHIPPED).expect("shipped"));
    }

    #[test]
    fn a_newer_file_in_the_home_wins_and_an_older_or_broken_one_changes_nothing() {
        let dir = home();
        let shipped = Rules::parse(SHIPPED).expect("shipped");
        let newer = format!(
            r#"{{ "format": 1, "revision": {}, "exclude": {{ "advert_hosts": ["ads.example"] }} }}"#,
            shipped.revision + 1
        );
        std::fs::write(dir.join(FILE), &newer).expect("written");
        let got = served(&dir);
        assert_eq!(got.revision, shipped.revision + 1);
        assert_eq!(got.exclude.advert_hosts, vec!["ads.example"]);

        let older =
            r#"{ "format": 1, "revision": 0, "exclude": { "advert_hosts": ["x.example"] } }"#;
        std::fs::write(dir.join(FILE), older).expect("written");
        assert_eq!(served(&dir), shipped);

        for bad in ["not json", "{}", r#"{ "format": 9, "revision": 99 }"#, ""] {
            std::fs::write(dir.join(FILE), bad).expect("written");
            assert_eq!(served(&dir), shipped, "{bad:?}");
        }

        let huge = vec![b' '; dm_protocol::MAX_RULES_BYTES + 1];
        std::fs::write(dir.join(FILE), huge).expect("written");
        assert_eq!(served(&dir), shipped);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
