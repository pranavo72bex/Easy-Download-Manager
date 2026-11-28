// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_challenge_is_read_with_its_quotes_and_commas() {
    let c = challenge(
        r#"Digest realm="files, inc", qop="auth,auth-int", nonce="abc\"def", opaque="xyz", algorithm=SHA-256, stale=TRUE"#,
    )
    .unwrap();
    assert_eq!(c.realm, "files, inc");
    assert_eq!(c.nonce, "abc\"def");
    assert_eq!(c.opaque.as_deref(), Some("xyz"));
    assert!(c.qop);
    assert_eq!(c.algorithm, Algorithm::Sha256);
    assert!(c.stale);
}

#[test]
fn what_edm_does_not_speak_is_not_a_challenge() {
    assert!(challenge("Basic realm=\"x\"").is_none());
    assert!(challenge("Digest realm=\"x\"").is_none(), "no nonce");
    assert!(
        challenge("Digest realm=\"x\", nonce=\"n\", algorithm=SHA-512-256").is_none(),
        "an algorithm EDM has not got"
    );
    assert!(
        challenge("Digest realm=\"x\", nonce=\"n\", qop=\"auth-int\"").is_none(),
        "a body hash on a GET"
    );
    assert!(challenge("Digestive nonce=\"n\"").is_none());
}

#[test]
fn the_rfc_7616_example_is_answered_as_the_rfc_shows() {
    // RFC 7616 §3.9.1, MD5, with its cnonce and nc. The cnonce is
    // EDM's own at run time, so the response is checked by recomputing
    // the RFC's with the RFC's cnonce through the same arithmetic.
    let c = challenge(
        r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#,
    )
    .unwrap();
    let a1 = c
        .algorithm
        .hash("Mufasa:http-auth@example.org:Circle of Life");
    let a2 = c.algorithm.hash("GET:/dir/index.html");
    let response = c.algorithm.hash(&format!(
        "{a1}:{}:00000001:f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ:auth:{a2}",
        c.nonce
    ));
    assert_eq!(response, "8ca523f5e9506fed4657c9700eebdbec");
}

#[test]
fn the_header_names_everything_the_server_needs() {
    let c = Challenge {
        realm: "r".into(),
        nonce: "n".into(),
        opaque: Some("o".into()),
        qop: true,
        algorithm: Algorithm::Sha256,
        stale: false,
    };
    let url = url::Url::parse("http://h/a/b.zip?x=1").unwrap();
    let h = authorization(&c, 3, "GET", &url, "me\"you", "p");
    assert!(h.starts_with("Digest "), "{h}");
    assert!(h.contains(r#"username="me\"you""#), "{h}");
    assert!(h.contains(r#"uri="/a/b.zip?x=1""#), "{h}");
    assert!(h.contains("nc=00000003"), "{h}");
    assert!(h.contains("qop=auth"), "{h}");
    assert!(h.contains(r#"opaque="o""#), "{h}");
    assert!(h.contains("algorithm=SHA-256"), "{h}");
    // Without qop, the older form: no nc or cnonce.
    let old = authorization(
        &Challenge {
            qop: false,
            ..c.clone()
        },
        1,
        "GET",
        &url,
        "me",
        "p",
    );
    assert!(!old.contains("nc="), "{old}");
    assert!(!old.contains("cnonce"), "{old}");
}

#[test]
fn a_login_is_read_out_of_the_address_decoded() {
    use crate::test_secrets::{escaped, marked};
    let password = marked("password");
    assert_eq!(
        login_in(&format!("https://me:{}@h/f", escaped(&password))),
        Some(("me".into(), password.clone()))
    );
    assert_eq!(login_in("https://h/f"), None);
    assert_eq!(login_in("not a url"), None);
}

#[test]
fn a_session_algorithm_hashes_the_login_once_more_with_both_nonces() {
    let c = challenge(
        r#"Digest realm = "r", stale, nonce="n", algorithm=MD5-sess, qop="auth,auth-int""#,
    )
    .unwrap();
    assert_eq!(
        (c.algorithm, c.qop, c.stale, c.realm.as_str()),
        (Algorithm::Md5Sess, true, false, "r")
    );
    let url = url::Url::parse("http://h/f.bin").unwrap();
    let h = authorization(&c, 1, "GET", &url, "me", "pw");
    assert!(h.contains("algorithm=MD5-sess"), "{h}");
    let field = |name: &str| {
        params(h.trim_start_matches("Digest "))
            .into_iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
            .unwrap()
    };
    let (cnonce, response) = (field("cnonce"), field("response"));
    let md5 = |t: &str| Algorithm::Md5.hash(t);
    let a1 = md5(&format!("{}:n:{cnonce}", md5("me:r:pw")));
    let a2 = md5("GET:/f.bin");
    assert_eq!(
        response,
        md5(&format!("{a1}:n:00000001:{cnonce}:auth:{a2}"))
    );
    let sha = Challenge {
        algorithm: Algorithm::Sha256Sess,
        ..c
    };
    assert!(authorization(&sha, 1, "GET", &url, "me", "pw").contains("algorithm=SHA-256-sess"));
    // No algorithm named is MD5.
    assert_eq!(
        challenge(r#"Digest realm="r", nonce="n""#)
            .unwrap()
            .algorithm,
        Algorithm::Md5
    );
}

#[test]
fn client_nonces_differ() {
    assert_ne!(cnonce("n"), cnonce("n"));
}
