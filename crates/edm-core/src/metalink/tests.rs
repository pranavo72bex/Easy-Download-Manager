// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

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
        "the ftp mirror is not one EDM can use"
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
fn a_digest_of_one_piece_is_not_taken_for_the_whole_file() {
    /* A sha-512 of one piece outranked the file's sha-256 by length, and
     * a correct download failed the check it was held to. */
    let file = "a".repeat(64);
    let piece = "b".repeat(128);
    let both = format!(
        r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink"><file name="f.bin">
        <pieces length="262144" type="sha-512"><hash>{piece}</hash></pieces>
        <hash type="sha-256">{file}</hash>
        <url>http://h/f.bin</url></file></metalink>"#
    );
    let checksum = parse(&both).unwrap().checksum.unwrap();
    assert_eq!(checksum.expected, file);
    // Pieces alone say nothing about the whole file.
    let only_pieces = format!(
        r#"<metalink xmlns="urn:ietf:params:xml:ns:metalink"><file name="f.bin">
        <pieces length="262144" type="sha-512"><hash>{piece}</hash></pieces>
        <url>http://h/f.bin</url></file></metalink>"#
    );
    assert_eq!(parse(&only_pieces).unwrap().checksum, None);
    // Version 3 keeps both under <verification>; the same rule holds.
    let v3 = format!(
        r#"<metalink version="3.0" xmlns="http://www.metalinker.org/"><files><file name="f.bin">
        <verification><hash type="sha256">{file}</hash>
        <pieces length="262144" type="sha512"><hash piece="0">{piece}</hash></pieces>
        </verification><resources><url type="http">http://h/f.bin</url></resources>
        </file></files></metalink>"#
    );
    assert_eq!(parse(&v3).unwrap().checksum.unwrap().expected, file);
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

/// A signed mirror address carries its `&` escaped, as XML needs; it was
/// read with the escapes dropped, `?a=1b=2`, and every such mirror failed.
#[test]
fn an_escaped_ampersand_in_a_mirror_address_is_kept() {
    let signed = r#"<metalink><file name="a.iso">
        <url>https://e.com/a.iso?a=1&amp;b=2&#38;c=&#x33;&lt;</url></file></metalink>"#;
    assert_eq!(
        parse(signed).unwrap().mirrors,
        ["https://e.com/a.iso?a=1&b=2&c=3<"]
    );
}
