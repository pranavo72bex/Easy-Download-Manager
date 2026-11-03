//! A proxy configuration script (PAC): what `FindProxyForURL` answers, and running it.
//!
//! The script is run by QuickJS (`tools.rs`), in a program of its own with the helpers a script
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
const PRELUDE: &str = r#"
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

/// The whole program to run: the helpers, the script, and a line printing its answer for `url`.
pub fn program(script: &str, url: &str, host: &str) -> String {
    let quote = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    format!(
        "{PRELUDE}\n{script}\nprint(String(FindProxyForURL({}, {})));\n",
        quote(url),
        quote(host)
    )
}

/// Runs `script` for `url` with the JavaScript engine at `engine` (QuickJS), the program written
/// in `staging` for the run. Fails closed: any trouble is an error, never `DIRECT`.
pub async fn evaluate(
    engine: &std::path::Path,
    script: &str,
    url: &str,
    host: &str,
    staging: &std::path::Path,
) -> Result<Vec<Hop>, String> {
    tokio::fs::create_dir_all(staging)
        .await
        .map_err(|e| format!("no room for the proxy script: {e}"))?;
    let file = staging.join(format!(".dm-pac-{}.js", std::process::id()));
    crate::videopage::write_private(&file, &program(script, url, host))
        .await
        .map_err(|e| format!("could not write the proxy script: {e}"))?;
    let _gone = crate::videopage::RemoveOnDrop(file.clone());
    let mut command = tokio::process::Command::new(engine);
    command.arg(&file);
    crate::no_window_async(&mut command);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(5), command.output())
        .await
        .map_err(|_| "the proxy script took too long".to_string())?
        .map_err(|e| format!("the proxy script would not run: {e}"))?;
    if !output.status.success() {
        return Err("the proxy script failed".into());
    }
    parse_result(String::from_utf8_lossy(&output.stdout).trim())
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
    fn the_program_carries_the_script_and_asks_it_about_the_address() {
        let p = program(
            "function FindProxyForURL(u,h){return 'DIRECT';}",
            "http://a/\"x",
            "a",
        );
        assert!(p.contains("function FindProxyForURL"));
        assert!(p.contains(r#"FindProxyForURL("http://a/\"x", "a")"#));
        assert!(p.contains("function shExpMatch"));
    }

    #[tokio::test]
    async fn a_missing_engine_is_an_error_and_never_direct() {
        let dir = tempfile::tempdir().unwrap();
        let r = evaluate(
            std::path::Path::new("/no/such/qjs"),
            "function FindProxyForURL(){return 'DIRECT';}",
            "http://a/",
            "a",
            dir.path(),
        )
        .await;
        assert!(r.is_err());
    }
}
