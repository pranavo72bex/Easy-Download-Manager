# 08. Browser integration and protocol

Goal: a click in the browser becomes a dm download, with the page's context,
and a download is never lost on the way.

Standards and platform interfaces: WebExtensions (Manifest V3 for Chromium,
the Firefox build), Native Messaging (stdio framing: a 32-bit length in the
machine's byte order, then UTF-8 JSON), `webRequest`, `downloads`,
`contextMenus`, `scripting`.

Code: `browser/` (crate `dm-catcher`, Rust to WebAssembly), `extension/` (the
loader scripts, manifests, popup), `desktop/crates/dm-protocol` (the shared
vocabulary), `desktop/crates/dm-shell/src/host*` (the native host).

## Components

| Part | Job | Where |
|---|---|---|
| Loader | Registers listeners on the first turn (Manifest V3 needs this), forwards each event to the WebAssembly module once ready. The only hand-written JavaScript besides the page hooks. | `extension/background.js` |
| Decision module | Whether a download is caught; what a menu click hands over; what media requests mean; how long an answer about the app is believed. Plain Rust, tested on the build machine. | `browser/src/decide/` |
| Page hooks | Read the media a player loads; read a site's JSON answers (W4); draw the "Download video" panel. | `extension/content.js`, `document.js`, `pkg/panel.js` |
| Protocol | Request and reply types, catch rules, site rules, versioning. One crate for both sides. | `dm-protocol` |
| Host | The desktop binary itself, started by the browser; relays to the running app through a watched directory and a loopback doorbell; starts the app if it is not running. | `dm-shell` |

## Messages

| Extension sends | Host answers |
|---|---|
| `engine.hello` | `engine.hello`: version, whether it reads video pages, the rules document |
| `settings.get` | `settings.get`: the catch rules (`enabled`, `extensions`, `minSizeKb`, `skipHosts`) |
| `download.start` `{id, url, referrer, caught, ...}` | `download.started` or `download.failed` with the reason |
| Batch of addresses; formats asked | Matching replies (`dm-protocol` `Request`, `Reply`) |

A frame carries the oldest protocol version that can read it (decisions
2026-10-17), so an old extension and a new app (or the reverse) work.

## Requirements

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| B-1 | **Catch.** A browser download whose type, address or name matches the catch rules, whose size is at least the minimum, and whose host is not skipped, is paused in the browser, handed to dm, and cancelled in the browser only when dm says it has it (B26, B37). | BUILT | `browser/src/decide`, `dm-shell` | Edge with the extension against a local server (`scripts/human-qa.py`): click links of 15 kinds; each lands in dm whole; the browser keeps no copy. |
| B-2 | **Never lose a download.** No app, no answer, a refusal, a server that will not serve dm: the browser's download is resumed; where it cannot be (a browser that cannot resume what a server will not range) it is started again and marked so it is not caught twice. A download the extension itself started is never caught. | BUILT | `browser/src`, `dm-shell` | Kill dm after the hand-off but before the answer; the browser finishes the file. |
| B-3 | **Hand-over context.** Address, final address, referrer, the page's cookies for that host, the user agent, the file name the browser chose, the size when known, POST data for a form download (rare), and the browser's proxy. All kept for that download only (N11). | BUILT (POST body PARTIAL) | `dm-protocol` `Start` | A server that needs referrer, cookie and agent together refuses without any one. |
| B-4 | **Menus and keys.** "Download link with dm"; "Download all links with dm" (every link goes to the queue); Alt-click leaves the download to the browser; a toolbar switch "Catch downloads". | BUILT | `browser/src` | B28 to B31. |
| B-5 | **Panel for media.** On a page that shows media, the panel offers the candidates (file 06, X1) and hands the choice over with the request headers. Where a page's security policy forbids WebAssembly the panel cannot load; the right-click menu still works. | BUILT (live coverage partial) | `pkg/panel.js` | Gaps rows 30, 64. |
| B-6 | **Registration.** The installed app registers its native-host manifest with each browser it finds, on every start, following the installed copy's path; ids of the known extensions are listed; Firefox's downloads panel is left to Firefox. | BUILT | `dm-shell` | Install in a clean VM; browsers find the host. |
| B-7 | **Host trust.** The host answers only an extension it knows (ids built in, plus a manifest it wrote). The browser's own manifest check is the one that counts; the host's check is the second. A request file in the watched directory is parsed with limits and removed after use. | BUILT | `dm-shell/src/host.rs`, `host/tests.rs` | Send a frame from an unknown id: refused. A huge or malformed frame: refused without a crash. |
| B-8 | **Control port.** A loopback-only port served by the app for the command line and the AI-client interface: bound to 127.0.0.1, an auth token read from a file of the person's own, waiting connections evicted, not refused. | BUILT | `dm-core/src/control.rs`, `dm-shell` | Connect from another host: refused. Wrong token: refused. 100 idle connections: the oldest evicted. |
| B-9 | **Site rules.** The app serves a rules document (shipped, or a newer verified one in the home folder); the hello carries it; the extension merges it. Data only. | PARTIAL (gaps row 68) | `dm-protocol/src/rules.rs` | X7. |
| B-10 | **Updates.** The update manifest is signed; the rules document's address, SHA-256 and size are in it; a failed check leaves the old one in place. | PARTIAL (written, not built) | `dm-shell` | Corrupt the download: the old document stays. |
| B-11 | **Firefox.** A manifest of its own and a signed build; the listing step on the add-on store currently fails (gaps row 51). | PARTIAL | `extension/manifest.firefox.json`, `release.yml` | Gaps row 51. |
| B-12 | **No kernel driver, no traffic sniffing outside the extension.** A browser with no extension is not caught. | BUILT (by design) | n/a | Documentation test: the Browser page of Options says which browsers are caught. |
| B-13 | **A video button drawn by the app itself** so page overlays cannot hide it. | TO BUILD (large; the maintainer's word needed, gaps row 70) | none | Live testing on pages with overlays. |

## Rules for protocol changes

* A new field is optional on read. A removed field is ignored on read for one
  release.
* `dm-protocol` has `#![warn(missing_docs)]` and a test for every request and
  reply: encode, decode, an unknown field, a missing optional field.
* Messages are small. Large data (cookies lists of thousands of entries,
  playlist pages) is chunked, with a limit the host enforces.
