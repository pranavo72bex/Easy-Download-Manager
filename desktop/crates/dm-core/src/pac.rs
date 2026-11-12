// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A proxy configuration script (PAC): what `FindProxyForURL` answers, and running it.
//!
//! The script is run in the computer's own JavaScript engine where there is one (Windows' own
//! Chakra, `crate::javascript`, as reference A runs proxy scripts in Windows' JScript), else by
//! QuickJS (`tools.rs`) or an installed Deno, in a program of its own with the helpers a script
//! expects (`shExpMatch`, `dnsDomainIs`, `isInNet`, `isPlainHostName`, `localHostOrDomainIs`)
//! written in front of it; the answer is read back by `parse_result`. A script that fails, takes
//! too long or answers nothing usable is an error: the caller refuses the download rather than
//! going around the proxy (a failed script never means DIRECT).

#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

use crate::proxy::Kind;

/// One way a script says a download may go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hop {
    /// Straight to the site.
    Direct,
    /// Through a proxy.
    Via {
        /// What kind it is.
        kind: Kind,
        /// Its host.
        host: String,
        /// Its port.
        port: u16,
    },
}

/// Reads a script's answer: `DIRECT`, `PROXY h:p`, `HTTPS h:p`, `SOCKS h:p`, `SOCKS4 h:p`,
/// `SOCKS5 h:p`, several joined by `;`, tried in order. Keywords are not case-sensitive.
pub fn parse_result(answer: &str) -> Result<Vec<Hop>, String> {
    let mut hops = Vec::new();
    for part in answer.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (word, rest) = match part.split_once(char::is_whitespace) {
            Some((w, r)) => (w, r.trim()),
            None => (part, ""),
        };
        let kind = match word.to_ascii_uppercase().as_str() {
            "DIRECT" => {
                hops.push(Hop::Direct);
                continue;
            }
            "PROXY" | "HTTP" => Kind::Http,
            "HTTPS" => Kind::Https,
            "SOCKS" | "SOCKS5" => Kind::Socks5,
            "SOCKS4" => Kind::Socks4,
            _ => {
                return Err(format!(
                    "the script answered \"{part}\", which is not a proxy"
                ))
            }
        };
        let (host, port) = rest
            .rsplit_once(':')
            .ok_or_else(|| format!("\"{part}\" has no port"))?;
        let port: u16 = port
            .trim()
            .parse()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| format!("\"{part}\" has no usable port"))?;
        let host = host.trim().trim_start_matches('[').trim_end_matches(']');
        if host.is_empty() {
            return Err(format!("\"{part}\" has no host"));
        }
        hops.push(Hop::Via {
            kind,
            host: host.to_string(),
            port,
        });
    }
    if hops.is_empty() {
        return Err("the script answered nothing".into());
    }
    Ok(hops)
}

/// `shExpMatch`: `*` is any run of characters, `?` one; the rest matches itself.
pub fn sh_exp_match(text: &str, pattern: &str) -> bool {
    fn go(t: &[char], p: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => {
                (0..=t.len()).any(|n| t.get(n..).is_some_and(|tail| go(tail, rest)))
            }
            Some((c, rest)) => match t.split_first() {
                Some((x, tail)) => (*c == '?' || c == x) && go(tail, rest),
                None => false,
            },
        }
    }
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    go(&t, &p)
}

/// `dnsDomainIs`: the host ends with the domain (`.example.com`).
pub fn dns_domain_is(host: &str, domain: &str) -> bool {
    host.to_ascii_lowercase()
        .ends_with(&domain.to_ascii_lowercase())
}

/// `isPlainHostName`: no dot in it.
pub fn is_plain_host_name(host: &str) -> bool {
    !host.contains('.')
}

/// `localHostOrDomainIs`: the host is the name, or is the name's first label alone.
pub fn local_host_or_domain_is(host: &str, name: &str) -> bool {
    host.eq_ignore_ascii_case(name)
        || (!host.contains('.')
            && name
                .split('.')
                .next()
                .is_some_and(|first| first.eq_ignore_ascii_case(host)))
}

/// `isInNet` for IPv4: the address lies in the network `pattern` with the mask.
pub fn is_in_net(address: &str, pattern: &str, mask: &str) -> bool {
    let (Ok(a), Ok(p), Ok(m)) = (
        address.parse::<std::net::Ipv4Addr>(),
        pattern.parse::<std::net::Ipv4Addr>(),
        mask.parse::<std::net::Ipv4Addr>(),
    ) else {
        return false;
    };
    u32::from(a) & u32::from(m) == u32::from(p) & u32::from(m)
}

/// The helpers, in JavaScript, to stand in front of the script. The names that need the network
/// (`dnsResolve`, `myIpAddress`) answer nothing, so a script cannot reach out from its sandbox.
const HELPERS: &str = r#"
function isPlainHostName(h){return String(h).indexOf('.')<0;}
function dnsDomainIs(h,d){h=String(h).toLowerCase();d=String(d).toLowerCase();return h.length>=d.length&&h.substring(h.length-d.length)===d;}
function localHostOrDomainIs(h,n){h=String(h).toLowerCase();n=String(n).toLowerCase();return h===n||(h.indexOf('.')<0&&n.split('.')[0]===h);}
function shExpMatch(s,p){var r='^'+String(p).replace(/[.+^${}()|[\]\\]/g,'\\$&').replace(/\*/g,'.*').replace(/\?/g,'.')+'$';return new RegExp(r).test(String(s));}
function __ip(s){var m=/^(\d+)\.(\d+)\.(\d+)\.(\d+)$/.exec(String(s));if(!m)return null;return ((+m[1]*256+ +m[2])*256+ +m[3])*256+ +m[4];}
function isInNet(a,p,m){var x=__ip(a),y=__ip(p),z=__ip(m);if(x===null||y===null||z===null)return false;for(var b=31;b>=0;b--){var bit=Math.pow(2,b);if(Math.floor(z/bit)%2===1&&Math.floor(x/bit)%2!==Math.floor(y/bit)%2)return false;}return true;}
function dnsResolve(h){return null;}
function myIpAddress(){return '127.0.0.1';}
function isResolvable(h){return false;}
function dnsDomainLevels(h){return String(h).split('.').length-1;}
function weekdayRange(){return true;}
function dateRange(){return true;}
function timeRange(){return true;}
"#;

/// Why a script is not run at all: one that names `import` (the way to QuickJS's modules for
/// files and programs, which no proxy script needs), or that hides words behind `\u` escapes.
pub fn refused(script: &str) -> Option<&'static str> {
    let words = script
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .any(|word| word == "import");
    if words {
        return Some(
            "the proxy script asks to import a module, which a proxy script has no need of",
        );
    }
    if script.contains("\\u") {
        return Some("the proxy script hides words behind \\u escapes");
    }
    None
}

/// Runs `script` for `url` in the computer's own JavaScript engine (Windows' own Chakra, in a
/// process of its own in the app: `crate::javascript`), with the helpers in front of it and the
/// answer taken as the value of `FindProxyForURL`. That engine is run with `eval` and the function
/// constructors off and holds nothing of the computer. Fails closed: any trouble is an error,
/// never `DIRECT`.
pub async fn evaluate_here(script: &str, url: &str, host: &str) -> Result<Vec<Hop>, String> {
    if let Some(why) = refused(script) {
        return Err(why.into());
    }
    let quote = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    let call = format!("String(FindProxyForURL({}, {}))", quote(url), quote(host));
    let answer = crate::javascript::run(
        &format!(
            "{HELPERS}
{script}
"
        ),
        &call,
        std::time::Duration::from_secs(5),
    )
    .await
    .map_err(|e| format!("the proxy script failed: {e}"))?;
    parse_result(answer.trim())
}

/// The address a request is sent to when the script gave it no way: this computer's discard
/// port, where nothing listens, so the request fails, and never goes out directly in its place
/// nor anywhere off this computer.
pub const NO_WAY: &str = "http://127.0.0.1:9";

/// What the scripts said, for this session.
#[derive(Default)]
struct Kept {
    /// Each script's text, by its address.
    texts: std::collections::HashMap<String, String>,
    /// The way to each site, by the script's address and the site (`scheme://host:port`): a
    /// proxy, `None` for straight there, or why there is none.
    ways: std::collections::HashMap<(String, String), Result<Option<url::Url>, String>>,
}

static KEPT: std::sync::LazyLock<std::sync::Mutex<Kept>> =
    std::sync::LazyLock::new(Default::default);

/// One lock for each script and site, held while the script is fetched or run for that site, so
/// a site is asked about once however many requests to it start together. One lock for them all
/// held every other site's first request behind a slow script or a slow server of one (gaps row
/// 184); each is kept for the session, as the answer it guards is.
static ASKING: std::sync::LazyLock<std::sync::Mutex<SiteLocks>> =
    std::sync::LazyLock::new(Default::default);

/// A lock for each script's address and site (`ASKING`).
type SiteLocks = std::collections::HashMap<(String, String), std::sync::Arc<std::sync::Mutex<()>>>;

/// Forgets every script and answer, so each is fetched and asked again: the settings changed.
pub fn forget_all() {
    *crate::lock(&KEPT) = Kept::default();
}

/// The site a script is asked about for `target`: its scheme, host and port, with no path (a
/// path can carry a session, and the answer is kept for the whole site).
fn site_of(target: &url::Url) -> Option<String> {
    let host = target.host_str()?;
    let port = target.port_or_known_default().unwrap_or(0);
    Some(format!(
        "{}://{}:{port}",
        target.scheme(),
        host.to_ascii_lowercase()
    ))
}

/// Why a script gave no way to `target`'s site, if one did not: said in place of "the
/// connection was refused".
pub fn refusal(target: &url::Url) -> Option<String> {
    let site = site_of(target)?;
    crate::lock(&KEPT)
        .ways
        .iter()
        .find_map(|((_, s), way)| (*s == site).then(|| way.clone().err()).flatten())
}

/// Where a request to `target` goes under the script at `address` (a web address, or a file's):
/// through the proxy it names (`Ok(Some)`), straight there (`Ok(None)`), or nowhere (why).
///
/// The first request to a site fetches and runs the script, on a thread of its own, and the
/// answer is kept for the session; this waits for it, so it can be called where nothing can be
/// awaited (reqwest's choice of proxy). The first way the script names is the one taken.
pub fn way_to(address: &str, target: &url::Url) -> Result<Option<url::Url>, String> {
    let site = site_of(target).ok_or_else(|| "the address has no host".to_string())?;
    let key = (address.to_string(), site.clone());
    if let Some(way) = crate::lock(&KEPT).ways.get(&key) {
        return way.clone();
    }
    let ask = || {
        let site_lock = std::sync::Arc::clone(crate::lock(&ASKING).entry(key.clone()).or_default());
        let _one = crate::lock(&site_lock);
        if let Some(way) = crate::lock(&KEPT).ways.get(&key) {
            return way.clone();
        }
        let host = target.host_str().unwrap_or_default().to_string();
        let (address, site) = (address.to_string(), site.clone());
        let way = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("the proxy script could not be run: {e}"))?;
            runtime.block_on(async move {
                let text = text_of(&address).await?;
                let hops = evaluate_here(&text, &format!("{site}/"), &host).await?;
                first_way(&hops)
            })
        })
        .join()
        .unwrap_or_else(|_| Err("the proxy script could not be run".into()));
        crate::lock(&KEPT).ways.insert(key.clone(), way.clone());
        way
    };
    // Waiting holds a thread the runtime may need: on one with several, the others take over
    // its work meanwhile.
    match tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(ask),
        _ => ask(),
    }
}

/// The first way of a script's answer, as reqwest takes a proxy.
fn first_way(hops: &[Hop]) -> Result<Option<url::Url>, String> {
    match hops.first() {
        None => Err("the script answered nothing".into()),
        Some(Hop::Direct) => Ok(None),
        Some(Hop::Via { kind, host, port }) => {
            let host = if host.contains(':') {
                format!("[{host}]")
            } else {
                host.clone()
            };
            url::Url::parse(&format!("{}://{host}:{port}", kind.scheme()))
                .map(Some)
                .map_err(|e| format!("the script named a proxy that is not one: {e}"))
        }
    }
}

/// The most of a script that is read: a script is a few kilobytes.
const SCRIPT_MOST: usize = 1024 * 1024;

/// The script at `address`, fetched once a session: a web address is fetched directly (the
/// script says where the proxy is, so it cannot be fetched through it), a `file:` address or a
/// path is read.
async fn text_of(address: &str) -> Result<String, String> {
    let kept = crate::lock(&KEPT).texts.get(address).cloned();
    if let Some(text) = kept {
        return Ok(text);
    }
    let trimmed = address.trim();
    let bytes = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let reply = crate::client()
            .get(trimmed)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| {
                format!(
                    "the proxy script could not be fetched: {}",
                    crate::error::network(&e)
                )
            })?;
        reply
            .bytes()
            .await
            .map_err(|e| format!("the proxy script could not be fetched: {e}"))?
            .to_vec()
    } else {
        let path = match url::Url::parse(trimmed) {
            Ok(u) if u.scheme() == "file" => u
                .to_file_path()
                .map_err(|_| "the proxy script's address names no file".to_string())?,
            _ => std::path::PathBuf::from(trimmed),
        };
        tokio::fs::read(&path)
            .await
            .map_err(|e| format!("the proxy script could not be read: {e}"))?
    };
    if bytes.len() > SCRIPT_MOST {
        return Err("the proxy script is too long to be one".into());
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !text.contains("FindProxyForURL") {
        return Err("that address holds no proxy script (no FindProxyForURL)".into());
    }
    crate::lock(&KEPT)
        .texts
        .insert(address.to_string(), text.clone());
    Ok(text)
}

#[cfg(test)]
mod tests {
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

    /// In Windows' own engine: an ordinary script answers, and one reaching for a way to make
    /// code from text fails, never answering DIRECT.
    #[cfg(windows)]
    #[tokio::test]
    async fn in_windows_own_engine_a_script_answers_and_cannot_make_code() {
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
        for reach in [
            "function FindProxyForURL(){return (function(){}).constructor('return \"DIRECT\"')();}",
            "function FindProxyForURL(){return Object.constructor('return \"DIRECT\"')();}",
            "function FindProxyForURL(){return (async function(){}).constructor.name;}",
            "function FindProxyForURL(){return eval('\"DIRECT\"');}",
            "function FindProxyForURL(){return import('os');}",
        ] {
            assert!(
                evaluate_here(reach, "http://a/", "a").await.is_err(),
                "{reach}"
            );
        }
    }

    /// In the computer's own engine, where it has one (JavaScriptCore on macOS, WebKitGTK's on
    /// Linux): an ordinary script answers, and one that imports is refused before it runs.
    #[cfg(not(windows))]
    #[tokio::test]
    async fn in_the_computers_own_engine_a_script_answers_and_an_import_is_refused() {
        let script = "import * as os from 'qjs:os'; function FindProxyForURL(){return 'DIRECT';}";
        let why = evaluate_here(script, "http://a/", "a").await.unwrap_err();
        assert!(why.contains("import a module"), "{why}");
        if !crate::javascript::system_engine_here() {
            eprintln!("not run further: {}", crate::javascript::NO_ENGINE);
            return;
        }
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
}
