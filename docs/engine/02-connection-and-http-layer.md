# 02. Connection and HTTP layer

Goal: every request the engine makes looks like a careful browser's, reaches
the right place by the right route, and tells the transfer layer the truth
about the file.

Standards: RFC 9110 (semantics), 9111 (caching headers used as validators),
9112 (HTTP/1.1), 7233 (ranges), 6265 (cookies), 7616 (Digest), 7617 (Basic),
8446 (TLS 1.3), 1928 and 1929 (SOCKS5), 959 and 3659 (FTP, MDTM and SIZE),
4217 (FTP over TLS).

## Requirements

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| N1 | HTTP/1.1 and HTTPS with certificate checking, TLS 1.2 and 1.3, one keep-alive pool per client, in-process (no system helper). | BUILT | `lib.rs` (client over a Rust HTTP stack and rustls) | Logging server over TLS with a test CA: file is fetched. A server with an expired or wrong-name certificate fails in words and is not retried (`retry::untrusted_certificate`). |
| N2 | `Accept-Encoding: identity` on file requests so byte offsets match the file; a body that arrives encoded anyway is decoded. | BUILT | `decode.rs`, `download/mod.rs` headers | Server that ignores identity and gzips: saved file equals the original. |
| N3 | Probe before planning: size, range support, name, type, validators, final address. The probe is a `GET` with `Range: bytes=0-` whose response is kept as the first piece (no wasted request), or a `HEAD` where a `GET` is refused. | BUILT | `probe.rs` | Logging server: for a one-range file the first request is the only request. |
| N4 | Range support is concluded from the answer: `206` with a matching `Content-Range`, or `Accept-Ranges: bytes` with a length. A `200` to a range request means no ranges. A `416` is handled (file shorter than thought, or already whole). | BUILT | `probe.rs`, `download/ranged.rs` | Four servers: honest ranges; `Accept-Ranges: none`; ranges advertised but ignored; `416` on `bytes=N-` at the end. Each ends byte-exact. |
| N5 | File name from, in order: the person; `Content-Disposition` (`filename*` RFC 8187 before `filename`); the final address path; a type-based extension. Unsafe characters and reserved Windows names are replaced; 240-byte limit. A name that differs from the address is asked about (B39). | BUILT | `probe.rs` (`RESERVED`, `BY_MIME`, `MAX_NAME_BYTES`), `dm-desktop/src/app/naming.rs` | Server replying `Content-Disposition` with UTF-8, quoted, and `../x` names; assert the saved name. |
| N6 | An address that should be a file but answers HTML asks first (B40). | BUILT | `probe.rs`, window | Server answering `text/html` for `/a.zip`. |
| N7 | Redirects are followed by dm itself: at most 10; never https to http; never from a public address to a private or loopback one; `Authorization` and cookies dropped when the host changes; `Referer` cut to its origin across hosts. 301, 302 and 303 on GET keep GET; 307 and 308 keep the method. | BUILT | `redirects.rs` | Logging servers on two hosts: the second sees no secrets. A loop of 11 hops fails in words. An https-to-http hop is refused. A hop to 127.0.0.1 from a public name is refused. |
| N8 | Proxies: HTTP (CONNECT for https), SOCKS4, SOCKS5 (name resolved at the proxy), the computer's settings, none; user and password. An enabled proxy with no host is a refusal. | BUILT | `proxy.rs`, `tests/proxy.rs` | Logging proxy records the CONNECT line, the SOCKS handshake bytes and the login. Downloads through each kind are byte-exact. |
| N9 | Proxy auto-config scripts (PAC). | TO BUILD (DECISION) | none | Needs a JavaScript evaluator; see file 06, option A. Without one, say "PAC scripts are not supported" in words and use the system proxy variables. Test: a PAC fixture that picks a proxy by host; the logging proxy sees only the matching host. |
| N10 | Per-server sign-in: Basic, Digest (MD5, SHA-256, `-sess`, `qop=auth`, `opaque`, stale nonce), NTLMv2, Negotiate answered with NTLM. The challenge is kept per server so later ranges carry it at once. | BUILT | `auth.rs`, `ntlm.rs` | Servers that demand each scheme. A wrong password fails once, in words, and is not retried. Kerberos (Negotiate with a ticket) is TO BUILD and optional (DECISION); its test needs a test KDC. |
| N11 | A login or cookie handed over for one download belongs to that download and that exact host, over https only. | BUILT | `lib.rs`, `redirects.rs`, `docs/decisions.md` 2026-10-25 | Same-host and other-host servers; the other host sees nothing. |
| N12 | Cookies: those the browser handed over (a header string) and a `cookies.txt` file; sent only to a matching host, path and secure flag, unexpired. Cookies a server sets during redirects or the probe (`Set-Cookie`) are kept for that download and sent to later requests for the same host per RFC 6265 (domain, path, secure, expiry). | PARTIAL: handed-over and file cookies BUILT (`cookies.rs`); a jar for `Set-Cookie` is not confirmed. Read `lib.rs` before building. | `cookies.rs` | Server that sets a cookie on the probe's 302 and refuses the file without it: the download succeeds. A cookie set for another domain is ignored. |
| N13 | `User-Agent`: a browser's by default (the one the browser handed over, when there is one); if a site refuses, try the next of a short list (dm's own, a plain one); an "identify as dm only" setting. | BUILT | `lib.rs` (`USER_AGENT`, `FALLBACK_AGENTS`), `tests/identify_as.rs` | Server that returns 403 to every agent but the third: the download succeeds and the log shows three agents in order. |
| N14 | `Referer`: the page the link came from when the browser gave it; none for plain addresses. Whether to send the address's own folder as `Referer` when none is given is a DECISION: the measurement in `docs/reference-behavior.md` shows reference A does. | PARTIAL | `lib.rs`, `redirects.rs` | Server that returns 403 without a `Referer` of its own origin: with the setting on the download works; with it off the failure is in words. |
| N15 | Timeouts: a connect timeout, and a read timeout between bytes (30 s today). A stalled connection is closed and its range retried. | BUILT | `lib.rs` (`READ_TIMEOUT`) | Server that sends 1 KiB then stalls: the range is retried on a new connection within about 31 s. |
| N16 | Validator headers: `If-Range` with a strong `ETag` (or `Last-Modified`) on every resumed range. Never a weak ETag. | BUILT | `download/ranged.rs`, `download/assemble.rs` | Server that honours `If-Range` and changes the file: the client gets `200` and restarts whole. (Servers that ignore it: file 03, S9.) |
| N17 | Connection reuse: a finished connection takes the next range with no new connect or login. A small probe response is reused for the first range. | BUILT | `probe.rs` (`REUSE_BELOW`), `tests/reuse.rs` | Logging server counts accepted TCP connections against requests: more requests than connections. |
| N18 | Mirrors: a list of places for one file, from a Metalink or the person; ranges may be spread across them; a bad mirror is dropped. | BUILT | `metalink.rs`, `download/mirrors.rs`, `tests/metalink.rs` | Two servers, one serving a wrong byte: the checksum from the Metalink catches it. |
| N19 | FTP and FTPS: USER and PASS, TYPE I, SIZE, MDTM, EPSV then PASV, REST, RETR; several data connections each with its own login and REST; explicit TLS (AUTH TLS, PBSZ, PROT P); lenient close; names from MLSD or LIST. | BUILT | `ftp/` (`session`, `transfer`, `transport`, `listing`, `reply`, `target`), `tests/ftp.rs` | Logging FTP server (file 10) records the command sequence. A server allowing 2 logins gets 2. A server without REST is fetched on one connection. |
| N20 | A cut FTP part is checked at its ends when a download resumes. | BUILT | `ftp/transfer.rs` (`docs/decisions.md` 2026-10-24) | Cut the control connection mid-part; the log shows the re-read span. |
| N21 | Second network path: bind every other connection to another interface (a phone, a second adapter). | BUILT | `second_path.rs`, `tests/second_path.rs` | The server records two source addresses. |
| N22 | IPv6 and dual stack with fallback (try the second family if the first does not connect in a short time). | PARTIAL (inherited from the HTTP client; not tested here) | `lib.rs` | Server listening on `::1` only, name resolving to both: the download works. |
| N23 | HTTP/2 and HTTP/3. Not required: ranged downloads want many connections, not one multiplexed one. Allowed where the client negotiates h2 for non-ranged requests. | DECISION (default: HTTP/1.1 for files) | `lib.rs` | A TLS server offering h2: the file is still fetched with N connections (the log shows N). |
| N24 | Limits on a hostile server: header block size, number of headers, line length, body size on an error status, redirect count. | BUILT | `retry.rs` (`unreadable_response`), `redirects.rs`, `tests/fuzz.rs`, `tests/memory.rs` | A 10 MB header line; a 1 GB body on a `404`: memory stays flat, the failure is in words. |
| N25 | The computer stays awake while downloading. | PARTIAL (window layer) | `dm-desktop` platform files | Start a download and read the OS power requests. |

## Notes for the implementer

* Do not log full addresses with a token, signature, key, password or session
  in the query (contract B47). Redact at the log.
* Every parse of a header goes through one function that returns `Option`.
  There is no `unwrap` on outside input.
* The probe decides everything later. If you change what it returns, change
  `plan.rs` tests in the same commit.
