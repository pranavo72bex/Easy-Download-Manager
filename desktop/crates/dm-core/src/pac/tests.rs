// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn answers_are_read_in_order_with_their_kinds() {
    let hops = parse_result("PROXY a.example:8080; socks5 b.example:1080 ; DIRECT").unwrap();
    assert_eq!(
        hops,
        vec![
            Hop::Via {
                kind: Kind::Http,
                host: "a.example".into(),
                port: 8080
            },
            Hop::Via {
                kind: Kind::Socks5,
                host: "b.example".into(),
                port: 1080
            },
            Hop::Direct,
        ]
    );
    assert_eq!(
        parse_result("SOCKS4 [::1]:9").unwrap(),
        vec![Hop::Via {
            kind: Kind::Socks4,
            host: "::1".into(),
            port: 9
        }]
    );
}

#[test]
fn an_answer_that_is_not_one_is_refused() {
    for bad in [
        "",
        " ; ",
        "PROXY nohost",
        "PROXY a:0",
        "PROXY a:99999",
        "FTP a:1",
        "PROXY :80",
        "undefined",
    ] {
        assert!(parse_result(bad).is_err(), "{bad:?} was taken");
    }
}

#[test]
fn the_helpers_answer_as_a_script_expects() {
    assert!(sh_exp_match("www.example.com", "*.example.com"));
    assert!(sh_exp_match("a.b", "a?b"));
    assert!(!sh_exp_match("example.com", "*.example.com"));
    assert!(dns_domain_is("WWW.Example.com", ".example.com"));
    assert!(!dns_domain_is("example.org", ".example.com"));
    assert!(is_plain_host_name("intranet"));
    assert!(!is_plain_host_name("a.b"));
    assert!(local_host_or_domain_is("www", "www.example.com"));
    assert!(!local_host_or_domain_is("ftp", "www.example.com"));
    assert!(is_in_net("10.1.2.3", "10.0.0.0", "255.0.0.0"));
    assert!(!is_in_net("11.1.2.3", "10.0.0.0", "255.0.0.0"));
    assert!(!is_in_net("not an address", "10.0.0.0", "255.0.0.0"));
}

#[test]
fn a_script_that_imports_or_hides_its_words_is_refused_before_it_runs() {
    for bad in [
        "function FindProxyForURL(u,h){import('qjs:os').then(o=>o.exec(['/bin/sh']));return 'DIRECT';}",
        "import * as os from 'qjs:os'; function FindProxyForURL(){return 'DIRECT';}",
        "function FindProxyForURL(){var s='\\u0069mport';return 'DIRECT';}",
    ] {
        assert!(refused(bad).is_some(), "{bad}");
    }
    // Words that only contain it, as a proxy script may name them, are fine.
    assert_eq!(
        refused("function FindProxyForURL(u,h){var important=1, imports_=2; return 'DIRECT';}"),
        None
    );
}

/// In dm's own interpreter: an ordinary script answers, `eval` and `import` are not there,
/// and code a script makes from text (`Function`) runs inside the same interpreter, reaching
/// nothing more than the script itself (Windows' Chakra had it switched off instead).
#[tokio::test]
async fn in_dm_js_a_script_answers_and_made_code_reaches_nothing_more() {
    let ok = evaluate_here(
        "function FindProxyForURL(u,h){return shExpMatch(h,'*.corp')&&isInNet('10.1.2.3','10.0.0.0','255.0.0.0')?'PROXY p:8080':'DIRECT';}",
        "http://a.corp/",
        "a.corp",
    )
    .await
    .unwrap();
    assert_eq!(
        ok,
        [Hop::Via {
            kind: crate::proxy::Kind::Http,
            host: "p".into(),
            port: 8080
        }]
    );
    for refused in [
        "function FindProxyForURL(){return eval('\"DIRECT\"');}",
        "function FindProxyForURL(){return import('os');}",
        "function FindProxyForURL(){return require('fs');}",
    ] {
        assert!(
            evaluate_here(refused, "http://a/", "a").await.is_err(),
            "{refused}"
        );
    }
    let made = evaluate_here(
        "function FindProxyForURL(){return (function(){}).constructor('return \"PROXY q:1\"')();}",
        "http://a/",
        "a",
    )
    .await
    .unwrap();
    assert_eq!(
        made,
        [Hop::Via {
            kind: crate::proxy::Kind::Http,
            host: "q".into(),
            port: 1
        }]
    );
}

/// In dm's own interpreter (dm-js): an ordinary script answers, and one that imports is
/// refused before it runs.
#[tokio::test]
async fn in_dm_js_a_script_answers_and_an_import_is_refused() {
    let script = "import * as os from 'qjs:os'; function FindProxyForURL(){return 'DIRECT';}";
    let why = evaluate_here(script, "http://a/", "a").await.unwrap_err();
    assert!(why.contains("import a module"), "{why}");
    let ok = evaluate_here(
        "function FindProxyForURL(u,h){return shExpMatch(h,'*.corp')&&isInNet('10.1.2.3','10.0.0.0','255.0.0.0')?'PROXY p:8080; DIRECT':'DIRECT';}",
        "http://a.corp/",
        "a.corp",
    )
    .await
    .unwrap();
    assert_eq!(
        ok,
        [
            Hop::Via {
                kind: Kind::Http,
                host: "p".into(),
                port: 8080
            },
            Hop::Direct
        ]
    );
}

#[test]
fn the_first_way_of_an_answer_is_the_one_taken() {
    assert_eq!(
        first_way(&[]),
        Err("the script answered nothing".to_string())
    );
    assert_eq!(first_way(&[Hop::Direct]), Ok(None));
    let via = |kind, host: &str| Hop::Via {
        kind,
        host: host.into(),
        port: 1080,
    };
    let v6 = first_way(&[via(Kind::Socks5, "::1"), Hop::Direct])
        .unwrap()
        .unwrap();
    assert_eq!(v6.as_str(), "socks5h://[::1]:1080");
    let named = first_way(&[via(Kind::Http, "p.example")]).unwrap().unwrap();
    assert_eq!(named.as_str(), "http://p.example:1080/");
    let why = first_way(&[via(Kind::System, "p")]).unwrap_err();
    assert!(why.contains("not one"), "{why}");
}

#[tokio::test]
async fn a_script_is_read_from_the_web_or_a_file_and_one_that_is_none_is_refused() {
    use axum::routing::get;
    let app = axum::Router::new()
        .route(
            "/proxy.pac",
            get(|| async { "function FindProxyForURL(u, h) { return 'DIRECT'; }" }),
        )
        .route(
            "/page.html",
            get(|| async { "<html>no script here</html>" }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let text = text_of(&format!(" http://{addr}/proxy.pac "))
        .await
        .unwrap();
    assert!(text.contains("FindProxyForURL"), "{text}");
    let why = text_of(&format!("http://{addr}/missing.pac"))
        .await
        .unwrap_err();
    assert!(why.contains("could not be fetched"), "{why}");
    let why = text_of(&format!("http://{addr}/page.html"))
        .await
        .unwrap_err();
    assert!(why.contains("no FindProxyForURL"), "{why}");

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("local.pac");
    std::fs::write(&file, "function FindProxyForURL(){return 'DIRECT';}").unwrap();
    let by_address = url::Url::from_file_path(&file).unwrap().to_string();
    assert!(text_of(&by_address).await.is_ok());
    let long = dir.path().join("long.pac");
    std::fs::write(&long, vec![b' '; SCRIPT_MOST + 1]).unwrap();
    assert_eq!(
        text_of(&long.display().to_string()).await,
        Err("the proxy script is too long to be one".to_string())
    );
    let why = text_of(&dir.path().join("none.pac").display().to_string())
        .await
        .unwrap_err();
    assert!(why.contains("could not be read"), "{why}");
}
