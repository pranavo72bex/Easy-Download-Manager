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

/// Held while a script is fetched or run, so a site is asked about once however many requests
/// to it start together.
static ASKING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The engine a test names in place of QuickJS.
static ENGINE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// Runs the scripts with `engine` in place of QuickJS found or fetched (`None` puts that back),
/// and forgets what they said. For tests.
#[doc(hidden)]
pub fn set_engine(engine: Option<std::path::PathBuf>) {
    *crate::lock(&ENGINE) = engine;
    forget_all();
}

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
        let _one = crate::lock(&ASKING);
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
                let engine = engine().await?;
                let text = text_of(&address).await?;
                let hops = evaluate(
                    &engine,
                    &text,
                    &format!("{site}/"),
                    &host,
                    &std::env::temp_dir(),
                )
                .await?;
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

/// The JavaScript engine to run a script with: the one a test named, else QuickJS, found or
/// fetched.
async fn engine() -> Result<std::path::PathBuf, String> {
    let named = crate::lock(&ENGINE).clone();
    if let Some(named) = named {
        return Ok(named);
    }
    crate::tools::ensure(&crate::client(), crate::tools::Tool::QuickJs)
        .await
        .map_err(|e| format!("no JavaScript engine to run the proxy script: {e}"))
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
