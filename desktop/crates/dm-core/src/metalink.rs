//! Reading a Metalink, which is a list of places one file can be fetched from.
//!
//! A single server puts a ceiling on a download that no amount of segmenting
//! can lift — its own upload rate, or a per-connection cap it applies on
//! purpose. Several mirrors of the same file do not share that ceiling. This
//! is what a Metalink describes: the same bytes, in several places, with the
//! checksums to prove they are the same bytes.
//!
//! Two versions are in use and both appear in the wild. Version 4 (RFC 5854)
//! puts urls directly under the file and ranks them by `priority`, where lower
//! is better. Version 3 nests them under `<resources>` and ranks by
//! `preference`, where higher is better. Both are read here, and both come out
//! as one list, best first — matching `extension/src/metalink.js`, because the
//! two implementations should not disagree about which mirror to try.

// Reads outside input: an index out of range here is a crash a server or
// a file could cause, so every access is checked.
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::checksum::Checksum;

/// Rank given to a mirror that states no preference at all: worse than
/// anything explicitly preferred, better than anything explicitly deferred.
const UNRANKED: i64 = 500;

/// What a Metalink says about the one file dm will fetch.
#[derive(Debug, Clone, PartialEq)]
pub struct Metalink {
    /// Empty when the document did not name it.
    pub filename: String,
    /// `None` when the document did not state one.
    pub size: Option<u64>,
    /// The strongest digest offered that dm can actually check.
    pub checksum: Option<Checksum>,
    /// Every usable mirror, best first.
    pub mirrors: Vec<String>,
}

/// Whether this names a mirror list rather than a file.
pub fn is_metalink(url: &str, content_type: Option<&str>) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if path.ends_with(".meta4") || path.ends_with(".metalink") {
        return true;
    }
    content_type
        .unwrap_or("")
        .to_ascii_lowercase()
        .contains("application/metalink")
}

/// Reads a Metalink into the one file dm will fetch.
///
/// A Metalink may describe many files; dm takes the first, because a download
/// is one file and silently queueing a dozen from one click would be a
/// surprise. `None` when there is nothing usable in it.
pub fn parse(xml: &str) -> Option<Metalink> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_end_names = false;

    // Everything is gathered for the *first* file element only, so `depth`
    // tracks whether we are still inside it.
    let mut in_file = false;
    let mut file_depth = 0usize;
    let mut depth = 0usize;
    let mut seen_metalink = false;
    let mut finished = false;

    let mut filename = String::new();
    let mut size: Option<u64> = None;
    let mut hashes: Vec<Checksum> = Vec::new();
    let mut ranked: Vec<(i64, String)> = Vec::new();

    /// What the character data arriving next belongs to.
    ///
    /// A tuple carried a rank only `url` used and a name only `hash` used,
    /// so two of the three slots were dead in every arm.
    enum Collecting {
        Size,
        Hash(String),
        Url(i64),
    }
    let mut collecting: Option<Collecting> = None;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Err(_) | Ok(Event::Eof) => break,

            Ok(Event::Start(tag)) => {
                depth += 1;
                let name = local_name(tag.name().as_ref());
                match name.as_str() {
                    "metalink" => seen_metalink = true,
                    "file" if !finished => {
                        in_file = true;
                        file_depth = depth;
                        filename = tag
                            .try_get_attribute("name")
                            .ok()
                            .flatten()
                            .map(|a| crate::probe::sanitize(&a.value))
                            .unwrap_or_default();
                    }
                    "size" if in_file => collecting = Some(Collecting::Size),
                    "hash" if in_file => {
                        collecting = Some(Collecting::Hash(
                            attribute(&tag, "type").unwrap_or_default(),
                        ));
                    }
                    "url" if in_file => collecting = Some(Collecting::Url(rank_of(&tag))),
                    _ => {}
                }
                text.clear();
            }

            Ok(Event::Text(chunk)) => {
                if collecting.is_some() {
                    // Escaped in the document; `&amp;` in a mirror URL is
                    // ordinary rather than exotic.
                    let raw = chunk.xml10_content();
                    match quick_xml::escape::unescape(&raw) {
                        Ok(decoded) => text.push_str(&decoded),
                        Err(_) => text.push_str(&raw),
                    }
                }
            }

            Ok(Event::End(tag)) => {
                let name = local_name(tag.name().as_ref());
                if let Some(what) = collecting.take() {
                    let body = text.trim();
                    match what {
                        Collecting::Size => size = body.parse::<u64>().ok().filter(|n| *n > 0),
                        // `sha-256` and `sha256` both appear; the parser wants
                        // the latter, and a bare digest is read by its length
                        // when the type attribute says nothing usable.
                        Collecting::Hash(named) => {
                            let cleaned: String =
                                body.chars().filter(|c| !c.is_whitespace()).collect();
                            let named = named.replace('-', "");
                            if let Some(parsed) = Checksum::parse(&format!("{named}:{cleaned}"))
                                .or_else(|| Checksum::parse(&cleaned))
                            {
                                hashes.push(parsed);
                            }
                        }
                        // dm speaks HTTP and nothing else, so an FTP mirror is
                        // not a mirror.
                        Collecting::Url(rank) => {
                            let lower = body.to_ascii_lowercase();
                            if lower.starts_with("http://") || lower.starts_with("https://") {
                                ranked.push((rank, body.to_string()));
                            }
                        }
                    }
                }
                text.clear();

                if name == "file" && in_file && depth == file_depth {
                    in_file = false;
                    // The first file is the one dm takes; the rest of the
                    // document is somebody else's download.
                    finished = true;
                }
                depth = depth.saturating_sub(1);
            }

            _ => {}
        }
    }

    if !seen_metalink || ranked.is_empty() {
        return None;
    }

    // Stable, so mirrors of equal rank keep the order the document gave them.
    ranked.sort_by_key(|(rank, _)| *rank);
    // Longest digest is the strongest: sha-512 over sha-256 over md5.
    hashes.sort_by_key(|c| std::cmp::Reverse(c.expected.len()));

    Some(Metalink {
        filename,
        size,
        checksum: hashes.into_iter().next(),
        mirrors: ranked.into_iter().map(|(_, url)| url).collect(),
    })
}

/// Namespaced documents are common, and `metalink:file` is still a file.
fn local_name(raw: &str) -> String {
    let name = raw.to_ascii_lowercase();
    name.rsplit(':').next().unwrap_or(&name).to_string()
}

fn attribute(tag: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    tag.try_get_attribute(name)
        .ok()
        .flatten()
        .map(|a| a.value.trim().to_ascii_lowercase())
}

/// Both rankings, turned into one where smaller is better.
fn rank_of(tag: &quick_xml::events::BytesStart) -> i64 {
    if let Some(priority) = attribute(tag, "priority").and_then(|v| v.parse::<i64>().ok()) {
        return priority;
    }
    if let Some(preference) = attribute(tag, "preference").and_then(|v| v.parse::<i64>().ok()) {
        return 1000 - preference;
    }
    UNRANKED
}

#[cfg(test)]
mod tests {
    use super::*;

    const V4: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
      <metalink xmlns="urn:ietf:params:xml:ns:metalink">
        <file name="example.iso">
          <size>1048576</size>
          <hash type="md5">900150983cd24fb0d6963f7d28e17f72</hash>
          <hash type="sha-256">ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad</hash>
          <url priority="10">https://slow.example.com/example.iso</url>
          <url priority="1">https://fast.example.com/example.iso</url>
          <url priority="5">ftp://ftp.example.com/example.iso</url>
        </file>
      </metalink>"#;

    const V3: &str = r#"<?xml version="1.0"?>
      <metalink version="3.0" xmlns="http://www.metalinker.org/">
        <files>
          <file name="example.iso">
            <size>1048576</size>
            <verification>
              <hash type="sha256">ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad</hash>
            </verification>
            <resources>
              <url type="http" preference="10">https://slow.example.com/a.iso</url>
              <url type="http" preference="90">https://fast.example.com/a.iso</url>
            </resources>
          </file>
        </files>
      </metalink>"#;

    #[test]
    fn a_url_or_a_content_type_can_say_it_is_a_mirror_list() {
        assert!(is_metalink("https://e.com/a.meta4", None));
        assert!(is_metalink("https://e.com/a.metalink?x=1", None));
        assert!(is_metalink(
            "https://e.com/a",
            Some("application/metalink4+xml")
        ));
        assert!(!is_metalink(
            "https://e.com/a.iso",
            Some("application/octet-stream")
        ));
    }

    #[test]
    fn version_four_ranks_mirrors_by_priority_smallest_first() {
        let read = parse(V4).unwrap();
        assert_eq!(read.filename, "example.iso");
        assert_eq!(read.size, Some(1048576));
        assert_eq!(
            read.mirrors,
            [
                "https://fast.example.com/example.iso",
                "https://slow.example.com/example.iso"
            ],
            "the ftp mirror is not one dm can use"
        );
    }

    #[test]
    fn version_three_ranks_mirrors_by_preference_largest_first() {
        // The two rankings run in opposite directions, which is the whole
        // reason this is worth a test rather than a glance.
        let read = parse(V3).unwrap();
        assert_eq!(
            read.mirrors,
            [
                "https://fast.example.com/a.iso",
                "https://slow.example.com/a.iso"
            ]
        );
        assert_eq!(read.size, Some(1048576));
    }

    #[test]
    fn the_strongest_digest_offered_is_the_one_kept() {
        // Files routinely publish md5 alongside sha-256, and checking the md5
        // when a sha-256 was available is a wasted opportunity.
        let read = parse(V4).unwrap();
        let checksum = read.checksum.unwrap();
        assert_eq!(checksum.algorithm, crate::checksum::Algorithm::Sha256);
        assert_eq!(checksum.expected.len(), 64);
    }

    #[test]
    fn a_hyphenated_algorithm_name_is_still_that_algorithm() {
        // `sha-256` in the document, `sha256` in the parser.
        assert_eq!(
            parse(V4).unwrap().checksum.unwrap().algorithm,
            crate::checksum::Algorithm::Sha256
        );
    }

    #[test]
    fn only_the_first_file_is_taken() {
        /* A Metalink may list a whole release. Queueing all of it from one
         * click would be a surprise, so the second file is ignored. */
        let two = V4.replace(
            "</file>",
            r#"</file><file name="other.iso"><url>https://e.com/other.iso</url></file>"#,
        );
        let read = parse(&two).unwrap();
        assert_eq!(read.filename, "example.iso");
        assert!(read.mirrors.iter().all(|m| m.contains("example.iso")));
    }

    #[test]
    fn a_name_cannot_walk_out_of_the_download_folder() {
        /* The `name` attribute is attacker-controlled exactly as a
         * Content-Disposition is, so it goes through the same sanitiser —
         * which is also why the leading dot goes and a download cannot
         * arrive hidden. */
        let nasty = V4.replace(r#"name="example.iso""#, r#"name="../../.bashrc""#);
        assert_eq!(parse(&nasty).unwrap().filename, "bashrc");

        let worse = V4.replace(r#"name="example.iso""#, r#"name="/etc/passwd""#);
        assert_eq!(parse(&worse).unwrap().filename, "passwd");

        let reserved = V4.replace(r#"name="example.iso""#, r#"name="nul.iso""#);
        assert_eq!(parse(&reserved).unwrap().filename, "_nul.iso");
    }

    #[test]
    fn a_document_with_no_usable_mirror_is_not_a_metalink() {
        let ftp_only = V4.replace("https://", "ftp://");
        assert_eq!(parse(&ftp_only), None);
        assert_eq!(parse("<html>not a metalink</html>"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn a_mirror_with_no_ranking_sits_between_the_ranked_ones() {
        let mixed = r#"<metalink><file name="a">
            <url priority="900">https://last.example.com/a</url>
            <url>https://middle.example.com/a</url>
            <url priority="1">https://first.example.com/a</url>
        </file></metalink>"#;
        assert_eq!(
            parse(mixed).unwrap().mirrors,
            [
                "https://first.example.com/a",
                "https://middle.example.com/a",
                "https://last.example.com/a"
            ]
        );
    }

    #[test]
    fn a_namespaced_document_reads_the_same_as_a_bare_one() {
        let prefixed = r#"<m:metalink xmlns:m="urn:ietf:params:xml:ns:metalink">
            <m:file name="a.iso"><m:size>10</m:size>
            <m:url>https://e.com/a.iso</m:url></m:file></m:metalink>"#;
        let read = parse(prefixed).unwrap();
        assert_eq!(read.filename, "a.iso");
        assert_eq!(read.size, Some(10));
        assert_eq!(read.mirrors, ["https://e.com/a.iso"]);
    }
}
